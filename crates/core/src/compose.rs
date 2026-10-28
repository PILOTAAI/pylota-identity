//! Outbound composition ([Outbound › Composition]): recipients, the custom-header rules of policy step
//! 11, the body (signature, AI disclosure, unsubscribe footer, forward note), the service headers and
//! the MIME of the sent copy. Pure: the Message-ID, the `Date` and the multipart boundaries are inputs,
//! because `mail-builder` would otherwise read the system clock.
//!
//! [Outbound › Composition]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use std::borrow::Cow;

use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::date::Date;
use mail_builder::headers::message_id::MessageId;
use mail_builder::headers::raw::Raw;
use mail_builder::mime::{BodyPart, MimePart};

use crate::mime::normalise_address;

/// The composed MIME may use this much; the transport adds up to 8 KiB of headers (G5).
pub const MAX_MIME_BYTES: usize = 5_242_880 - 8_192;
/// Decoded attachment bytes of one message.
pub const MAX_ATTACHMENT_BYTES: usize = 26_214_400;
/// The loop guard (N13): a computed hop at or above this is refused.
pub const MAX_HOP: u32 = 10;
/// Cloudflare's limit for `In-Reply-To` and `References` values.
pub const MAX_THREAD_HEADER_BYTES: usize = 2048;
/// Every transport's recipient limit (strategy B's journal copy keeps one of Cloudflare's 50).
pub const HARD_MAX_RECIPIENTS: usize = 49;

/// Why a recipient address is refused at policy step 6.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddressError {
    Invalid,
    /// A non-ASCII (SMTPUTF8) local part (A3).
    Unsupported,
}

/// Step 6: one recipient in stored form (lower case, A-label domain), with an ASCII local part of at
/// most 64 octets and an RFC 5321 shape.
pub fn recipient(address: &str) -> Result<String, AddressError> {
    let a = address.trim();
    let (local, domain) = a.rsplit_once('@').ok_or(AddressError::Invalid)?;
    if local.is_empty() || local.len() > 64 || domain.is_empty() {
        return Err(AddressError::Invalid);
    }
    if !local.is_ascii() {
        return Err(AddressError::Unsupported);
    }
    let atext = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.".contains(c);
    if !local.chars().all(atext)
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
    {
        return Err(AddressError::Invalid);
    }
    let n = normalise_address(a).ok_or(AddressError::Invalid)?;
    if n.len() > 254 {
        return Err(AddressError::Invalid);
    }
    Ok(n)
}

/// Why a custom header is refused at policy step 11.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// `400 header_not_allowed`.
    NotAllowed(String),
    /// `400 invalid_request` at `path`.
    Invalid { path: String, message: String },
}

/// The non-`X-` names a caller may set, in the casing they are sent in.
const ALLOWED: &[&str] = &[
    "Importance",
    "Priority",
    "Sensitivity",
    "Keywords",
    "Comments",
    "Organization",
];

