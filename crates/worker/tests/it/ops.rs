//! The state alert evaluator against the workerd harness (Observability § 5.4, build plan M17).

use serde_json::{Value, json};

use crate::{cron, d1, fakes, send};

/// The evaluator's memory (`platform_state` row `alert_evaluator`).
fn evaluator_state() -> Value {
    d1("SELECT value_json FROM platform_state WHERE key = 'alert_evaluator'")
        .get(0)
        .and_then(|r| r["value_json"].as_str())
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null)
}

/// The `alert.fired` and `alert.resolved` rows of `key`, oldest first: `(action, details)`.
fn rows(key: &str) -> Vec<(String, Value)> {
    d1(&format!(
        "SELECT action, details_json FROM audit_log WHERE action IN ('alert.fired','alert.resolved') AND target_id = '{key}' ORDER BY created_at, id"
    ))
    .as_array()
    .unwrap()
    .iter()
    .map(|r| {
        (
            r["action"].as_str().unwrap().to_owned(),
            serde_json::from_str(r["details_json"].as_str().unwrap_or("null")).unwrap(),
        )
    })
    .collect()
}

fn actions(key: &str) -> Vec<String> {
    rows(key).into_iter().map(|(a, _)| a).collect()
}

fn minute() {
    cron("* * * * *");
}

/// A report as an object or job writes it (`ops::alerts::report`).
fn report(action: &str, key: &str, severity: &str) {
    let id = crate::slug("aud");
    d1(&format!(
        "INSERT INTO audit_log (id, action, target_type, target_id, details_json, created_at) VALUES ('aud_{id}', '{action}', 'alert', '{key}', '{{\"severity\":\"{severity}\",\"values\":{{}},\"reported\":true}}', {})",
        fakes::worker_now()
    ));
}

/// `it::ops::alert_evaluator_transitions` (Observability § 5.4): a level condition (a dead-letter item
/// older than 15 minutes) fires once with its severity and values, re-notifies after 6 hours without
/// an audit row, and resolves after two runs without it; a reported condition (`vector_drift`) is true
/// in the run that reads the report and resolves after two runs without one; a self-resolved one
/// (`erasure_stalled`) keeps firing until its reporter resolves it; a failed job raises
/// `job_failed:{kind}` with the kind's severity until a later job of the kind completes.
#[test]
fn alert_evaluator_transitions() {
    let _r = fakes::Reset;
    let now = fakes::worker_now();
    let dlq = format!("dlq_{}", crate::slug("q"));
    d1(&format!(
        "INSERT INTO dlq_items (id, queue, message_id, body_json, body_sha256, first_seen_at) VALUES ('{dlq}', 'pm-screen', '{dlq}', '{{}}', 'h', {})",
        now - 20 * 60_000
    ));
    minute();
    let fired = rows("dlq:pm-screen");
    assert_eq!(fired.len(), 1, "{fired:?}");
    assert_eq!(fired[0].0, "alert.fired");
    assert_eq!(fired[0].1["severity"], "page");
    assert!(fired[0].1["values"]["open"].as_i64().unwrap() >= 1);
    minute();
    assert_eq!(actions("dlq:pm-screen"), ["alert.fired"], "fires once");
    let state = evaluator_state();
    assert_eq!(state["keys"]["dlq:pm-screen"]["notices"], 0);

    // Six hours later it notifies again: a notice, no audit row.
    fakes::advance(6 * 3_600_000 + 60_000);
    minute();
    assert_eq!(actions("dlq:pm-screen"), ["alert.fired"]);
    assert_eq!(evaluator_state()["keys"]["dlq:pm-screen"]["notices"], 1);

    // Redriven: one run without it is not enough, the second resolves.
    d1(&format!(
        "UPDATE dlq_items SET redriven_at = {} WHERE queue = 'pm-screen'",
        fakes::worker_now()
    ));
    minute();
    assert_eq!(evaluator_state()["keys"]["dlq:pm-screen"]["false_runs"], 1);
    minute();
    assert_eq!(actions("dlq:pm-screen"), ["alert.fired", "alert.resolved"]);
    assert!(evaluator_state()["keys"].get("dlq:pm-screen").is_none());

    // Reported: the reporter's row is the fired row; two runs without a report resolve it. Earlier
    // suites' reconciliations may have reported it already, so only the rows from here on count.
    minute();
    minute();
    let earlier = actions("vector_drift").len();
    report("alert.fired", "vector_drift", "ticket");
    minute();
    assert_eq!(
        evaluator_state()["keys"]["vector_drift"]["severity"],
        "ticket"
    );
    assert_eq!(
        actions("vector_drift")[earlier..],
        ["alert.fired"],
        "no second fired row"
    );
    minute();
    minute();
    assert_eq!(
        actions("vector_drift")[earlier..],
        ["alert.fired", "alert.resolved"]
    );

    // Self-resolved: firing until the erasure job resolves it.
    let stalled = format!("erasure_stalled:era_{}", crate::slug("s"));
    report("alert.fired", &stalled, "page");
    for _ in 0..3 {
        minute();
    }
    assert!(evaluator_state()["keys"].get(&stalled).is_some());
    assert_eq!(actions(&stalled), ["alert.fired"]);
    report("alert.resolved", &stalled, "page");
    minute();
    assert!(evaluator_state()["keys"].get(&stalled).is_none());

    // A failed `reindex` job: a ticket until a later `reindex` of the tenant completes.
    let t = crate::tenant(&crate::platform(), "evaluator jobs")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (failed, done) = (crate::slug("jf"), crate::slug("jd"));
    let now = fakes::worker_now();
    d1(&format!(
        "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_at, updated_at) VALUES ('job_{failed}', '{t}', 'reindex', 'failed', '', '{{}}', {now}, {now})"
    ));
    minute();
    let fired = rows("job_failed:reindex");
    let last = fired.last().unwrap();
    assert_eq!(last.0, "alert.fired");
    assert_eq!(last.1["severity"], "ticket");
    assert!(
        last.1["values"]["job_ids"]
            .as_array()
            .unwrap()
            .contains(&json!(format!("job_{failed}")))
    );
    d1(&format!(
        "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_at, updated_at) VALUES ('job_{done}', '{t}', 'reindex', 'completed', '', '{{}}', {}, {})",
        now + 1,
        now + 1
    ));
    minute();
    minute();
    assert_eq!(
        actions("job_failed:reindex").last().map(String::as_str),
        Some("alert.resolved")
    );
}

