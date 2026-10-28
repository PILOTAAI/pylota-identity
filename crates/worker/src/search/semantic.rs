//! The semantic query path ([Search § 8]): embed `semantic_text` (an isolate LRU of 256 query vectors
//! for 10 minutes, `RL_EMBED` asked first), query Vectorize in the tenant namespace with the identity
//! filter and the pre-filters, keep each message's best chunk, and read back from the mailboxes, which
//! re-apply visibility and every filter. Any failure answers `Err(())`: the caller degrades.
//!
//! [Search § 8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use std::cell::RefCell;
use std::collections::HashMap;

use pylota_identity_api_types::ids::crockford_value;
use pylota_identity_core::query::Direction;
use pylota_identity_core::search::chunk::CHUNKER_VERSION;
use pylota_identity_platform::ai::AiOptions;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{Ai as _, Clock as _, Platform, RateLimiter as _, VectorIndex as _};
use serde_json::{Map, Value, json};

use super::wire::Cand;
use super::{Fan, Item, Mode, Ranked, coverage, degraded_error, facets, join};
use crate::errors::ApiError;
use crate::mailbox::search::{ReadBackIn, SearchOp};

pub const TOP_K: u32 = 100;
pub const DIMENSIONS: usize = 1024;
const CACHE_SIZE: usize = 256;
const CACHE_MS: i64 = 10 * 60_000;
const EMBED_TIMEOUT_MS: u32 = 1_000;
const VECTORIZE_TIMEOUT_MS: u32 = 1_000;
/// Identities per Vectorize filter, keeping its JSON under 1,900 bytes.
const IDS_PER_FILTER: usize = 50;

thread_local! {
    static QUERY_VECTORS: RefCell<HashMap<String, (i64, Vec<f32>)>> = RefCell::new(HashMap::new());
}

/// The model that embeds queries: the previous one while a re-embed is running.
pub fn query_model<P: Platform>(p: &P) -> String {
    let c = p.config();
    match (&c.embed_model_previous, p.vectors_next().is_some()) {
        (Some(prev), true) => prev.clone(),
        _ => c.embed_model.clone(),
    }
}

/// `{model_slug}@{chunker_version}` of the generation reads use (`bge-m3@1`).
pub fn model_tag<P: Platform>(p: &P) -> String {
    tag_of(&query_model(p))
}

pub fn tag_of(model: &str) -> String {
    format!(
        "{}@{CHUNKER_VERSION}",
        model.rsplit('/').next().unwrap_or(model)
    )
}

/// The vectors of a `{ shape, data }` embedding answer, each checked to have 1,024 values.
pub fn vectors_of(out: &Value, n: usize) -> Option<Vec<Vec<f32>>> {
    let data = out["data"].as_array()?;
    if data.len() != n {
        return None;
    }
    data.iter()
        .map(|v| {
            let v: Vec<f32> = v
                .as_array()?
                .iter()
                .map(|x| x.as_f64().map(|f| f as f32))
                .collect::<Option<_>>()?;
            (v.len() == DIMENSIONS).then_some(v)
        })
        .collect()
}

/// The query vector, from the cache or Workers AI.
async fn embed_query<P: Platform>(p: &P, tenant_id: &str, text: &str) -> Result<Vec<f32>, ()> {
    let model = query_model(p);
    let key = pylota_identity_core::ses::sha256_hex(format!("{model}\0{text}").as_bytes());
    let now = p.clock().now_ms();
    let hit = QUERY_VECTORS.with(|c| {
        c.borrow()
            .get(&key)
            .filter(|(at, _)| now - at < CACHE_MS)
            .map(|(_, v)| v.clone())
    });
    if let Some(v) = hit {
        return Ok(v);
    }
    // F16: an interactive embedding over the tenant's share is not queued; search degrades.
    if !p
        .limits()
        .allow(RlBucket::Embed, tenant_id)
        .await
        .unwrap_or(false)
    {
        crate::metrics::count(p, "ai_deferred_total", Some(tenant_id), 1);
        return Err(());
    }
    let input = json!({ "text": [text] });
    let opts = AiOptions::content(p.config().ai_gateway.clone());
    let call = p.ai().run::<Value, Value>(&model, &input, &opts);
    let out = match join::before(call, p.clock().sleep(EMBED_TIMEOUT_MS)).await {
        Some(Ok(v)) => v,
        Some(Err(e)) => {
            if pylota_identity_platform::ai::is_rate_limited(&e) {
                crate::metrics::count(p, "ai_rate_limited_total", Some(tenant_id), 1);
            }
            return Err(());
        }
        None => return Err(()),
    };
    let v = vectors_of(&out, 1).and_then(|mut v| v.pop()).ok_or(())?;
    QUERY_VECTORS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= CACHE_SIZE {
            if let Some(oldest) = c
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            {
                c.remove(&oldest);
            }
        }
        c.insert(key, (now, v.clone()));
    });
    Ok(v)
}