fn x_name(name: &str) -> bool {
    name.len() > 2
        && name.len() <= 100
        && name[..2].eq_ignore_ascii_case("x-")
        && name[2..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Step 11 (Headers): validates the caller's headers and returns them with the six fixed names in
/// their sending casing. `service_count` is the number of non-`X-` headers the service adds, which
/// count towards the limit of 20.
pub fn check_headers(
    headers: &[(String, String)],
    service_count: usize,
) -> Result<Vec<(String, String)>, HeaderError> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut total = 0usize;
    let mut fixed = service_count;
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        let reserved = lower.starts_with("x-pylota-") || lower == "x-ai-generated";
        let sent = if reserved {
            return Err(HeaderError::NotAllowed(name.clone()));
        } else if x_name(name) {
            name.clone()
        } else if let Some(n) = ALLOWED.iter().find(|n| n.eq_ignore_ascii_case(name)) {
            fixed += 1;
            (*n).to_owned()
        } else {
            return Err(HeaderError::NotAllowed(name.clone()));
        };
        let path = format!("headers.{name}");
        let invalid = |message: &str| HeaderError::Invalid {
            path: path.clone(),
            message: message.to_owned(),
        };
        if out.iter().any(|(n, _)| n.eq_ignore_ascii_case(&sent)) {
            return Err(invalid("Two headers differ only in case."));
        }
        if value.is_empty() || value.len() > 2048 || value.contains(['\r', '\n']) {
            return Err(invalid(
                "A header value is 1 to 2,048 bytes without CR or LF.",
            ));
        }
        let allowed_values: &[&str] = match sent.as_str() {
            "Importance" => &["high", "normal", "low"],
            "Priority" => &["normal", "non-urgent", "urgent"],
            "Sensitivity" => &["personal", "private", "company-confidential"],
            _ => &[],
        };
        if !allowed_values.is_empty() && !allowed_values.contains(&value.as_str()) {
            return Err(invalid("This header takes one of the documented values."));
        }
        total += sent.len() + value.len() + 4;
        out.push((sent, value.clone()));
    }
    if fixed > 20 {
        return Err(HeaderError::Invalid {
            path: "headers".into(),
            message: "At most 20 non-X- headers.".into(),
        });
    }
    if total > 16_384 {
        return Err(HeaderError::Invalid {
            path: "headers".into(),
            message: "Custom headers total at most 16 KB.".into(),
        });
    }
    Ok(out)
}

/// `ai_disclosure.mode` (E8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Disclosure {
    None,
    Footer(String),
    Header,
}

/// The transfer note's original (C6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Forwarded {
    pub from: String,
    /// RFC 2822 date of the original.
    pub date: String,
    pub subject: String,
    pub to: String,
    pub text: String,
    /// The original's sanitised HTML.
    pub html: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutAttachment {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
    /// Set on an inline part.
    pub content_id: Option<String>,
}

/// Everything the composition reads; the mailbox fills it.
#[derive(Clone, Debug, Default)]
pub struct ComposeInput {
    /// Without angle brackets: `{message ulid}@{from domain}`.
    pub message_id: String,
    pub date_s: i64,
    /// A seed for the multipart boundaries (the message ID).
    pub boundary_seed: String,
    pub from: (String, String),
    pub reply_to: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub text: Option<String>,
    pub html: Option<String>,
    pub signature_text: Option<String>,
    pub signature_html: Option<String>,
    pub disclosure: Option<Disclosure>,
    /// `kind: marketing`: the HTTPS URL and an optional mailto (G9).
    pub unsubscribe: Option<(String, Option<String>)>,
    /// A notification's one-click URL: headers only, no footer.
    pub list_unsubscribe: Option<String>,
    pub forward: Option<Forwarded>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub auto_reply: bool,
    pub hop: u32,
    pub custom_headers: Vec<(String, String)>,
    pub attachments: Vec<OutAttachment>,
}

/// The composed message: the bodies and headers a structured transport sends, and the MIME copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composed {
    pub text: Option<String>,
    pub html: Option<String>,
    /// Service and custom headers, in sending order.
    pub headers: Vec<(String, String)>,
    pub mime: Vec<u8>,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Plain text from HTML (Inbound › Text derivation).
pub fn text_of_html(html: &str) -> String {
    let doc = crate::sanitize::parse(html);
    crate::text::derive_text(&doc.body())
}

/// `References` as `<id>` values, the entries after the first dropped oldest first until the value fits
/// Cloudflare's 2,048 bytes.
pub fn references_value(ids: &[String]) -> Option<String> {
    let mut ids: Vec<&String> = ids.iter().collect();
    loop {
        if ids.is_empty() {
            return None;
        }
        let v = ids
            .iter()
            .map(|i| format!("<{i}>"))
            .collect::<Vec<_>>()
            .join(" ");
        if v.len() <= MAX_THREAD_HEADER_BYTES || ids.len() == 1 {
            return (v.len() <= MAX_THREAD_HEADER_BYTES).then_some(v);
        }
        ids.remove(1);
    }
}

/// The transfer note of a forward, text form.
fn forward_note(f: &Forwarded) -> String {
    format!(
        "---------- Forwarded message ---------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n\n{}",
        f.from, f.date, f.subject, f.to, f.text
    )
}

