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

/// Turns `read_only` off when a test ends, whatever happened, so the rest of the suite can write.
struct ReadOnlyOff;

impl Drop for ReadOnlyOff {
    fn drop(&mut self) {
        let _ = crate::put(
            "/v1/platform/switches",
            &crate::platform(),
            json!({ "name": "read_only", "state": "off", "reason": "test cleanup" }),
        );
    }
}

/// Fires one reported alert of `key` with `severity` now (as its reporter would).
fn fire(key: &str, severity: &str) {
    report("alert.fired", key, severity);
}

/// `it::ops::j32_alert_channels` (J32, Observability § 5.5): a page alert sends one alert email through
/// the system identity to `PM_ALERT_EMAIL`, with the alert key, the runbook and no content, and another
/// after 6 hours; ticket alerts arrive in one summary in the 08:00 UTC hour; more than 20 page emails
/// in an hour collapse into one that says how many more; `GET /v1/platform/status` reports
/// `alert_email: missing` without an address; with `read_only` on an alert email still leaves while a
/// tenant's queued send waits.
#[test]
fn j32_alert_channels() {
    let _r = fakes::Reset;
    let _off = ReadOnlyOff;
    let ops = format!("ops-{}@operator.example", crate::slug("o"));
    fakes::set_config("PM_ALERT_EMAIL", Some(&ops));
    // The status before and after an address is configured.
    fakes::set_config(
        "PM_SECURITY_CONTACT",
        Some("https://operator.example/security"),
    );
    let status = |k: &str| crate::get("/v1/platform/status", &crate::platform()).body[k].clone();
    assert_eq!(status("alert_email"), "configured");

    let key = format!("transport_breaker_open:{}", crate::slug("t"));
    fire(&key, "page");
    minute();
    let mails = crate::eventually(60, "the alert email", || {
        let m = fakes::mail_sent(&ops);
        (!m.is_empty()).then_some(m)
    });
    assert_eq!(mails.len(), 1);
    let text = mails[0].to_string();
    assert!(text.contains(&format!("page: {key}")), "{text}");
    assert!(
        text.contains("observability.html#email-sending-outage"),
        "{text}"
    );

    // Re-notified 6 hours later.
    fire(&key, "page");
    fakes::advance(6 * 3_600_000 + 60_000);
    fire(&key, "page");
    minute();
    crate::eventually(60, "the re-notification", || {
        (fakes::mail_sent(&ops).len() >= 2).then_some(())
    });

    // The daily ticket summary, in the 08:00 UTC hour.
    let ticket = format!("webhook_failing:whk_{}", crate::slug("w"));
    fire(&ticket, "ticket");
    minute();
    fakes::advance_to_minute(8 * 60 + 5);
    fire(&ticket, "ticket");
    minute();
    let summary = crate::eventually(60, "the ticket summary", || {
        fakes::mail_sent(&ops)
            .into_iter()
            .find(|m| m.to_string().contains("ticket: "))
    });
    assert!(summary.to_string().contains(&ticket), "{summary}");

    // 25 page alerts in one hour: 20 emails, then one that says 5 more (in a fresh hour).
    fakes::advance(3_600_000);
    let before = fakes::mail_sent(&ops).len();
    for i in 0..25 {
        fire(
            &format!("transport_breaker_open:burst{i}{}", crate::slug("b")),
            "page",
        );
    }
    minute();
    let all = crate::eventually(90, "the capped emails", || {
        let m = fakes::mail_sent(&ops);
        (m.len() >= before + 21).then_some(m)
    });
    assert_eq!(all.len(), before + 21, "20 alerts and one more");
    assert!(all.iter().any(|m| m.to_string().contains("5 more alerts")));

    // read_only: alert email still leaves.
    let r = crate::put(
        "/v1/platform/switches",
        &crate::platform(),
        json!({ "name": "read_only", "state": "on", "reason": "j32 drill" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    fakes::advance(3_600_000);
    let n = fakes::mail_sent(&ops).len();
    fire(
        &format!("transport_breaker_open:ro{}", crate::slug("r")),
        "page",
    );
    minute();
    crate::eventually(60, "the alert email while read-only", || {
        (fakes::mail_sent(&ops).len() > n).then_some(())
    });
    let r = crate::put(
        "/v1/platform/switches",
        &crate::platform(),
        json!({ "name": "read_only", "state": "off", "reason": "j32 drill over" }),
    );
    assert_eq!(r.status, 200);

    // Without an address: `missing`, and nothing is sent.
    fakes::set_config("PM_ALERT_EMAIL", None);
    assert_eq!(status("alert_email"), "missing");
    for _ in 0..3 {
        minute();
    }
}

/// `it::ops::j33_auto_containment` (J33, Observability § 5.6; the Free-plan rule `free_sending_off`
/// arrives with billing, M22): an `rpc_owner_mismatch` turns `read_only` on: a non-platform write gets
/// `503` (`read_only`), a GET still answers, a tenant's queued send waits `queued`, and
/// `containment:read_only` pages until a platform key turns it off and two runs pass; five
/// auto-paused identities across three of one partner's tenants within an hour suspend the partner
/// (`partner.auto_suspend`, `containment:partner_suspended`), five in one tenant do not; a tenant
/// paused for `abuse_burst` keeps `containment:tenant_paused` firing until it is resumed.
#[test]
fn j33_auto_containment() {
    let _r = fakes::Reset;
    let _off = ReadOnlyOff;
    // An owner mismatch: a mailbox called with another tenant's ID.
    let s = send::open_sender("containment", "live");
    let other = crate::tenant(&crate::platform(), "other owner")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mbox = d1(&format!(
        "SELECT mailbox_do_id FROM identities WHERE id = '{}'",
        s.iid
    ))[0]["mailbox_do_id"]
        .as_str()
        .unwrap()
        .to_owned();
    crate::hook(
        "POST",
        "/__test/rpc",
        Some(json!({ "class": "mailbox", "object_id": mbox,
            "envelope": { "v": 1, "tenant_id": other, "identity_id": s.iid, "request_id": "req_mismatch",
                "actor_key_id": null, "deadline_ms": 9_999_999_999_999i64,
                "req": { "op": "confirm_stored", "message_id": "msg_x" } } })),
    );
    crate::eventually(30, "read_only on", || {
        let r = d1("SELECT state, set_by FROM ops_switches WHERE name = 'read_only'");
        (r[0]["state"] == "on" && r[0]["set_by"] == "auto").then_some(())
    });
    fakes::advance(61_000);
    let w = crate::post(
        &format!("/v1/tenants/{}/identities", s.tid),
        &s.key,
        json!({ "username": "blocked", "display_name": "Blocked" }),
    );
    assert_eq!(w.status, 503, "{}", w.body);
    assert_eq!(w.body["error"]["details"]["reason"], "read_only");
    assert_eq!(
        crate::get(&format!("/v1/identities/{}", s.iid), &s.key).status,
        200
    );
    minute();
    assert!(actions("containment:read_only").contains(&"alert.fired".to_owned()));
    assert!(actions("rpc_owner_mismatch").contains(&"alert.fired".to_owned()));

    // Only a platform key undoes it; the page resolves two runs later.
    let r = crate::put(
        "/v1/platform/switches",
        &s.key,
        json!({ "name": "read_only", "state": "off", "reason": "no" }),
    );
    assert_ne!(r.status, 200, "only a platform key: {}", r.body);
    let r = crate::put(
        "/v1/platform/switches",
        &crate::platform(),
        json!({ "name": "read_only", "state": "off", "reason": "fixed" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["set_by"], "platform");
    fakes::advance(61_000);
    minute();
    minute();
    assert_eq!(
        actions("containment:read_only").last().map(String::as_str),
        Some("alert.resolved")
    );

    // Partner: five auto-pauses across three tenants within an hour.
    let (pid, _) = crate::partner("contained partner", &["audit:read"]);
    let mut tenants = Vec::new();
    for i in 0..3 {
        let t = format!("ten_{}", crate::slug(&format!("pt{i}")));
        d1(&format!(
            "INSERT INTO tenants (id, slug, name, mode, address_suffix, suffix_fold, policy_json, quota_do_id, notify_do_id, partner_id, created_at, updated_at) \
             VALUES ('{t}', '{t}', 'x', 'live', '{t}', '{t}', '{{}}', '', '', '{pid}', 1, 1)"
        ));
        tenants.push(t);
    }
    let now = fakes::worker_now();
    let pause = |tenant: &str, n: usize| {
        for _ in 0..n {
            d1(&format!(
                "INSERT INTO audit_log (id, tenant_id, action, target_type, target_id, created_at) VALUES ('aud_{}', '{tenant}', 'identity.auto_pause', 'identity', 'idn_x', {now})",
                crate::slug("p")
            ));
        }
    };
    // Five in one tenant: nothing.
    pause(&tenants[0], 5);
    minute();
    assert_eq!(
        d1(&format!("SELECT status FROM partners WHERE id = '{pid}'"))[0]["status"],
        "active"
    );
    pause(&tenants[1], 1);
    pause(&tenants[2], 1);
    minute();
    assert_eq!(
        d1(&format!("SELECT status FROM partners WHERE id = '{pid}'"))[0]["status"],
        "suspended"
    );
    assert_eq!(
        d1(&format!(
            "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'partner.auto_suspend' AND target_id = '{pid}'"
        ))[0]["n"],
        1
    );
    minute();
    assert!(actions("containment:partner_suspended").contains(&"alert.fired".to_owned()));
    let r = crate::patch(
        &format!("/v1/partners/{pid}"),
        &crate::platform(),
        json!({ "status": "active" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    minute();
    minute();
    assert_eq!(
        actions("containment:partner_suspended")
            .last()
            .map(String::as_str),
        Some("alert.resolved")
    );

    // A tenant paused for an abuse burst: firing while the pause lasts.
    d1(&format!(
        "UPDATE tenants SET sending_paused_at = {now}, sending_pause_reason = 'abuse_burst' WHERE id = '{}'",
        s.tid
    ));
    minute();
    assert!(actions("containment:tenant_paused").contains(&"alert.fired".to_owned()));
    d1(&format!(
        "UPDATE tenants SET sending_paused_at = NULL, sending_pause_reason = NULL WHERE id = '{}'",
        s.tid
    ));
    minute();
    minute();
    assert_eq!(
        actions("containment:tenant_paused")
            .last()
            .map(String::as_str),
        Some("alert.resolved")
    );
}

/// `it::ops::j36_break_glass_key` (J36, Observability § 5.7): with `PM_BREAK_GLASS_KEY_ID` set, the
/// break-glass key mints a new 1-day platform key; the request writes `key.break_glass_used` and fires
/// `break_glass_key_used` (a page) at once; another key's use fires nothing; a revoked break-glass key
/// gets `401`; with the variable unset nothing fires.
#[test]
fn j36_break_glass_key() {
    let _r = fakes::Reset;
    let (id, bg) = crate::mint(
        &crate::platform(),
        json!({ "name": "break-glass", "level": "platform", "permissions": ["keys:manage", "audit:read"] }),
    );
    let fired = || {
        d1(&format!(
            "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'key.break_glass_used' AND actor_key_id = '{id}'"
        ))[0]["n"]
            .as_u64()
            .unwrap()
    };
    // Unset: nothing.
    assert_eq!(crate::get("/v1/me", &bg).status, 200);
    assert_eq!(fired(), 0);
    fakes::set_config("PM_BREAK_GLASS_KEY_ID", Some(&id));
    let before = actions("break_glass_key_used").len();
    let r = crate::post(
        "/v1/keys",
        &bg,
        json!({ "name": "session", "level": "platform", "permissions": ["audit:read"], "expires_at": pylota_identity_core::days::rfc3339(fakes::worker_now() + 86_400_000) }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(fired(), 1);
    assert_eq!(
        actions("break_glass_key_used").len(),
        before + 1,
        "fired at once"
    );
    // Another key's use fires nothing.
    let session = r.body["secret"].as_str().unwrap().to_owned();
    assert_eq!(crate::get("/v1/me", &session).status, 200);
    assert_eq!(fired(), 1);
    // Revoked: 401.
    let r = crate::delete(&format!("/v1/keys/{id}"), &crate::platform());
    assert!(r.status < 300, "{}", r.body);
    assert_eq!(crate::get("/v1/me", &bg).status, 401);
}

/// `it::ops::heartbeat_missing` (J32): with `PM_HEARTBEAT_KEY_ID` set, `heartbeat_missing` stays quiet
/// in a new key's first hour, fires once the key's `last_used_at` (or, never used, `created_at`) is more
/// than an hour old, resolves after the key is used again, and fires when the key is revoked.
#[test]
fn heartbeat_missing() {
    let _r = fakes::Reset;
    let (id, key) = crate::mint(
        &crate::platform(),
        json!({ "name": "heartbeat", "level": "platform", "permissions": ["audit:read"] }),
    );
    fakes::set_config("PM_HEARTBEAT_KEY_ID", Some(&id));
    let before = actions("heartbeat_missing");
    let last = || actions("heartbeat_missing").last().cloned();
    minute();
    assert_eq!(
        actions("heartbeat_missing").len(),
        before.len(),
        "not in its first hour"
    );
    fakes::advance(61 * 60_000);
    minute();
    assert_eq!(last().as_deref(), Some("alert.fired"));
    assert_eq!(crate::get("/v1/platform/status", &key).status, 200);
    crate::eventually(30, "last_used_at", || {
        d1(&format!(
            "SELECT last_used_at FROM api_keys WHERE id = '{id}'"
        ))[0]["last_used_at"]
            .as_i64()
            .filter(|t| *t > fakes::worker_now() - 120_000)
    });
    minute();
    minute();
    assert_eq!(last().as_deref(), Some("alert.resolved"));
    let r = crate::delete(&format!("/v1/keys/{id}"), &crate::platform());
    assert!(r.status < 300, "{}", r.body);
    minute();
    assert_eq!(last().as_deref(), Some("alert.fired"));
    fakes::set_config("PM_HEARTBEAT_KEY_ID", None);
    minute();
    minute();
}

/// `it::ops::job_failed_alerts` (Observability § 5.3): a failed retention job fires
/// `job_failed:retention` (a page) and a failed backup `job_failed:backup` (a ticket); each resolves
/// once a later job of the same kind and tenant completes.
#[test]
fn job_failed_alerts() {
    let _r = fakes::Reset;
    let t = crate::tenant(&crate::platform(), "job alerts")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let now = fakes::worker_now();
    let job = |kind: &str, status: &str, at: i64| {
        d1(&format!(
            "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_at, updated_at) VALUES ('job_{}', '{t}', '{kind}', '{status}', '', '{{}}', {at}, {at})",
            crate::slug(kind)
        ));
    };
    job("retention", "failed", now);
    job("backup", "failed", now);
    minute();
    let sev = |k: &str| {
        rows(k)
            .last()
            .map(|r| (r.0.clone(), r.1["severity"].clone()))
    };
    assert_eq!(
        sev("job_failed:retention"),
        Some(("alert.fired".into(), json!("page")))
    );
    assert_eq!(
        sev("job_failed:backup"),
        Some(("alert.fired".into(), json!("ticket")))
    );
    job("retention", "completed", now + 1);
    job("backup", "completed", now + 1);
    minute();
    minute();
    assert_eq!(sev("job_failed:retention").unwrap().0, "alert.resolved");
    assert_eq!(sev("job_failed:backup").unwrap().0, "alert.resolved");
}

/// `it::ops::domain_rate_state_alerts` (Observability § 5.3): `RecordOutcome` with `domain_id` keeps
/// per-domain counters; 2 bounces in 51 outcomes within an hour fire `bounce_rate:{tenant}:{domain}`
/// through the delivery consumer, and another domain of the tenant counts apart (the complaint
/// decision, 1 in 200 within 24 hours, is `worker::quota::domain_rate_alerts`).
#[test]
fn domain_rate_state_alerts() {
    use crate::delivery::{cf_event, domain_of, now_rfc3339, outcomes, provider_id};
    let _r = fakes::Reset;
    let s = send::open_sender("domain rates", "live");
    let dom = domain_of(&s.addr);
    let other = format!("dom_{}", crate::slug("od"));
    let ids: Vec<String> = (0..10).map(|i| format!("idn_rates{i:02}")).collect();
    // Domain one: 49 delivered and a bounce, then a real bounce event (2 of 51).
    outcomes(&s.tid, &ids, &dom, 49, "delivered");
    outcomes(&s.tid, &ids, &dom, 1, "bounced");
    let mid = send::send_ok(&s, &["bounce-me@example.net"], "rates");
    send::wait_status(&s, &mid, "submitted");
    let pid = provider_id(&s.iid, &mid);
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "bounced",
            &pid,
            &s.addr,
            "bounce-me@example.net",
            None,
            &now_rfc3339(0),
        ),
    );
    let bounce_key = format!("bounce_rate:{}:{dom}", s.tid);
    crate::eventually(60, "the bounce_rate report", || {
        (!rows(&bounce_key).is_empty()).then_some(())
    });
    assert!(
        rows(&format!("bounce_rate:{}:{other}", s.tid)).is_empty(),
        "per domain"
    );
    // The complaint decision (1 in 200 within 24 hours) is the quota's, checked by
    // `worker::quota::domain_rate_alerts`; the consumer reports whatever it decides, as above.
    // The tenant's other domain has its own counters: outcomes there fire nothing for this one.
    outcomes(&s.tid, &ids, &other, 40, "bounced");
    assert!(rows(&format!("bounce_rate:{}:{other}", s.tid)).is_empty());
    minute();
    minute();
}

/// `it::ops::freeze_holds_everything` (Observability § 9, Restore from PITR step 2; the SES and Stripe
/// hooks arrive with M23 and M22): with `PM_FREEZE = "on"`, `email()` answers a temporary failure
/// before reading anything (`tempfail_frozen`); tenant keys get `503 unavailable` (`frozen`) with
/// `Retry-After: 300` while platform keys work; a send queued by a platform key stays `queued`, its
/// pointer re-enqueued; the crons only log; `/health` shows `frozen: true`; and
/// `POST /v1/platform/identities/{id}/restore` answers `409 not_frozen` once unfrozen. Durable
/// Objects keep the configuration they started with in the harness, so their alarm hold is
/// checked natively.
#[test]
fn freeze_holds_everything() {
    let _r = fakes::Reset;
    let s = send::open_sender("frozen", "live");
    fakes::set_config("PM_FREEZE", Some("on"));
    fakes::set_config("PM_FREEZE_SINCE", Some("2026-10-30T10:00:00Z"));
    assert_ne!(
        crate::inject(
            "a@frozen.example",
            &s.addr,
            &crate::mail("a@frozen.example", &s.addr, "while frozen", "x")
        ),
        200,
        "a temporary failure"
    );
    let r = crate::get(&format!("/v1/identities/{}", s.iid), &s.key);
    assert_eq!(r.status, 503, "{}", r.body);
    assert_eq!(r.body["error"]["details"]["reason"], "frozen");
    assert_eq!(r.header("retry-after"), Some("300"));
    assert_eq!(
        crate::get(&format!("/v1/identities/{}", s.iid), &crate::platform()).status,
        200
    );
    let health = crate::call("GET", "/health", None, None, &[]);
    assert_eq!(health.body["frozen"], true, "{}", health.body);
    let status = crate::get("/v1/platform/status", &crate::platform()).body;
    assert_eq!(status["frozen"], true);
    assert_eq!(status["frozen_since"], "2026-10-30T10:00:00Z");

    // A platform key's send is accepted and waits.
    let r = send::submit(
        &format!("/v1/identities/{}/messages", s.iid),
        &crate::platform(),
        &crate::slug("fz"),
        json!({ "to": ["x@example.net"], "subject": "held", "text": "t" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let mid = r.body["id"].as_str().unwrap().to_owned();
    minute();
    std::thread::sleep(std::time::Duration::from_secs(3));
    let m = crate::get(
        &format!("/v1/identities/{}/messages/{mid}", s.iid),
        &crate::platform(),
    );
    assert_eq!(
        m.body["status"], "queued",
        "held, never canceled: {}",
        m.body
    );

    // The restore endpoint: allowed only while frozen; local development has no PITR.
    let r = crate::post(
        &format!("/v1/platform/identities/{}/restore", s.iid),
        &crate::platform(),
        json!({ "bookmark": "local" }),
    );
    assert_ne!(r.status, 409, "{}", r.body);
    fakes::set_config("PM_FREEZE", None);
    fakes::set_config("PM_FREEZE_SINCE", None);
    let r = crate::post(
        &format!("/v1/platform/identities/{}/restore", s.iid),
        &crate::platform(),
        json!({ "bookmark": "local" }),
    );
    assert_eq!(r.status, 409, "{}", r.body);
    assert_eq!(r.body["error"]["code"], "not_frozen");
}
