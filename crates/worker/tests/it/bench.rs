//! `it::bench` (NFR-PERF-1–5): the figures are reported, and CI warns above the targets. The search
//! benchmarks fill their mailboxes with `/__test/bulk-seed` and are `#[ignore]`d: the nightly workflow
//! runs them (Testing § 6.9).

use std::time::Instant;

use serde_json::json;

use crate::send::{open_sender, send_ok, wait_status};

fn p95(mut v: Vec<u128>) -> u128 {
    v.sort_unstable();
    v[(v.len() * 95).div_ceil(100) - 1]
}

#[test]
fn send_api_p95() {
    let s = open_sender("bench api", "test");
    let mut times = Vec::new();
    for i in 0..20 {
        let t = Instant::now();
        send_ok(&s, &["delivered@simulator.invalid"], &format!("bench {i}"));
        times.push(t.elapsed().as_millis());
    }
    let p = p95(times);
    println!("NFR-PERF-1 send API p95: {p} ms (target 500 ms)");
    if p > 500 {
        println!("::warning::send API p95 {p} ms is above the 500 ms target");
    }
    let _ = json!({});
}

#[test]
fn queue_to_transport_p95() {
    let s = open_sender("bench transport", "test");
    let mut times = Vec::new();
    for i in 0..10 {
        let t = Instant::now();
        let mid = send_ok(
            &s,
            &["delivered@simulator.invalid"],
            &format!("bench t {i}"),
        );
        wait_status(&s, &mid, "delivered");
        times.push(t.elapsed().as_millis());
    }
    let p = p95(times);
    println!(
        "NFR-PERF-2 queue to transport p95: {p} ms (target 5 s; workerd's local queue batches add up to 5 s)"
    );
    if p > 5_000 {
        println!("::warning::queue to transport p95 {p} ms is above the 5 s target");
    }
}

