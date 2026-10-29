//! Compilation ([Search § 3.4]): the tree becomes one FTS5 `MATCH` expression, a trigram fallback, a SQL
//! filter over alias `m`, and the inputs of the semantic leg. SQL text comes only from the fixed
//! fragments below and every value is a bound parameter; a `MATCH` string holds only quoted strings,
//! `AND`/`OR`/`NOT`, parentheses, `{subject} :` and `*` after a quote, so raw input never reaches FTS5
//! syntax (FR-SRCH-3, F1).
//!
//! [Search § 3.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use std::collections::HashSet;

use super::ast::{
    AddrMatch, AttachmentClass, DateBound, Direction, Expr, Filter, Query, TextField, TextKind,
    TextLeaf, TypeMatch, mime_rule_sql, print_filter,
};
use super::dates::{ago, bound_ms, resolve};
use super::{CompileCtx, QueryError, expected};
use crate::address::org_domain;

/// The message date ([Search § 4]): `sent_at` clamped to five minutes after receipt.
pub const MSG_DATE: &str = "MIN(COALESCE(m.sent_at, m.received_at), m.received_at + 300000)";
/// `semantic_text` is cut to this many characters.
pub const MAX_SEMANTIC_CHARS: usize = 2_000;
/// `tri_match` holds at most this many quoted strings.
pub const MAX_TRIGRAM_STRINGS: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledQuery {
    /// One FTS5 expression over `fts`, or `None`.
    pub fts_match: Option<String>,
    /// The trigram fallback over `fts_tri`.
    pub tri_match: Option<String>,
    /// Boolean SQL over alias `m`; may contain FTS sub-selects.
    pub filter: SqlFragment,
    /// Normalised values of free-text terms that look like references.
    pub ref_like: Vec<String>,
    /// For snippets and `why`.
    pub positive_terms: Vec<TermPattern>,
    /// Free text for embedding and reranking (`""` if none).
    pub semantic_text: String,
    pub vector_prefilter: VectorFilter,
    /// Some filter can only be checked on read-back.
    pub needs_post_filter: bool,
    /// Canonical form of each top-level positive filter, e.g. `type:pdf`.
    pub why_filters: Vec<String>,
    /// `is:quarantined` with `quarantine:review`: the request reads quarantined mail.
    pub forces_include_quarantined: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SqlFragment {
    pub sql: String,
    pub params: Vec<SqlParam>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlParam {
    Text(String),
    Int(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermPattern {
    pub text: String,
    pub prefix: bool,
    pub phrase: bool,
    pub field: TextField,
}

/// What Vectorize can filter before `topK` ([Search § 8]); `sent_at` is in Unix seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VectorFilter {
    pub direction: Option<Direction>,
    pub sent_at_gte: Option<i64>,
    pub sent_at_lt: Option<i64>,
    pub has_attachment: bool,
    pub thread_id: Option<String>,
    /// Only when the `from:@d` domain is its own organisational domain.
    pub sender_domain: Option<String>,
}

/// `mode: "semantic"` needs free text (`invalid_query` at position 0 otherwise).
pub fn require_semantic_text(c: &CompiledQuery) -> Result<(), QueryError> {
    if c.semantic_text.is_empty() {
        return Err(QueryError {
            position: 0,
            expected: expected::SEMANTIC.to_owned(),
            found: None,
        });
    }
    Ok(())
}

pub fn compile(q: &Query, ctx: &CompileCtx<'_>) -> CompiledQuery {
    let resolved = resolve(q, ctx.tz, ctx.now_ms).query;
    let conjuncts = flatten(resolved.root.as_ref());
    let originals = flatten(q.root.as_ref());

    let mut sql = Sql::default();
    let mut pos_fts: Vec<String> = Vec::new();
    let mut neg_fts: Vec<String> = Vec::new();
    let mut filters: Vec<String> = Vec::new();
    for c in &conjuncts {
        let (text, filter) = kinds(c);
        match c {
            Expr::Not(inner) if text && !filter && fts_ok(inner) => neg_fts.push(fts(inner)),
            _ if text && !filter && fts_ok(c) => pos_fts.push(fts(c)),
            _ => filters.push(sql.expr(c, ctx)),
        }
    }
    let fts_match = if pos_fts.is_empty() {
        for n in neg_fts {
            filters.push(sql.not_match(n));
        }
        None
    } else {
        let mut m = pos_fts.join(" AND ");
        for n in neg_fts {
            m = format!("({m}) NOT {n}");
        }
        Some(m)
    };
    let filter = SqlFragment {
        sql: if filters.is_empty() {
            "1".to_owned()
        } else {
            filters.join(" AND ")
        },
        params: sql.params,
    };

    let mut positive = Vec::new();
    if let Some(root) = &q.root {
        positive_leaves(root, &mut positive);
    }
    positive.sort_by_key(|l| l.span.start);

    let (vector_prefilter, needs_post_filter) = vector_filter(&conjuncts);
    CompiledQuery {
        fts_match,
        tri_match: tri_match(&positive),
        filter,
        ref_like: ref_like(&conjuncts, ctx),
        positive_terms: positive
            .iter()
            .map(|l| TermPattern {
                text: l.kind.text().to_owned(),
                prefix: matches!(l.kind, TextKind::Term { prefix: true, .. }),
                phrase: matches!(l.kind, TextKind::Phrase(_)),
                field: l.field,
            })
            .collect(),
        semantic_text: semantic_text(&positive),
        vector_prefilter,
        needs_post_filter,
        why_filters: originals
            .iter()
            .filter_map(|c| match c {
                Expr::Filter(f) => Some(print_filter(f)),
                _ => None,
            })
            .collect(),
        forces_include_quarantined: ctx.quarantine_review
            && has_positive_quarantined(q.root.as_ref()),
    }
}

/// Top-level conjuncts: the children of a root `And`, or the root itself.
fn flatten(root: Option<&Expr>) -> Vec<&Expr> {
    match root {
        None => Vec::new(),
        Some(Expr::And(xs)) => xs.iter().collect(),
        Some(e) => vec![e],
    }
}

/// (has a text leaf, has a filter leaf)
fn kinds(e: &Expr) -> (bool, bool) {
    match e {
        Expr::Text(_) => (true, false),
        Expr::Filter(_) => (false, true),
        Expr::Not(x) => kinds(x),
        Expr::And(xs) | Expr::Or(xs) => xs.iter().fold((false, false), |(t, f), x| {
            let (a, b) = kinds(x);
            (t || a, f || b)
        }),
    }
}

/// Whether a pure-text subtree has an FTS5 form: every `Not` is the right side of an `AND` with at
/// least one positive sibling.
fn fts_ok(e: &Expr) -> bool {
    match e {
        Expr::Text(_) => true,
        Expr::Filter(_) | Expr::Not(_) => false,
        Expr::Or(xs) => xs.iter().all(fts_ok),
        Expr::And(xs) => {
            let mut positive = false;
            for x in xs {
                match x {
                    Expr::Not(inner) if fts_ok(inner) => {}
                    x if fts_ok(x) => positive = true,
                    _ => return false,
                }
            }
            positive
        }
    }
}

/// The FTS5 form of an `fts_ok` subtree, with explicit parentheses (FTS5 gives `NOT` higher precedence
/// than `AND`).
fn fts(e: &Expr) -> String {
    match e {
        Expr::Text(leaf) => fts_leaf(leaf),
        Expr::Or(xs) => format!("({})", xs.iter().map(fts).collect::<Vec<_>>().join(" OR ")),
        Expr::And(xs) => {
            let pos: Vec<String> = xs
                .iter()
                .filter(|x| !matches!(x, Expr::Not(_)))
                .map(fts)
                .collect();
            let mut out = format!("({})", pos.join(" AND "));
            for x in xs {
                if let Expr::Not(inner) = x {
                    out = format!("({out} NOT {})", fts(inner));
                }
            }
            out
        }
        // Not reached for `fts_ok` input; a quoted empty string matches nothing.
        Expr::Not(_) | Expr::Filter(_) => "\"\"".to_owned(),
    }
}

fn fts_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn fts_leaf(leaf: &TextLeaf) -> String {
    let mut s = fts_quote(leaf.kind.text());
    if let TextKind::Term { prefix: true, .. } = leaf.kind {
        s.push_str(" *");
    }
    match leaf.field {
        TextField::Any => s,
        TextField::Subject => format!("{{subject}} : ({s})"),
    }
}

#[derive(Default)]
struct Sql {
    params: Vec<SqlParam>,
}

impl Sql {
    fn text(&mut self, v: &str) {
        self.params.push(SqlParam::Text(v.to_owned()));
    }

    fn int(&mut self, v: i64) {
        self.params.push(SqlParam::Int(v));
    }

    fn not_match(&mut self, m: String) -> String {
        self.params.push(SqlParam::Text(m));
        "m.rowid NOT IN (SELECT rowid FROM fts WHERE fts MATCH ?)".to_owned()
    }

    /// Any subtree as SQL: a text leaf is an FTS sub-select, `Not` never lets `NULL` through.
    fn expr(&mut self, e: &Expr, ctx: &CompileCtx<'_>) -> String {
        match e {
            Expr::Text(leaf) => {
                self.params.push(SqlParam::Text(fts_leaf(leaf)));
                "m.rowid IN (SELECT rowid FROM fts WHERE fts MATCH ?)".to_owned()
            }
            Expr::Filter(f) => format!("({})", self.filter(f, ctx)),
            Expr::Not(x) => format!("NOT COALESCE({}, 0)", self.paren(x, ctx)),
            Expr::And(xs) | Expr::Or(xs) => {
                let op = if matches!(e, Expr::And(_)) {
                    " AND "
                } else {
                    " OR "
                };
                let parts: Vec<String> = xs.iter().map(|x| self.expr(x, ctx)).collect();
                format!("({})", parts.join(op))
            }
        }
    }

    fn paren(&mut self, e: &Expr, ctx: &CompileCtx<'_>) -> String {
        let s = self.expr(e, ctx);
        if s.starts_with('(') && matches!(e, Expr::Filter(_) | Expr::And(_) | Expr::Or(_)) {
            s
        } else {
            format!("({s})")
        }
    }

    fn addr(&mut self, a: &AddrMatch, address: &str, name: Option<&str>) -> String {
        match a {
            AddrMatch::Exact(x) => {
                self.text(x);
                format!("{address} = ?")
            }
            AddrMatch::Domain(d) => {
                for _ in 0..4 {
                    self.text(d);
                }
                format!(
                    "(substr({address}, -length(?)-1) = '@' || ? OR substr({address}, -length(?)-1) = '.' || ?)"
                )
            }
            AddrMatch::Fuzzy(s) => match name {
                Some(name) => {
                    self.text(s);
                    self.text(s);
                    format!(
                        "(instr(lower(COALESCE({name},'')), ?) > 0 OR instr(COALESCE({address},''), ?) > 0)"
                    )
                }
                None => {
                    self.text(s);
                    format!("instr(COALESCE({address},''), ?) > 0")
                }
            },
        }
    }

    fn from(&mut self, a: &AddrMatch) -> String {
        self.addr(a, "m.from_address", Some("m.from_name"))
    }

    fn to(&mut self, a: &AddrMatch) -> String {
        let inner = self.addr(
            a,
            "json_extract(r.value,'$.address')",
            Some("json_extract(r.value,'$.name')"),
        );
        let delivered = self.addr(a, "m.delivered_to", None);
        format!(
            "EXISTS (SELECT 1 FROM (SELECT value FROM json_each(m.to_json) UNION ALL SELECT value FROM json_each(m.cc_json) UNION ALL SELECT value FROM json_each(m.bcc_json)) r WHERE {inner}) OR {delivered}"
        )
    }

    fn filter(&mut self, f: &Filter, ctx: &CompileCtx<'_>) -> String {
        match f {
            Filter::From(a) => self.from(a),
            Filter::To(a) => self.to(a),
            Filter::Participant(a) => {
                let from = self.from(a);
                let to = self.to(a);
                format!("({from}) OR ({to})")
            }
            Filter::Ref(r) => {
                let values: Vec<&str> = r.candidates.iter().map(|c| c.value.as_str()).collect();
                self.text(&serde_json::Value::from(values).to_string());
                "EXISTS (SELECT 1 FROM refs rf WHERE rf.message_rowid = m.rowid AND rf.value IN (SELECT value FROM json_each(?)))".to_owned()
            }
            Filter::Label(l) => {
                self.text(l);
                "EXISTS (SELECT 1 FROM labels l WHERE l.message_rowid = m.rowid AND l.label = ?)"
                    .to_owned()
            }
            Filter::HasAttachment => "EXISTS (SELECT 1 FROM attachments a WHERE a.message_rowid = m.rowid AND COALESCE(a.disposition,'attachment') = 'attachment')".to_owned(),
            Filter::Filename(s) => {
                self.text(s);
                "EXISTS (SELECT 1 FROM attachments a WHERE a.message_rowid = m.rowid AND instr(lower(COALESCE(a.filename,'')), ?) > 0)".to_owned()
            }
            Filter::Type(t) => {
                let rule = match t {
                    TypeMatch::Mime(mime) => {
                        self.text(mime);
                        "COALESCE(a.sniffed_type, a.content_type) = ?".to_owned()
                    }
                    TypeMatch::Class(AttachmentClass::Other) => {
                        let all: Vec<String> = AttachmentClass::ALL
                            .iter()
                            .flat_map(|c| c.mime_rules())
                            .map(|r| self.mime_rule(r))
                            .collect();
                        format!("NOT COALESCE(({}), 0)", all.join(" OR "))
                    }
                    TypeMatch::Class(c) => {
                        let rules: Vec<String> =
                            c.mime_rules().iter().map(|r| self.mime_rule(r)).collect();
                        rules.join(" OR ")
                    }
                };
                format!("EXISTS (SELECT 1 FROM attachments a WHERE a.message_rowid = m.rowid AND ({rule}))")
            }
            Filter::After(_) | Filter::NewerThan(_) => {
                let t = match f {
                    Filter::NewerThan(d) => ago(*d, ctx.tz, ctx.now_ms),
                    Filter::After(b) => bound_ms(b, ctx.tz),
                    _ => 0,
                };
                self.int(t);
                self.int(t);
                format!("{MSG_DATE} >= ? AND m.received_at >= ? - 300000")
            }
            Filter::Before(_) | Filter::OlderThan(_) => {
                let t = match f {
                    Filter::OlderThan(d) => ago(*d, ctx.tz, ctx.now_ms),
                    Filter::Before(b) => bound_ms(b, ctx.tz),
                    _ => 0,
                };
                self.int(t);
                format!("{MSG_DATE} < ?")
            }
            Filter::Direction(d) => {
                self.text(d.as_str());
                "m.direction = ?".to_owned()
            }
            Filter::Thread(t) => {
                self.text(t);
                "m.thread_seq = (SELECT seq FROM threads WHERE id = ?)".to_owned()
            }
            Filter::IsUnread => "m.read = 0".to_owned(),
            Filter::IsNeedsReply => "m.direction = 'inbound' AND json_extract(m.triage_json,'$.needs_reply') >= 0.5 AND NOT EXISTS (SELECT 1 FROM messages o WHERE o.thread_seq = m.thread_seq AND o.direction = 'outbound' AND o.status NOT IN ('canceled','rejected','failed','suppressed') AND o.received_at > m.received_at)".to_owned(),
            Filter::IsQuarantined => "m.status = 'quarantined'".to_owned(),
            Filter::Category(c) => {
                self.text(c);
                "json_extract(m.triage_json,'$.category') = ?".to_owned()
            }
        }
    }

    fn mime_rule(&mut self, rule: &str) -> String {
        let (sql, value) = mime_rule_sql("COALESCE(a.sniffed_type, a.content_type)", rule);
        self.text(value);
        sql
    }
}

/// Text leaves outside any `Not`, in tree order.
fn positive_leaves<'a>(e: &'a Expr, out: &mut Vec<&'a TextLeaf>) {
    match e {
        Expr::Text(l) => out.push(l),
        Expr::And(xs) | Expr::Or(xs) => xs.iter().for_each(|x| positive_leaves(x, out)),
        Expr::Not(_) | Expr::Filter(_) => {}
    }
}

fn has_positive_quarantined(e: Option<&Expr>) -> bool {
    match e {
        Some(Expr::Filter(Filter::IsQuarantined)) => true,
        Some(Expr::And(xs) | Expr::Or(xs)) => xs.iter().any(|x| has_positive_quarantined(Some(x))),
        _ => false,
    }
}

/// Each positive leaf of at least 3 characters, quoted, and for leaves of 4 to 24 characters its
/// distinct trigrams; all joined with ` OR `, at most 64 strings.
fn tri_match(leaves: &[&TextLeaf]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |s: String, parts: &mut Vec<String>| {
        if parts.len() < MAX_TRIGRAM_STRINGS && seen.insert(s.clone()) {
            parts.push(fts_quote(&s));
        }
    };
    for leaf in leaves {
        let text = leaf.kind.text();
        let chars: Vec<char> = text.chars().collect();
        if chars.len() < 3 {
            continue;
        }
        add(text.to_owned(), &mut parts);
        if (4..=24).contains(&chars.len()) {
            for w in chars.windows(3) {
                add(w.iter().collect::<String>().to_lowercase(), &mut parts);
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join(" OR "))
}

/// Positive free-text (`TextField::Any`) leaves in input order, phrases unquoted, joined with spaces.
fn semantic_text(leaves: &[&TextLeaf]) -> String {
    let joined = leaves
        .iter()
        .filter(|l| l.field == TextField::Any)
        .map(|l| l.kind.text())
        .collect::<Vec<_>>()
        .join(" ");
    joined.chars().take(MAX_SEMANTIC_CHARS).collect()
}

/// Positive top-level terms accepted by an enabled normaliser whose normalised form has a digit.
fn ref_like(conjuncts: &[&Expr], ctx: &CompileCtx<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in conjuncts {
        let Expr::Text(TextLeaf {
            kind:
                TextKind::Term {
                    text,
                    prefix: false,
                },
            field: TextField::Any,
            ..
        }) = c
        else {
            continue;
        };
        for v in crate::refs::normalise(text, ctx.ref_packs, ctx.custom_refs, ctx.tz.name()) {
            if v.value.bytes().any(|b| b.is_ascii_digit()) && !out.contains(&v.value) {
                out.push(v.value);
            }
        }
    }
    out
}

/// Only top-level positive filters Vectorize can express; anything else needs the read-back.
fn vector_filter(conjuncts: &[&Expr]) -> (VectorFilter, bool) {
    let mut v = VectorFilter::default();
    let mut post = false;
    for c in conjuncts {
        let Expr::Filter(f) = c else {
            post |= kinds(c).1;
            continue;
        };
        match f {
            Filter::Direction(d) if v.direction.is_none() => v.direction = Some(*d),
            Filter::After(DateBound::Instant(t)) => {
                let s = t.div_euclid(1000);
                v.sent_at_gte = Some(v.sent_at_gte.map_or(s, |x| x.max(s)));
            }
            Filter::Before(DateBound::Instant(t)) => {
                let s = t.div_euclid(1000) + i64::from(t.rem_euclid(1000) != 0);
                v.sent_at_lt = Some(v.sent_at_lt.map_or(s, |x| x.min(s)));
            }
            Filter::HasAttachment => v.has_attachment = true,
            Filter::Thread(t) if v.thread_id.is_none() => v.thread_id = Some(t.clone()),
            Filter::From(AddrMatch::Domain(d))
                if v.sender_domain.is_none() && org_domain(d) == d =>
            {
                v.sender_domain = Some(d.clone());
            }
            _ => post = true,
        }
    }
    (v, post)
}
