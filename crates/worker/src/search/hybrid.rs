//! Hybrid ([Search § 9]): the keyword leg (top 200) and the semantic leg in parallel, reciprocal rank
//! fusion (`core::fusion`), the reranker over the first 50, and the degradation table: no semantic leg
//! is keyword only, no reranker is RRF only, both `degraded: true` (`PM_RERANK_MODEL = none` is not a
//! degradation).
//!
//! [Search § 9]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use std::collections::HashMap;

use pylota_identity_core::fusion::{self, Doc, RERANK_TOP, RerankItem};
use pylota_identity_platform::ai::AiOptions;
use pylota_identity_platform::{Ai as _, Clock as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use super::semantic::{facets_of, semantic_leg};
use super::wire::Detail;
use super::{Fan, HYBRID_CANDIDATES, Item, Ranked, coverage, degraded_error, join, keyword_leg};
use crate::errors::ApiError;
use crate::mailbox::search::{DetailsIn, SearchOp};

const RERANK_TIMEOUT_MS: u32 = 1_500;
/// The keyword window of a passage, and the most of a context sent.
const PASSAGE_CHARS: u32 = 1_200;
const CONTEXT_CHARS: usize = 1_500;

pub async fn hybrid_mode<P: Platform>(
    fan: &Fan<'_, P>,
    months: Option<Vec<(String, i64, i64)>>,
) -> Result<Ranked, ApiError> {
    let plan = fan.plan;
    let want_facets = months.is_some();
    let semantic = !plan.compiled.semantic_text.is_empty();
    let (kw, sem) = join::both(
        keyword_leg(fan, HYBRID_CANDIDATES, None, want_facets),
        async {
            if semantic {
                semantic_leg(fan).await
            } else {
                Ok(Ok(Vec::new()))
            }
        },
    )
    .await;
    let kw = kw?;
    let sem = sem?;
    let mut rows = kw.facet_rows.clone();
    rows.resize(plan.scope.targets.len(), Vec::new());
    let (items, degraded) = match sem {
        // No free text: the keyword order is the answer, and nothing degraded.
        Ok(_) if !semantic => (kw.items, false),
        Err(()) if plan.require_mode => return Err(degraded_error()),
        Err(()) => (kw.items, true),
        Ok(sem_items) => {
            for i in &sem_items {
                if !rows[i.idx].contains(&i.c.rowid) {
                    rows[i.idx].push(i.c.rowid);
                }
            }
            fuse(fan, kw.items, sem_items).await?
        }
    };
    let facets = match months {
        Some(m) => Some(facets_of(fan, rows, m).await?),
        None => None,
    };
    Ok(Ranked {
        items,
        facets,
        coverage: Some(coverage(fan).await?),
        degraded,
    })
}

/// RRF over both lists, then the reranker; answers the ranked items and whether reranking failed.
async fn fuse<P: Platform>(
    fan: &Fan<'_, P>,
    kw: Vec<Item>,
    sem: Vec<Item>,
) -> Result<(Vec<Item>, bool), ApiError> {
    let doc = |i: &Item| Doc {
        ord: i.ord,
        date: i.c.msg_date,
    };
    let fused = fusion::rrf(
        &kw.iter().map(doc).collect::<Vec<_>>(),
        &sem.iter().map(doc).collect::<Vec<_>>(),
    );
    let mut by: HashMap<u64, Item> = HashMap::new();
    for i in sem {
        by.insert(i.ord, i);
    }
    for i in kw {
        by.entry(i.ord)
            .and_modify(|s| {
                let cosine = s.cosine;
                *s = i.clone();
                s.cosine = cosine;
            })
            .or_insert(i);
    }
    let (rerank, degraded) = match &fan.p.config().rerank_model {
        None => (None, false),
        Some(model) => {
            let top: Vec<&Item> = fused
                .iter()
                .take(RERANK_TOP)
                .filter_map(|f| by.get(&f.ord))
                .collect();
            match rerank(fan, model, &top).await? {
                Some(r) => (Some(r), false),
                None => (None, true),
            }
        }
    };
    let ranked = fusion::rank(&fused, rerank.as_deref());
    let items = ranked
        .into_iter()
        .filter_map(|r| {
            let mut i = by.remove(&r.ord)?;
            i.score = r.score;
            i.rerank = r.rerank;
            Some(i)
        })
        .collect();
    Ok((items, degraded))
}

#[derive(Deserialize)]
struct RerankOut {
    response: Vec<RerankRow>,
}

#[derive(Deserialize)]
struct RerankRow {
    id: usize,
    score: f64,
}

/// The reranker's items for `top` (in order), or `None` on an error or timeout.
async fn rerank<P: Platform>(
    fan: &Fan<'_, P>,
    model: &str,
    top: &[&Item],
) -> Result<Option<Vec<RerankItem>>, ApiError> {
    if top.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let plan = fan.plan;
    // Passages: the best chunk for semantic hits, else a keyword window.
    let mut per: Vec<Vec<(i64, Option<String>)>> = vec![Vec::new(); plan.scope.targets.len()];
    for i in top {
        per[i.idx].push((i.c.rowid, i.c.vector_id.clone()));
    }
    let ops = per
        .iter()
        .map(|items| {
            (!items.is_empty()).then(|| {
                SearchOp::Details(DetailsIn {
                    items: items.clone(),
                    c: plan.wire.clone(),
                    snippet_chars: PASSAGE_CHARS,
                })
            })
        })
        .collect();
    let details: Vec<Option<Vec<Detail>>> = fan.each(ops).await?;
    let mut passage: HashMap<(usize, i64), String> = HashMap::new();
    for (idx, d) in details.into_iter().enumerate() {
        for d in d.unwrap_or_default() {
            passage.insert((idx, d.rowid), d.snippet);
        }
    }
    let contexts: Vec<Value> = top
        .iter()
        .map(|i| {
            let text = format!(
                "{}\n{}",
                i.c.subject.as_deref().unwrap_or(""),
                passage.get(&(i.idx, i.c.rowid)).map_or("", String::as_str)
            );
            json!({ "text": text.chars().take(CONTEXT_CHARS).collect::<String>() })
        })
        .collect();
    let p = fan.p;
    let input = json!({ "query": plan.compiled.semantic_text, "top_k": contexts.len(), "contexts": contexts });
    let opts = AiOptions::content(p.config().ai_gateway.clone());
    let call = p.ai().run::<Value, RerankOut>(model, &input, &opts);
    Ok(
        match join::before(call, p.clock().sleep(RERANK_TIMEOUT_MS)).await {
            Some(Ok(out)) => Some(
                out.response
                    .into_iter()
                    .filter(|r| r.id < top.len())
                    .map(|r| RerankItem {
                        id: r.id,
                        score: r.score,
                    })
                    .collect(),
            ),
            _ => None,
        },
    )
}
