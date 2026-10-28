//! The typed log line (Observability § 1, Design conventions § 8): one JSON object per line with an
//! `event` name from a fixed set and typed fields, never free text, never content or clear-text
//! addresses. M17 Foundation adds the catalogued labels; until then every line goes through here.

use pylota_identity_platform::console::{self, Stream};
use serde_json::{Map, Value};

/// A field value: IDs, codes, counts and booleans only.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Str(String),
    Int(i64),
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

fn line(level: &str, event: &'static str, fields: &[(&'static str, Field)]) -> String {
    let mut m = Map::new();
    m.insert("level".into(), level.into());
    m.insert("event".into(), event.into());
    for (k, v) in fields {
        m.insert(
            (*k).to_owned(),
            match v {
                Field::Str(s) => Value::from(s.as_str()),
                Field::Int(i) => Value::from(*i),
                Field::Bool(b) => Value::from(*b),
            },
        );
    }
    Value::Object(m).to_string()
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
