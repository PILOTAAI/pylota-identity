//! Search inside the mailbox ([Search § 5, § 6, § 8, § 12]): keyword candidates (FTS5, references,
//! filters, the trigram fallback), the semantic read-back, per-hit details (snippets, `why`, attachment
//! hits), facets, coverage, contacts, and the chunk bookkeeping of the index pipeline. Every query
//! applies the visibility predicate; the front Worker merges, ranks and pages.
//!
//! [Search § 5, § 6, § 8, § 12]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use std::collections::HashMap;

use pylota_identity_core::query::AttachmentClass;
use pylota_identity_core::search::snippet;
use pylota_identity_platform::PResult;
use pylota_identity_platform::d1::SqlValue;
use pylota_identity_platform::durable::Sql;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::search::wire::{Cand, Compiled, Detail};

/// The columns of a candidate; `m` is `messages`, `t` its thread.
const CAND_COLS: &str = "m.rowid AS rowid, m.id AS id, t.id AS thread_id, m.thread_seq AS thread_seq, m.direction AS direction, m.status AS status, m.from_address AS from_address, m.from_name AS from_name, m.subject AS subject, m.sent_at AS sent_at, m.received_at AS received_at, MIN(COALESCE(m.sent_at, m.received_at), m.received_at + 300000) AS msg_date, m.verdict AS verdict, m.known_sender AS known_sender";
/// The visibility predicate (F7) with `?2` as_of and `?3` include_quarantined.
const VISIBLE: &str = " AND m.status NOT IN ('hidden','throttled') AND (m.status NOT IN ('quarantined','screening') OR ?3 = 1) AND m.received_at <= ?2 AND (";
/// Candidates read for the facet set.
const FACET_SET: u32 = 5_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeywordIn {
    pub c: Compiled,
    pub as_of: i64,
    pub include_quarantined: bool,
    /// How many candidates to return (1,000 keyword, 200 hybrid).
    pub limit: u32,
    /// Month buckets `[label, start_ms, end_ms]` when facets are wanted.
    pub months: Option<Vec<(String, i64, i64)>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KeywordOut {
    pub cands: Vec<Cand>,
    pub facets: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadBackIn {
    pub vector_ids: Vec<String>,
    pub c: Compiled,
    pub as_of: i64,
    pub include_quarantined: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetailsIn {
    /// `(rowid, best chunk of a semantic hit)`.
    pub items: Vec<(i64, Option<String>)>,
    pub c: Compiled,
    pub snippet_chars: u32,
}

fn bool_v(b: bool) -> SqlValue {
    i64::from(b).into()
}

/// The statement `head … WHERE where_ VISIBLE filter) tail` with `?1`, as_of, include and the filter
/// parameters bound.
fn run<S: Sql>(
    s: &S,
    head: &str,
    where_: &str,
    tail: &str,
    first: SqlValue,
    inp_c: &Compiled,
    as_of: i64,
    include: bool,
) -> PResult<Vec<Cand>> {
    let mut sql = String::with_capacity(1024);
    sql.push_str(head);
    sql.push_str(" WHERE ");
    sql.push_str(where_);
    sql.push_str(VISIBLE);
    sql.push_str(inp_c.filter());
    sql.push(')');
    sql.push_str(tail);
    let mut params = vec![first, as_of.into(), bool_v(include)];
    params.extend(inp_c.params());
    s.query(&sql, &params)
}

/// The keyword score of [Search § 5.3].
fn kw_score(s: f64, ref_hit: bool, textual: bool) -> f64 {
    if !textual {
        return 1.0;
    }
    let bm = if s > 0.0 { s / (s + 4.0) } else { 0.0 };
    0.7 * bm + 0.3 * if ref_hit { 1.0 } else { 0.0 }
}

fn trigrams(s: &str) -> Vec<String> {
    let c: Vec<char> = s.chars().collect();
    let mut v: Vec<String> = c.windows(3).map(|w| w.iter().collect()).collect();
    v.sort();
    v.dedup();
    v
}

fn jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.iter().filter(|x| b.contains(x)).count() as f64;
    inter / (a.len() as f64 + b.len() as f64 - inter)
}

/// Keyword candidates ([Search § 5.2–5.4]) and, on a first page, facets.
pub fn keyword<S: Sql>(s: &S, inp: &KeywordIn) -> PResult<KeywordOut> {
    let c = &inp.c;
    let textual = c.fts_match.is_some() || !c.ref_like.is_empty();
    let head_m = {
        let mut h = String::from("SELECT ");
        h.push_str(CAND_COLS);
        h.push_str(", 0.0 AS s FROM messages m JOIN threads t ON t.seq = m.thread_seq");
        h
    };
    let order = " ORDER BY s DESC, msg_date DESC, m.rowid DESC LIMIT 5000";
    let mut by_row: HashMap<i64, Cand> = HashMap::new();
    let mut order_rows: Vec<i64> = Vec::new();
    let refs_cover_text = !c.ref_like.is_empty() && c.ref_like.len() >= c.terms.len();
    if let Some(m) = c.fts_match.as_ref().filter(|_| !refs_cover_text) {
        let mut h = String::from("SELECT ");
        h.push_str(CAND_COLS);
        h.push_str(", -bm25(fts, 8.0, 4.0, 3.0, 1.0, 1.5, 10.0) AS s FROM fts JOIN messages m ON m.rowid = fts.rowid JOIN threads t ON t.seq = m.thread_seq");
        for r in run(
            s,
            &h,
            "fts MATCH ?1",
            order,
            m.clone().into(),
            c,
            inp.as_of,
            inp.include_quarantined,
        )? {
            order_rows.push(r.rowid);
            by_row.insert(r.rowid, r);
        }
    }
    if !c.ref_like.is_empty() {
        let refs = serde_json::to_string(&c.ref_like).unwrap_or_default();
        for r in run(
            s,
            &head_m,
            "EXISTS (SELECT 1 FROM refs rf WHERE rf.message_rowid = m.rowid AND rf.value IN (SELECT value FROM json_each(?1)))",
            " ORDER BY msg_date DESC, m.rowid DESC LIMIT 1000",
            refs.into(),
            c,
            inp.as_of,
            inp.include_quarantined,
        )? {
            match by_row.get_mut(&r.rowid) {
                Some(x) => x.ref_hit = true,
                None => {
                    order_rows.push(r.rowid);
                    let mut r = r;
                    r.ref_hit = true;
                    by_row.insert(r.rowid, r);
                }
            }
        }
    }
    if !textual || (c.fts_match.is_none() && c.ref_like.is_empty()) {
        for r in run(
            s,
            &head_m,
            "?1 = ?1",
            " ORDER BY msg_date DESC, m.rowid DESC LIMIT 5000",
            1i64.into(),
            c,
            inp.as_of,
            inp.include_quarantined,
        )? {
            order_rows.push(r.rowid);
            by_row.insert(r.rowid, r);
        }
    }
    let mut cands: Vec<Cand> = order_rows
        .into_iter()
        .filter_map(|r| by_row.remove(&r))
        .map(|mut x| {
            x.kw = kw_score(x.s, x.ref_hit, textual);
            x
        })
        .collect();
    cands.sort_by(|a, b| {
        b.kw.total_cmp(&a.kw)
            .then(b.msg_date.cmp(&a.msg_date))
            .then(b.rowid.cmp(&a.rowid))
    });
    let facet_rows: Vec<i64> = cands
        .iter()
        .take(FACET_SET as usize)
        .map(|x| x.rowid)
        .collect();
    cands.truncate(inp.limit as usize);
    // The trigram fallback (F5).
    if cands.len() < 3
        && let Some(tri) = &c.tri_match
    {
        let mut h = String::from("SELECT ");
        h.push_str(CAND_COLS);
        h.push_str(", -bm25(fts_tri, 8.0, 4.0, 10.0) AS s FROM fts_tri JOIN messages m ON m.rowid = fts_tri.rowid JOIN threads t ON t.seq = m.thread_seq");
        let leaves: Vec<String> = c
            .terms
            .iter()
            .map(|t| t.0.to_lowercase())
            .filter(|t| t.chars().count() >= 3)
            .collect();
        for r in run(
            s,
            &h,
            "fts_tri MATCH ?1",
            " ORDER BY s DESC LIMIT 200",
            tri.clone().into(),
            c,
            inp.as_of,
            inp.include_quarantined,
        )? {
            if cands.iter().any(|x| x.rowid == r.rowid) {
                continue;
            }
            let hay = haystack(s, &r)?;
            let words: Vec<String> = hay
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|w| w.len() >= 2)
                .map(str::to_owned)
                .collect();
            let ok = leaves.iter().all(|leaf| {
                hay.contains(leaf.as_str()) || {
                    let lt = trigrams(leaf);
                    words.iter().any(|w| jaccard(&lt, &trigrams(w)) >= 0.45)
                }
            });
            if ok {
                let mut r = r;
                r.kw = 0.5 * r.s / (r.s + 4.0);
                r.fuzzy = true;
                cands.push(r);
            }
        }
    }
    let facets = match &inp.months {
        Some(m) => Some(facets(s, &facet_rows, m)?),
        None => None,
    };
    Ok(KeywordOut { cands, facets })
}

/// The subject, participants and references of a message, lower case.
fn haystack<S: Sql>(s: &S, r: &Cand) -> PResult<String> {
    #[derive(Deserialize)]
    struct H {
        to_json: String,
        cc_json: String,
    }
    #[derive(Deserialize)]
    struct V {
        value: String,
    }
    let h: Option<H> = s.first(
        "SELECT to_json, cc_json FROM messages WHERE rowid = ?1",
        &[r.rowid.into()],
    )?;
    let refs: Vec<V> = s.query(
        "SELECT value FROM refs WHERE message_rowid = ?1",
        &[r.rowid.into()],
    )?;
    let mut out = String::new();
    for part in [
        r.subject.as_deref(),
        r.from_address.as_deref(),
        r.from_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        out.push_str(part);
        out.push(' ');
    }
    if let Some(h) = h {
        out.push_str(&h.to_json);
        out.push(' ');
        out.push_str(&h.cc_json);
    }
    for v in refs {
        out.push(' ');
        out.push_str(&v.value);
    }
    Ok(out.to_lowercase())
}

/// The `CASE` that names the attachment class of `x`, generated from the class table.
pub fn class_case(x: &str) -> String {
    let mut sql = String::from("CASE");
    for class in AttachmentClass::ALL {
        for rule in class.mime_rules() {
            sql.push_str(" WHEN ");
            sql.push_str(x);
            sql.push_str(if rule.contains('%') {
                " LIKE '"
            } else {
                " = '"
            });
            sql.push_str(rule);
            sql.push_str("' THEN '");
            sql.push_str(class.as_str());
            sql.push('\'');
        }
    }
    sql.push_str(" ELSE 'other' END");
    sql
}

#[derive(Deserialize)]
struct Vn {
    v: Option<String>,
    n: i64,
}

fn counts(rows: Vec<Vn>) -> Value {
    let mut o = serde_json::Map::new();
    for r in rows {
        if let Some(v) = r.v {
            o.insert(v, r.n.into());
        }
    }
    Value::Object(o)
}

/// The six facets over `rowids` ([Search § 5.7]).
pub fn facets<S: Sql>(s: &S, rowids: &[i64], months: &[(String, i64, i64)]) -> PResult<Value> {
    let ids: SqlValue = serde_json::to_string(rowids).unwrap_or_default().into();
    let q = |sql: &str, extra: Vec<SqlValue>| -> PResult<Value> {
        let mut p = vec![ids.clone()];
        p.extend(extra);
        Ok(counts(s.query(sql, &p)?))
    };
    let sender = q(
        "SELECT m.from_address AS v, COUNT(*) AS n FROM messages m WHERE m.rowid IN (SELECT value FROM json_each(?1)) AND m.from_address IS NOT NULL GROUP BY v ORDER BY n DESC, v ASC LIMIT 10",
        vec![],
    )?;
    let domain = q(
        "SELECT m.sender_domain AS v, COUNT(*) AS n FROM messages m WHERE m.rowid IN (SELECT value FROM json_each(?1)) AND m.sender_domain IS NOT NULL GROUP BY v ORDER BY n DESC, v ASC LIMIT 10",
        vec![],
    )?;
    let category = q(
        "SELECT json_extract(m.triage_json,'$.category') AS v, COUNT(*) AS n FROM messages m WHERE m.rowid IN (SELECT value FROM json_each(?1)) AND json_extract(m.triage_json,'$.category') IS NOT NULL GROUP BY v ORDER BY n DESC, v ASC LIMIT 10",
        vec![],
    )?;
    let label = q(
        "SELECT l.label AS v, COUNT(DISTINCT l.message_rowid) AS n FROM labels l WHERE l.message_rowid IN (SELECT value FROM json_each(?1)) GROUP BY v ORDER BY n DESC, v ASC LIMIT 10",
        vec![],
    )?;
    let mut att = String::from("SELECT ");
    att.push_str(&class_case("COALESCE(a.sniffed_type, a.content_type)"));
    att.push_str(" AS v, COUNT(DISTINCT a.message_rowid) AS n FROM attachments a WHERE a.message_rowid IN (SELECT value FROM json_each(?1)) AND COALESCE(a.disposition,'attachment') = 'attachment' GROUP BY v ORDER BY n DESC, v ASC LIMIT 10");
    let attachment_type = q(&att, vec![])?;
    let buckets: Vec<Value> = months.iter().map(|(l, a, b)| json!([l, a, b])).collect();
    let month = q(
        "SELECT json_extract(b.value,'$[0]') AS v, COUNT(*) AS n FROM messages m JOIN json_each(?2) b ON MIN(COALESCE(m.sent_at, m.received_at), m.received_at + 300000) >= json_extract(b.value,'$[1]') AND MIN(COALESCE(m.sent_at, m.received_at), m.received_at + 300000) < json_extract(b.value,'$[2]') WHERE m.rowid IN (SELECT value FROM json_each(?1)) GROUP BY v ORDER BY v DESC LIMIT 24",
        vec![Value::Array(buckets).to_string().into()],
    )?;
    Ok(
        json!({ "sender": sender, "sender_domain": domain, "month": month, "label": label,
               "attachment_type": attachment_type, "category": category }),
    )
}

/// The semantic read-back ([Search § 8] step 5): every filter, visibility, no text clause.
pub fn read_back<S: Sql>(s: &S, inp: &ReadBackIn) -> PResult<Vec<Cand>> {
    let mut h = String::from("SELECT ");
    h.push_str(CAND_COLS);
    h.push_str(", 0.0 AS s, c.vector_id AS vector_id FROM chunks c JOIN messages m ON m.rowid = c.message_rowid JOIN threads t ON t.seq = m.thread_seq");
    run(
        s,
        &h,
        "c.vector_id IN (SELECT value FROM json_each(?1))",
        "",
        serde_json::to_string(&inp.vector_ids)
            .unwrap_or_default()
            .into(),
        &inp.c,
        inp.as_of,
        inp.include_quarantined,
    )
}

#[derive(Deserialize)]
struct Body {
    subject: Option<String>,
    extracted_text: Option<String>,
    text: Option<String>,
    snippet: Option<String>,
}

#[derive(Deserialize)]
struct RefHit {
    value: String,
    source: Option<String>,
}

#[derive(Deserialize)]
struct Att {
    id: String,
    filename: Option<String>,
    text_status: String,
}

#[derive(Deserialize)]
struct One {
    #[allow(dead_code)]
    x: i64,
}

fn location(source: Option<&str>) -> (String, Option<(String, Option<u32>)>) {
    match source {
        Some(src) if src.starts_with("att:") => {
            let mut it = src[4..].split(':');
            let id = it.next().unwrap_or("").to_owned();
            let page = it.next().and_then(|p| p.parse().ok());
            let label = match page {
                Some(p) => format!("attachment p.{p}"),
                None => "attachment".to_owned(),
            };
            (label, Some((id, page)))
        }
        Some("subject") => ("subject".into(), None),
        _ => ("body".into(), None),
    }
}

/// Snippets, `why` and attachment hits of the page's hits ([Search § 5.5, § 5.6]).
pub fn details<S: Sql>(s: &S, inp: &DetailsIn) -> PResult<Vec<Detail>> {
    let terms = inp.c.term_patterns();
    let chars = inp.snippet_chars as usize;
    let refs: SqlValue = serde_json::to_string(&inp.c.ref_like)
        .unwrap_or_default()
        .into();
    let groups = [
        ("{subject}", "subject"),
        ("{participants}", "participants"),
        ("{body_new body_full}", "body"),
        ("{attachments}", "attachment"),
    ];
    let mut out = Vec::new();
    for (rowid, vector_id) in &inp.items {
        let Some(b) = s.first::<Body>(
            "SELECT subject, extracted_text, text, snippet FROM messages WHERE rowid = ?1",
            &[(*rowid).into()],
        )?
        else {
            continue;
        };
        let mut why: Vec<String> = Vec::new();
        let mut attachment_hits: Vec<Value> = Vec::new();
        let atts: Vec<Att> = s.query(
            "SELECT id, filename, text_status FROM attachments WHERE message_rowid = ?1 ORDER BY rowid",
            &[(*rowid).into()],
        )?;
        if !inp.c.ref_like.is_empty() {
            let hits: Vec<RefHit> = s.query(
                "SELECT value, source FROM refs WHERE message_rowid = ?1 AND value IN (SELECT value FROM json_each(?2))",
                &[(*rowid).into(), refs.clone()],
            )?;
            for h in hits {
                let (label, att) = location(h.source.as_deref());
                why.push(format!("ref:{} ({label})", h.value));
                if let Some((id, page)) = att
                    && !attachment_hits
                        .iter()
                        .any(|a| a["attachment_id"] == id.as_str())
                {
                    let filename = atts
                        .iter()
                        .find(|a| a.id == id)
                        .and_then(|a| a.filename.clone());
                    attachment_hits
                        .push(json!({ "attachment_id": id, "filename": filename, "page": page }));
                }
            }
        }
        if let Some(m) = &inp.c.fts_match {
            for (g, label) in groups {
                let mut expr = String::from(g);
                expr.push_str(" : (");
                expr.push_str(m);
                expr.push(')');
                let hit: Option<One> = s
                    .first(
                        "SELECT rowid AS x FROM fts WHERE fts MATCH ?1 AND rowid = ?2",
                        &[expr.into(), (*rowid).into()],
                    )
                    .unwrap_or(None);
                if hit.is_some() {
                    for t in &terms {
                        why.push(format!("text:\"{}\" ({label})", t.text));
                    }
                    if label == "attachment" {
                        for a in atts.iter().filter(|a| a.text_status == "ready") {
                            if !attachment_hits
                                .iter()
                                .any(|x| x["attachment_id"] == a.id.as_str())
                            {
                                attachment_hits.push(json!({ "attachment_id": a.id, "filename": a.filename, "page": null }));
                            }
                        }
                    }
                }
            }
        }
        why.extend(inp.c.why_filters.iter().cloned());
        if inp.c.attachment_query && atts.iter().any(|a| a.text_status == "unavailable") {
            why.push("attachment_text_unavailable".into());
        }
        // The snippet: the first source that holds a term, else the best chunk, else the stored one.
        let sources = [
            b.extracted_text.as_deref(),
            b.subject.as_deref(),
            b.text.as_deref(),
        ];
        let snip = match sources
            .iter()
            .flatten()
            .find(|t| !terms.is_empty() && snippet::contains_any(t, &terms))
        {
            Some(t) => snippet::snippet(t, &terms, chars),
            None => {
                let chunk = vector_id
                    .as_ref()
                    .and_then(|v| chunk_text(s, v, b.extracted_text.as_deref()).ok().flatten());
                match chunk {
                    Some(t) => snippet::snippet(&t, &[], chars),
                    None => snippet::snippet(
                        b.snippet
                            .as_deref()
                            .or(b.extracted_text.as_deref())
                            .unwrap_or(""),
                        &[],
                        chars,
                    ),
                }
            }
        };
        out.push(Detail {
            rowid: *rowid,
            snippet: snip,
            why,
            attachment_hits,
        });
    }
    Ok(out)
}

/// The body text of a chunk, cut from `extracted_text` by its character range.
fn chunk_text<S: Sql>(s: &S, vector_id: &str, extracted: Option<&str>) -> PResult<Option<String>> {
    #[derive(Deserialize)]
    struct C {
        attachment_id: Option<String>,
        char_start: i64,
        char_end: i64,
    }
    let c: Option<C> = s.first(
        "SELECT attachment_id, char_start, char_end FROM chunks WHERE vector_id = ?1",
        &[vector_id.into()],
    )?;
    Ok(match (c, extracted) {
        (Some(c), Some(t)) if c.attachment_id.is_none() => Some(
            t.chars()
                .skip(c.char_start.max(0) as usize)
                .take((c.char_end - c.char_start).max(0) as usize)
                .collect(),
        ),
        _ => None,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    pub eligible: i64,
    pub covered: i64,
}

/// `semantic_coverage` counts ([Search § 6.5]).
pub fn coverage<S: Sql>(s: &S, as_of: i64, model: &str) -> PResult<Coverage> {
    #[derive(Deserialize)]
    struct R {
        eligible: i64,
        covered: Option<i64>,
    }
    let r: Option<R> = s.first(
        "SELECT COUNT(*) AS eligible, SUM(CASE WHEN EXISTS (SELECT 1 FROM chunks c WHERE c.message_rowid = m.rowid AND c.status <> 'deleting') AND NOT EXISTS (SELECT 1 FROM chunks c WHERE c.message_rowid = m.rowid AND (c.status IN ('pending','failed') OR (c.status = 'embedded' AND c.model <> ?2))) THEN 1 WHEN NOT EXISTS (SELECT 1 FROM chunks c WHERE c.message_rowid = m.rowid) AND COALESCE(length(m.extracted_text), 0) = 0 AND COALESCE(length(m.subject), 0) = 0 THEN 1 ELSE 0 END) AS covered FROM messages m WHERE m.status NOT IN ('hidden','throttled','quarantined','screening') AND m.received_at <= ?1",
        &[as_of.into(), model.into()],
    )?;
    Ok(r.map_or_else(Coverage::default, |r| Coverage {
        eligible: r.eligible,
        covered: r.covered.unwrap_or(0),
    }))
}

/// The thread rows of `group_by: "thread"`.
pub fn threads<S: Sql>(s: &S, seqs: &[i64]) -> PResult<Vec<Value>> {
    s.query(
        "SELECT seq, id, subject, participants_json, message_count, last_at FROM threads WHERE seq IN (SELECT value FROM json_each(?1))",
        &[serde_json::to_string(seqs).unwrap_or_default().into()],
    )
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContactsIn {
    pub q: String,
    pub limit: u32,
    pub review: bool,
    /// The boundary `(match_rank, weight, last_seen_at, address)` of the previous page.
    pub after: Option<(i64, i64, i64, String)>,
}

/// Contacts search ([Search § 12.1]).
pub fn contacts<S: Sql>(s: &S, inp: &ContactsIn) -> PResult<Vec<Value>> {
    let (r, w, l, a) = inp
        .after
        .clone()
        .map_or((None, None, None, None), |(r, w, l, a)| {
            (Some(r), Some(w), Some(l), Some(a))
        });
    s.query(
        "SELECT * FROM (SELECT c.address, c.name, c.domain, c.first_seen_at, c.last_seen_at, c.inbound_count, c.outbound_count, t.id AS last_thread_id, (c.inbound_count + 2 * c.outbound_count) AS weight, \
           CASE WHEN ?1 = '' THEN 0 WHEN c.address = ?1 THEN 3 WHEN c.address >= ?1 AND c.address < ?1 || char(1114111) THEN 2 \
                WHEN instr(lower(COALESCE(c.name,'')), ?1) = 1 OR instr(lower(COALESCE(c.name,'')), ' ' || ?1) > 0 THEN 2 \
                WHEN c.domain = ?1 OR substr(c.domain, -length(?1) - 1) = '.' || ?1 THEN 1 ELSE -1 END AS match_rank \
         FROM contacts c LEFT JOIN threads t ON t.seq = c.last_thread_seq \
         WHERE (?3 = 1 OR c.outbound_count > 0 OR EXISTS (SELECT 1 FROM messages m WHERE m.from_address = c.address AND m.status = 'received'))) \
         WHERE match_rank >= 0 AND (?4 IS NULL OR (match_rank, weight, last_seen_at, ?7) < (?4, ?5, ?6, address) ) \
         ORDER BY match_rank DESC, weight DESC, last_seen_at DESC, address ASC LIMIT ?2",
        &[inp.q.clone().into(), (inp.limit + 1).into(), bool_v(inp.review), r.into(), w.into(), l.into(), a.into()],
    )
}

// ---- Index bookkeeping ([Search § 6.3, § 6.4, § 6.6]) ----------------------------------------------

/// What an `Embed` job needs of a message.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexSource {
    pub skip: bool,
    pub subject: String,
    pub text: String,
    pub sender_domain: Option<String>,
    pub direction: String,
    pub verdict: Option<String>,
    pub thread_id: String,
    /// The clamped message date in Unix seconds.
    pub sent_at: i64,
    pub has_attachment: bool,
    /// `(attachment id, position, text_r2_key)` of attachments whose text is ready.
    pub attachments: Vec<(String, i64, String)>,
}

pub fn index_source<S: Sql>(s: &S, message_id: &str) -> PResult<IndexSource> {
    #[derive(Deserialize)]
    struct M {
        rowid: i64,
        status: String,
        subject: Option<String>,
        extracted_text: Option<String>,
        text: Option<String>,
        sender_domain: Option<String>,
        direction: String,
        verdict: Option<String>,
        thread_id: String,
        msg_date: i64,
    }
    #[derive(Deserialize)]
    struct A {
        id: String,
        text_status: String,
        text_r2_key: Option<String>,
        disposition: Option<String>,
    }
    let erased: Option<One> = s.first(
        "SELECT 1 AS x FROM erased_ids WHERE message_id = ?1",
        &[message_id.into()],
    )?;
    let m: Option<M> = s.first(
        "SELECT m.rowid AS rowid, m.status, m.subject, m.extracted_text, m.text, m.sender_domain, m.direction, m.verdict, t.id AS thread_id, MIN(COALESCE(m.sent_at, m.received_at), m.received_at + 300000) AS msg_date FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE m.id = ?1",
        &[message_id.into()],
    )?;
    let Some(m) = m.filter(|m| {
        erased.is_none()
            && !matches!(
                m.status.as_str(),
                "quarantined" | "screening" | "hidden" | "throttled"
            )
    }) else {
        return Ok(IndexSource {
            skip: true,
            ..IndexSource::default()
        });
    };
    let atts: Vec<A> = s.query(
        "SELECT id, text_status, text_r2_key, disposition FROM attachments WHERE message_rowid = ?1 ORDER BY rowid",
        &[m.rowid.into()],
    )?;
    let text = match m.extracted_text.filter(|t| !t.is_empty()) {
        Some(t) => t,
        None => m.text.unwrap_or_default().chars().take(65_536).collect(),
    };
    Ok(IndexSource {
        skip: false,
        subject: m.subject.unwrap_or_default(),
        text,
        sender_domain: m.sender_domain,
        direction: m.direction,
        verdict: m.verdict,
        thread_id: m.thread_id,
        sent_at: m.msg_date / 1000,
        has_attachment: atts
            .iter()
            .any(|a| a.disposition.as_deref() != Some("inline")),
        attachments: atts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.text_status == "ready")
            .filter_map(|(k, a)| Some((a.id.clone(), k as i64, a.text_r2_key.clone()?)))
            .collect(),
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkRow {
    pub vector_id: String,
    pub attachment_id: Option<String>,
    pub ordinal: i64,
    pub page: Option<i64>,
    pub char_start: i64,
    pub char_end: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PutChunksOut {
    pub erased: bool,
    pub embed: Vec<String>,
    pub delete: Vec<String>,
}

fn message_rowid<S: Sql>(s: &S, message_id: &str) -> PResult<Option<i64>> {
    #[derive(Deserialize)]
    struct R {
        rowid: i64,
    }
    let erased: Option<One> = s.first(
        "SELECT 1 AS x FROM erased_ids WHERE message_id = ?1",
        &[message_id.into()],
    )?;
    if erased.is_some() {
        return Ok(None);
    }
    Ok(s.first::<R>(
        "SELECT rowid FROM messages WHERE id = ?1",
        &[message_id.into()],
    )?
    .map(|r| r.rowid))
}

/// `index.put_chunks`: new rows `pending`, identical embedded rows kept, rows no longer produced
/// `deleting`. Answers the IDs to embed and to delete.
pub fn put_chunks<S: Sql>(
    s: &S,
    message_id: &str,
    chunks: &[ChunkRow],
    model: &str,
    now: i64,
) -> PResult<PutChunksOut> {
    let Some(rowid) = message_rowid(s, message_id)? else {
        return Ok(PutChunksOut {
            erased: true,
            ..PutChunksOut::default()
        });
    };
    #[derive(Deserialize)]
    struct Ex {
        vector_id: String,
        char_start: i64,
        char_end: i64,
        model: String,
        status: String,
    }
    let existing: Vec<Ex> = s.query(
        "SELECT vector_id, char_start, char_end, model, status FROM chunks WHERE message_rowid = ?1",
        &[rowid.into()],
    )?;
    let mut out = PutChunksOut::default();
    for c in chunks {
        let same = existing.iter().find(|e| e.vector_id == c.vector_id);
        match same {
            Some(e)
                if e.status == "embedded"
                    && e.char_start == c.char_start
                    && e.char_end == c.char_end
                    && e.model == model => {}
            _ => {
                s.exec(
                    "INSERT INTO chunks (vector_id, message_rowid, attachment_id, ordinal, page, char_start, char_end, model, status, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9) \
                     ON CONFLICT (vector_id) DO UPDATE SET attachment_id = excluded.attachment_id, ordinal = excluded.ordinal, page = excluded.page, char_start = excluded.char_start, char_end = excluded.char_end, model = excluded.model, status = 'pending', updated_at = excluded.updated_at",
                    &[c.vector_id.clone().into(), rowid.into(), c.attachment_id.clone().into(), c.ordinal.into(), c.page.into(), c.char_start.into(), c.char_end.into(), model.into(), now.into()],
                )?;
                out.embed.push(c.vector_id.clone());
            }
        }
    }
    for e in existing
        .iter()
        .filter(|e| !chunks.iter().any(|c| c.vector_id == e.vector_id))
    {
        s.exec(
            "UPDATE chunks SET status = 'deleting', updated_at = ?2 WHERE vector_id = ?1",
            &[e.vector_id.clone().into(), now.into()],
        )?;
        out.delete.push(e.vector_id.clone());
    }
    for e in existing.iter().filter(|e| e.status == "deleting") {
        if !out.delete.contains(&e.vector_id) {
            out.delete.push(e.vector_id.clone());
        }
    }
    Ok(out)
}

/// `index.mark`: `false` when the message was erased meanwhile (the writer deletes what it upserted).
pub fn mark<S: Sql>(
    s: &S,
    message_id: &str,
    ids: &[String],
    status: &str,
    model: &str,
    now: i64,
) -> PResult<bool> {
    if message_rowid(s, message_id)?.is_none() {
        return Ok(false);
    }
    s.exec(
        "UPDATE chunks SET status = ?2, model = ?3, updated_at = ?4 WHERE vector_id IN (SELECT value FROM json_each(?1)) AND status <> 'deleting'",
        &[serde_json::to_string(ids).unwrap_or_default().into(), status.into(), model.into(), now.into()],
    )?;
    Ok(true)
}

pub fn drop_deleting<S: Sql>(s: &S, ids: &[String]) -> PResult<()> {
    s.exec(
        "DELETE FROM chunks WHERE status = 'deleting' AND vector_id IN (SELECT value FROM json_each(?1))",
        &[serde_json::to_string(ids).unwrap_or_default().into()],
    )?;
    Ok(())
}

/// Every vector ID of a message (cancel, erasure).
pub fn vector_ids<S: Sql>(s: &S, message_id: &str) -> PResult<Vec<String>> {
    #[derive(Deserialize)]
    struct V {
        vector_id: String,
    }
    Ok(s
        .query::<V>(
            "SELECT c.vector_id FROM chunks c JOIN messages m ON m.rowid = c.message_rowid WHERE m.id = ?1",
            &[message_id.into()],
        )?
        .into_iter()
        .map(|v| v.vector_id)
        .collect())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReconcileOut {
    /// Messages with `pending` rows older than an hour, or `failed` rows.
    pub retry: Vec<String>,
    /// Eligible messages with text and no chunk rows, older than an hour.
    pub missing: Vec<String>,
    /// `(vector_id, message_id)` of embedded rows updated in the last 26 hours.
    pub sample: Vec<(String, String)>,
    pub embedded_rows: i64,
    pub pending_rows: i64,
    pub failed_rows: i64,
}

pub fn reconcile<S: Sql>(s: &S, now: i64) -> PResult<ReconcileOut> {
    #[derive(Deserialize)]
    struct I {
        id: String,
    }
    #[derive(Deserialize)]
    struct Sm {
        vector_id: String,
        id: String,
    }
    #[derive(Deserialize)]
    struct Cn {
        e: i64,
        p: i64,
        f: i64,
    }
    let hour = now - 3_600_000;
    let retry: Vec<I> = s.query(
        "SELECT DISTINCT m.id FROM chunks c JOIN messages m ON m.rowid = c.message_rowid WHERE (c.status = 'pending' AND c.updated_at < ?1) OR c.status = 'failed' LIMIT 500",
        &[hour.into()],
    )?;
    let missing: Vec<I> = s.query(
        "SELECT m.id FROM messages m WHERE m.status NOT IN ('hidden','throttled','quarantined','screening') AND m.received_at < ?1 AND NOT EXISTS (SELECT 1 FROM chunks c WHERE c.message_rowid = m.rowid) AND (COALESCE(length(m.extracted_text), 0) > 0 OR COALESCE(length(m.subject), 0) > 0) LIMIT 500",
        &[hour.into()],
    )?;
    let sample: Vec<Sm> = s.query(
        "SELECT c.vector_id, m.id FROM chunks c JOIN messages m ON m.rowid = c.message_rowid WHERE c.status = 'embedded' AND c.updated_at > ?1 ORDER BY c.updated_at DESC LIMIT 200",
        &[(now - 26 * 3_600_000).into()],
    )?;
    let n: Option<Cn> = s.first(
        "SELECT COALESCE(SUM(status = 'embedded'), 0) AS e, COALESCE(SUM(status = 'pending'), 0) AS p, COALESCE(SUM(status = 'failed'), 0) AS f FROM chunks",
        &[],
    )?;
    let n = n.unwrap_or(Cn { e: 0, p: 0, f: 0 });
    Ok(ReconcileOut {
        retry: retry.into_iter().map(|i| i.id).collect(),
        missing: missing.into_iter().map(|i| i.id).collect(),
        sample: sample.into_iter().map(|x| (x.vector_id, x.id)).collect(),
        embedded_rows: n.e,
        pending_rows: n.p,
        failed_rows: n.f,
    })
}

/// Rows a reconciliation found missing in Vectorize go back to `pending`.
pub fn mark_pending<S: Sql>(s: &S, ids: &[String], now: i64) -> PResult<()> {
    s.exec(
        "UPDATE chunks SET status = 'pending', updated_at = ?2 WHERE vector_id IN (SELECT value FROM json_each(?1))",
        &[serde_json::to_string(ids).unwrap_or_default().into(), now.into()],
    )?;
    Ok(())
}

/// The source of `find related` ([Search § 12.2]): thread, body chunks, references, subject words.
pub fn related_source<S: Sql>(s: &S, message_id: &str, include: bool) -> PResult<Option<Value>> {
    #[derive(Deserialize)]
    struct M {
        rowid: i64,
        thread_id: String,
        thread_seq: i64,
        subject: Option<String>,
    }
    #[derive(Deserialize)]
    struct V {
        v: String,
    }
    let m: Option<M> = s.first(
        "SELECT m.rowid AS rowid, t.id AS thread_id, m.thread_seq AS thread_seq, m.subject FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE m.id = ?1 AND m.status NOT IN ('hidden','throttled') AND (m.status NOT IN ('quarantined','screening') OR ?2 = 1)",
        &[message_id.into(), bool_v(include)],
    )?;
    let Some(m) = m else { return Ok(None) };
    let chunks: Vec<V> = s.query(
        "SELECT vector_id AS v FROM chunks WHERE message_rowid = ?1 AND attachment_id IS NULL AND status = 'embedded' ORDER BY ordinal LIMIT 3",
        &[m.rowid.into()],
    )?;
    let refs: Vec<V> = s.query(
        "SELECT DISTINCT value AS v FROM refs WHERE message_rowid = ?1 LIMIT 5",
        &[m.rowid.into()],
    )?;
    Ok(Some(json!({
        "rowid": m.rowid, "thread_id": m.thread_id, "thread_seq": m.thread_seq, "subject": m.subject,
        "chunks": chunks.into_iter().map(|v| v.v).collect::<Vec<_>>(),
        "refs": refs.into_iter().map(|v| v.v).collect::<Vec<_>>(),
    })))
}

/// The reference values of messages (`rowid → values`), for the related boost.
pub fn refs_of<S: Sql>(s: &S, rowids: &[i64]) -> PResult<Vec<(i64, String)>> {
    #[derive(Deserialize)]
    struct R {
        message_rowid: i64,
        value: String,
    }
    Ok(s
        .query::<R>(
            "SELECT DISTINCT message_rowid, value FROM refs WHERE message_rowid IN (SELECT value FROM json_each(?1))",
            &[serde_json::to_string(rowids).unwrap_or_default().into()],
        )?
        .into_iter()
        .map(|r| (r.message_rowid, r.value))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_and_classes() {
        assert_eq!(kw_score(0.0, false, false), 1.0);
        assert!((kw_score(4.0, true, true) - (0.35 + 0.3)).abs() < 1e-9);
        assert!(class_case("x").contains("WHEN x = 'application/pdf' THEN 'pdf'"));
        assert!(class_case("x").contains("WHEN x LIKE 'image/%' THEN 'image'"));
        let a = trigrams("plate");
        assert!(jaccard(&a, &trigrams("plates")) >= 0.45);
        assert!(jaccard(&a, &trigrams("zebra")) < 0.45);
    }
}