/// Body steps 1–5 and the service headers.
fn bodies(inp: &ComposeInput) -> (Option<String>, Option<String>, Vec<(String, String)>) {
    let mut text = inp
        .text
        .clone()
        .or_else(|| inp.html.as_deref().map(text_of_html))
        .unwrap_or_default();
    let mut html = inp.html.clone();
    if let Some(f) = &inp.forward {
        text.push_str("\n\n");
        text.push_str(&forward_note(f));
        if let Some(h) = html.as_mut() {
            let note = escape(&forward_note(&Forwarded {
                text: String::new(),
                ..f.clone()
            }))
            .replace('\n', "<br>");
            h.push_str(&format!(
                "<br><br>{note}<blockquote>{}</blockquote>",
                f.html.clone().unwrap_or_else(|| escape(&f.text))
            ));
        }
    }
    if let Some(sig) = inp.signature_text.as_deref().filter(|s| !s.is_empty()) {
        text.push_str("\n\n-- \n");
        text.push_str(sig);
    }
    if let Some(h) = html.as_mut() {
        let sig = inp
            .signature_html
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                inp.signature_text
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(escape)
            });
        if let Some(sig) = sig {
            h.push_str("<br><br>-- <br>");
            h.push_str(&sig);
        }
    }
    let mut headers = Vec::new();
    if let Some(irt) = &inp.in_reply_to {
        headers.push(("In-Reply-To".to_owned(), format!("<{irt}>")));
    }
    if let Some(r) = references_value(&inp.references) {
        headers.push(("References".to_owned(), r));
    }
    if inp.auto_reply {
        headers.push(("Auto-Submitted".to_owned(), "auto-replied".to_owned()));
    }
    headers.push(("X-Pylota-Identity-Hop".to_owned(), inp.hop.to_string()));
    match &inp.disclosure {
        Some(Disclosure::Footer(f)) if !f.is_empty() => {
            text.push_str("\n\n");
            text.push_str(f);
            if let Some(h) = html.as_mut() {
                h.push_str(&format!("<p>{}</p>", escape(f)));
            }
        }
        Some(Disclosure::Header) => {
            headers.push(("X-AI-Generated".to_owned(), "true".to_owned()));
        }
        _ => {}
    }
    if let Some((url, mailto)) = &inp.unsubscribe {
        let value = match mailto {
            Some(m) => format!("<{url}>, <mailto:{m}>"),
            None => format!("<{url}>"),
        };
        headers.push(("List-Unsubscribe".to_owned(), value));
        headers.push((
            "List-Unsubscribe-Post".to_owned(),
            "List-Unsubscribe=One-Click".to_owned(),
        ));
        if !text.contains(url.as_str()) {
            text.push_str(&format!("\n\nUnsubscribe: {url}"));
        }
        if let Some(h) = html.as_mut()
            && !h.contains(url.as_str())
        {
            h.push_str(&format!(
                "<p><a href=\"{}\">Unsubscribe</a></p>",
                escape(url)
            ));
        }
    } else if let Some(url) = &inp.list_unsubscribe {
        headers.push(("List-Unsubscribe".to_owned(), format!("<{url}>")));
        headers.push((
            "List-Unsubscribe-Post".to_owned(),
            "List-Unsubscribe=One-Click".to_owned(),
        ));
    }
    headers.extend(inp.custom_headers.iter().cloned());
    (Some(text), html, headers)
}

