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

const QUEUES: [&str; 6] = [
    "pm-inbound",
    "pm-outbound",
    "pm-delivery-events",
    "pm-webhooks",
    "pm-index",
    "pm-screen",
];

/// `it::ops::j8_dlq_consumer` (J8, Observability § 8): a message of each dead-letter queue is recorded
/// in `dlq_items` once (a repeat is absorbed) and acked; it is counted in the `dlq_items` observation
/// and raises `dlq:{queue}` after 15 minutes; `GET /v1/platform/dlq` lists it without its body, with
/// the queue, status and tenant filters; `POST …/redrive` publishes it back, sets `redriven_at`,
/// increments `redrive_count` and writes `dlq.redrive`; a damaged body is refused with `500`; a
/// non-platform key gets `403`.
#[test]
fn j8_dlq_consumer() {
    let _r = fakes::Reset;
    let t = crate::tenant(&crate::platform(), "dlq tenant")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let tag = crate::slug("j8");
    let mut ids = Vec::new();
    for q in QUEUES {
        let msg = format!("{tag}-{q}");
        let body =
            json!({ "kind": "probe", "tenant_id": t, "envelope_to": "secret@recipient.example" });
        for _ in 0..2 {
            let r = crate::hook(
                "POST",
                "/__test/dlq",
                Some(json!({ "queue": q, "messages": [{ "id": msg, "body": body }] })),
            );
            assert_eq!(r.body["settled"], json!(["ack"]), "{q}: {}", r.body);
        }
        let rows = d1(&format!(
            "SELECT id, kind, tenant_id, body_sha256 FROM dlq_items WHERE queue = '{q}' AND message_id = '{msg}'"
        ));
        assert_eq!(
            rows.as_array().unwrap().len(),
            1,
            "{q}: a repeat is absorbed"
        );
        assert_eq!(rows[0]["kind"], "probe");
        assert_eq!(rows[0]["tenant_id"], t.as_str());
        ids.push((q, rows[0]["id"].as_str().unwrap().to_owned()));
    }

    // Listed without the body, filtered by queue, status and tenant.
    let p = crate::platform();
    let r = crate::get(&format!("/v1/platform/dlq?tenant_id={t}"), &p);
    assert_eq!(r.status, 200, "{}", r.body);
    let data = r.body["data"].as_array().unwrap();
    assert_eq!(data.len(), 6);
    assert!(
        !r.body.to_string().contains("secret@recipient.example"),
        "never the body"
    );
    assert!(
        data.iter()
            .all(|i| i.get("body_json").is_none() && i["redriven_at"].is_null())
    );
    let r = crate::get(
        &format!("/v1/platform/dlq?tenant_id={t}&queue=pm-index"),
        &p,
    );
    assert_eq!(r.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(crate::get("/v1/platform/dlq?queue=nope", &p).status, 400);
    let partner = crate::partner("dlq partner", &["audit:read"]).1;
    assert_eq!(crate::get("/v1/platform/dlq", &partner).status, 403);

    // 15 minutes later every queue alerts.
    fakes::advance(16 * 60_000);
    minute();
    for (q, _) in &ids {
        assert!(
            actions(&format!("dlq:{q}")).contains(&"alert.fired".to_owned()),
            "{q}"
        );
    }

    // Redrive: published back (pm-webhooks is tapped before its consumer), then recorded.
    let (_, wid) = ids.iter().find(|(q, _)| *q == "pm-webhooks").unwrap();
    let before = fakes::queued("pm-webhooks").len();
    let r = crate::post(&format!("/v1/platform/dlq/{wid}/redrive"), &p, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["redrive_count"], 1);
    assert!(r.body["redriven_at"].is_string());
    crate::eventually(30, "the redriven body", || {
        fakes::queued("pm-webhooks")[before..]
            .iter()
            .any(|b| b["kind"] == "probe" && b["tenant_id"] == t.as_str())
            .then_some(())
    });
    assert_eq!(
        d1(&format!(
            "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'dlq.redrive' AND target_id = '{wid}'"
        ))[0]["n"],
        1
    );
    let r = crate::get(
        &format!("/v1/platform/dlq?tenant_id={t}&status=redriven"),
        &p,
    );
    assert_eq!(r.body["data"].as_array().unwrap().len(), 1);

    // A damaged body is refused and nothing is published.
    let (_, oid) = ids.iter().find(|(q, _)| *q == "pm-outbound").unwrap();
    d1(&format!(
        "UPDATE dlq_items SET body_json = '{{\"kind\":\"edited\"}}' WHERE id = '{oid}'"
    ));
    let r = crate::post(&format!("/v1/platform/dlq/{oid}/redrive"), &p, json!({}));
    assert_eq!(r.status, 500, "{}", r.body);
    assert_eq!(
        crate::post(
            "/v1/platform/dlq/dlq_01ARYZ6S41TSV4RRFFQ69G5FAV/redrive",
            &p,
            json!({})
        )
        .status,
        404
    );

    // Closed again so the alerts resolve for the rest of the suite.
    d1(&format!(
        "UPDATE dlq_items SET redriven_at = {} WHERE tenant_id = '{t}'",
        fakes::worker_now()
    ));
}

/// `it::ops::platform_status` (Observability § 5.5): with a platform key holding `audit:read`,
/// `GET /v1/platform/status` returns the freeze flag, the switches, `alert_email`, the firing alerts,
/// the master-key slots with `remaining`, `activated_at` and `resealed_at`, and capacity; a partner key
/// and a platform key without `audit:read` get `403`.
#[test]
fn platform_status() {
    let _r = fakes::Reset;
    let (_, audit) = crate::mint(
        &crate::platform(),
        json!({ "name": "heartbeat", "level": "platform", "permissions": ["audit:read"] }),
    );
    cron("* * * * *");
    let r = crate::get("/v1/platform/status", &audit);
    assert_eq!(r.status, 200, "{}", r.body);
    let s = &r.body;
    assert_eq!(s["frozen"], false);
    assert!(s["frozen_since"].is_null());
    assert_eq!(
        s["switches"],
        json!({ "read_only": "off", "free_sending": "on", "emergency_prune": "off", "agent_signup": "on" })
    );
    assert!(
        ["configured", "missing"].contains(&s["alert_email"].as_str().unwrap()),
        "{s}"
    );
    assert!(s["firing"].is_array());
    assert_eq!(s["master_key"]["active_slot"], "a");
    assert_eq!(s["master_key"]["remaining"], 0);
    assert!(
        s["master_key"]["activated_at"].is_string() && s["master_key"]["resealed_at"].is_string()
    );
    assert!(s["capacity"]["tenants"].as_u64().unwrap() >= 1);
    assert!(s["capacity"]["namespaces_pct"].is_number());
    assert!(
        !s.to_string()
            .contains(&std::env::var("PM_ITEST_MASTER_KEY").unwrap())
    );
    // A firing alert is listed with its severity.
    let key = format!("job_failed:{}", crate::slug("kind"));
    d1(&format!(
        "INSERT INTO audit_log (id, action, target_type, target_id, details_json, created_at) VALUES ('aud_{}', 'alert.fired', 'alert', '{key}', '{{\"severity\":\"ticket\",\"values\":{{}},\"reported\":true}}', {})",
        crate::slug("a"),
        fakes::worker_now()
    ));
    cron("* * * * *");
    let firing = crate::get("/v1/platform/status", &audit).body["firing"].clone();
    assert!(
        firing
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["alert"] == key.as_str()
                && f["severity"] == "ticket"
                && f["fired_at"].is_string()),
        "{firing}"
    );
    let partner = crate::partner("status partner", &["audit:read"]).1;
    assert_eq!(crate::get("/v1/platform/status", &partner).status, 403);
    let (_, ops) = crate::mint(
        &crate::platform(),
        json!({ "name": "ops only", "level": "platform", "permissions": ["platform:ops"] }),
    );
    assert_eq!(crate::get("/v1/platform/status", &ops).status, 403);
    cron("* * * * *");
    cron("* * * * *");
}
