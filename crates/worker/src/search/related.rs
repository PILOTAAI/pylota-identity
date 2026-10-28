//! Find related ([Search § 12.2]): the nearest neighbours of the source message's first three body
//! chunks outside its thread, boosted by shared references, read back with visibility. Without
//! embedded chunks, or with Vectorize down, a keyword search over the source's references and subject
//! words outside its thread answers instead.
//!
//! [Search § 12.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use std::collections::HashMap;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::{Clock as _, Platform, VectorIndex as _};
use serde::Deserialize;
use serde_json::{Value, json};

use super::wire::{Cand, Detail};
use super::{Fan, Item, Mode, Params, Plan, Scope, join, message_hit, plan, round3, run};
use crate::errors::ApiError;
use crate::mailbox::search::{DetailsIn, ReadBackIn, SearchOp};

const TOP_K: u32 = 50;
const TIMEOUT_MS: u32 = 1_000;
const WORDS: usize = 5;
const STOP: &[&str] = &[
    "about", "after", "again", "from", "have", "hello", "just", "please", "re", "fwd", "that",
    "their", "there", "these", "this", "with", "your", "will", "would",
];

#[derive(Deserialize)]
struct Source {
    rowid: i64,
    thread_id: String,
    subject: Option<String>,
    chunks: Vec<String>,
    refs: Vec<String>,
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// The fallback query: the top references and up to 5 distinctive subject words, OR-ed, outside the
/// source thread. `None` when there is nothing to search for.
pub fn fallback_query(refs: &[String], subject: &str, thread_id: &str) -> Option<String> {
    let mut words: Vec<String> = subject
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 4 && !STOP.contains(&w.as_str()))
        .collect();
    words.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then(a.cmp(b)));
    words.dedup();
    words.truncate(WORDS);
    let mut leaves: Vec<String> = refs
        .iter()
        .take(5)
        .map(|r| format!("ref:{}", quote(r)))
        .collect();
    leaves.extend(words.iter().map(|w| quote(w)));
    if leaves.is_empty() {
        return None;
    }
    Some(format!("({}) -thread:{thread_id}", leaves.join(" OR ")))
}

/// `GET …/messages/{message_id}/related`: `{ data: [SearchHit] }`.
#[allow(clippy::too_many_arguments)]
pub async fn related<P: Platform>(
    p: &P,
    scope: Scope,
    message_id: &str,
    limit: u32,
    review: bool,
    key_id: &str,
    request_id: &str,
    safe_why: bool,
) -> Result<Value, ApiError> {
    let params = |q: String| Params {
        q,
        mode: Mode::Keyword,
        thread: false,
        limit,
        snippet_chars: 240,
        facets: false,
        include_quarantined: false,
        review,
        require_mode: false,
        cursor: None,
        direction: None,
        labels: Vec::new(),
        after_ms: None,
        before_ms: None,
        key_id: key_id.to_owned(),
        request_id: request_id.to_owned(),
        safe_why,
    };
    let base = plan(p, scope, params(String::new())).await?;
    let fan = Fan::new(p, &base);
    let src: Vec<Option<Option<Source>>> = fan
        .each(vec![Some(SearchOp::RelatedSource {
            message_id: message_id.to_owned(),
            include_quarantined: false,
        })])
        .await?;
    let Some(src) = src.into_iter().next().flatten().flatten() else {
        return Err(ApiError::not_found(ErrorCode::MessageNotFound));
    };
    let semantic = if src.chunks.is_empty() {
        None
    } else {
        neighbours(&fan, &src).await?
    };
    let (items, shared) = match semantic {
        Some(x) => x,
        None => {
            // The keyword fallback over a fresh plan in the same scope.
            let Some(q) = fallback_query(
                &src.refs,
                src.subject.as_deref().unwrap_or(""),
                &src.thread_id,
            ) else {
                return Ok(json!({ "data": [] }));
            };
            let Plan { scope, .. } = base;
            let kw = plan(p, scope, params(q)).await?;
            let body = run(p, &kw).await?;
            let hits: Vec<Value> = body["hits"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|h| h["message_id"] != message_id)
                .collect();
            return Ok(json!({ "data": hits }));
        }
    };
    let mut items = items;
    items.retain(|i| i.c.rowid != src.rowid);
    items.truncate(limit as usize);
    let rowids: Vec<(i64, Option<String>)> = items
        .iter()
        .map(|i| (i.c.rowid, i.c.vector_id.clone()))
        .collect();
    let details: Vec<Option<Vec<Detail>>> = fan
        .each(vec![(!rowids.is_empty()).then(|| {
            SearchOp::Details(DetailsIn {
                items: rowids,
                c: base.wire.clone(),
                snippet_chars: 240,
            })
        })])
        .await?;
    let mut by: HashMap<i64, Detail> = details
        .into_iter()
        .flatten()
        .flatten()
        .map(|d| (d.rowid, d))
        .collect();
    let idn = &base.scope.targets[0];
    let data: Vec<Value> = items
        .iter()
        .map(|i| {
            let d = by.remove(&i.c.rowid).unwrap_or_default();
            let mut why = vec![format!("semantic:{:.2}", i.cosine.unwrap_or(0.0))];
            for (v, kind) in shared.get(&i.c.rowid).into_iter().flatten() {
                why.push(if safe_why {
                    pylota_identity_core::injection::why_ref(kind, v)
                } else {
                    format!("ref:{v}")
                });
            }
            why.truncate(super::MAX_WHY);
            let mut h = message_hit(idn, i, d, why);
            h["score"] = round3(i.score.clamp(0.0, 1.0)).into();
            h
        })
        .collect();
    Ok(json!({ "data": data }))
}

