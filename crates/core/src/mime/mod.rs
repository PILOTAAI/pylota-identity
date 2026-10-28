//! MIME parsing with caps ([Inbound › Parsing]): `mail-parser` reads the message, and this module walks
//! the parsed tree with the nesting and part caps, selects the bodies, collects attachments (unpacking
//! TNEF), and records the flags of B2, B5, B9 and B13.
//!
//! [Inbound › Parsing]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

pub mod dsn;
pub mod smime;
pub mod tnef;

use std::collections::BTreeSet;

use mail_parser::{Address, Message, MessageParser, MessagePart, MimeHeaders, PartType};
use serde::Serialize;

use crate::address::normalise_domain;
use crate::thread::{normalise_msg_id, split_references};

/// Parsing caps ([limits](https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/limits.md#mail)).
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub max_depth: usize,
    pub max_parts: usize,
    /// Nested `message/rfc822` levels whose text is extracted.
    pub nested_text_levels: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Caps {
            max_depth: 32,
            max_parts: 500,
            nested_text_levels: 3,
        }
    }
}

/// Message flags set while parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    ParseDegraded,
    Encrypted,
    MessageIdSynthetic,
    HeadersOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Mailbox {
    /// Lower case, IDNA A-label domain.
    pub address: String,
    /// Control characters removed, at most 256 characters; empty when absent.
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureType {
    Smime,
    Pgp,
}

/// `auth_json.signature` (B9): v1 records a signature and never verifies it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Signature {
    #[serde(rename = "type")]
    pub kind: SignatureType,
    pub status: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartOrigin {
    Mime,
    /// Unpacked from a TNEF `winmail.dat`.
    Tnef,
}

/// A summary of a nested `message/rfc822` part (B6): content of the outer message, never a thread member.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Nested {
    pub subject: Option<String>,
    pub from: Option<Mailbox>,
    pub message_id: Option<String>,
    /// The nested message's text (its own text body, or text derived from its HTML by the caller); `None`
    /// beyond the nested-text cap.
    pub text: Option<String>,
    pub html: Option<String>,
}

/// One attachment: every leaf that is not a selected body, including inline images.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Attachment {
    /// `type/subtype`, lower case, as declared.
    pub content_type: String,
    pub filename: Option<String>,
    /// Without angle brackets.
    pub content_id: Option<String>,
    pub inline: bool,
    pub size: usize,
    #[serde(skip)]
    pub data: Vec<u8>,
    pub origin: PartOrigin,
    pub nested: Option<Box<Nested>>,
}

/// The header fields the pipeline reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Headers {
    pub auto_submitted: Option<String>,
    pub list_id: Option<String>,
    pub list_unsubscribe: Option<String>,
    pub list_post: Option<String>,
    pub precedence: Option<String>,
    pub x_autoreply: bool,
    pub x_autorespond: bool,
    pub x_auto_response_suppress: Option<String>,
    /// `X-Pylota-Identity-Hop`, when it parses.
    pub hop: Option<u32>,
    /// `Authentication-Results`, topmost first, unfolded.
    pub authentication_results: Vec<String>,
    pub disposition_notification_to: Option<String>,
    /// The top-level `Content-Type`, `type/subtype` lower case.
    pub content_type: String,
    /// Its `report-type` parameter, lower case.
    pub report_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ParsedMessage {
    /// The first parseable `From` mailbox.
    pub from: Option<Mailbox>,
    /// How many `From` mailboxes there were (B13: more than one caps the verdict).
    pub from_count: usize,
    pub sender: Option<Mailbox>,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub reply_to: Vec<Mailbox>,
    pub subject: Option<String>,
    /// `Date` in Unix seconds, when it parses.
    pub date: Option<i64>,
    /// Normalised, or `{raw_sha256}@synthetic.invalid` (flag `message_id_synthetic`).
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub headers: Headers,
    /// The selected bodies, decoded to UTF-8; `None` for encrypted messages.
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
    pub flags: BTreeSet<Flag>,
    pub signature: Option<Signature>,
}

