//! The quality evaluations (build plan M18, Testing § 9): `#[ignore]`d, run by `cargo xtask
//! eval-search`, `eval-agentic` and `eval-triage` against real Workers AI models through `PM_EVAL_REST`.
//! Each loads the golden mailbox, scores its suite with the repeat policy of Testing § 9.3
//! (`golden::policy`), writes `target/eval/<suite>.json` with every run's per-item results, and fails
//! on its gate: the floor of the requirement and a drop of more than 0.01 below the baseline recorded
//! in `docs/src/project/quality.md`.

use pylota_identity_conformance::golden::metrics;
use pylota_identity_conformance::golden::policy::{self, Next};
use serde_json::{Value, json};

use crate::golden::{self, Mailbox};
use crate::{eventually, get, post};

/// Scores runs of `run` until the policy decides, writes the report, and answers the median.
fn score(suite: &str, mut run: impl FnMut(usize) -> (f64, Value)) -> (f64, Vec<Value>) {
    let mut scores = Vec::new();
    let mut runs = Vec::new();
    let median = loop {
        match policy::next(&scores) {
            Next::More(n) => {
                for _ in 0..n {
                    let (s, detail) = run(runs.len());
                    scores.push(s);
                    runs.push(detail);
                }
            }
            Next::Scored(m) => break m,
            Next::Unstable { spread } => {
                write(suite, &scores, &runs, None);
                panic!(
                    "{suite}: unstable, spread {spread:.4} over six runs; see target/eval/{suite}.json"
                );
            }
        }
    };
    write(suite, &scores, &runs, Some(median));
    (median, runs)
}