/// Composes the message (steps 1–6 of the body, the headers and the MIME copy without `Bcc`).
pub fn compose(inp: &ComposeInput) -> Composed {
    let (text, html, headers) = bodies(inp);
    let boundary = |n: u8| format!("pm-{}-{n}", inp.boundary_seed);
    let mut b = MessageBuilder::new()
        .message_id(MessageId::new(inp.message_id.clone()))
        .date(Date::new(inp.date_s))
        .from(Address::new_address(
            Some(inp.from.1.clone()).filter(|n| !n.is_empty()),
            inp.from.0.clone(),
        ))
        .subject(inp.subject.clone());
    let list = |v: &[String]| {
        Address::new_list(
            v.iter()
                .map(|a| Address::new_address(None::<String>, a.clone()))
                .collect(),
        )
    };
    if !inp.to.is_empty() {
        b = b.to(list(&inp.to));
    }
    if !inp.cc.is_empty() {
        b = b.cc(list(&inp.cc));
    }
    if let Some(r) = &inp.reply_to {
        b = b.reply_to(Address::new_address(None::<String>, r.clone()));
    }
    for (n, v) in &headers {
        b = b.header(n.clone(), Raw::new(v.clone()));
    }
    let text_part = |t: &str| MimePart::new(ContentType::new("text/plain"), t.to_owned());
    let alt = match (&text, &html) {
        (Some(t), Some(h)) => MimePart::new(
            ContentType::new("multipart/alternative").attribute("boundary", boundary(2)),
            BodyPart::Multipart(vec![
                text_part(t),
                MimePart::new(ContentType::new("text/html"), h.clone()),
            ]),
        ),
        (Some(t), None) => text_part(t),
        (None, Some(h)) => MimePart::new(ContentType::new("text/html"), h.clone()),
        (None, None) => text_part(""),
    };
    let body = if inp.attachments.is_empty() {
        alt
    } else {
        let mut parts = vec![alt];
        for a in &inp.attachments {
            let part = MimePart::new(
                ContentType::new(a.content_type.clone()),
                BodyPart::Binary(Cow::Owned(a.bytes.clone())),
            );
            parts.push(match &a.content_id {
                Some(cid) => part.inline().cid(cid.clone()),
                None => part.attachment(a.filename.clone()),
            });
        }
        MimePart::new(
            ContentType::new("multipart/mixed").attribute("boundary", boundary(1)),
            BodyPart::Multipart(parts),
        )
    };
    let mime = b.body(body).write_to_vec().unwrap_or_default();
    Composed {
        text,
        html,
        headers,
        mime,
    }
}