impl ParsedMessage {
    fn empty(message_id: String) -> Self {
        ParsedMessage {
            from: None,
            from_count: 0,
            sender: None,
            to: Vec::new(),
            cc: Vec::new(),
            reply_to: Vec::new(),
            subject: None,
            date: None,
            message_id,
            in_reply_to: None,
            references: Vec::new(),
            headers: Headers::default(),
            text: None,
            html: None,
            attachments: Vec::new(),
            flags: BTreeSet::new(),
            signature: None,
        }
    }

    pub fn has(&self, flag: Flag) -> bool {
        self.flags.contains(&flag)
    }
}

/// Parses `raw` (never fails: a message with no headers parses to an empty, degraded result).
pub fn parse(raw: &[u8], caps: &Caps) -> ParsedMessage {
    parse_with(raw, caps, false)
}

/// The third attempt after two failed parses (B2): headers only, flag `parse_degraded`.
pub fn parse_headers_only(raw: &[u8]) -> ParsedMessage {
    parse_with(raw, &Caps::default(), true)
}

fn parse_with(raw: &[u8], caps: &Caps, headers_only: bool) -> ParsedMessage {
    let parser = MessageParser::default();
    let message = if headers_only {
        parser.parse_headers(raw)
    } else {
        parser.parse(raw)
    };
    let mut out = ParsedMessage::empty(synthetic_id(raw));
    if headers_only {
        out.flags.insert(Flag::HeadersOnly);
        out.flags.insert(Flag::ParseDegraded);
    }
    let Some(message) = message else {
        out.flags.insert(Flag::ParseDegraded);
        out.flags.insert(Flag::MessageIdSynthetic);
        return out;
    };
    read_headers(&message, &mut out);
    if !headers_only {
        let mut walk = Walk {
            caps,
            parts_seen: 0,
            out: &mut out,
            opaque: Vec::new(),
        };
        walk.message(&message, 0, 0);
    }
    out
}

fn synthetic_id(raw: &[u8]) -> String {
    format!("{}@synthetic.invalid", crate::ses::sha256_hex(raw))
}