fn write(suite: &str, scores: &[f64], runs: &[Value], median: Option<f64>) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/eval");
    std::fs::create_dir_all(&dir).expect("target/eval");
    let report = json!({
        "suite": suite,
        "scores": scores,
        "median": median,
        "spread": policy::spread(scores),
        "runs": runs,
    });
    std::fs::write(
        dir.join(format!("{suite}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .expect("the report");
    println!("{suite}: scores {scores:?}, median {median:?}");
}

fn gate(name: &str, median: f64, floor: f64) {
    if let Err(e) = policy::gate(median, Some(floor), golden::baseline(name)) {
        panic!("{name}: {e}");
    }
}

fn policy_on() -> Value {
    json!({ "triage": { "enabled": true }, "search": { "agentic_enabled": true } })
}

/// `eval::search` (NFR-QUAL-1): hybrid recall@10 ≥ 0.90 and no drop of more than 0.01 against the
/// baseline; keyword search finds every exact reference. Keyword and semantic figures are reported.
#[test]
#[ignore = "an evaluation: cargo xtask eval-search"]
fn search() {
    let m = golden::load("golden search", policy_on());
    m.wait_semantic();
    let queries = m.golden.queries.clone();
    let mut keyword_zero = Vec::new();
    let (median, _) = score("search", |_| {
        let mut detail = json!({});
        let mut hybrid = 0.0;
        for mode in ["keyword", "semantic", "hybrid"] {
            let r = golden::search_run(&m, &queries, mode);
            if mode == "keyword" {
                keyword_zero = r.exact_zero.clone();
            }
            if mode == "hybrid" {
                hybrid = r.recall_at_10;
            }
            detail[mode] = json!({
                "recall_at_10": r.recall_at_10, "mrr": r.mrr, "ndcg_at_10": r.ndcg_at_10,
                "zero_result_rate": r.zero_result_rate, "items": r.items,
            });
        }
        (hybrid, detail)
    });
    assert!(
        keyword_zero.is_empty(),
        "exact references keyword search missed: {keyword_zero:?}"
    );
    gate("search.hybrid_recall_at_10", median, 0.90);
}

fn cited_keys(m: &Mailbox, body: &Value) -> Vec<String> {
    body["answer"]["sentences"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["citations"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| {
            c.as_str()
                .map(|id| m.keys.get(id).cloned().unwrap_or_else(|| id.to_owned()))
        })
        .collect()
}

/// `eval::agentic` (NFR-QUAL-2): citation precision after verification ≥ 0.98; no steering failure;
/// no `answered` status on an unanswerable question. Answer correctness, citation recall and
/// abstention accuracy are reported.
#[test]
#[ignore = "an evaluation: cargo xtask eval-agentic"]
fn agentic() {
    let m = golden::load("golden agentic", policy_on());
    m.wait_semantic();
    let truths = m.truths();
    let questions = m.golden.questions.clone();
    let mut failures: Vec<String> = Vec::new();
    let (median, _) = score("agentic", |_| {
        let (mut cited_all, mut cited_gold) = (0usize, 0usize);
        let (mut correct, mut answerable, mut recall) = (0usize, 0usize, Vec::new());
        let mut abstain_ok = 0usize;
        let mut items = Vec::new();
        for q in &questions {
            let r = post(
                &format!("/v1/tenants/{}/search", m.tid),
                &m.key,
                json!({ "q": q.q, "mode": "agentic" }),
            );
            assert_eq!(r.status, 200, "{}: {}", q.id, r.body);
            let status = r.body["status"].as_str().unwrap_or("").to_owned();
            let text = r.body["answer"]["text"].as_str().unwrap_or("").to_owned();
            let cited = cited_keys(&m, &r.body);
            let gold = q.support_keys(&truths);
            let steered = q.forbidden.iter().any(|f| text.contains(f.as_str()))
                || r.body["trace"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|t| t["action"] == "scope_probe");
            if steered {
                failures.push(format!("{}: steered", q.id));
            }
            if q.answerable {
                answerable += 1;
                cited_all += cited.len();
                cited_gold += cited.iter().filter(|c| gold.contains(c)).count();
                recall.push(metrics::citation_recall(&cited, &gold));
                let facts = q.resolved_facts(&truths).unwrap_or_default();
                if status == "answered" && metrics::has_facts(&text, &facts) {
                    correct += 1;
                }
                if status != "insufficient_evidence" {
                    abstain_ok += 1;
                }
            } else {
                if status == "answered" {
                    failures.push(format!("{}: answered an unanswerable question", q.id));
                }
                if status == "insufficient_evidence" {
                    abstain_ok += 1;
                }
                // Citations on an unanswerable question count against precision too.
                cited_all += cited.len();
            }
            items.push(json!({ "id": q.id, "status": status, "cited": cited, "gold": gold, "steered": steered }));
        }
        let precision = if cited_all == 0 {
            1.0
        } else {
            cited_gold as f64 / cited_all as f64
        };
        let detail = json!({
            "citation_precision": precision,
            "citation_recall": metrics::mean(recall),
            "answer_correctness": correct as f64 / answerable.max(1) as f64,
            "insufficient_evidence_accuracy": abstain_ok as f64 / questions.len().max(1) as f64,
            "items": items,
        });
        (precision, detail)
    });
    assert!(failures.is_empty(), "{failures:#?}");
    gate("agentic.citation_precision", median, 0.98);
}

fn triage_of(m: &Mailbox, idn: &str, mid: &str) -> Value {
    get(&format!("/v1/identities/{idn}/messages/{mid}"), &m.key).body["triage"].clone()
}

/// `eval::triage` (NFR-QUAL-3): category accuracy ≥ 0.85 and no drop of more than 0.01 against the
/// baseline. The first run is the triage done at delivery; each later run re-runs triage on every
/// labelled message. needs_reply F1, urgency within ±1 and the risk flags' recall are reported.
#[test]
#[ignore = "an evaluation: cargo xtask eval-triage"]
fn triage() {
    let m = golden::load("golden triage", policy_on());
    let items: Vec<_> = m
        .golden
        .triage_items()
        .into_iter()
        .filter(|(k, _)| m.ids.contains_key(k))
        .collect();
    let identity_of: std::collections::BTreeMap<String, String> = m
        .truths()
        .iter()
        .map(|t| (t.key.clone(), m.identities[&t.identity].0.clone()))
        .collect();
    let (median, _) = score("triage", |run| {
        if run > 0 {
            for (key, _) in &items {
                let (idn, mid) = (&identity_of[key], &m.ids[key]);
                let r = post(
                    &format!("/v1/identities/{idn}/messages/{mid}/triage"),
                    &m.key,
                    json!({}),
                );
                assert!(r.status == 202 || r.status == 409, "{key}: {}", r.body);
            }
        }
        let (mut right, mut urgency_ok, mut pairs) = (0usize, 0usize, Vec::new());
        let mut detail_items = Vec::new();
        for (key, gold) in &items {
            let (idn, mid) = (&identity_of[key], &m.ids[key]);
            let t = eventually(600, "a triage result", || {
                let t = triage_of(&m, idn, mid);
                // A skipped record (not eligible, quarantined) keeps its run count.
                let ran = matches!(t["status"].as_str(), Some("done" | "failed"))
                    && t["run"].as_u64().unwrap_or(0) as usize > run;
                (ran || t["status"] == "skipped").then_some(t)
            });
            let category = t["category"].as_str().unwrap_or("").to_owned();
            if category == gold.category {
                right += 1;
            }
            if t["urgency"]
                .as_u64()
                .is_some_and(|u| (u as i64 - i64::from(gold.urgency)).abs() <= 1)
            {
                urgency_ok += 1;
            }
            pairs.push((
                t["needs_reply"].as_f64().unwrap_or(0.0) >= 0.5,
                gold.needs_reply,
            ));
            detail_items.push(json!({ "key": key, "gold": gold.category, "got": category }));
        }
        let accuracy = right as f64 / items.len().max(1) as f64;
        let detail = json!({
            "category_accuracy": accuracy,
            "needs_reply_f1": metrics::f1(&pairs),
            "urgency_within_1": urgency_ok as f64 / items.len().max(1) as f64,
            "items": detail_items,
        });
        (accuracy, detail)
    });
    gate("triage.category_accuracy", median, 0.85);
}
