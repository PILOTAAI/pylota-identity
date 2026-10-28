//! Hybrid ranking ([Search § 9]): reciprocal rank fusion of the keyword and semantic lists with
//! `k = 60`, reranker score normalisation, and the final blend that keeps reranked candidates above the
//! tail.
//!
//! [Search § 9]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use std::cmp::Ordering;
use std::collections::HashMap;

pub const RRF_K: f64 = 60.0;
/// A message ranked first in both lists.
pub const RRF_MAX: f64 = 2.0 / 61.0;
/// The fused list keeps this many candidates.
pub const RRF_KEEP: usize = 200;
/// The reranker sees the first this many fused candidates.
pub const RERANK_TOP: usize = 50;

/// One message in a ranked list (best first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Doc {
    /// Row ID (identity scope) or packed identity index and row ID (tenant scope).
    pub ord: u64,
    /// Message date, Unix ms.
    pub date: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fused {
    pub ord: u64,
    pub date: i64,
    /// 1-based positions.
    pub rank_kw: Option<u32>,
    pub rank_sem: Option<u32>,
    pub rrf: f64,
}

/// `rrf(d) = Σ 1 / (60 + rank_L(d))` over the lists that hold `d`, sorted by `rrf` descending, then
/// `rank_kw` ascending (absent last), then date and row ID descending; the first 200. A message listed
/// twice in one list keeps its best rank.
pub fn rrf(keyword: &[Doc], semantic: &[Doc]) -> Vec<Fused> {
    let mut by_ord: HashMap<u64, usize> = HashMap::new();
    let mut out: Vec<Fused> = Vec::new();
    for (list, is_kw) in [(keyword, true), (semantic, false)] {
        for (i, d) in list.iter().enumerate() {
            let rank = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let idx = *by_ord.entry(d.ord).or_insert_with(|| {
                out.push(Fused {
                    ord: d.ord,
                    date: d.date,
                    rank_kw: None,
                    rank_sem: None,
                    rrf: 0.0,
                });
                out.len() - 1
            });
            let slot = if is_kw {
                &mut out[idx].rank_kw
            } else {
                &mut out[idx].rank_sem
            };
            if slot.is_none() {
                *slot = Some(rank);
            }
        }
    }
    for f in &mut out {
        f.rrf = [f.rank_kw, f.rank_sem]
            .into_iter()
            .flatten()
            .map(|r| 1.0 / (RRF_K + f64::from(r)))
            .sum();
    }
    out.sort_by(|a, b| {
        b.rrf
            .total_cmp(&a.rrf)
            .then_with(|| {
                a.rank_kw
                    .unwrap_or(u32::MAX)
                    .cmp(&b.rank_kw.unwrap_or(u32::MAX))
            })
            .then_with(|| b.date.cmp(&a.date))
            .then_with(|| b.ord.cmp(&a.ord))
    });
    out.truncate(RRF_KEEP);
    out
}

/// What a reranker batch holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RerankScore {
    Probability,
    Logit,
}

/// Spike S6 (2026-10-27): `@cf/baai/bge-reranker-base` returns probabilities. [`classify_batch`] is the
/// guard that still catches a model returning logits.
pub const PINNED_RERANK_SCORE: RerankScore = RerankScore::Probability;

/// Logits if any score lies outside `[0, 1]` (or is not finite), else probabilities.
pub fn classify_batch(scores: &[f64]) -> RerankScore {
    if scores.iter().all(|s| (0.0..=1.0).contains(s)) {
        RerankScore::Probability
    } else {
        RerankScore::Logit
    }
}

/// `p = 1 / (1 + e^(−score))` over the whole batch when it holds logits, else `p = score`. A `NaN`
/// becomes 0.
pub fn normalise_rerank(scores: &[f64]) -> Vec<f64> {
    let logits = classify_batch(scores) == RerankScore::Logit;
    scores
        .iter()
        .map(|&s| {
            let p = if logits { 1.0 / (1.0 + (-s).exp()) } else { s };
            if p.is_nan() { 0.0 } else { p }
        })
        .collect()
}

/// One item of the reranker response: `id` indexes the contexts sent (the first [`RERANK_TOP`] fused
/// candidates, in order).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RerankItem {
    pub id: usize,
    pub score: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ranked {
    pub ord: u64,
    pub date: i64,
    pub rrf: f64,
    /// The normalised reranker probability, for a reranked candidate.
    pub rerank: Option<f64>,
    pub score: f64,
}