/// `count`, or `PM_BENCH_MESSAGES` when set (a quick local run of the benchmarks).
fn scale(count: u64) -> u64 {
    std::env::var("PM_BENCH_MESSAGES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(count)
}

/// Bulk-seeds `count` synthetic messages into an identity's mailbox, and their vectors into the fake
/// Vectorize (Testing § 6.9).
fn seed(tenant_id: &str, identity_id: &str, count: u64, seed: u64) {
    let mut start = 0;
    while start < count {
        let n = (count - start).min(5_000);
        let r = crate::hook(
            "POST",
            "/__test/bulk-seed",
            Some(json!({ "identity_id": identity_id, "start": start, "count": n, "seed": seed })),
        );
        assert_eq!(r.status, 200, "{}", r.body);
        crate::fakes::seed_vectors(tenant_id, identity_id, r.body["rows"].as_array().unwrap());
        start += n;
    }
}

/// The search timings of `queries` against `path`.
fn timings(path: &str, key: &str, queries: &[serde_json::Value]) -> Vec<u128> {
    let mut times = Vec::new();
    for _ in 0..3 {
        for q in queries {
            let t = Instant::now();
            let r = crate::post(path, key, q.clone());
            times.push(t.elapsed().as_millis());
            assert_eq!(r.status, 200, "{}", r.body);
        }
    }
    times
}

fn report(name: &str, label: &str, p: u128, target: u128) {
    println!("{name} {label} p95: {p} ms (target {target} ms)");
    if p > target {
        println!("::warning::{label} p95 {p} ms is above the {target} ms target");
    }
}

const QUERIES: &[&str] = &[
    "invoice deposit",
    "van hire weekend",
    "\"damage report\"",
    "from:vendor7.example",
    "insurance OR excess",
    "refund -overdue",
    "brakes inspection garage",
    "charging cable",
];

/// NFR-PERF-3: keyword search over a 50,000-message mailbox.
#[test]
#[ignore = "nightly: seeds 50,000 messages (Testing § 6.9)"]
fn keyword_p95() {
    let (tid, idn, _, key) = crate::mailbox_fixture("bench keyword");
    seed(&tid, &idn, scale(50_000), 7);
    let qs: Vec<_> = QUERIES
        .iter()
        .map(|q| json!({ "q": q, "mode": "keyword" }))
        .collect();
    let p = p95(timings(&format!("/v1/identities/{idn}/search"), &key, &qs));
    report("NFR-PERF-3", "keyword search", p, 200);
}

/// NFR-PERF-4: hybrid search (fake Workers AI and Vectorize) over a 50,000-message mailbox.
#[test]
#[ignore = "nightly: seeds 50,000 messages (Testing § 6.9)"]
fn hybrid_p95() {
    let (tid, idn, _, key) = crate::mailbox_fixture("bench hybrid");
    seed(&tid, &idn, scale(50_000), 11);
    let qs: Vec<_> = QUERIES
        .iter()
        .map(|q| json!({ "q": q, "mode": "hybrid" }))
        .collect();
    let p = p95(timings(&format!("/v1/identities/{idn}/search"), &key, &qs));
    report("NFR-PERF-4", "hybrid search", p, 800);
}

/// NFR-PERF-5: tenant search fanned out over 10 identities.
#[test]
#[ignore = "nightly: seeds 10 mailboxes (Testing § 6.9)"]
fn tenant_fanout_p95() {
    let p = crate::platform();
    let t = crate::tenant(&p, "bench fanout");
    let tid = t["id"].as_str().unwrap().to_owned();
    for i in 0..10 {
        let idn = crate::identity(&p, &tid, &format!("fan{i}"));
        seed(
            &tid,
            idn["id"].as_str().unwrap(),
            scale(50_000) / 10,
            100 + i,
        );
    }
    let (_, key) = crate::mint(
        &p,
        json!({ "name": "fan", "level": "tenant", "tenant_id": tid, "permissions": ["search:read"] }),
    );
    let qs: Vec<_> = QUERIES
        .iter()
        .map(|q| json!({ "q": q, "mode": "hybrid" }))
        .collect();
    let p95v = p95(timings(&format!("/v1/tenants/{tid}/search"), &key, &qs));
    report(
        "NFR-PERF-5",
        "tenant search over 10 identities",
        p95v,
        1_000,
    );
}

/// NFR-PERF-6: agentic search, total p95 and time to the first `evidence` event, against 8 s and
/// 1.5 s, with a scripted planner (two searches, a read, an answer) over a seeded mailbox.
#[test]
#[ignore = "nightly: seeds a mailbox (Testing § 6.9)"]
fn agentic_p95() {
    use crate::agentic::{answer, completion};
    let (tid, idn, _, _) = crate::mailbox_fixture("bench agentic");
    seed(&tid, &idn, scale(5_000), 21);
    let p = crate::platform();
    let (_, key) = crate::mint(
        &p,
        json!({ "name": "agentic", "level": "tenant", "tenant_id": tid, "permissions": ["search:read", "search:agentic"] }),
    );
    let path = format!("/v1/identities/{idn}/search");
    let (mut total, mut first) = (Vec::new(), Vec::new());
    for q in QUERIES.iter().take(6) {
        crate::fakes::ai_script(
            "qwen",
            vec![
                completion(
                    &[("a", "search", json!({ "q": q, "mode": "keyword" }))],
                    None,
                ),
                completion(
                    &[("b", "search", json!({ "q": "invoice", "mode": "hybrid" }))],
                    None,
                ),
                completion(&[], Some("READY")),
                answer(&[], "insufficient_evidence"),
            ],
        );
        let t = Instant::now();
        let mut seen: Option<u128> = None;
        let (status, _, _) = crate::stream_post(
            &path,
            &key,
            json!({ "q": q, "mode": "agentic", "stream": true }),
            |text| {
                if seen.is_none() && text.contains("event: evidence") {
                    seen = Some(t.elapsed().as_millis());
                }
                false
            },
        );
        assert_eq!(status, 200);
        total.push(t.elapsed().as_millis());
        first.push(seen.unwrap_or(u128::MAX));
    }
    report("NFR-PERF-6", "agentic search", p95(total), 8_000);
    report("NFR-PERF-6", "agentic first evidence", p95(first), 1_500);
}
