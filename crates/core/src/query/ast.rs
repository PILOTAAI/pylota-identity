//! The typed query tree ([Search § 3.2]) and its canonical serialisation (`query.parsed`).
//!
//! [Search § 3.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use chrono::{DateTime, NaiveDate, SecondsFormat};

use crate::refs::RefValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// `None` for an empty (or all-whitespace) query.
    pub root: Option<Expr>,
    /// Characters in `q`.
    pub source_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// At least two children, none of them `And`.
    And(Vec<Expr>),
    /// At least two children, none of them `Or`.
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Text(TextLeaf),
    Filter(Filter),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLeaf {
    pub kind: TextKind,
    pub field: TextField,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextKind {
    /// `prefix` for a trailing `*`; `text` excludes it.
    Term {
        text: String,
        prefix: bool,
    },
    Phrase(String),
}

impl TextKind {
    pub fn text(&self) -> &str {
        match self {
            Self::Term { text, .. } | Self::Phrase(text) => text,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextField {
    Any,
    /// `subject:`
    Subject,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    From(AddrMatch),
    To(AddrMatch),
    Participant(AddrMatch),
    Ref(RefMatch),
    /// `^[a-z0-9][a-z0-9_:-]{0,63}$`
    Label(String),
    HasAttachment,
    /// Lower case, NFKC.
    Filename(String),
    Type(TypeMatch),
    After(DateBound),
    Before(DateBound),
    NewerThan(RelDuration),
    OlderThan(RelDuration),
    Direction(Direction),
    /// `thr_` + ULID (upper case).
    Thread(String),
    IsUnread,
    IsNeedsReply,
    IsQuarantined,
    /// As spelled in the effective category list.
    Category(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddrMatch {
    /// Lower case, IDNA A-label domain.
    Exact(String),
    /// The domain and its subdomains.
    Domain(String),
    /// Lower-cased substring of the display name or address.
    Fuzzy(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefMatch {
    pub raw: String,
    /// Normalised forms, de-duplicated, at most 8 (`core::refs::candidates`).
    pub candidates: Vec<RefValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeMatch {
    Class(AttachmentClass),
    /// Lower case; compared for equality with the effective type.
    Mime(String),
}

/// Shared by `type:`, the `attachment_type` facet and the `why` list ([Search § 3.2] class table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttachmentClass {
    Pdf,
    Doc,
    Sheet,
    Slides,
    Image,
    Archive,
    Ics,
    Eml,
    Text,
    Html,
    Audio,
    Video,
    Other,
}

/// One MIME rule as SQL on `col`: the condition, with one `?` for the value, and the value. A prefix
/// rule (`…%`) compares the lowercased prefix instead of using `LIKE`: workerd's SQLite refuses `LIKE`
/// patterns over 50 bytes, and the OOXML rules are longer.
pub fn mime_rule_sql<'r>(col: &str, rule: &'r str) -> (String, &'r str) {
    match rule.strip_suffix('%') {
        Some(prefix) => (
            format!("lower(substr({col}, 1, {})) = ?", prefix.len()),
            prefix,
        ),
        None => (format!("{col} = ?"), rule),
    }
}

impl AttachmentClass {
    pub const ALL: [Self; 13] = [
        Self::Pdf,
        Self::Doc,
        Self::Sheet,
        Self::Slides,
        Self::Image,
        Self::Archive,
        Self::Ics,
        Self::Eml,
        Self::Text,
        Self::Html,
        Self::Audio,
        Self::Video,
        Self::Other,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Doc => "doc",
            Self::Sheet => "sheet",
            Self::Slides => "slides",
            Self::Image => "image",
            Self::Archive => "archive",
            Self::Ics => "ics",
            Self::Eml => "eml",
            Self::Text => "text",
            Self::Html => "html",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Other => "other",
        }
    }

    /// The names `type:` accepts (lower case).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "pdf" => Self::Pdf,
            "doc" | "docx" | "word" => Self::Doc,
            "sheet" | "xls" | "xlsx" | "csv" | "spreadsheet" => Self::Sheet,
            "slides" | "ppt" | "pptx" => Self::Slides,
            "image" | "jpg" | "jpeg" | "png" | "gif" | "heic" => Self::Image,
            "archive" | "zip" => Self::Archive,
            "ics" | "calendar" => Self::Ics,
            "eml" => Self::Eml,
            "text" | "txt" => Self::Text,
            "html" => Self::Html,
            "audio" => Self::Audio,
            "video" => Self::Video,
            "other" => Self::Other,
            _ => return None,
        })
    }

    /// MIME rules on the effective type; a trailing `%` makes the rule a prefix match ([`mime_rule_sql`]).
    /// `Other` is everything else.
    pub const fn mime_rules(self) -> &'static [&'static str] {
        match self {
            Self::Pdf => &["application/pdf"],
            Self::Doc => &[
                "application/msword",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.%",
                "application/vnd.oasis.opendocument.text",
            ],
            Self::Sheet => &[
                "application/vnd.ms-excel",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.%",
                "application/vnd.oasis.opendocument.spreadsheet",
                "text/csv",
            ],
            Self::Slides => &[
                "application/vnd.ms-powerpoint",
                "application/vnd.openxmlformats-officedocument.presentationml.%",
            ],
            Self::Image => &["image/%"],
            Self::Archive => &[
                "application/zip",
                "application/x-7z-compressed",
                "application/x-rar-compressed",
                "application/gzip",
                "application/x-tar",
            ],
            Self::Ics => &["text/calendar"],
            Self::Eml => &["message/rfc822"],
            Self::Text => &["text/plain"],
            Self::Html => &["text/html"],
            Self::Audio => &["audio/%"],
            Self::Video => &["video/%"],
            Self::Other => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateBound {
    /// A calendar day, resolved at local midnight in the tenant time zone.
    Day(CivilDate),
    /// Unix milliseconds.
    Instant(i64),
}

pub type CivilDate = NaiveDate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelDuration {
    pub n: u32,
    pub unit: DurUnit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurUnit {
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl DurUnit {
    pub const fn letter(self) -> char {
        match self {
            Self::Hour => 'h',
            Self::Day => 'd',
            Self::Week => 'w',
            Self::Month => 'm',
            Self::Year => 'y',
        }
    }

    /// The accepted range of `n` ([Search § 4]).
    pub const fn max(self) -> u32 {
        match self {
            Self::Hour => 87_600,
            Self::Day => 3_650,
            Self::Week => 520,
            Self::Month => 120,
            Self::Year => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}

/// Character offsets into `q`, end exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// The canonical serialisation of the tree (`query.parsed`): operator names in lower case, normalised
/// values, phrases in double quotes, `OR` explicit, parentheses only where needed. Parsing it again
/// gives the same tree (spans aside).
pub fn print(q: &Query) -> String {
    let mut out = String::new();
    if let Some(e) = &q.root {
        expr(e, &mut out);
    }
    out
}

fn expr(e: &Expr, out: &mut String) {
    match e {
        Expr::And(items) => {
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                expr(x, out); // OR binds tighter than AND: no parentheses
            }
        }
        Expr::Or(items) => {
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(" OR ");
                }
                grouped(x, matches!(x, Expr::And(_)), out);
            }
        }
        Expr::Not(x) => {
            out.push('-');
            grouped(
                x,
                matches!(**x, Expr::And(_) | Expr::Or(_) | Expr::Not(_)),
                out,
            );
        }
        Expr::Text(leaf) => text(leaf, out),
        Expr::Filter(f) => out.push_str(&print_filter(f)),
    }
}

fn grouped(e: &Expr, parens: bool, out: &mut String) {
    if parens {
        out.push('(');
        expr(e, out);
        out.push(')');
    } else {
        expr(e, out);
    }
}

fn text(leaf: &TextLeaf, out: &mut String) {
    if leaf.field == TextField::Subject {
        out.push_str("subject:");
    }
    match &leaf.kind {
        TextKind::Term { text, prefix } => {
            out.push_str(text);
            if *prefix {
                out.push('*');
            }
        }
        TextKind::Phrase(p) => out.push_str(&quote(p)),
    }
}

/// `"…"` with `"` and `\` escaped.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// A `value` in the grammar: non-empty, no whitespace, `"`, `(` or `)`.
pub(crate) fn is_bare(s: &str) -> bool {
    !s.is_empty() && s.chars().all(super::parser::is_value_char)
}

fn bare_or_quoted(s: &str) -> String {
    if is_bare(s) { s.to_owned() } else { quote(s) }
}

/// One filter in canonical form, e.g. `from:@brightwell.example` or `type:pdf` (also the `why` list).
pub fn print_filter(f: &Filter) -> String {
    let addr = |op: &str, a: &AddrMatch| {
        let v = match a {
            AddrMatch::Exact(x) => x.clone(),
            AddrMatch::Domain(d) if super::parser::classify_addr(d) == *a => d.clone(),
            AddrMatch::Domain(d) => format!("@{d}"),
            AddrMatch::Fuzzy(s) if is_bare(s) && super::parser::classify_addr(s) == *a => s.clone(),
            AddrMatch::Fuzzy(s) => quote(s),
        };
        format!("{op}:{v}")
    };
    match f {
        Filter::From(a) => addr("from", a),
        Filter::To(a) => addr("to", a),
        Filter::Participant(a) => addr("participant", a),
        Filter::Ref(r) => format!("ref:{}", bare_or_quoted(&r.raw)),
        Filter::Label(l) => format!("label:{l}"),
        Filter::HasAttachment => "has:attachment".to_owned(),
        Filter::Filename(s) => format!("filename:{}", bare_or_quoted(s)),
        Filter::Type(TypeMatch::Class(c)) => format!("type:{}", c.as_str()),
        Filter::Type(TypeMatch::Mime(m)) => format!("type:{}", bare_or_quoted(m)),
        Filter::After(b) => format!("after:{}", bound(b)),
        Filter::Before(b) => format!("before:{}", bound(b)),
        Filter::NewerThan(d) => format!("newer_than:{}{}", d.n, d.unit.letter()),
        Filter::OlderThan(d) => format!("older_than:{}{}", d.n, d.unit.letter()),
        Filter::Direction(d) => format!("in:{}", d.as_str()),
        Filter::Thread(t) => format!("thread:{t}"),
        Filter::IsUnread => "is:unread".to_owned(),
        Filter::IsNeedsReply => "is:needs_reply".to_owned(),
        Filter::IsQuarantined => "is:quarantined".to_owned(),
        Filter::Category(c) => format!("category:{}", bare_or_quoted(c)),
    }
}

fn bound(b: &DateBound) -> String {
    match b {
        DateBound::Day(d) => d.format("%Y-%m-%d").to_string(),
        DateBound::Instant(ms) => DateTime::from_timestamp_millis(*ms).map_or_else(
            || ms.to_string(),
            |t| t.to_rfc3339_opts(SecondsFormat::AutoSi, true),
        ),
    }
}
