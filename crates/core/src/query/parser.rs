//! The hand-written recursive-descent parser ([Search § 3.1]). It works on the characters of `q`, keeps
//! one `Vec<char>` of them, recurses at most 8 groups deep, and returns a `QueryError` for every input
//! it does not accept; it never panics.
//!
//! [Search § 3.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use chrono::{DateTime, Datelike as _, NaiveDate};
use unicode_normalization::UnicodeNormalization as _;

use super::ast::{
    AddrMatch, AttachmentClass, DateBound, Direction, DurUnit, Expr, Filter, Query, RefMatch,
    RelDuration, Span, TextField, TextKind, TextLeaf, TypeMatch,
};
use super::{ParseCtx, QueryError, expected};
use crate::address::normalise_domain;
use crate::ids::{IdPrefix, is_id};

pub const MAX_CHARS: usize = 1024;
pub const MAX_LEAVES: u32 = 32;
pub const MAX_DEPTH: u32 = 8;

pub(crate) fn is_value_char(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '"' | '(' | ')'))
}

pub(super) fn parse(input: &str, ctx: &ParseCtx<'_>) -> Result<Query, QueryError> {
    let s: Vec<char> = input.chars().take(MAX_CHARS + 1).collect();
    let mut p = Parser {
        s,
        pos: 0,
        leaves: 0,
        depth: 0,
        ctx,
    };
    if p.s.len() > MAX_CHARS {
        return Err(p.err(MAX_CHARS, expected::LIMIT));
    }
    p.skip_ws();
    let root = if p.eof() {
        None
    } else {
        let e = p.and_expr()?;
        p.skip_ws();
        if !p.eof() {
            // `and_expr` stops only at the end, at `)`, or before an error it reports itself.
            return Err(p.err(p.pos, expected::TERM));
        }
        Some(e)
    };
    Ok(Query {
        root,
        source_len: p.s.len() as u32,
    })
}

struct Parser<'a> {
    s: Vec<char>,
    pos: usize,
    leaves: u32,
    depth: u32,
    ctx: &'a ParseCtx<'a>,
}

type R<T> = Result<T, QueryError>;