/// `it::ops::provider_quota_80` (G3, Observability § 5.3): with `PM_DAILY_SEND_QUOTA` set, the
/// Cloudflare-transport recipients accepted today count in the default tenant's `deployment_day`; the
/// evaluator fires the `provider_quota_80` ticket once they reach 80% of the quota, and not before.
/// Without the variable nothing is counted.
#[test]
fn provider_quota_80() {
    let _r = fakes::Reset;
    let s = send::open_sender("quota eighty", "live");
    let rcpt = |n: usize| -> Vec<String> {
        (0..n)
            .map(|i| format!("q{i}-{}@recipient.example", crate::slug("r")))
            .collect()
    };
    let send_to = |to: &[String]| {
        let to: Vec<&str> = to.iter().map(String::as_str).collect();
        send::send_ok(&s, &to, "Quota");
    };
    // Unset: nothing counted, so the first sends below start from zero today.
    let before = actions("provider_quota_80").len();
    send_to(&rcpt(3));
    minute();
    assert_eq!(
        actions("provider_quota_80").len(),
        before,
        "unset: never fires"
    );

    fakes::set_config("PM_DAILY_SEND_QUOTA", Some("10"));
    let quota = 10;
    send_to(&rcpt(7));
    minute();
    assert_eq!(
        actions("provider_quota_80").len(),
        before,
        "70% does not fire"
    );
    send_to(&rcpt(1));
    minute();
    let fired = rows("provider_quota_80");
    assert_eq!(fired.len(), before + 1, "{fired:?}");
    let (action, details) = fired.last().unwrap();
    assert_eq!(action, "alert.fired");
    assert_eq!(details["severity"], "ticket");
    assert_eq!(details["values"]["quota"], quota);
    assert_eq!(details["values"]["count"], 8);
}