/// The headers a structured transport sends, read back from the stored copy: the service and custom
/// headers composition wrote, unfolded, in order.
pub fn transport_headers(mime: &[u8]) -> Vec<(String, String)> {
    let end = mime
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(mime.len());
    let block = String::from_utf8_lossy(&mime[..end]);
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in block.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = fields.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((n, v)) = line.split_once(':') {
            fields.push((n.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let sent = |n: &str| {
        let l = n.to_ascii_lowercase();
        l.starts_with("x-")
            || ALLOWED.iter().any(|a| a.eq_ignore_ascii_case(n))
            || matches!(
                l.as_str(),
                "in-reply-to"
                    | "references"
                    | "auto-submitted"
                    | "list-unsubscribe"
                    | "list-unsubscribe-post"
            )
    };
    fields.into_iter().filter(|(n, _)| sent(n)).collect()
}

/// Strategy A (spike S7): the `messageId` Cloudflare returns is the header value. Checks the
/// `<…@{from domain}>` form and returns it without the brackets; `None` when the form differs.
pub fn derive_cf_message_id(provider_id: &str, from_domain: &str) -> Option<String> {
    let inner = provider_id.strip_prefix('<')?.strip_suffix('>')?;
    let (_, domain) = inner.rsplit_once('@')?;
    domain
        .eq_ignore_ascii_case(from_domain)
        .then(|| inner.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn header_rules() {
        let ok = check_headers(
            &h(&[
                ("X-Booking-Ref", "BK-1"),
                ("importance", "high"),
                ("keywords", "a"),
            ]),
            0,
        )
        .unwrap();
        assert_eq!(ok[1], ("Importance".to_owned(), "high".to_owned()));
        for bad in [
            "X-Bad Name",
            "X-",
            "Importance ",
            "x-pylota-trace",
            "x-ai-generated",
            "From",
            "List-Unsubscribe",
        ] {
            assert!(
                matches!(
                    check_headers(&h(&[(bad, "v")]), 0),
                    Err(HeaderError::NotAllowed(_))
                ),
                "{bad}"
            );
        }
        for (n, v) in [
            ("Importance", "urgent"),
            ("priority", "high"),
            ("Sensitivity", "secret"),
        ] {
            assert!(matches!(
                check_headers(&h(&[(n, v)]), 0),
                Err(HeaderError::Invalid { .. })
            ));
        }
        assert!(matches!(
            check_headers(&h(&[("Importance", "high"), ("importance", "low")]), 0),
            Err(HeaderError::Invalid { .. })
        ));
    }

    #[test]
    fn recipients() {
        assert_eq!(recipient(" Jo@Example.NET ").unwrap(), "jo@example.net");
        assert_eq!(recipient("jö@example.net"), Err(AddressError::Unsupported));
        for bad in ["jo", "@x.net", "a..b@x.net", "jo@", "jo@x"] {
            assert_eq!(recipient(bad), Err(AddressError::Invalid), "{bad}");
        }
    }

    fn input() -> ComposeInput {
        ComposeInput {
            message_id: "01jx@agents.example".into(),
            date_s: 1_790_000_000,
            boundary_seed: "01jx".into(),
            from: ("a@agents.example".into(), "Acme".into()),
            to: vec!["jo@example.net".into()],
            subject: "Hello".into(),
            text: Some("Body".into()),
            html: Some("<p>Body</p>".into()),
            signature_text: Some("Acme".into()),
            hop: 1,
            ..ComposeInput::default()
        }
    }

    #[test]
    fn deterministic_mime() {
        let mut inp = input();
        inp.attachments.push(OutAttachment {
            filename: "a.txt".into(),
            content_type: "text/plain".into(),
            bytes: b"hi".to_vec(),
            content_id: None,
        });
        let a = compose(&inp);
        let b = compose(&inp);
        assert_eq!(a.mime, b.mime, "no clock and no random boundary");
        let s = String::from_utf8_lossy(&a.mime);
        assert!(s.contains("Message-ID: <01jx@agents.example>"));
        assert!(s.contains("pm-01jx-1") && s.contains("pm-01jx-2"));
        assert!(s.contains("X-Pylota-Identity-Hop: 1"));
        assert!(a.text.unwrap().ends_with("Body\n\n-- \nAcme"));
        let h = transport_headers(&a.mime);
        assert_eq!(
            h,
            vec![("X-Pylota-Identity-Hop".to_owned(), "1".to_owned())]
        );
    }

    #[test]
    fn disclosure_unsubscribe_forward() {
        let mut inp = input();
        inp.disclosure = Some(Disclosure::Footer("Written by an AI agent.".into()));
        inp.unsubscribe = Some(("https://u.example/x".into(), None));
        let c = compose(&inp);
        let t = c.text.unwrap();
        assert!(
            t.contains("Written by an AI agent.")
                && t.ends_with("Unsubscribe: https://u.example/x")
        );
        assert!(c.html.unwrap().contains("<p>Written by an AI agent.</p>"));
        assert!(c.headers.iter().any(|(n, _)| n == "List-Unsubscribe-Post"));
        let mut inp = input();
        inp.disclosure = Some(Disclosure::Header);
        inp.forward = Some(Forwarded {
            from: "B <b@x.example>".into(),
            subject: "Invoice".into(),
            text: "original".into(),
            ..Forwarded::default()
        });
        let c = compose(&inp);
        assert!(
            c.headers
                .contains(&("X-AI-Generated".into(), "true".into()))
        );
        let t = c.text.unwrap();
        assert!(t.contains("---------- Forwarded message ---------") && t.contains("original"));
    }

    #[test]
    fn references_fit_2048() {
        let ids: Vec<String> = (0..60).map(|i| format!("{i:040}@x.example")).collect();
        let v = references_value(&ids).unwrap();
        assert!(v.len() <= 2048);
        assert!(v.starts_with(&format!("<{}>", ids[0])));
        assert!(v.ends_with(&format!("<{}>", ids[59])));
    }

    #[test]
    fn strategy_a() {
        assert_eq!(
            derive_cf_message_id("<abc@agents.example>", "agents.example").as_deref(),
            Some("abc@agents.example")
        );
        assert_eq!(derive_cf_message_id("abc", "agents.example"), None);
    }
}