impl Parser<'_> {
    fn eof(&self) -> bool {
        self.pos >= self.s.len()
    }

    fn peek(&self) -> Option<char> {
        self.s.get(self.pos).copied()
    }

    fn skip_ws(&mut self) -> usize {
        let start = self.pos;
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn err(&self, pos: usize, expected: &str) -> QueryError {
        let found: String = self
            .s
            .iter()
            .skip(pos)
            .take_while(|c| !c.is_whitespace())
            .take(32)
            .collect();
        let found = if pos >= self.s.len() {
            None
        } else if found.is_empty() {
            self.s.get(pos).map(char::to_string)
        } else {
            Some(found)
        };
        QueryError {
            position: pos as u32,
            expected: expected.to_owned(),
            found,
        }
    }

    /// `OR` standing alone: upper case, followed by whitespace, the end, or a bracket or quote.
    fn at_or(&self) -> bool {
        self.s.get(self.pos) == Some(&'O')
            && self.s.get(self.pos + 1) == Some(&'R')
            && self
                .s
                .get(self.pos + 2)
                .is_none_or(|&c| c.is_whitespace() || matches!(c, '(' | ')' | '"'))
    }

    fn leaf(&mut self, start: usize) -> R<()> {
        self.leaves += 1;
        if self.leaves > MAX_LEAVES {
            return Err(self.err(start, expected::LIMIT));
        }
        Ok(())
    }

    /// `and_expr = or_expr , { ws1 , or_expr }`, flattened.
    fn and_expr(&mut self) -> R<Expr> {
        let mut items = Vec::new();
        push_flat(&mut items, self.or_expr()?, true);
        loop {
            let save = self.pos;
            let ws = self.skip_ws();
            if self.eof() || self.peek() == Some(')') {
                self.pos = save;
                break;
            }
            if ws == 0 {
                return Err(self.err(self.pos, expected::WHITESPACE));
            }
            push_flat(&mut items, self.or_expr()?, true);
        }
        Ok(collapse(items, Expr::And))
    }

    /// `or_expr = unary , { ws1 , "OR" , ws1 , unary }`, flattened.
    fn or_expr(&mut self) -> R<Expr> {
        let mut items = Vec::new();
        push_flat(&mut items, self.unary()?, false);
        loop {
            let save = self.pos;
            if self.skip_ws() == 0 || !self.at_or() {
                self.pos = save;
                break;
            }
            let or_at = self.pos;
            self.pos += 2;
            let ws = self.skip_ws();
            if self.eof() || self.peek() == Some(')') {
                return Err(self.err(or_at, expected::TERM));
            }
            if ws == 0 {
                return Err(self.err(self.pos, expected::WHITESPACE));
            }
            push_flat(&mut items, self.unary()?, false);
        }
        Ok(collapse(items, Expr::Or))
    }

    /// `unary = [ "-" ] , primary`; the `-` touches the primary.
    fn unary(&mut self) -> R<Expr> {
        if self.peek() != Some('-') {
            return self.primary();
        }
        let at = self.pos;
        self.pos += 1;
        match self.peek() {
            None | Some(')' | '-') => Err(self.err(at, expected::TERM)),
            Some(c) if c.is_whitespace() => Err(self.err(at, expected::TERM)),
            _ => Ok(Expr::Not(Box::new(self.primary()?))),
        }
    }

    fn primary(&mut self) -> R<Expr> {
        let start = self.pos;
        match self.peek() {
            None | Some(')') => Err(self.err(start, expected::TERM)),
            Some('(') => self.group(),
            Some('"') => {
                self.leaf(start)?;
                let p = self.phrase()?;
                if p.is_empty() {
                    return Err(self.err(start, expected::TERM));
                }
                Ok(self.text(TextKind::Phrase(p), TextField::Any, start))
            }
            Some(_) if self.at_or() => Err(self.err(start, expected::TERM)),
            Some(_) => {
                let mut j = start;
                while self
                    .s
                    .get(j)
                    .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
                {
                    j += 1;
                }
                if j > start && self.s.get(j) == Some(&':') {
                    let name: String = self.s[start..j].iter().collect();
                    return match Op::from_name(&name.to_ascii_lowercase()) {
                        Some(op) => {
                            self.leaf(start)?;
                            self.pos = j + 1;
                            self.operator(op, start)
                        }
                        None => Err(self.err(start, expected::UNKNOWN_OPERATOR)),
                    };
                }
                self.leaf(start)?;
                let run = self.value_run();
                let kind = term(&run).ok_or_else(|| self.err(start, expected::PREFIX))?;
                Ok(self.text(kind, TextField::Any, start))
            }
        }
    }

    fn group(&mut self) -> R<Expr> {
        let open = self.pos;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err(open, expected::LIMIT));
        }
        self.pos += 1;
        self.skip_ws();
        if self.eof() {
            return Err(self.err(open, expected::CLOSING_PAREN));
        }
        let inner = self.and_expr()?;
        self.skip_ws();
        if self.peek() != Some(')') {
            return Err(self.err(open, expected::CLOSING_PAREN));
        }
        self.pos += 1;
        self.depth -= 1;
        Ok(inner)
    }

    /// After the opening `"`: the unescaped text up to the closing `"`. `\"` and `\\` are escapes; any
    /// other backslash is literal.
    fn phrase(&mut self) -> R<String> {
        let open = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err(open, expected::CLOSING_QUOTE)),
                Some('"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some('\\') if matches!(self.s.get(self.pos + 1), Some('"' | '\\')) => {
                    out.push(self.s[self.pos + 1]);
                    self.pos += 2;
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn value_run(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(is_value_char) {
            self.pos += 1;
        }
        self.s[start..self.pos].iter().collect()
    }

    fn text(&self, kind: TextKind, field: TextField, start: usize) -> Expr {
        Expr::Text(TextLeaf {
            kind,
            field,
            span: Span {
                start: start as u32,
                end: self.pos as u32,
            },
        })
    }

    /// After `name:`. `start` is the offset of the operator name.
    fn operator(&mut self, op: Op, start: usize) -> R<Expr> {
        let at = self.pos;
        let (value, quoted) = if self.peek() == Some('"') {
            (self.phrase()?, true)
        } else {
            (self.value_run(), false)
        };
        if value.is_empty() {
            return Err(self.err(at, expected::OPERATOR_VALUE));
        }
        let bad = |e: &str| self.err(at, e);
        let lower = value.to_lowercase();
        let f = match op {
            Op::From | Op::To | Op::Participant => {
                let a = if quoted {
                    AddrMatch::Fuzzy(lower)
                } else {
                    classify_addr(&value)
                };
                match op {
                    Op::From => Filter::From(a),
                    Op::To => Filter::To(a),
                    _ => Filter::Participant(a),
                }
            }
            Op::Subject => {
                let kind = if quoted {
                    TextKind::Phrase(value)
                } else {
                    term(&value).ok_or_else(|| bad(expected::PREFIX))?
                };
                return Ok(self.text(kind, TextField::Subject, start));
            }
            Op::Ref => Filter::Ref(RefMatch {
                candidates: crate::refs::candidates(
                    &value,
                    self.ctx.ref_packs,
                    self.ctx.custom_refs,
                    self.ctx.tz.name(),
                ),
                raw: value,
            }),
            Op::Label => {
                let ok = lower.chars().count() <= 64
                    && lower.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
                    && lower.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | ':' | '-')
                    });
                if !ok {
                    return Err(bad(expected::LABEL));
                }
                Filter::Label(lower)
            }
            Op::Has if lower == "attachment" => Filter::HasAttachment,
            Op::Has => return Err(bad(expected::HAS)),
            Op::Filename => Filter::Filename(fold_nfkc(&value)),
            Op::Type if lower.contains('/') => Filter::Type(TypeMatch::Mime(lower)),
            Op::Type => Filter::Type(TypeMatch::Class(
                AttachmentClass::from_name(&lower).ok_or_else(|| bad(expected::TYPE))?,
            )),
            Op::After | Op::Before => {
                let b = date_bound(&value).ok_or_else(|| bad(expected::DATE))?;
                if op == Op::After {
                    Filter::After(b)
                } else {
                    Filter::Before(b)
                }
            }
            Op::NewerThan | Op::OlderThan => {
                let d = duration(&lower).ok_or_else(|| bad(expected::DURATION))?;
                if op == Op::NewerThan {
                    Filter::NewerThan(d)
                } else {
                    Filter::OlderThan(d)
                }
            }
            Op::In => match lower.as_str() {
                "inbound" => Filter::Direction(Direction::Inbound),
                "outbound" => Filter::Direction(Direction::Outbound),
                _ => return Err(bad(expected::DIRECTION)),
            },
            Op::Thread => {
                let id = value
                    .split_once('_')
                    .filter(|(p, _)| p.eq_ignore_ascii_case("thr"))
                    .map(|(_, u)| format!("thr_{}", u.to_ascii_uppercase()))
                    .filter(|id| is_id(id, IdPrefix::Thread))
                    .ok_or_else(|| bad(expected::THREAD))?;
                Filter::Thread(id)
            }
            Op::Is => match lower.as_str() {
                "unread" => Filter::IsUnread,
                "needs_reply" => Filter::IsNeedsReply,
                "quarantined" => Filter::IsQuarantined,
                _ => return Err(bad(expected::IS)),
            },
            Op::Category => {
                let c = self
                    .ctx
                    .categories
                    .iter()
                    .find(|c| c.to_lowercase() == lower)
                    .ok_or_else(|| {
                        bad(&format!(
                            "category: one of {}",
                            self.ctx.categories.join(", ")
                        ))
                    })?;
                Filter::Category((*c).to_owned())
            }
        };
        Ok(Expr::Filter(f))
    }
}