/// The final order. With a reranker response, a reranked candidate scores
/// `0.5 + 0.5 · (0.8 · p + 0.2 · rrf / rrf_max)` and the rest `0.5 · rrf / rrf_max`, so reranked
/// candidates always rank above the tail. Without one (error, timeout or `PM_RERANK_MODEL = none`),
/// `rrf / rrf_max`. Ties by date, then row ID, descending.
pub fn rank(fused: &[Fused], rerank: Option<&[RerankItem]>) -> Vec<Ranked> {
    let mut p: Vec<Option<f64>> = vec![None; fused.len().min(RERANK_TOP)];
    if let Some(items) = rerank {
        let probs = normalise_rerank(&items.iter().map(|i| i.score).collect::<Vec<_>>());
        for (item, prob) in items.iter().zip(probs) {
            if let Some(slot @ None) = p.get_mut(item.id) {
                *slot = Some(prob.clamp(0.0, 1.0));
            }
        }
    }
    let with_rerank = rerank.is_some();
    let mut out: Vec<Ranked> = fused
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let rel = f.rrf / RRF_MAX;
            let rerank = p.get(i).copied().flatten();
            let score = match (with_rerank, rerank) {
                (false, _) => rel,
                (true, Some(p)) => 0.5 + 0.5 * (0.8 * p + 0.2 * rel),
                (true, None) => 0.5 * rel,
            };
            Ranked {
                ord: f.ord,
                date: f.date,
                rrf: f.rrf,
                rerank,
                score,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.date.cmp(&a.date))
            .then_with(|| b.ord.cmp(&a.ord))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(ords: &[u64]) -> Vec<Doc> {
        ords.iter()
            .map(|&ord| Doc {
                ord,
                date: 1_000 + ord as i64,
            })
            .collect()
    }

    #[test]
    fn rrf_k60() {
        // kw: 1, 2, 3, 4    sem: 3, 1, 5
        let f = rrf(&docs(&[1, 2, 3, 4]), &docs(&[3, 1, 5]));
        let get = |ord| f.iter().find(|x| x.ord == ord).unwrap();
        assert!((get(1).rrf - (1.0 / 61.0 + 1.0 / 62.0)).abs() < 1e-15);
        assert!((get(3).rrf - (1.0 / 63.0 + 1.0 / 61.0)).abs() < 1e-15);
        assert!((get(5).rrf - 1.0 / 63.0).abs() < 1e-15);
        assert_eq!((get(2).rank_kw, get(2).rank_sem), (Some(2), None));
        // 1/61 + 1/62 > 1/63 + 1/61 > 1/62 > 1/63 > 1/64
        let order: Vec<u64> = f.iter().map(|x| x.ord).collect();
        assert_eq!(order, [1, 3, 2, 5, 4]);
        assert!((RRF_MAX - 2.0 / 61.0).abs() < 1e-15);

        // An exact tie is broken by rank_kw: kw [7, 8], sem [8, 7] give equal sums.
        let t = rrf(&docs(&[7, 8]), &docs(&[8, 7]));
        assert_eq!(t[0].rrf.to_bits(), t[1].rrf.to_bits());
        assert_eq!(t.iter().map(|x| x.ord).collect::<Vec<_>>(), [7, 8]);
        // At most 200 are kept; a duplicate keeps its best rank.
        let many: Vec<u64> = (0..300).collect();
        assert_eq!(rrf(&docs(&many), &[]).len(), RRF_KEEP);
        let dup = rrf(&docs(&[4, 4, 5]), &[]);
        assert_eq!(dup.iter().find(|x| x.ord == 4).unwrap().rank_kw, Some(1));
        assert_eq!(dup.iter().find(|x| x.ord == 5).unwrap().rank_kw, Some(3));
    }

    #[test]
    fn rrf_ties_by_date_then_ord() {
        let a = Doc { ord: 1, date: 10 };
        let b = Doc { ord: 2, date: 20 };
        let c = Doc { ord: 3, date: 20 };
        // Equal scores: newer first, then the higher row ID.
        let fused: Vec<Fused> = [a, b, c]
            .iter()
            .map(|d| Fused {
                ord: d.ord,
                date: d.date,
                rank_kw: None,
                rank_sem: Some(1),
                rrf: 1.0 / 61.0,
            })
            .collect();
        let r = rank(&fused, None);
        assert_eq!(r.iter().map(|x| x.ord).collect::<Vec<_>>(), [3, 2, 1]);
    }

    #[test]
    fn rerank_normalise() {
        // A batch with any score outside [0, 1] is logits: the sigmoid applies to every score.
        let l = normalise_rerank(&[2.0, 0.0, 0.5]);
        assert_eq!(classify_batch(&[2.0, 0.0, 0.5]), RerankScore::Logit);
        assert!((l[0] - 1.0 / (1.0 + (-2.0f64).exp())).abs() < 1e-12);
        assert!((l[1] - 0.5).abs() < 1e-12);
        assert!((l[2] - 1.0 / (1.0 + (-0.5f64).exp())).abs() < 1e-12);
        // Probabilities pass through (the values spike S6 saw).
        assert_eq!(classify_batch(&[0.375, 0.00008]), PINNED_RERANK_SCORE);
        assert_eq!(normalise_rerank(&[0.375, 0.00008]), [0.375, 0.00008]);
        assert_eq!(normalise_rerank(&[f64::NAN, -1.0])[0], 0.0);

        // Reranked candidates rank above the tail, whatever their RRF.
        let kw: Vec<Doc> = docs(&(0..60).collect::<Vec<_>>());
        let fused = rrf(&kw, &[]);
        // The reranker scores context 49 (RRF rank 50) highest and context 0 lowest.
        let items: Vec<RerankItem> = (0..RERANK_TOP)
            .map(|id| RerankItem {
                id,
                score: if id == 49 { 0.9 } else { 0.0001 },
            })
            .collect();
        let r = rank(&fused, Some(&items));
        assert_eq!(r[0].ord, fused[49].ord);
        let reranked = r.iter().filter(|x| x.rerank.is_some()).count();
        assert_eq!(reranked, RERANK_TOP);
        assert!(r[..RERANK_TOP].iter().all(|x| x.rerank.is_some()));
        assert!(
            r[RERANK_TOP..]
                .iter()
                .all(|x| x.rerank.is_none() && x.score < 0.5)
        );
        let top = &r[0];
        let rel = fused[49].rrf / RRF_MAX;
        assert!((top.score - (0.5 + 0.5 * (0.8 * 0.9 + 0.2 * rel))).abs() < 1e-12);
        // Without a reranker, RRF order with score rrf / rrf_max.
        let plain = rank(&fused, None);
        assert_eq!(
            plain.iter().map(|x| x.ord).collect::<Vec<_>>(),
            fused.iter().map(|x| x.ord).collect::<Vec<_>>()
        );
        assert!((plain[0].score - fused[0].rrf / RRF_MAX).abs() < 1e-12);
    }
}
