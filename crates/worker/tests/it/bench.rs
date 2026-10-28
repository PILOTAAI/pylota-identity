//! `it::bench` (NFR-PERF-1, NFR-PERF-2): the figures are reported, and CI warns above the targets.

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