/// Adds `e` to an `And` (or `Or`) list, splicing in a child of the same kind (from a group).
fn push_flat(items: &mut Vec<Expr>, e: Expr, and: bool) {
    match e {
        Expr::And(xs) if and => items.extend(xs),
        Expr::Or(xs) if !and => items.extend(xs),
        e => items.push(e),
    }
}

fn collapse(mut items: Vec<Expr>, f: fn(Vec<Expr>) -> Expr) -> Expr {
    if items.len() == 1 {
        items.remove(0)
    } else {
        f(items)
    }
}

/// A term from a value run: a trailing `*` makes a prefix term, which needs at least 2 characters
/// before it. `None` for `*` alone or a 1-character prefix.
fn term(run: &str) -> Option<TextKind> {
    match run.strip_suffix('*') {
        Some(text) if text.chars().count() < 2 => None,
        Some(text) => Some(TextKind::Term {
            text: text.to_owned(),
            prefix: true,
        }),
        None => Some(TextKind::Term {
            text: run.to_owned(),
            prefix: false,
        }),
    }
}

/// A bare `from:`/`to:`/`participant:` value: `user@domain` (exact), `@domain` or `domain.tld`
/// (domain and subdomains), anything else a case-folded substring.
pub(crate) fn classify_addr(v: &str) -> AddrMatch {
    if let Some(d) = v.strip_prefix('@') {
        if let Some(d) = normalise_domain(d) {
            return AddrMatch::Domain(d);
        }
    } else if let Some((local, d)) = v.rsplit_once('@') {
        if let (false, Some(d)) = (local.is_empty(), normalise_domain(d)) {
            return AddrMatch::Exact(format!("{}@{d}", local.to_lowercase()));
        }
    } else if let Some(d) = normalise_domain(v).filter(|d| {
        d.rsplit('.')
            .next()
            .is_some_and(|t| t.bytes().any(|b| b.is_ascii_alphabetic()))
    }) {
        return AddrMatch::Domain(d);
    }
    AddrMatch::Fuzzy(v.to_lowercase())
}