/// The neighbours, and the references each shares with the source.
type Neighbours = (Vec<Item>, HashMap<i64, Vec<(String, String)>>);

/// Steps 2–5 with Vectorize: `None` when Vectorize did not answer (the fallback runs, degraded).
async fn neighbours<P: Platform>(
    fan: &Fan<'_, P>,
    src: &Source,
) -> Result<Option<Neighbours>, ApiError> {
    let p = fan.p;
    let plan = fan.plan;
    let idn = &plan.scope.targets[0];
    let filter = json!({
        "identity_id": { "$eq": idn.id },
        "thread_id": { "$ne": src.thread_id },
    });
    let queries: Vec<_> = src
        .chunks
        .iter()
        .take(3)
        .map(|id| {
            let filter = &filter;
            async move {
                let q = p
                    .vectors()
                    .query_by_id(&plan.scope.tenant.id, id, TOP_K, filter);
                join::before(q, p.clock().sleep(TIMEOUT_MS)).await
            }
        })
        .collect();
    let mut best: HashMap<String, (String, f64)> = HashMap::new();
    for r in join::bounded(queries, 3).await {
        let Some(Ok(matches)) = r else {
            return Ok(None);
        };
        for m in matches {
            let Some(msg) = super::semantic::message_of(&m.id) else {
                continue;
            };
            let score = f64::from(m.score);
            let e = best.entry(msg.to_owned()).or_insert((m.id.clone(), score));
            if score > e.1 {
                *e = (m.id.clone(), score);
            }
        }
    }
    let cos: HashMap<String, f64> = best.values().cloned().collect();
    let vector_ids: Vec<String> = cos.keys().cloned().collect();
    if vector_ids.is_empty() {
        return Ok(Some((Vec::new(), HashMap::new())));
    }
    let back: Vec<Option<Vec<Cand>>> = fan
        .each(vec![Some(SearchOp::ReadBack(ReadBackIn {
            vector_ids,
            c: plan.wire.clone(),
            as_of: plan.as_of,
            include_quarantined: false,
        }))])
        .await?;
    let cands = back.into_iter().flatten().flatten().collect::<Vec<_>>();
    // Shared references: +0.1 each, at most +0.2.
    let rowids: Vec<i64> = cands.iter().map(|c| c.rowid).collect();
    let refs: Vec<Option<Vec<(i64, String, String)>>> = fan
        .each(vec![
            (!rowids.is_empty()).then_some(SearchOp::RefsOf { rowids }),
        ])
        .await?;
    let mut shared: HashMap<i64, Vec<(String, String)>> = HashMap::new();
    for (rowid, v, kind) in refs.into_iter().flatten().flatten() {
        if src.refs.contains(&v) {
            shared.entry(rowid).or_default().push((v, kind));
        }
    }
    let mut items: Vec<Item> = cands
        .into_iter()
        .filter_map(|c| {
            let cs = *cos.get(c.vector_id.as_ref()?)?;
            let boost = 0.1 * shared.get(&c.rowid).map_or(0, Vec::len).min(2) as f64;
            Some(Item {
                idx: 0,
                ord: plan.ord(0, c.rowid),
                score: cs.max(0.0) + boost,
                cosine: Some(cs),
                rerank: None,
                c,
            })
        })
        .collect();
    items.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.c.msg_date.cmp(&a.c.msg_date))
            .then(b.ord.cmp(&a.ord))
    });
    Ok(Some((items, shared)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_query_shape() {
        let q = fallback_query(
            &["AB12CDE".into()],
            "Re: Brightwell invoice for the plate",
            "thr_X",
        )
        .unwrap();
        assert_eq!(
            q,
            "(ref:\"AB12CDE\" OR \"brightwell\" OR \"invoice\" OR \"plate\") -thread:thr_X"
        );
        assert_eq!(fallback_query(&[], "Re: hi", "thr_X"), None);
    }
}
