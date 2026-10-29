//! The search query language ([Search § 3–4]): a recursive-descent parser over `q` into a typed tree,
//! its canonical serialisation, date resolution in the tenant time zone, and compilation into a quoted
//! FTS5 expression plus bound-parameter SQL. Raw input never reaches `MATCH` (F1); dates reach the
//! mailbox only as UTC milliseconds (F9).
//!
//! [Search § 3–4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

mod ast;
mod compile;
mod dates;
mod parser;

pub use ast::{
    AddrMatch, AttachmentClass, CivilDate, DateBound, Direction, DurUnit, Expr, Filter, Query,
    RefMatch, RelDuration, Span, TextField, TextKind, TextLeaf, TypeMatch, mime_rule_sql, print,
    print_filter,
};
pub use chrono_tz::Tz;
pub use compile::{
    CompiledQuery, MSG_DATE, SqlFragment, SqlParam, TermPattern, VectorFilter, compile,
    require_semantic_text,
};
pub use dates::{ResolvedQuery, ago, bound_ms, local_midnight_ms, month_buckets, resolve};
pub use parser::{MAX_CHARS, MAX_DEPTH, MAX_LEAVES, fold_nfkc};

use crate::refs::{CustomRef, Pack};

/// `400 invalid_query`: `details.position` (character offset), `details.expected`, `details.found`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    pub position: u32,
    pub expected: String,
    /// The text at `position`, at most 32 characters; `None` at the end of input.
    pub found: Option<String>,
}

impl QueryError {
    pub const CODE: &'static str = "invalid_query";
}

/// `details.expected` values ([Search § 3.3]).
pub mod expected {
    pub const CLOSING_QUOTE: &str = "closing '\"'";
    pub const UNKNOWN_OPERATOR: &str = "operator name (from, to, participant, subject, ref, label, has, filename, type, after, before, newer_than, older_than, in, thread, is, category) or quoted text";
    pub const OPERATOR_VALUE: &str = "operator value";
    pub const DIRECTION: &str = "inbound or outbound";
    pub const HAS: &str = "attachment";
    pub const IS: &str = "unread, needs_reply or quarantined";
    pub const LABEL: &str = "label name (a-z, 0-9, _, :, -; at most 64 characters)";
    pub const TYPE: &str = "attachment type (pdf, doc, sheet, slides, image, archive, ics, eml, text, html, audio, video, other) or MIME type";
    pub const DATE: &str = "date as YYYY-MM-DD, YYYY/MM/DD or RFC 3339";
    pub const DURATION: &str = "duration as <n>h, <n>d, <n>w, <n>m or <n>y";
    pub const THREAD: &str = "thread ID (thr_…)";
    pub const CLOSING_PAREN: &str = "')'";
    pub const TERM: &str = "term";
    pub const PREFIX: &str = "term of at least 2 characters before '*'";
    pub const WHITESPACE: &str = "whitespace";
    pub const LIMIT: &str = "shorter query (max 1024 characters, 32 terms, depth 8)";
    pub const SEMANTIC: &str = "free text for semantic search";
}

/// The tenant settings the parser needs.
#[derive(Clone, Copy, Debug)]
pub struct ParseCtx<'a> {
    /// The effective triage category list (`category:` must name one).
    pub categories: &'a [&'a str],
    /// `policy.search.refs_packs` and `custom_refs`, for `ref:` candidates.
    pub ref_packs: &'a [Pack],
    pub custom_refs: &'a [CustomRef],
    /// `tenants.timezone` (the country for phone numbers).
    pub tz: Tz,
}

impl Default for ParseCtx<'static> {
    /// No categories, pack `core`, UTC: what the fuzz target uses.
    fn default() -> Self {
        Self {
            categories: &[],
            ref_packs: &[Pack::Core],
            custom_refs: &[],
            tz: Tz::UTC,
        }
    }
}