/// Control characters removed, at most `max` characters.
pub fn clean_display(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Lower-cases an address and converts its domain to an A-label; `None` without a valid domain.
pub fn normalise_address(address: &str) -> Option<String> {
    let a = address.trim();
    let (local, domain) = a.rsplit_once('@')?;
    if local.is_empty() || local.chars().any(|c| c.is_control() || c == ' ') {
        return None;
    }
    Some(format!(
        "{}@{}",
        local.to_lowercase(),
        normalise_domain(domain)?
    ))
}

fn mailboxes(value: Option<&Address<'_>>) -> (Vec<Mailbox>, usize) {
    let mut out = Vec::new();
    let mut count = 0;
    let Some(value) = value else {
        return (out, 0);
    };
    for addr in value.iter() {
        count += 1;
        if let Some(address) = addr.address.as_deref().and_then(normalise_address) {
            out.push(Mailbox {
                address,
                name: clean_display(addr.name.as_deref().unwrap_or(""), 256),
            });
        }
    }
    (out, count)
}

fn unfold(s: &str) -> String {
    s.split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_headers(m: &Message<'_>, out: &mut ParsedMessage) {
    // Every `From` header in order (mail-parser's `from()` returns the last one); B13 takes the first
    // parseable mailbox.
    let mut from = Vec::new();
    let mut from_count = 0;
    for value in m.header_values(mail_parser::HeaderName::From) {
        if let mail_parser::HeaderValue::Address(a) = value {
            let (mbx, n) = mailboxes(Some(a));
            from.extend(mbx);
            from_count += n;
        } else {
            from_count += 1;
        }
    }
    out.from_count = from_count;
    out.from = from.into_iter().next();
    if out.from.is_none() || from_count > 1 {
        out.flags.insert(Flag::ParseDegraded);
    }
    out.sender = mailboxes(m.sender()).0.into_iter().next();
    out.to = mailboxes(m.to()).0;
    out.cc = mailboxes(m.cc()).0;
    out.reply_to = mailboxes(m.reply_to()).0;
    out.reply_to.truncate(10);
    out.subject = m.subject().map(|s| clean_display(s, 998));
    out.date = m.date().map(mail_parser::DateTime::to_timestamp);

    let mut message_id = None;
    let mut in_reply_to = None;
    let mut references = None;
    let h = &mut out.headers;
    for (name, value) in m.headers_raw() {
        let v = unfold(value);
        let first = |slot: &mut Option<String>| {
            if slot.is_none() {
                *slot = Some(v.clone());
            }
        };
        match name.to_ascii_lowercase().as_str() {
            "message-id" => first(&mut message_id),
            "in-reply-to" => first(&mut in_reply_to),
            "references" => first(&mut references),
            "auto-submitted" => first(&mut h.auto_submitted),
            "list-id" => first(&mut h.list_id),
            "list-unsubscribe" => first(&mut h.list_unsubscribe),
            "list-post" => first(&mut h.list_post),
            "precedence" => first(&mut h.precedence),
            "x-autoreply" => h.x_autoreply = true,
            "x-autorespond" => h.x_autorespond = true,
            "x-auto-response-suppress" => first(&mut h.x_auto_response_suppress),
            "x-pylota-identity-hop" => {
                if h.hop.is_none() {
                    h.hop = v.parse().ok();
                }
            }
            "authentication-results" => h.authentication_results.push(v),
            "disposition-notification-to" => first(&mut h.disposition_notification_to),
            _ => {}
        }
    }
    match message_id.as_deref().and_then(normalise_msg_id) {
        Some(id) => out.message_id = id,
        None => {
            out.flags.insert(Flag::MessageIdSynthetic);
        }
    }
    out.in_reply_to = in_reply_to.as_deref().and_then(|v| {
        split_references(v)
            .into_iter()
            .next()
            .or_else(|| normalise_msg_id(v))
    });
    out.references = references
        .as_deref()
        .map(split_references)
        .unwrap_or_default();

    let root = &m.parts[0];
    let (ct, report_type) = content_type(root);
    out.headers.content_type = ct;
    out.headers.report_type = report_type;
}

/// `type/subtype` (lower case; `text/plain` when absent) and the `report-type` parameter.
fn content_type(part: &MessagePart<'_>) -> (String, Option<String>) {
    match part.content_type() {
        Some(ct) => (
            format!(
                "{}/{}",
                ct.ctype().to_ascii_lowercase(),
                ct.subtype().unwrap_or("").to_ascii_lowercase()
            ),
            ct.attribute("report-type").map(str::to_ascii_lowercase),
        ),
        None => match part.body {
            PartType::Multipart(_) => ("multipart/mixed".to_owned(), None),
            PartType::Message(_) => ("message/rfc822".to_owned(), None),
            _ => ("text/plain".to_owned(), None),
        },
    }
}

fn param(part: &MessagePart<'_>, name: &str) -> Option<String> {
    part.content_type()
        .and_then(|ct| ct.attribute(name))
        .map(str::to_ascii_lowercase)
}

fn is_attachment_disposition(part: &MessagePart<'_>) -> bool {
    part.content_disposition()
        .is_some_and(|d| d.ctype().eq_ignore_ascii_case("attachment"))
}

/// What a subtree contributes to body selection.
#[derive(Default, Clone, Copy)]
struct Bodies {
    text: Option<usize>,
    html: Option<usize>,
}

struct Walk<'c, 'o> {
    caps: &'c Caps,
    parts_seen: usize,
    out: &'o mut ParsedMessage,
    /// Contents of opaque S/MIME `signed-data` parts, walked after the message that holds them.
    opaque: Vec<Vec<u8>>,
}

impl Walk<'_, '_> {
    fn degrade(&mut self) {
        self.out.flags.insert(Flag::ParseDegraded);
    }

    /// Walks the top-level message: selects the bodies, then turns every other leaf into an attachment.
    fn message(&mut self, m: &Message<'_>, depth: usize, nested_level: usize) {
        let mut leaves = Vec::new();
        let bodies = self.select(m, 0, depth, &mut leaves);
        let encrypted = self.out.has(Flag::Encrypted);
        for id in leaves {
            let part = &m.parts[id];
            let chosen = Some(id) == bodies.text || Some(id) == bodies.html;
            if chosen && !encrypted {
                self.body(m, part, Some(id) == bodies.html);
            } else {
                self.attachment(part, nested_level);
            }
        }
        for content in std::mem::take(&mut self.opaque) {
            if depth < self.caps.max_depth
                && let Some(inner) = MessageParser::default().parse(&content)
            {
                self.message(&inner, depth + 1, nested_level);
            }
        }
        if !self.out.has(Flag::Encrypted) {
            self.inline_pgp();
        }
    }

    /// Body candidates of the subtree at `id`; every leaf is pushed to `leaves` in order.
    fn select(
        &mut self,
        m: &Message<'_>,
        id: usize,
        depth: usize,
        leaves: &mut Vec<usize>,
    ) -> Bodies {
        let Some(part) = m.parts.get(id) else {
            return Bodies::default();
        };
        if depth > self.caps.max_depth || self.parts_seen >= self.caps.max_parts {
            self.degrade();
            return Bodies::default();
        }
        self.parts_seen += 1;
        // mail-parser also marks an empty body (headers with no separator line) as a problem.
        if part.is_encoding_problem && part.offset_end > part.offset_body {
            self.degrade();
        }
        let (ct, _) = content_type(part);
        match &part.body {
            PartType::Multipart(children) => {
                match ct.as_str() {
                    "multipart/encrypted" => {
                        self.out.flags.insert(Flag::Encrypted);
                    }
                    "multipart/signed" => {
                        let kind = match param(part, "protocol").as_deref() {
                            Some("application/pgp-signature") => SignatureType::Pgp,
                            _ => SignatureType::Smime,
                        };
                        self.signed(kind);
                    }
                    _ => {}
                }
                let alternative = ct == "multipart/alternative";
                let mut acc = Bodies::default();
                for &child in children {
                    let b = self.select(m, child as usize, depth + 1, leaves);
                    if alternative {
                        // RFC 2046: the last alternative of each type wins.
                        acc.text = b.text.or(acc.text);
                        acc.html = b.html.or(acc.html);
                    } else {
                        acc.text = acc.text.or(b.text);
                        acc.html = acc.html.or(b.html);
                    }
                }
                acc
            }
            _ => {
                leaves.push(id);
                let mut b = Bodies::default();
                if ct == "application/pkcs7-mime" || ct == "application/x-pkcs7-mime" {
                    match param(part, "smime-type").as_deref() {
                        Some("signed-data") => {
                            self.signed(SignatureType::Smime);
                            if let PartType::Binary(b) | PartType::InlineBinary(b) = &part.body
                                && let Some(content) = smime::signed_content(b)
                            {
                                self.opaque.push(content);
                            }
                        }
                        _ => {
                            self.out.flags.insert(Flag::Encrypted);
                        }
                    }
                }
                if !is_attachment_disposition(part) {
                    match (&part.body, ct.as_str()) {
                        (PartType::Html(_), _) => b.html = Some(id),
                        (PartType::Text(_), "text/plain") => b.text = Some(id),
                        _ => {}
                    }
                }
                b
            }
        }
    }

    fn signed(&mut self, kind: SignatureType) {
        if self.out.signature.is_none() {
            self.out.signature = Some(Signature {
                kind,
                status: "present_unverified",
            });
        }
    }

    /// Inline PGP in the text body: an armoured message is encrypted, a cleartext signature is recorded.
    fn inline_pgp(&mut self) {
        let Some(text) = &self.out.text else { return };
        if text.contains("-----BEGIN PGP MESSAGE-----") {
            self.out.flags.insert(Flag::Encrypted);
            self.out.text = None;
            self.out.html = None;
        } else if text.contains("-----BEGIN PGP SIGNED MESSAGE-----") {
            self.signed(SignatureType::Pgp);
        }
    }

    fn body(&mut self, m: &Message<'_>, part: &MessagePart<'_>, html: bool) {
        let text = match &part.body {
            PartType::Text(t) | PartType::Html(t) => t.as_ref(),
            _ => return,
        };
        if text.contains('\u{fffd}') {
            let raw = m
                .raw_message
                .get(part.offset_body as usize..part.offset_end as usize)
                .unwrap_or(&[]);
            if !raw.windows(3).any(|w| w == [0xef, 0xbf, 0xbd]) {
                self.degrade();
            }
        }
        let slot = if html {
            &mut self.out.html
        } else {
            &mut self.out.text
        };
        if slot.is_none() {
            *slot = Some(text.to_owned());
        }
    }

    fn attachment(&mut self, part: &MessagePart<'_>, nested_level: usize) {
        let (ct, _) = content_type(part);
        let data: Vec<u8> = match &part.body {
            PartType::Text(t) | PartType::Html(t) => t.as_bytes().to_vec(),
            PartType::Binary(b) | PartType::InlineBinary(b) => b.to_vec(),
            PartType::Message(nested) => nested.raw_message.to_vec(),
            PartType::Multipart(_) => return,
        };
        let filename = part.attachment_name().map(sanitise_filename);
        let content_id = part.content_id().map(|c| {
            c.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_owned()
        });
        let inline = part
            .content_disposition()
            .is_some_and(|d| d.ctype().eq_ignore_ascii_case("inline"));

        let is_tnef = ct == "application/ms-tnef"
            || filename
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case("winmail.dat"));
        if is_tnef && let Ok(files) = tnef::extract(&data) {
            for f in files {
                self.out.attachments.push(Attachment {
                    content_type: "application/octet-stream".to_owned(),
                    filename: Some(sanitise_filename(&f.name)),
                    content_id: None,
                    inline: false,
                    size: f.data.len(),
                    data: f.data,
                    origin: PartOrigin::Tnef,
                    nested: None,
                });
            }
            return;
        }

        let nested = match &part.body {
            PartType::Message(inner) => Some(Box::new(self.nested(inner, nested_level + 1))),
            _ => None,
        };
        let filename = filename.or_else(|| {
            nested
                .as_ref()
                .map(|n| match n.subject.as_deref().map(sanitise_filename) {
                    Some(s) if !s.is_empty() => format!("{s}.eml"),
                    _ => "forwarded.eml".to_owned(),
                })
        });
        self.out.attachments.push(Attachment {
            content_type: ct,
            filename,
            content_id,
            inline,
            size: data.len(),
            data,
            origin: PartOrigin::Mime,
            nested,
        });
    }

    fn nested(&mut self, inner: &Message<'_>, level: usize) -> Nested {
        let mut sub = ParsedMessage::empty(synthetic_id(&inner.raw_message));
        read_headers(inner, &mut sub);
        let with_text = level <= self.caps.nested_text_levels;
        if with_text {
            let mut walk = Walk {
                caps: self.caps,
                parts_seen: self.parts_seen,
                out: &mut sub,
                opaque: Vec::new(),
            };
            walk.message(inner, 0, level);
            self.parts_seen = walk.parts_seen;
            if sub.has(Flag::ParseDegraded) {
                self.degrade();
            }
        }
        let synthetic = sub.has(Flag::MessageIdSynthetic);
        Nested {
            subject: sub.subject,
            from: sub.from,
            message_id: (!synthetic).then_some(sub.message_id),
            text: if with_text { sub.text } else { None },
            html: if with_text { sub.html } else { None },
        }
    }
}

/// The last path component, control characters and `/\:` removed, at most 255 characters.
pub fn sanitise_filename(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && *c != ':')
        .take(255)
        .collect();
    cleaned.trim().trim_matches('.').to_owned()
}

