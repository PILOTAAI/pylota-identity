//! The inbound pipeline's pure steps, in the consumer's order ([Inbound › The pm-inbound consumer]):
//! parse, sanitise, derive and strip text, cut quotes, classify, assess attachments, authenticate. The
//! corpus runner compares its [`Outcome`] with each case's expectations; the workerd runner (M7) compares
//! the Message object the API returns with the same expectations.
//!
//! [Inbound › The pm-inbound consumer]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use std::collections::BTreeSet;

use pylota_identity_core::attach::{self, Risk};
use pylota_identity_core::auth::{self, DnsAnswers, Spf, VerdictInput};
use pylota_identity_core::classify::{self, Classification};
use pylota_identity_core::mime::{self, Caps, Flag, ParsedMessage};
use pylota_identity_core::{quote, sanitize, text, thread};

/// What the pipeline produced for one message.
#[derive(Debug)]
pub struct Outcome {
    pub parsed: ParsedMessage,
    /// Message flags: the parser's (without `headers_only`) plus `hidden_text`.
    pub flags: BTreeSet<String>,
    /// `text`: the text body, or text derived from the HTML; hidden characters removed.
    pub text: Option<String>,
    pub html_sanitized: Option<String>,
    pub extracted_text: String,
    pub cids: Vec<String>,
    /// Per attachment: sniffed type and risk.
    pub assessments: Vec<attach::Assessment>,
    pub classification: Classification,
    pub dkim: Vec<auth::DkimEntry>,
    pub dmarc: auth::Dmarc,
    pub verdict: auth::Verdict,
}

/// Runs the pure steps over `raw`. `trusted_authserv_id` is `PM_TRUSTED_AUTHSERV_ID`.
pub fn run(
    raw: &[u8],
    envelope_from: &str,
    dns: &DnsAnswers,
    trusted_authserv_id: &str,
) -> Outcome {
    let parsed = mime::parse(raw, &Caps::default());
    let mut flags: BTreeSet<String> = parsed
        .flags
        .iter()
        .filter(|f| **f != Flag::HeadersOnly)
        .map(|f| {
            serde_json::to_value(f)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        })
        .collect();
    let encrypted = parsed.has(Flag::Encrypted);

    let doc = parsed
        .html
        .as_deref()
        .filter(|_| !encrypted)
        .map(sanitize::parse);
    let mut hidden = doc.as_ref().is_some_and(|d| d.hidden_text);
    let html_sanitized = doc.as_ref().map(|d| {
        let (clean, chars) = sanitize::strip_hidden_chars(&d.sanitized_html());
        hidden |= chars.flags();
        clean
    });
    let text = if encrypted {
        None
    } else {
        parsed
            .text
            .clone()
            .or_else(|| doc.as_ref().map(|d| text::derive_text(&d.body())))
            .map(|t| {
                let (clean, chars) = sanitize::strip_hidden_chars(&t);
                hidden |= chars.flags();
                clean
            })
    };
    let extracted_text = text
        .as_deref()
        .map(|t| {
            let body = doc.as_ref().map(sanitize::Document::body);
            sanitize::strip_hidden_chars(&quote::extract_new_content(t, body.as_ref())).0
        })
        .unwrap_or_default();
    if hidden {
        flags.insert("hidden_text".to_owned());
    }

    let classification = classify::classify(&parsed, envelope_from);
    let assessments = parsed
        .attachments
        .iter()
        .map(|a| attach::assess(&a.data, a.filename.as_deref(), &a.content_type))
        .collect();

    let ar = &parsed.headers.authentication_results;
    let trusted = auth::trusted_results(ar, trusted_authserv_id);
    let signatures = auth::verify_signatures(raw, dns);
    let mut dkim = signatures.dkim;
    let spf = Spf::from_trusted(trusted.as_ref(), envelope_from);
    let from_domain = parsed
        .from
        .as_ref()
        .map(|f| pylota_identity_core::address::domain_of(&f.address).to_owned())
        .unwrap_or_default();
    let dmarc = auth::dmarc(&from_domain, &mut dkim, &spf, dns, trusted.as_ref());
    let verdict = auth::verdict(&VerdictInput {
        loopback: false,
        dmarc: &dmarc,
        spf_checked: spf.checked,
        multiple_from: parsed.from_count > 1,
        rescue: None,
    });

    Outcome {
        cids: doc.as_ref().map(|d| d.cids.clone()).unwrap_or_default(),
        parsed,
        flags,
        text,
        html_sanitized,
        extracted_text,
        assessments,
        classification,
        dkim,
        dmarc,
        verdict,
    }
}

impl Outcome {
    pub fn normalised_subject(&self) -> String {
        thread::normalise_subject(self.parsed.subject.as_deref().unwrap_or(""))
    }

    pub fn risks(&self) -> Vec<Risk> {
        self.assessments.iter().filter_map(|a| a.risk).collect()
    }
}