/// The Vectorize filter for a group of identities: the compiled pre-filter and the request's
/// `direction`, `after` and `before`.
pub fn filter(plan: &super::Plan, ids: &[String]) -> Value {
    let mut f = Map::new();
    f.insert(
        "identity_id".into(),
        match ids {
            [one] => json!({ "$eq": one }),
            _ => json!({ "$in": ids }),
        },
    );
    let vf = &plan.compiled.vector_prefilter;
    let direction = vf
        .direction
        .map(|d| match d {
            Direction::Inbound => "inbound".to_owned(),
            Direction::Outbound => "outbound".to_owned(),
        })
        .or_else(|| plan.direction.clone());
    if let Some(d) = direction {
        f.insert("direction".into(), json!({ "$eq": d }));
    }
    let secs = |ms: i64| ms.div_euclid(1000);
    let gte = [vf.sent_at_gte, plan.after_ms.map(secs)]
        .into_iter()
        .flatten()
        .max();
    let lt = [
        vf.sent_at_lt,
        plan.before_ms.map(|ms| (ms + 999).div_euclid(1000)),
    ]
    .into_iter()
    .flatten()
    .min();
    let mut range = Map::new();
    if let Some(g) = gte {
        range.insert("$gte".into(), g.into());
    }
    if let Some(l) = lt {
        range.insert("$lt".into(), l.into());
    }
    if !range.is_empty() {
        f.insert("sent_at".into(), Value::Object(range));
    }
    if vf.has_attachment {
        f.insert("has_attachment".into(), json!({ "$eq": true }));
    }
    if let Some(t) = &vf.thread_id {
        f.insert("thread_id".into(), json!({ "$eq": t }));
    }
    if let Some(d) = &vf.sender_domain {
        f.insert("sender_domain".into(), json!({ "$eq": d }));
    }
    Value::Object(f)
}

/// The message ID of a vector ID (`msg_` and a ULID before the first `:`).
pub fn message_of(vector_id: &str) -> Option<&str> {
    let id = vector_id.split(':').next()?;
    let ulid = id.strip_prefix("msg_")?;
    (ulid.len() == 26 && ulid.bytes().all(|b| crockford_value(b).is_some())).then_some(id)
}