/// `sent_at`: the `Date` header when it lies between `received_at − 10 years` and `received_at + 1 day`,
/// otherwise `received_at` (all in Unix milliseconds).
pub fn sent_at(date_seconds: Option<i64>, received_at_ms: i64) -> i64 {
    const DAY: i64 = 86_400_000;
    match date_seconds.and_then(|s| s.checked_mul(1000)) {
        Some(ms) if ms >= received_at_ms - 3653 * DAY && ms <= received_at_ms + DAY => ms,
        _ => received_at_ms,
    }
}

/// The header fields of a raw message as received, in order (Inbound › Read path, `include=headers`):
/// unfolded, at most `max_fields` fields and `max_bytes` of header block, each value cut to
/// `value_bytes` (on a character boundary).
pub fn header_fields(
    raw: &[u8],
    max_fields: usize,
    max_bytes: usize,
    value_bytes: usize,
) -> Vec<(String, String)> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    let block = String::from_utf8_lossy(&raw[..end.min(max_bytes)]);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in block.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = out.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if out.len() >= max_fields {
            break;
        }
        if let Some((n, v)) = line.split_once(':') {
            out.push((n.trim().to_owned(), v.trim().to_owned()));
        }
    }
    for (_, v) in &mut out {
        if v.len() > value_bytes {
            let mut cut = value_bytes;
            while !v.is_char_boundary(cut) {
                cut -= 1;
            }
            v.truncate(cut);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested_multiparts(depth: usize) -> Vec<u8> {
        let mut s = String::from("From: a@x.example\r\nSubject: deep\r\n");
        for i in 0..depth {
            s.push_str(&format!(
                "Content-Type: multipart/mixed; boundary=\"b{i}\"\r\n\r\n--b{i}\r\n"
            ));
        }
        s.push_str("Content-Type: text/plain\r\n\r\ndeep text\r\n");
        for i in (0..depth).rev() {
            s.push_str(&format!("--b{i}--\r\n"));
        }
        s.into_bytes()
    }

    /// Covers: B2
    #[test]
    fn b2_caps() {
        let shallow = parse(&nested_multiparts(10), &Caps::default());
        assert_eq!(shallow.text.as_deref().map(str::trim), Some("deep text"));
        assert!(!shallow.has(Flag::ParseDegraded));

        let deep = parse(&nested_multiparts(40), &Caps::default());
        assert!(deep.has(Flag::ParseDegraded));
        assert_eq!(deep.text, None);

        let mut many =
            String::from("From: a@x.example\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n");
        for i in 0..600 {
            many.push_str(&format!(
                "--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=f{i}.bin\r\n\r\nx\r\n"
            ));
        }
        many.push_str("--b--\r\n");
        let p = parse(many.as_bytes(), &Caps::default());
        assert!(p.has(Flag::ParseDegraded));
        // The root multipart counts as one of the 500 parts.
        assert_eq!(p.attachments.len(), 499);

        // Headers only (the third attempt).
        let h = parse_headers_only(&nested_multiparts(3));
        assert!(h.has(Flag::HeadersOnly) && h.has(Flag::ParseDegraded));
        assert_eq!(h.subject.as_deref(), Some("deep"));
        assert_eq!(h.text, None);

        // Nothing parseable at all.
        let empty = parse(b"", &Caps::default());
        assert!(empty.has(Flag::ParseDegraded) && empty.has(Flag::MessageIdSynthetic));
        assert!(empty.message_id.ends_with("@synthetic.invalid"));
    }

    /// Covers: B9
    #[test]
    fn b9_signed_and_encrypted() {
        use base64::Engine as _;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let opaque = smime::tests::sample(b"Content-Type: text/plain\r\n\r\nSigned body\r\n");
        let raw = format!(
            "From: a@x.example\r\nContent-Type: application/pkcs7-mime; smime-type=signed-data; name=smime.p7m\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n{}\r\n",
            b64(&opaque)
        );
        let p = parse(raw.as_bytes(), &Caps::default());
        assert_eq!(p.text.as_deref().map(str::trim), Some("Signed body"));
        assert_eq!(
            p.signature.as_ref().map(|s| s.kind),
            Some(SignatureType::Smime)
        );
        assert!(!p.has(Flag::Encrypted));
        assert_eq!(p.attachments.len(), 1);

        let enveloped = "From: a@x.example\r\nContent-Type: application/pkcs7-mime; smime-type=enveloped-data\r\n\
                         Content-Transfer-Encoding: base64\r\n\r\nMIAGCSqGSIb3DQEHA6CAMIACAQA=\r\n";
        let p = parse(enveloped.as_bytes(), &Caps::default());
        assert!(p.has(Flag::Encrypted));
        assert_eq!((p.text, p.html), (None, None));
        assert_eq!(p.attachments.len(), 1);

        let pgp = "From: a@x.example\r\n\r\n-----BEGIN PGP MESSAGE-----\r\nhQEMA\r\n-----END PGP MESSAGE-----\r\n";
        let p = parse(pgp.as_bytes(), &Caps::default());
        assert!(p.has(Flag::Encrypted));
        assert_eq!(p.text, None);
    }

    /// Covers: B13
    #[test]
    fn b13_first_from_header_wins() {
        let raw =
            b"From: ana@brightwell.example\r\nFrom: eve@example.net\r\nSubject: x\r\n\r\nbody";
        let p = parse(raw, &Caps::default());
        assert_eq!(
            p.from.as_ref().map(|f| f.address.as_str()),
            Some("ana@brightwell.example")
        );
        assert_eq!(p.from_count, 2);
        assert!(p.has(Flag::ParseDegraded));
        // Headers with no body are not degraded.
        let p = parse(
            b"From: ana@brightwell.example\r\nSubject: x\r\nMessage-ID: <a@b.example>\r\n",
            &Caps::default(),
        );
        assert!(p.flags.is_empty(), "{:?}", p.flags);
    }

    #[test]
    fn sent_at_window() {
        let now = 1_793_138_400_000;
        assert_eq!(sent_at(Some(1_793_138_000), now), 1_793_138_000_000);
        assert_eq!(sent_at(Some(1_793_138_400 + 2 * 86_400), now), now);
        assert_eq!(sent_at(Some(0), now), now);
        assert_eq!(sent_at(None, now), now);
        assert_eq!(sent_at(Some(i64::MAX), now), now);
    }

    #[test]
    fn filenames() {
        assert_eq!(sanitise_filename("../../etc/passwd"), "passwd");
        assert_eq!(
            sanitise_filename("C:\\Users\\x\\inv\u{0}oice.pdf"),
            "invoice.pdf"
        );
        assert_eq!(sanitise_filename("..."), "");
    }

    #[test]
    fn header_fields_unfold_and_cap() {
        let raw = b"Subject: a\r\n b\r\nX-Long: 0123456789\r\nTo: x@y\r\n\r\nBody: no\r\n";
        assert_eq!(
            header_fields(raw, 200, 65536, 4),
            vec![
                ("Subject".into(), "a b".into()),
                ("X-Long".into(), "0123".into()),
                ("To".into(), "x@y".into())
            ]
        );
        assert_eq!(header_fields(raw, 1, 65536, 2048).len(), 1);
    }
}
