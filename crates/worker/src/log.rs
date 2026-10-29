//! The typed log line (Observability § 1, Design conventions § 8): one JSON object per line with an
//! `event` name from a fixed set and typed fields, never free text, never content or clear-text
//! addresses. Every string value is scrubbed on the way out (I5): a value that looks like an address
//! or carries content, and any value of a content field, is written as `[redacted]`; control
//! characters are dropped and values are cut at 256 characters.

use pylota_identity_platform::console::{self, Stream};
use serde_json::{Map, Value};

/// A field value: IDs, codes, counts and booleans only.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Str(String),
    Int(i64),
    Num(f64),
    Bool(bool),
}

impl From<&str> for Field {
    fn from(v: &str) -> Self {
        Field::Str(v.to_owned())
    }
}
impl From<String> for Field {
    fn from(v: String) -> Self {
        Field::Str(v)
    }
}
impl From<i64> for Field {
    fn from(v: i64) -> Self {
        Field::Int(v)
    }
}
impl From<bool> for Field {
    fn from(v: bool) -> Self {
        Field::Bool(v)
    }
}

/// Fields whose values are message content or addresses whatever they hold.
const CONTENT_FIELDS: &[&str] = &[
    "subject",
    "body",
    "text",
    "html",
    "from",
    "to",
    "cc",
    "bcc",
    "reply_to",
    "address",
    "email",
    "q",
    "query",
    "snippet",
    "filename",
    "display_name",
    "headers",
    "smtp_response",
];

/// The redaction marker.
pub const REDACTED: &str = "[redacted]";

/// A string value as it may be logged.
pub fn scrub(field: &str, v: &str) -> String {
    if CONTENT_FIELDS.contains(&field) {
        return REDACTED.to_owned();
    }
    let looks_like_address = v
        .split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == ',')
        .any(|w| {
            w.split_once('@')
                .is_some_and(|(l, d)| !l.is_empty() && d.contains('.'))
        });
    // Prose (several words) is content; codes, IDs and patterns are single tokens.
    let prose = v.split_whitespace().count() > 3;
    if looks_like_address || prose {
        return REDACTED.to_owned();
    }
    v.chars().filter(|c| !c.is_control()).take(256).collect()
}

fn line(level: &str, event: &'static str, fields: &[(&'static str, Field)]) -> String {
    let mut m = Map::new();
    m.insert("level".into(), level.into());
    m.insert("event".into(), event.into());
    for (k, v) in fields {
        m.insert(
            (*k).to_owned(),
            match v {
                Field::Str(s) => Value::from(scrub(k, s)),
                Field::Int(i) => Value::from(*i),
                Field::Num(n) => {
                    serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number)
                }
                Field::Bool(b) => Value::from(*b),
            },
        );
    }
    Value::Object(m).to_string()
}

pub fn debug(event: &'static str, fields: &[(&'static str, Field)]) {
    console::write(Stream::Out, &line("debug", event, fields));
}

pub fn info(event: &'static str, fields: &[(&'static str, Field)]) {
    console::write(Stream::Out, &line("info", event, fields));
}

pub fn warn(event: &'static str, fields: &[(&'static str, Field)]) {
    console::write(Stream::Out, &line("warn", event, fields));
}

pub fn error(event: &'static str, fields: &[(&'static str, Field)]) {
    console::write(Stream::Err, &line("error", event, fields));
}

/// A security event (Security § 12): logged at error level with `security: true`.
pub fn security(event: &'static str, fields: &[(&'static str, Field)]) {
    let mut all = fields.to_vec();
    all.push(("security", Field::Bool(true)));
    console::write(Stream::Err, &line("error", event, &all));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Log scrubbing (I5): addresses, prose and content fields never reach a line; codes, IDs and
    /// route patterns do.
    #[test]
    fn lines_are_scrubbed() {
        let l = line(
            "info",
            "x",
            &[
                ("code", "invalid_request".into()),
                ("route", "/v1/identities/{identity_id}/messages".into()),
                ("detail", "bob@example.org".into()),
                ("reason", "Re: your invoice is attached here".into()),
                ("subject", "hello".into()),
                ("message_id", "msg_01J9Z5".into()),
                ("tenant_id", "ten_1\n\u{7}".into()),
                ("value", Field::Num(1.5)),
            ],
        );
        let v: Value = serde_json::from_str(&l).unwrap();
        assert_eq!(v["code"], "invalid_request");
        assert_eq!(v["route"], "/v1/identities/{identity_id}/messages");
        assert_eq!(v["detail"], REDACTED);
        assert_eq!(v["reason"], REDACTED);
        assert_eq!(v["subject"], REDACTED);
        assert_eq!(v["message_id"], "msg_01J9Z5");
        assert_eq!(v["tenant_id"], "ten_1");
        assert_eq!(v["value"], 1.5);
        assert!(!l.contains('@'));
        assert_eq!(scrub("x", &"a".repeat(300)).len(), 256);
    }
}