/// The semantic leg: messages ordered by their best chunk's cosine, read back with visibility.
/// `score = max(0, cosine)`.
pub async fn semantic_leg<P: Platform>(
    fan: &Fan<'_, P>,
) -> Result<Result<Vec<Item>, ()>, ApiError> {
    let plan = fan.plan;
    let p = fan.p;
    let text = plan.compiled.semantic_text.as_str();
    if text.is_empty() {
        return Ok(Err(()));
    }
    let Ok(vector) = embed_query(p, &plan.scope.tenant.id, text).await else {
        return Ok(Err(()));
    };
    let ids: Vec<String> = plan.scope.targets.iter().map(|t| t.id.clone()).collect();
    if ids.is_empty() {
        return Ok(Ok(Vec::new()));
    }
    let queries: Vec<_> = ids
        .chunks(IDS_PER_FILTER)
        .map(|group| {
            let f = filter(plan, group);
            let vector = &vector;
            async move {
                let q = p.vectors().query(&plan.scope.tenant.id, vector, TOP_K, &f);
                join::before(q, p.clock().sleep(VECTORIZE_TIMEOUT_MS)).await
            }
        })
        .collect();
    let mut matches = Vec::new();
    for r in join::bounded(queries, 8).await {
        match r {
            Some(Ok(m)) => matches.extend(m),
            _ => return Ok(Err(())),
        }
    }
    matches.sort_by(|a, b| b.score.total_cmp(&a.score));
    // Each message's best chunk, in order.
    let mut best: Vec<(String, f64)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for m in matches {
        if let Some(msg) = message_of(&m.id)
            && seen.insert(msg.to_owned())
        {
            best.push((m.id.clone(), f64::from(m.score)));
        }
    }
    best.truncate(TOP_K as usize);
    let cos: HashMap<String, f64> = best.iter().cloned().collect();
    let vector_ids: Vec<String> = best.iter().map(|b| b.0.clone()).collect();
    let ops = plan
        .scope
        .targets
        .iter()
        .map(|_| {
            (!vector_ids.is_empty()).then(|| {
                SearchOp::ReadBack(ReadBackIn {
                    vector_ids: vector_ids.clone(),
                    c: plan.wire.clone(),
                    as_of: plan.as_of,
                    include_quarantined: plan.include_quarantined,
                })
            })
        })
        .collect();
    let outs: Vec<Option<Vec<Cand>>> = fan.each(ops).await?;
    let mut items = Vec::new();
    for (idx, cands) in outs.into_iter().enumerate() {
        for c in cands.unwrap_or_default() {
            let Some(cs) = c.vector_id.as_ref().and_then(|v| cos.get(v)).copied() else {
                continue;
            };
            items.push(Item {
                idx,
                ord: plan.ord(idx, c.rowid),
                c,
                score: cs.max(0.0),
                cosine: Some(cs),
                rerank: None,
            });
        }
    }
    items.sort_by(|a, b| {
        b.cosine
            .unwrap_or(0.0)
            .total_cmp(&a.cosine.unwrap_or(0.0))
            .then(b.c.msg_date.cmp(&a.c.msg_date))
            .then(b.ord.cmp(&a.ord))
    });
    Ok(Ok(items))
}

/// The facets of explicit row sets, per identity, merged.
pub async fn facets_of<P: Platform>(
    fan: &Fan<'_, P>,
    rows: Vec<Vec<i64>>,
    months: Vec<(String, i64, i64)>,
) -> Result<Value, ApiError> {
    let ops = rows
        .into_iter()
        .map(|r| {
            Some(SearchOp::Facets {
                rowids: r,
                months: months.clone(),
            })
        })
        .collect();
    let outs: Vec<Option<Value>> = fan.each(ops).await?;
    Ok(facets::merge(
        &outs.into_iter().flatten().collect::<Vec<_>>(),
    ))
}

/// `mode: "semantic"`; `None` when the leg failed (the caller falls back to keyword).
pub async fn semantic_mode<P: Platform>(
    fan: &Fan<'_, P>,
    months: Option<Vec<(String, i64, i64)>>,
) -> Result<Option<Ranked>, ApiError> {
    debug_assert_eq!(fan.plan.mode, Mode::Semantic);
    let items = match semantic_leg(fan).await? {
        Ok(items) => items,
        Err(()) if fan.plan.require_mode => return Err(degraded_error()),
        Err(()) => return Ok(None),
    };
    let facets = match months {
        Some(m) => {
            let mut rows = vec![Vec::new(); fan.plan.scope.targets.len()];
            for i in &items {
                rows[i.idx].push(i.c.rowid);
            }
            Some(facets_of(fan, rows, m).await?)
        }
        None => None,
    };
    Ok(Some(Ranked {
        items,
        facets,
        coverage: Some(coverage(fan).await?),
        degraded: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_ids_and_shapes() {
        assert_eq!(
            message_of("msg_01J9Z3K8V4QW7X2M5N6P8R0T1Y:a2:7"),
            Some("msg_01J9Z3K8V4QW7X2M5N6P8R0T1Y")
        );
        assert_eq!(message_of("msg_short:0"), None);
        assert_eq!(message_of("thr_01J9Z3K8V4QW7X2M5N6P8R0T1Y:0"), None);
        assert_eq!(tag_of("@cf/baai/bge-m3"), "bge-m3@1");
        let one = json!({ "shape": [1, 1024], "data": [vec![0.5; 1024]] });
        assert_eq!(vectors_of(&one, 1).unwrap()[0].len(), 1024);
        assert!(vectors_of(&one, 2).is_none());
        assert!(vectors_of(&json!({ "data": [[1.0, 2.0]] }), 1).is_none());
    }
}
