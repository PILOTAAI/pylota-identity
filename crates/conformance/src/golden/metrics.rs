//! The evaluation metrics of Search § 13.2–13.3 and Triage § 13.

use std::collections::BTreeMap;

/// `|relevant ∩ top k| / min(|relevant|, k)`; relevant = grade ≥ 1. `None` without relevant items.
pub fn recall_at(ranked: &[String], grades: &BTreeMap<String, u8>, k: usize) -> Option<f64> {
    let relevant = grades.values().filter(|g| **g > 0).count();
    if relevant == 0 {
        return None;
    }
    let hit = ranked
        .iter()
        .take(k)
        .filter(|r| grades.get(*r).is_some_and(|g| *g > 0))
        .count();
    Some(hit as f64 / relevant.min(k) as f64)
}

/// `1 / rank` of the first relevant hit within the top 50, else 0.
pub fn reciprocal_rank(ranked: &[String], grades: &BTreeMap<String, u8>) -> f64 {
    ranked
        .iter()
        .take(50)
        .position(|r| grades.get(r).is_some_and(|g| *g > 0))
        .map_or(0.0, |i| 1.0 / (i + 1) as f64)
}

/// `DCG@10 / IDCG@10`, gain `2^grade − 1`, discount `log2(i + 1)` for the 1-based rank `i`.
pub fn ndcg_at_10(ranked: &[String], grades: &BTreeMap<String, u8>) -> f64 {
    let gain = |g: u8| f64::from((1u32 << g) - 1);
    let dcg: f64 = ranked
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, r)| gain(*grades.get(r).unwrap_or(&0)) / ((i + 2) as f64).log2())
        .sum();
    let mut ideal: Vec<u8> = grades.values().copied().collect();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let idcg: f64 = ideal
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, g)| gain(*g) / ((i + 2) as f64).log2())
        .sum();
    if idcg == 0.0 { 0.0 } else { dcg / idcg }
}

/// Cited keys in the gold support / all cited keys; `None` when nothing is cited.
pub fn citation_precision(cited: &[String], gold: &[String]) -> Option<f64> {
    if cited.is_empty() {
        return None;
    }
    Some(cited.iter().filter(|c| gold.contains(c)).count() as f64 / cited.len() as f64)
}

/// Gold support keys cited / gold support keys; `None` without gold support.
pub fn citation_recall(cited: &[String], gold: &[String]) -> Option<f64> {
    if gold.is_empty() {
        return None;
    }
    Some(gold.iter().filter(|g| cited.contains(g)).count() as f64 / gold.len() as f64)
}

/// Lower case, with spaces, commas and currency words removed: "GBP 1,412.80" → "1412.80".
pub fn normalise(s: &str) -> String {
    s.to_lowercase()
        .replace("gbp", "")
        .replace('£', "")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .collect()
}

/// Whether `answer` contains every key fact, compared normalised.
pub fn has_facts(answer: &str, facts: &[String]) -> bool {
    let a = normalise(answer);
    facts.iter().all(|f| a.contains(&normalise(f)))
}

/// The mean of the `Some` values; 0 when there are none.
pub fn mean(values: impl IntoIterator<Item = Option<f64>>) -> f64 {
    let v: Vec<f64> = values.into_iter().flatten().collect();
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// F1 of binary predictions `(predicted, gold)`.
pub fn f1(pairs: &[(bool, bool)]) -> f64 {
    let tp = pairs.iter().filter(|(p, g)| *p && *g).count() as f64;
    let fp = pairs.iter().filter(|(p, g)| *p && !*g).count() as f64;
    let fneg = pairs.iter().filter(|(p, g)| !*p && *g).count() as f64;
    if tp == 0.0 {
        0.0
    } else {
        2.0 * tp / (2.0 * tp + fp + fneg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(items: &[(&str, u8)]) -> BTreeMap<String, u8> {
        items.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    fn r(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    /// The formulas of Search § 13.2: recall@10 caps the denominator at 10, MRR looks 50 deep, nDCG
    /// is 1 for the ideal order.
    #[test]
    fn search_metrics() {
        let grades = g(&[("a", 3), ("b", 1), ("c", 0)]);
        assert_eq!(recall_at(&r(&["x", "a", "c"]), &grades, 10), Some(0.5));
        assert_eq!(recall_at(&r(&["b", "a"]), &grades, 10), Some(1.0));
        assert_eq!(recall_at(&r(&[]), &g(&[]), 10), None);
        let many: BTreeMap<String, u8> = (0..20).map(|i| (format!("m{i}"), 1)).collect();
        let top: Vec<String> = (0..10).map(|i| format!("m{i}")).collect();
        assert_eq!(recall_at(&top, &many, 10), Some(1.0));
        assert_eq!(reciprocal_rank(&r(&["x", "y", "b"]), &grades), 1.0 / 3.0);
        assert_eq!(reciprocal_rank(&r(&["x"]), &grades), 0.0);
        assert!((ndcg_at_10(&r(&["a", "b"]), &grades) - 1.0).abs() < 1e-9);
        assert!(ndcg_at_10(&r(&["b", "a"]), &grades) < 1.0);
    }

    #[test]
    fn answer_metrics() {
        assert_eq!(citation_precision(&r(&["a", "x"]), &r(&["a"])), Some(0.5));
        assert_eq!(citation_precision(&r(&[]), &r(&["a"])), None);
        assert_eq!(citation_recall(&r(&["a"]), &r(&["a", "b"])), Some(0.5));
        assert!(has_facts(
            "The total was £1,412.80, paid.",
            &r(&["GBP 1412.80"])
        ));
        assert!(!has_facts("The total was 412.00", &r(&["412.80"])));
        assert_eq!(f1(&[(true, true), (false, true), (true, false)]), 0.5);
        assert_eq!(mean([Some(1.0), None, Some(0.0)]), 0.5);
    }
}
