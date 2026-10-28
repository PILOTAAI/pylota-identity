//! The inbound pipeline's pure steps, in the consumer's order ([Inbound › The pm-inbound consumer]):
//! parse, sanitise, derive and strip text, cut quotes, classify, assess attachments, authenticate. The
//! `pm-inbound` consumer runs them on every message (Worker, M7); the corpus runner compares the
//! [`Outcome`] of [`run`] with each case's expectations, and the workerd runner compares the Message
//! object the API returns with the same expectations.
//!
//! [Inbound › The pm-inbound consumer]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use std::collections::BTreeSet;

use crate::attach::{self, Risk};
use crate::auth::{self, DnsAnswers, Spf, VerdictInput};
use crate::classify::{self, Classification};
use crate::mime::{self, Caps, Flag, ParsedMessage};
use crate::{quote, sanitize, text, thread};

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

/// The content steps (Inbound › Sanitising, Hidden text, Text derivation, Quote stripping).
#[derive(Debug, Default)]
pub struct Content {
    /// Message flags: the parser's (without `headers_only`) plus `hidden_text`.
    pub flags: BTreeSet<String>,
    pub text: Option<String>,
    pub html_sanitized: Option<String>,
    pub extracted_text: String,
    pub cids: Vec<String>,
}

/// Step 8 of the consumer over a parsed message.
pub fn content(parsed: &ParsedMessage) -> Content {
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
    Content {
        cids: doc.as_ref().map(|d| d.cids.clone()).unwrap_or_default(),
        flags,
        text,
        html_sanitized,
        extracted_text,
    }
}

/// What authentication found (Inbound › Authentication verdict, steps 1–7).
#[derive(Debug)]
pub struct Authentication {
    pub trusted: Option<auth::AuthResults>,
    pub spf: Spf,
    pub dkim: Vec<auth::DkimEntry>,
    pub dmarc: auth::Dmarc,
    pub verdict: auth::Verdict,
    pub multiple_from: bool,
}

/// The `From` domain authentication evaluates (empty without `From`).
pub fn from_domain(parsed: &ParsedMessage) -> String {
    parsed
        .from
        .as_ref()
        .map(|f| crate::address::domain_of(&f.address).to_owned())
        .unwrap_or_default()
}

/// Steps 1 and 3–7 over `raw`, with the DNS answers of step 2. `spf` replaces the trusted header's
/// SPF (the SES source); `loopback` passes as `pass` (L3).
pub fn authenticate(
    raw: &[u8],
    parsed: &ParsedMessage,
    envelope_from: &str,
    dns: &DnsAnswers,
    trusted_authserv_id: &str,
    spf: Option<Spf>,
    loopback: bool,
) -> Authentication {
    let trusted =
        auth::trusted_results(&parsed.headers.authentication_results, trusted_authserv_id);
    let signatures = auth::verify_signatures(raw, dns);
    let mut dkim = signatures.dkim;
    let spf = spf.unwrap_or_else(|| Spf::from_trusted(trusted.as_ref(), envelope_from));
    let dmarc = auth::dmarc(&from_domain(parsed), &mut dkim, &spf, dns, trusted.as_ref());
    let multiple_from = parsed.from_count > 1;
    let verdict = auth::verdict(&VerdictInput {
        loopback,
        dmarc: &dmarc,
        spf_checked: spf.checked,
        multiple_from,
        rescue: None,
    });
    Authentication {
        trusted,
        spf,
        dkim,
        dmarc,
        verdict,
        multiple_from,
    }
}

/// Runs the pure steps over `raw`. `trusted_authserv_id` is `PM_TRUSTED_AUTHSERV_ID`.
pub fn run(
    raw: &[u8],
    envelope_from: &str,
    dns: &DnsAnswers,
    trusted_authserv_id: &str,
) -> Outcome {
    let parsed = mime::parse(raw, &Caps::default());
    let c = content(&parsed);
    let classification = classify::classify(&parsed, envelope_from);
    let assessments = parsed
        .attachments
        .iter()
        .map(|a| attach::assess(&a.data, a.filename.as_deref(), &a.content_type))
        .collect();
    let a = authenticate(
        raw,
        &parsed,
        envelope_from,
        dns,
        trusted_authserv_id,
        None,
        false,
    );
    Outcome {
        cids: c.cids,
        parsed,
        flags: c.flags,
        text: c.text,
        html_sanitized: c.html_sanitized,
        extracted_text: c.extracted_text,
        assessments,
        classification,
        dkim: a.dkim,
        dmarc: a.dmarc,
        verdict: a.verdict,
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

/// The `<a href>` links of sanitised HTML: `(href, visible text)` in document order.
pub fn links(html_sanitized: &str) -> Vec<(String, String)> {
    static A: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static TAG: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let a = A.get_or_init(|| {
        regex::Regex::new(r#"(?is)<a\s[^>]*?href="([^"]*)"[^>]*>(.*?)</a>"#).expect("fixed pattern")
    });
    let tag = TAG.get_or_init(|| regex::Regex::new(r"(?s)<[^>]*>").expect("fixed pattern"));
    a.captures_iter(html_sanitized)
        .take(500)
        .map(|c| {
            let href = c[1].replace("&amp;", "&");
            let text = tag.replace_all(&c[2], "").trim().replace("&amp;", "&");
            (href, text)
        })
        .collect()
}

fn link_host(s: &str) -> Option<String> {
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .trim()
        .to_ascii_lowercase();
    (host.contains('.') && !host.contains(' ')).then_some(host)
}

/// The content signals of the spam score (Inbound › Trust signals › Spam score): hidden text, a link
/// whose visible text names another domain than its target, an HTML-only message of few words and many
/// links, and a shouting subject. Ingest adds the verdict and sender signals.
pub fn spam_base(
    subject: &str,
    has_text_part: bool,
    text: &str,
    links: &[(String, String)],
    hidden: bool,
) -> f64 {
    let mut score = 0.0;
    if hidden {
        score += 0.20;
    }
    let mismatch =
        links.iter().any(
            |(href, visible)| match (link_host(visible), link_host(href)) {
                (Some(v), Some(h)) => {
                    crate::address::org_domain(&v) != crate::address::org_domain(&h)
                }
                _ => false,
            },
        );
    if mismatch {
        score += 0.20;
    }
    if !has_text_part && links.len() >= 3 && text.chars().count() < 200 {
        score += 0.10;
    }
    let letters: Vec<char> = subject.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 10
        && letters.iter().filter(|c| c.is_uppercase()).count() * 10 >= letters.len() * 7
    {
        score += 0.10;
    }
    score
}

#[cfg(test)]
mod signal_tests {
    use super::*;

    #[test]
    fn links_and_spam_base() {
        let l = links(
            r#"<p>Go <a href="https://evil.example/x?a=1&amp;b=2">bank.example</a> <a href="mailto:a@b.c">mail</a></p>"#,
        );
        assert_eq!(
            l[0],
            (
                "https://evil.example/x?a=1&b=2".to_owned(),
                "bank.example".to_owned()
            )
        );
        assert_eq!(l.len(), 2);
        assert!((spam_base("hello", true, "x", &l, false) - 0.20).abs() < 1e-9);
        assert!((spam_base("FREE MONEY NOW!!!", true, "x", &[], true) - 0.30).abs() < 1e-9);
        assert_eq!(spam_base("Hi", true, "x", &[], false), 0.0);
    }
}