/// The inputs of compilation besides the tree.
#[derive(Clone, Copy, Debug)]
pub struct CompileCtx<'a> {
    pub tz: Tz,
    /// The request time from the platform clock, or the cursor's `as_of` on later pages.
    pub now_ms: i64,
    pub ref_packs: &'a [Pack],
    pub custom_refs: &'a [CustomRef],
    /// The key holds `quarantine:review`.
    pub quarantine_review: bool,
}

/// Parses `q` ([Search § 3.1]). Every input either parses or returns an `invalid_query` error with a
/// position; nothing panics.
pub fn parse(input: &str, ctx: &ParseCtx<'_>) -> Result<Query, QueryError> {
    parser::parse(input, ctx)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::{DateTime, NaiveDate};
    use proptest::prelude::*;

    use super::*;

    const CATEGORIES: &[&str] = &["billing", "receipts"];
    const UK: &[Pack] = &[Pack::Core, Pack::UkVehicle];
    const NOW: i64 = 1_793_059_200_000; // 2026-10-27T00:00:00Z

    fn pctx() -> ParseCtx<'static> {
        ParseCtx {
            categories: CATEGORIES,
            ref_packs: UK,
            custom_refs: &[],
            tz: Tz::Europe__London,
        }
    }

    fn cctx(tz: Tz, now_ms: i64) -> CompileCtx<'static> {
        CompileCtx {
            tz,
            now_ms,
            ref_packs: UK,
            custom_refs: &[],
            quarantine_review: false,
        }
    }

    fn p(q: &str) -> Query {
        parse(q, &pctx()).unwrap_or_else(|e| panic!("{q}: {e:?}"))
    }

    fn e(q: &str) -> QueryError {
        parse(q, &pctx()).expect_err(q)
    }

    fn c(q: &str) -> CompiledQuery {
        compile(&p(q), &cctx(Tz::Europe__London, NOW))
    }

    fn ms(rfc3339: &str) -> i64 {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .timestamp_millis()
    }

    /// The tree with every span zeroed: `print` loses offsets, nothing else.
    fn strip(e: &Expr) -> Expr {
        match e {
            Expr::And(xs) => Expr::And(xs.iter().map(strip).collect()),
            Expr::Or(xs) => Expr::Or(xs.iter().map(strip).collect()),
            Expr::Not(x) => Expr::Not(Box::new(strip(x))),
            Expr::Text(l) => Expr::Text(TextLeaf {
                span: Span::default(),
                ..l.clone()
            }),
            Expr::Filter(f) => Expr::Filter(f.clone()),
        }
    }

    fn term(t: &str) -> Expr {
        Expr::Text(TextLeaf {
            kind: TextKind::Term {
                text: t.into(),
                prefix: false,
            },
            field: TextField::Any,
            span: Span::default(),
        })
    }

    fn leaves<'a>(e: &'a Expr, out: &mut Vec<&'a TextLeaf>) {
        match e {
            Expr::Text(l) => out.push(l),
            Expr::And(xs) | Expr::Or(xs) => xs.iter().for_each(|x| leaves(x, out)),
            Expr::Not(x) => leaves(x, out),
            Expr::Filter(_) => {}
        }
    }

    /// The strings a `MATCH` may quote: leaf texts and their lower-case trigrams.
    fn allowed(q: &Query) -> HashSet<String> {
        let mut ls = Vec::new();
        if let Some(r) = &q.root {
            leaves(r, &mut ls);
        }
        let mut out = HashSet::new();
        for l in ls {
            let t = l.kind.text();
            out.insert(t.to_owned());
            let cs: Vec<char> = t.chars().collect();
            for w in cs.windows(3) {
                out.insert(w.iter().collect::<String>().to_lowercase());
            }
        }
        out
    }

    /// A `MATCH` string holds only quoted strings from the tree, `AND`/`OR`/`NOT`, balanced
    /// parentheses, `{subject} :`, and `*` right after a quoted string.
    fn check_match(m: &str, allowed: &HashSet<String>) -> Result<(), String> {
        let cs: Vec<char> = m.chars().collect();
        let (mut i, mut depth) = (0usize, 0i32);
        while i < cs.len() {
            match cs[i] {
                ' ' => i += 1,
                '(' => {
                    depth += 1;
                    i += 1;
                }
                ')' => {
                    depth -= 1;
                    if depth < 0 {
                        return Err(format!("unbalanced ) in {m}"));
                    }
                    i += 1;
                }
                '"' => {
                    let mut s = String::new();
                    i += 1;
                    loop {
                        match cs.get(i) {
                            None => return Err(format!("unterminated string in {m}")),
                            Some('"') if cs.get(i + 1) == Some(&'"') => {
                                s.push('"');
                                i += 2;
                            }
                            Some('"') => {
                                i += 1;
                                break;
                            }
                            Some(&ch) => {
                                s.push(ch);
                                i += 1;
                            }
                        }
                    }
                    if !allowed.contains(&s) {
                        return Err(format!("string {s:?} is not from the tree in {m}"));
                    }
                    if cs.get(i..i + 2) == Some(&[' ', '*']) {
                        i += 2;
                    }
                }
                '{' => {
                    let rest: String = cs[i..].iter().take(11).collect();
                    if rest != "{subject} :" {
                        return Err(format!("column filter in {m}"));
                    }
                    i += 11;
                }
                _ => {
                    let word: String = cs[i..]
                        .iter()
                        .take_while(|c| c.is_ascii_uppercase())
                        .collect();
                    if !matches!(word.as_str(), "AND" | "OR" | "NOT") {
                        return Err(format!("bare {:?} in {m}", &cs[i..]));
                    }
                    i += word.len();
                    if cs.get(i).is_some_and(|c| *c != ' ') {
                        return Err(format!("operator not followed by a space in {m}"));
                    }
                }
            }
        }
        if depth == 0 {
            Ok(())
        } else {
            Err(format!("unbalanced ( in {m}"))
        }
    }

    /// Every `MATCH` the compiled query carries: `fts_match`, `tri_match` and each SQL `MATCH ?`.
    fn matches_of(cq: &CompiledQuery) -> Vec<String> {
        let mut out: Vec<String> = cq.fts_match.iter().chain(&cq.tri_match).cloned().collect();
        let sql = &cq.filter.sql;
        let mut idx = 0;
        for (at, _) in sql.match_indices('?') {
            if sql[..at].ends_with("MATCH ")
                && let Some(SqlParam::Text(t)) = cq.filter.params.get(idx)
            {
                out.push(t.clone());
            }
            idx += 1;
        }
        assert_eq!(
            idx,
            cq.filter.params.len(),
            "one parameter per placeholder: {sql}"
        );
        out
    }

    /// Token soup: operators, FTS5 syntax, quotes, brackets, Unicode.
    fn soup() -> impl Strategy<Value = String> {
        let atoms = prop::sample::select(vec![
            "a",
            "bc",
            "Invoice",
            "AB12CDE",
            "OR",
            "or",
            "AND",
            "NOT",
            "NEAR",
            "NEAR(a b)",
            "-",
            "(",
            ")",
            "\"",
            "\\",
            "*",
            "ab*",
            "x*",
            "subject:",
            "subject:Fiat",
            "SUBJECT:\"q 3\"",
            "from:jo@example.net",
            "from:@ex.example",
            "to:Rivera",
            "participant:\"Jo R\"",
            "ref:AB12-CDE",
            "ref:£4",
            "ref:\"412.80 GBP\"",
            "in:inbound",
            "in:spam",
            "after:2026-01-01",
            "before:2026/02/03",
            "after:2026-09-01T10:00:00.5Z",
            "before:2026-13-01",
            "newer_than:5d",
            "older_than:2m",
            "newer_than:5x",
            "is:unread",
            "is:quarantined",
            "category:billing",
            "category:spam",
            "type:pdf",
            "type:application/pdf",
            "type:exe",
            "label:urgent",
            "label:Bad Label",
            "has:attachment",
            "thread:thr_01J9Z3K8V4QW7X2M5N6P8R0T1Y",
            "thread:x",
            "filename:Ｒeport",
            "col:x",
            "body:x",
            "{subject}",
            "^a",
            "a+b",
            ":",
            "10:30",
            "é",
            "\u{202e}",
            "\0",
            "\"a b\"",
            "\"x\\\"y\"",
            "\"\"",
            "''",
            "\"AND\"",
            "\"x\"\"y\"",
        ]);
        let gap = prop::sample::select(vec!["", " ", " ", "  ", "\t"]);
        prop::collection::vec((atoms, gap), 0..24)
            .prop_map(|v| v.into_iter().map(|(a, g)| format!("{a}{g}")).collect())
    }

    /// Well-formed queries: valid leaves (including FTS5-looking text) under AND, OR, `-` and groups.
    fn wellformed() -> impl Strategy<Value = String> {
        let leaf = prop_oneof![
            "[a-zA-Z0-9éß+^:.@_'{}*-]{1,8}".prop_filter_map("a valid term", |t| {
                parse(&t, &pctx())
                    .ok()
                    .filter(|q| matches!(q.root, Some(Expr::Text(_))))
                    .map(|_| t)
            }),
            "[^\"\\\\]{1,8}".prop_map(|s| format!("\"{s}\"")),
            prop::sample::select(vec![
                "from:Jo@Example.net",
                "to:@ex.example",
                "participant:\"Jo \\\"R\\\\\"",
                "from:rivera",
                "ref:\"ab12 cde\"",
                "label:a:b",
                "type:SHEET",
                "type:Image/PNG",
                "filename:\"A B\"",
                "after:2026-02-28",
                "before:2026-09-01T10:00:00.123+01:00",
                "newer_than:007d",
                "older_than:1y",
                "in:OUTBOUND",
                "thread:thr_01j9z3k8v4qw7x2m5n6p8r0t1y",
                "is:needs_reply",
                "category:Billing",
                "has:attachment",
                "subject:ab*",
                "subject:\"x y\"",
                "NEAR",
                "\"NEAR(a b)\"",
                "\"{subject} : x\"",
                "AND",
                "NOT",
            ])
            .prop_map(str::to_owned),
        ];
        leaf.prop_recursive(4, 24, 3, |inner| {
            prop_oneof![
                (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a} {b}")),
                (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a} OR {b}")),
                inner.clone().prop_map(|a| format!("-({a})")),
                inner.prop_map(|a| format!("( {a} )")),
            ]
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 20000, ..ProptestConfig::default() })]

        /// Well-formed input parses (unless it crosses a limit).
        #[test]
        fn f1_wellformed_parses(q in wellformed()) {
            if let Err(err) = parse(&q, &pctx()) {
                prop_assert_eq!(err.expected.as_str(), expected::LIMIT, "{:?}", q);
            }
        }

        /// Any input parses or fails with a position inside it; nothing panics.
        #[test]
        fn f1_parse_is_total(q in prop_oneof![soup(), any::<String>(), "\\PC{0,1100}"]) {
            match parse(&q, &pctx()) {
                Ok(t) => prop_assert_eq!(t.source_len as usize, q.chars().count()),
                Err(err) => {
                    prop_assert!(err.position as usize <= q.chars().count());
                    prop_assert!(!err.expected.is_empty());
                    prop_assert!(err.found.as_ref().is_none_or(|f| f.chars().count() <= 32));
                    prop_assert_eq!(err.found.is_none(), err.position as usize >= q.chars().count());
                }
            }
        }

        /// Every compiled `MATCH` is built from quoted strings of the tree and FTS5 syntax we chose.
        #[test]
        fn f1_match_is_quoted(q in prop_oneof![soup(), wellformed(), "\\PC{0,200}"]) {
            if let Ok(t) = parse(&q, &pctx()) {
                let cq = compile(&t, &cctx(Tz::Europe__London, NOW));
                let ok = allowed(&t);
                for m in matches_of(&cq) {
                    if let Err(why) = check_match(&m, &ok) {
                        prop_assert!(false, "{} from {:?}", why, q);
                    }
                }
            }
        }

        /// `parse(print(tree)) == tree`, spans aside.
        #[test]
        fn f1_print_round_trip(q in prop_oneof![soup(), wellformed()]) {
            if let Ok(t) = parse(&q, &pctx()) {
                let printed = print(&t);
                prop_assume!(printed.chars().count() <= MAX_CHARS);
                let again = parse(&printed, &pctx());
                prop_assert!(again.is_ok(), "{:?} printed as {:?}: {:?}", q, printed, again);
                let again = again.unwrap();
                prop_assert_eq!(t.root.as_ref().map(strip), again.root.as_ref().map(strip));
                prop_assert_eq!(print(&again), printed);
            }
        }
    }

    #[test]
    fn f1_injection_is_quoted() {
        // `col:` looks like an operator and is refused; quoting searches for the text literally.
        assert_eq!(e("col:y").expected, expected::UNKNOWN_OPERATOR);
        let q = r#"NEAR a+b ^x "c\" OR" "col:y" "{subject}" NOT AND*"#;
        let cq = c(q);
        assert_eq!(
            cq.fts_match.as_deref(),
            Some(
                r#""NEAR" AND "a+b" AND "^x" AND "c"" OR" AND "col:y" AND "{subject}" AND "NOT" AND "AND" *"#
            )
        );
        for m in matches_of(&cq) {
            check_match(&m, &allowed(&p(q))).unwrap();
        }
    }

    #[test]
    fn grammar_precedence() {
        let and_or = Expr::And(vec![term("a"), Expr::Or(vec![term("b"), term("c")])]);
        assert_eq!(p("a b OR c").root.as_ref().map(strip), Some(and_or.clone()));
        assert_eq!(p("a (b OR c)").root.as_ref().map(strip), Some(and_or));
        assert_eq!(
            p("a OR b c").root.as_ref().map(strip),
            Some(Expr::And(vec![
                Expr::Or(vec![term("a"), term("b")]),
                term("c")
            ]))
        );
        // Lower-case and mixed-case `or` are terms.
        assert_eq!(
            p("a or b").root.as_ref().map(strip),
            Some(Expr::And(vec![term("a"), term("or"), term("b")]))
        );
        assert_eq!(
            p("a Or b").root.as_ref().map(strip),
            Some(Expr::And(vec![term("a"), term("Or"), term("b")]))
        );
        // Groups flatten into their parent; `OR` chains are one node.
        assert_eq!(
            p("(a b) c").root.as_ref().map(strip),
            Some(Expr::And(vec![term("a"), term("b"), term("c")]))
        );
        assert_eq!(
            p("a OR (b OR c)").root.as_ref().map(strip),
            Some(Expr::Or(vec![term("a"), term("b"), term("c")]))
        );
        // `-` binds to one primary; `10:30` is a term; operator names ignore case.
        assert_eq!(
            p("-a b").root.as_ref().map(strip),
            Some(Expr::And(vec![Expr::Not(Box::new(term("a"))), term("b")]))
        );
        assert_eq!(p("10:30").root.as_ref().map(strip), Some(term("10:30")));
        assert_eq!(print(&p("FROM:Jo@Example.NET")), "from:jo@example.net");
        // Canonical form: parentheses only where needed.
        assert_eq!(print(&p("((a OR b)) (c d) OR e")), "a OR b (c d) OR e");
        assert_eq!(print(&p("-(-a) -(b c)")), "-(-a) -(b c)");
        assert_eq!(p("   ").root, None);
    }

    #[test]
    fn errors_position() {
        let row = |q: &str, pos: u32, exp: &str| {
            let err = e(q);
            assert_eq!((err.position, err.expected.as_str()), (pos, exp), "{q}");
            err
        };
        assert_eq!(
            row(r#"foo "bar"#, 4, expected::CLOSING_QUOTE)
                .found
                .as_deref(),
            Some("\"bar")
        );
        assert_eq!(
            row("form:x", 0, expected::UNKNOWN_OPERATOR)
                .found
                .as_deref(),
            Some("form:x")
        );
        row("a https://x.example", 2, expected::UNKNOWN_OPERATOR);
        assert_eq!(row("from:", 5, expected::OPERATOR_VALUE).found, None);
        row("from: x", 5, expected::OPERATOR_VALUE);
        row("subject:\"\"", 8, expected::OPERATOR_VALUE);
        row("in:spam", 3, expected::DIRECTION);
        row("is:starred", 3, expected::IS);
        row("has:link", 4, expected::HAS);
        row("type:exe", 5, expected::TYPE);
        row("label:-x", 6, expected::LABEL);
        row("after:2026-13-01", 6, expected::DATE);
        row("before:2026-02-30", 7, expected::DATE);
        row("newer_than:5x", 11, expected::DURATION);
        row("newer_than:0d", 11, expected::DURATION);
        row("older_than:11y", 11, expected::DURATION);
        row("newer_than:87601h", 11, expected::DURATION);
        row("thread:abc", 7, expected::THREAD);
        row("category:spam", 9, "category: one of billing, receipts");
        row("(a b", 0, expected::CLOSING_PAREN);
        row("a (b (c)", 2, expected::CLOSING_PAREN);
        row("a)", 1, expected::TERM);
        row("()", 1, expected::TERM);
        row("a OR", 2, expected::TERM);
        row("OR a", 0, expected::TERM);
        row("(a OR) b", 3, expected::TERM);
        row("- a", 0, expected::TERM);
        row("a -", 2, expected::TERM);
        row("--a", 0, expected::TERM);
        row("*", 0, expected::PREFIX);
        row("a*", 0, expected::PREFIX);
        row("\"\"", 0, expected::TERM);
        row("a(b)", 1, expected::WHITESPACE);
        // Limits: length, leaves, depth.
        assert_eq!(
            row(&"a ".repeat(600), 1024, expected::LIMIT)
                .found
                .as_deref(),
            Some("a")
        );
        let many: Vec<String> = (0..33).map(|i| format!("t{i}")).collect();
        let q = many.join(" ");
        row(&q, q.find("t32").unwrap() as u32, expected::LIMIT);
        p(&many[..32].join(" "));
        row("((((((((( a )))))))))", 8, expected::LIMIT);
        p("(((((((( a ))))))))");
        // Semantic mode needs free text.
        let err = require_semantic_text(&c("from:jo@example.net subject:x")).unwrap_err();
        assert_eq!(
            (err.position, err.expected.as_str()),
            (0, expected::SEMANTIC)
        );
        assert!(require_semantic_text(&c("budget")).is_ok());
        // `is:quarantined` without `quarantine:review` parses; it only matches nothing later.
        let cq = c("is:quarantined");
        assert!(!cq.forces_include_quarantined);
        assert!(cq.filter.sql.contains("m.status = 'quarantined'"));
        let review = CompileCtx {
            quarantine_review: true,
            ..cctx(Tz::Europe__London, NOW)
        };
        assert!(compile(&p("is:quarantined"), &review).forces_include_quarantined);
        assert_eq!(QueryError::CODE, "invalid_query");
    }

    #[test]
    fn compile_examples() {
        // Negated text joins the FTS expression when a positive one exists, else becomes SQL.
        let cq = c("invoice -draft");
        assert_eq!(cq.fts_match.as_deref(), Some(r#"("invoice") NOT "draft""#));
        assert_eq!(cq.filter.sql, "1");
        let cq = c("-draft");
        assert_eq!(cq.fts_match, None);
        assert_eq!(
            cq.filter.sql,
            "m.rowid NOT IN (SELECT rowid FROM fts WHERE fts MATCH ?)"
        );
        assert_eq!(cq.filter.params, [SqlParam::Text(r#""draft""#.into())]);
        // `NOT` inside a text conjunct only on the right of a positive `AND` sibling.
        let cq = c("(a -b) OR cd*");
        assert_eq!(
            cq.fts_match.as_deref(),
            Some(r#"((("a") NOT "b") OR "cd" *)"#)
        );
        let cq = c("a OR -b");
        assert_eq!(cq.fts_match, None);
        assert_eq!(
            cq.filter.sql,
            "(m.rowid IN (SELECT rowid FROM fts WHERE fts MATCH ?) OR NOT COALESCE((m.rowid IN (SELECT rowid FROM fts WHERE fts MATCH ?)), 0))"
        );
        // Mixed conjuncts filter through FTS sub-selects; filters bind every value.
        let cq = c("budget (report OR from:jo@example.net) subject:inv* label:urgent");
        assert_eq!(
            cq.fts_match.as_deref(),
            Some(r#""budget" AND {subject} : ("inv" *)"#)
        );
        assert_eq!(
            cq.filter.sql,
            "(m.rowid IN (SELECT rowid FROM fts WHERE fts MATCH ?) OR (m.from_address = ?)) AND (EXISTS (SELECT 1 FROM labels l WHERE l.message_rowid = m.rowid AND l.label = ?))"
        );
        assert_eq!(
            cq.filter.params,
            [
                SqlParam::Text(r#""report""#.into()),
                SqlParam::Text("jo@example.net".into()),
                SqlParam::Text("urgent".into())
            ]
        );
        assert_eq!(cq.semantic_text, "budget report");
        assert_eq!(cq.why_filters, ["label:urgent"]);
        // Trigram fallback: the leaf, then its distinct trigrams.
        assert_eq!(
            c("Fiat").tri_match.as_deref(),
            Some(r#""Fiat" OR "fia" OR "iat""#)
        );
        assert_eq!(c("ab").tri_match, None);
        // Ref-like terms: a normaliser accepts them and the value has a digit.
        assert_eq!(c("AB12CDE invoice").ref_like, ["AB12CDE"]);
        let r = p("ref:\"ab12 cde\"");
        let Some(Expr::Filter(Filter::Ref(m))) = &r.root else {
            panic!()
        };
        assert!(m.candidates.iter().any(|v| v.value == "AB12CDE"));
        // The vector pre-filter takes only what Vectorize can express.
        let cq = c(
            "from:@brightwell.example in:inbound has:attachment after:2026-09-01 before:2026-10-01 thread:thr_01J9Z3K8V4QW7X2M5N6P8R0T1Y",
        );
        assert_eq!(
            cq.vector_prefilter,
            VectorFilter {
                direction: Some(Direction::Inbound),
                sent_at_gte: Some(ms("2026-08-31T23:00:00Z") / 1000),
                sent_at_lt: Some(ms("2026-09-30T23:00:00Z") / 1000),
                has_attachment: true,
                thread_id: Some("thr_01J9Z3K8V4QW7X2M5N6P8R0T1Y".into()),
                sender_domain: Some("brightwell.example".into()),
            }
        );
        assert!(!cq.needs_post_filter);
        assert_eq!(cq.why_filters[0], "from:brightwell.example");
        assert!(c("from:@mail.brightwell.example").needs_post_filter);
        assert!(c("label:x").needs_post_filter);
        // `type:other` is everything no class rule matches; `NULL` never satisfies a negation.
        let cq = c("type:other");
        assert!(cq.filter.sql.contains("NOT COALESCE(("));
        assert_eq!(cq.filter.params.len(), 22);
    }

    #[test]
    fn f9_timezone() {
        // Facet months are local: October 2026 in London starts at 2026-09-30T23:00Z (BST).
        let m = month_buckets(Tz::Europe__London, NOW, 3);
        assert_eq!(
            m[0],
            ("2026-10".into(), 1_790_809_200_000, 1_793_491_200_000)
        );
        assert_eq!((m[1].0.as_str(), m[1].2), ("2026-09", m[0].1));
        assert_eq!(m[2].0, "2026-08");
        let london = cctx(Tz::Europe__London, NOW);
        let bound =
            |q: &str, ctx: &CompileCtx<'_>| match &resolve(&p(q), ctx.tz, ctx.now_ms).query.root {
                Some(Expr::Filter(Filter::After(DateBound::Instant(t))))
                | Some(Expr::Filter(Filter::Before(DateBound::Instant(t)))) => *t,
                other => panic!("{q}: {other:?}"),
            };
        // Local midnight: BST in summer, GMT in winter; RFC 3339 instants as is.
        assert_eq!(
            bound("after:2026-07-01", &london),
            ms("2026-06-30T23:00:00Z")
        );
        assert_eq!(
            bound("before:2026/01/15", &london),
            ms("2026-01-15T00:00:00Z")
        );
        assert_eq!(
            bound("after:2026-10-25", &london),
            ms("2026-10-24T23:00:00Z")
        );
        assert_eq!(
            bound("after:2026-09-01T10:00:00+02:00", &london),
            ms("2026-09-01T08:00:00Z")
        );
        let tokyo = cctx(Tz::Asia__Tokyo, NOW);
        assert_eq!(
            bound("after:2026-07-01", &tokyo),
            ms("2026-06-30T15:00:00Z")
        );
        // A DST gap at midnight: the first valid instant after it.
        let sao_paulo = cctx(Tz::America__Sao_Paulo, NOW);
        assert_eq!(
            bound("after:2018-11-04", &sao_paulo),
            ms("2018-11-04T03:00:00Z")
        );
        // A whole skipped day (Samoa, 30 December 2011).
        let apia = cctx(Tz::Pacific__Apia, NOW);
        assert_eq!(bound("after:2011-12-30", &apia), ms("2011-12-30T10:00:00Z"));
        // Midnight occurring twice (Cuba ends DST at 01:00): the earlier.
        let havana = cctx(Tz::America__Havana, NOW);
        assert_eq!(
            bound("after:2019-11-03", &havana),
            ms("2019-11-03T04:00:00Z")
        );

        // Calendar arithmetic in the tenant zone: a DST change never shifts a `d` boundary by an hour.
        let at = |now: &str| cctx(Tz::Europe__London, ms(now));
        let mon = at("2026-03-30T11:00:00Z"); // 12:00 BST, the day after the clocks went forward
        assert_eq!(bound("newer_than:1d", &mon), ms("2026-03-29T11:00:00Z"));
        assert_eq!(bound("newer_than:2d", &mon), ms("2026-03-28T12:00:00Z"));
        assert_eq!(bound("newer_than:24h", &mon), ms("2026-03-29T11:00:00Z"));
        assert_eq!(bound("older_than:1w", &mon), ms("2026-03-23T12:00:00Z"));
        // Months clamp to the end of the month; years keep 29 February only in leap years.
        assert_eq!(
            bound("newer_than:1m", &at("2026-03-31T11:00:00Z")),
            ms("2026-02-28T12:00:00Z")
        );
        assert_eq!(
            bound("newer_than:1m", &at("2028-03-31T11:00:00Z")),
            ms("2028-02-29T12:00:00Z")
        );
        assert_eq!(
            bound("newer_than:1y", &at("2028-02-29T12:00:00Z")),
            ms("2027-02-28T12:00:00Z")
        );
        assert_eq!(
            bound("newer_than:10y", &at("2026-10-27T00:00:00Z")),
            ms("2016-10-26T23:00:00Z")
        );

        // A cursor's `as_of` is `now`: later pages resolve to the same bounds whatever the clock says.
        let as_of = ms("2026-10-27T09:30:00Z");
        let page1 = compile(
            &p("newer_than:3d invoice"),
            &cctx(Tz::Europe__London, as_of),
        );
        let page2 = compile(
            &p("newer_than:3d invoice"),
            &cctx(Tz::Europe__London, as_of),
        );
        assert_eq!(page1, page2);
        assert_eq!(
            page1.filter.params[0],
            SqlParam::Int(ms("2026-10-24T08:30:00Z"))
        );
        let later = compile(
            &p("newer_than:3d invoice"),
            &cctx(Tz::Europe__London, as_of + 3_600_000),
        );
        assert_ne!(page1.filter.params, later.filter.params);

        // The tree keeps the calendar day; only resolution needs the zone.
        assert_eq!(
            p("after:2026-07-01").root,
            Some(Expr::Filter(Filter::After(DateBound::Day(
                NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
            ))))
        );
        assert_eq!(
            local_midnight_ms(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), Tz::UTC),
            ms("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            print(&p("after:2026-09-01T10:00:00.5+00:00")),
            "after:2026-09-01T10:00:00.500Z"
        );
    }
}