/// Lower case and NFKC, repeated until stable (at most 4 rounds).
pub fn fold_nfkc(s: &str) -> String {
    let mut cur = s.to_owned();
    for _ in 0..4 {
        let next: String = cur.nfkc().collect::<String>().to_lowercase();
        if next == cur {
            break;
        }
        cur = next;
    }
    cur
}

/// `YYYY-MM-DD`, `YYYY/MM/DD` or an RFC 3339 instant (years 0000–9999 in UTC).
fn date_bound(v: &str) -> Option<DateBound> {
    let b = v.as_bytes();
    if b.len() == 10 && b[4] == b[7] && matches!(b[4], b'-' | b'/') {
        let num = |r: std::ops::Range<usize>| -> Option<u32> {
            let s = v.get(r)?;
            s.bytes()
                .all(|c| c.is_ascii_digit())
                .then(|| s.parse().ok())?
        };
        let d = NaiveDate::from_ymd_opt(num(0..4)? as i32, num(5..7)?, num(8..10)?)?;
        return Some(DateBound::Day(d));
    }
    let t = DateTime::parse_from_rfc3339(v).ok()?.to_utc();
    (0..=9999)
        .contains(&t.year())
        .then(|| DateBound::Instant(t.timestamp_millis()))
}

/// `<n><unit>` within the bounds of [Search § 4].
fn duration(v: &str) -> Option<RelDuration> {
    let unit = match v.chars().last()? {
        'h' => DurUnit::Hour,
        'd' => DurUnit::Day,
        'w' => DurUnit::Week,
        'm' => DurUnit::Month,
        'y' => DurUnit::Year,
        _ => return None,
    };
    let digits = &v[..v.len() - 1];
    if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    (1..=unit.max())
        .contains(&n)
        .then_some(RelDuration { n, unit })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    From,
    To,
    Participant,
    Subject,
    Ref,
    Label,
    Has,
    Filename,
    Type,
    After,
    Before,
    NewerThan,
    OlderThan,
    In,
    Thread,
    Is,
    Category,
}

impl Op {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "from" => Self::From,
            "to" => Self::To,
            "participant" => Self::Participant,
            "subject" => Self::Subject,
            "ref" => Self::Ref,
            "label" => Self::Label,
            "has" => Self::Has,
            "filename" => Self::Filename,
            "type" => Self::Type,
            "after" => Self::After,
            "before" => Self::Before,
            "newer_than" => Self::NewerThan,
            "older_than" => Self::OlderThan,
            "in" => Self::In,
            "thread" => Self::Thread,
            "is" => Self::Is,
            "category" => Self::Category,
            _ => return None,
        })
    }
}
