//! Retention through workerd (Privacy and erasure § 5): the tenant job's raw, message and event
//! purges (I4), the global job's steps, and the nightly backup copy.

use serde_json::{Value, json};

use crate::erasure::{job_alarm, run_job};
use crate::send::open_sender;
use crate::{
    d1, eventually, fakes, get, hook, inbound, mail, mailbox_query, messages_of, patch, platform,
    post,
};

fn r2_keys(prefix: &str, backup: bool) -> Vec<String> {
    let mut body = json!({ "op": "list", "key": prefix });
    if backup {
        body["bucket"] = "backup".into();
    }
    hook("POST", "/__test/r2", Some(body)).body["keys"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|k| k.as_str().map(str::to_owned))
        .collect()
}

fn received(s: &crate::send::Sender, from: &str, subject: &str) -> Value {
    let (o, smtp) = inbound(from, &s.addr, &mail(from, &s.addr, subject, "x"));
    assert_eq!(o, "accepted", "{smtp}");
    eventually(30, "the message", || {
        messages_of(&s.iid, &s.key, "")
            .into_iter()
            .find(|m| m["subject"] == subject)
    })
}

fn audits(tenant: &str) -> Vec<Value> {
    d1(&format!(
        "SELECT details_json FROM audit_log WHERE tenant_id = '{tenant}' AND action = 'retention.purge'"
    ))
    .as_array()
    .cloned()
    .unwrap_or_default()
}

/// I4: raw MIME older than `raw_days` is deleted (from both buckets), the raw endpoint answers
/// `410 raw_expired`, and the purge is audit-logged.
#[test]
fn i4_raw() {
    let _reset = fakes::Reset;
    let s = open_sender("i4 raw", "live");
    let m = received(&s, "una@i4.example", "i4 raw");
    let id = m["id"].as_str().unwrap();
    let raw = format!("/v1/identities/{}/messages/{id}/raw", s.iid);
    assert_eq!(get(&raw, &s.key).status, 200);
    fakes::advance(91 * 86_400_000);
    let (_, st) = run_job("retention", Some(&s.tid), json!({}));
    assert_eq!(st, "completed");
    let r = get(&raw, &s.key);
    assert_eq!((r.status, r.code()), (410, "raw_expired"));
    let prefix = format!("t/{}/i/{}/m/{id}/raw.eml", s.tid, s.iid);
    assert!(r2_keys(&prefix, false).is_empty() && r2_keys(&prefix, true).is_empty());
    let a = audits(&s.tid);
    assert!(
        a.iter().any(|d| d["details_json"]
            .as_str()
            .unwrap_or("")
            .contains("\"step\":\"raw\"")),
        "{a:?}"
    );
    // The message itself stays (no message_days).
    assert!(
        messages_of(&s.iid, &s.key, "")
            .iter()
            .any(|x| x["id"] == id)
    );
}

/// I4: with `message_days` set, messages and their index rows are purged, except held threads.
#[test]
fn i4_messages() {
    let _reset = fakes::Reset;
    let s = open_sender("i4 messages", "live");
    let r = patch(
        &format!("/v1/tenants/{}/policy", s.tid),
        &s.key,
        json!({ "retention": { "message_days": 1 } }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let gone = received(&s, "vic@i4m.example", "i4 goes");
    let kept = received(&s, "vic@i4m.example", "i4 held");
    let r = post(
        &format!(
            "/v1/identities/{}/threads/{}/hold",
            s.iid,
            kept["thread_id"].as_str().unwrap()
        ),
        &platform(),
        json!({ "reason": "audit" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    fakes::advance(2 * 86_400_000);
    let (_, st) = run_job("retention", Some(&s.tid), json!({}));
    assert_eq!(st, "completed");
    let ids: Vec<Value> = messages_of(&s.iid, &s.key, "")
        .iter()
        .map(|m| m["id"].clone())
        .collect();
    assert!(!ids.contains(&gone["id"]));
    assert!(ids.contains(&kept["id"]));
    let fts = mailbox_query(&s.iid, "SELECT COUNT(*) AS n FROM refs")[0]["n"]
        .as_i64()
        .unwrap();
    assert!(fts <= 2, "the purged message's refs went with it");
}

/// I4: events older than `events_days` leave the outbox and the event index.
#[test]
fn i4_events() {
    let _reset = fakes::Reset;
    let s = open_sender("i4 events", "live");
    let r = patch(
        &format!("/v1/tenants/{}/policy", s.tid),
        &s.key,
        json!({ "retention": { "events_days": 10 } }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    received(&s, "wyn@i4e.example", "i4 event");
    eventually(30, "the outbox to dispatch", || {
        (mailbox_query(
            &s.iid,
            "SELECT COUNT(*) AS n FROM outbox WHERE dispatched_at IS NULL",
        )[0]["n"]
            == 0)
            .then_some(())
    });
    let before = mailbox_query(&s.iid, "SELECT COUNT(*) AS n FROM outbox")[0]["n"]
        .as_i64()
        .unwrap();
    assert!(before > 0);
    fakes::advance(11 * 86_400_000);
    let (_, st) = run_job("retention", Some(&s.tid), json!({}));
    assert_eq!(st, "completed");
    // Dispatched events past the cutoff are gone (a row still waiting for dispatch stays).
    let left = mailbox_query(
        &s.iid,
        "SELECT type, dispatched_at FROM outbox WHERE dispatched_at IS NOT NULL",
    );
    assert!(left.is_empty(), "{left:?}");
    let n = d1(&format!(
        "SELECT COUNT(*) AS n FROM event_index WHERE tenant_id = '{}' AND type LIKE 'message.%'",
        s.tid
    ))[0]["n"]
        .as_i64()
        .unwrap();
    assert_eq!(n, 0);
}

/// The global job runs its steps in order, deletes exactly the rows past their cutoff, re-queues
/// unrouted outage copies, and writes one audit row per step.
#[test]
fn global_job_steps() {
    let s = open_sender("global steps", "live");
    let t = &s.tid;
    d1(&format!(
        "INSERT INTO idempotency_records (scope, key_id, tenant_id, idem_key, method, path, fingerprint, status, created_at, expires_at) VALUES ('{t}', 'k', '{t}', 'old-{t}', 'POST', '/x', 'f', 'completed', 1, 2), ('{t}', 'k', '{t}', 'new-{t}', 'POST', '/x', 'f', 'completed', 1, 99999999999999); \
         INSERT INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, occurred_at) VALUES ('evt_gold{t}', NULL, NULL, 'webhook.test', 'platform', 'x', 1); \
         INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_at, updated_at, completed_at) VALUES ('job_gold{t}', '{t}', 'reindex', 'completed', 'x', '{{}}', 1, 1, 1); \
         INSERT INTO usage_daily (tenant_id, day, metric, value) VALUES ('{t}', '2020-01-01', 'sends', 3), ('{t}', '2999-01-01', 'sends', 3); \
         INSERT INTO dlq_items (id, queue, message_id, body_json, body_sha256, tenant_id, first_seen_at) VALUES ('dlq_gold{t}', 'pm-inbound', 'm', '{{}}', 'h', '{t}', 1)"
    ));
    let staged = format!(
        "inbound-staging/2026/01/01/{}.eml",
        crate::slug("global staged")
    );
    let r = hook(
        "POST",
        "/__test/r2",
        Some(
            json!({ "op": "put", "key": staged, "body_base64": "eA==", "custom": {
            "envelope_from": "a@b.example", "envelope_to": "nobody.none@agents.example", "received_at": "1000" } }),
        ),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let (jid, st) = run_job("retention", None, json!({ "global": true }));
    assert_eq!(st, "completed");
    let steps: Vec<String> = job_alarm(&jid)["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        steps,
        [
            "idempotency",
            "platform_events",
            "jobs",
            "usage",
            "dlq",
            "signing_keys",
            "identity_keys",
            "staging",
            "held_erasures",
            "audit"
        ]
    );
    let n = |sql: &str| d1(sql)[0]["n"].as_i64().unwrap();
    assert_eq!(
        n(&format!(
            "SELECT COUNT(*) AS n FROM idempotency_records WHERE tenant_id = '{t}'"
        )),
        1
    );
    assert_eq!(
        n(&format!(
            "SELECT COUNT(*) AS n FROM event_index WHERE id = 'evt_gold{t}'"
        )),
        0
    );
    assert_eq!(
        n(&format!(
            "SELECT COUNT(*) AS n FROM jobs WHERE id = 'job_gold{t}'"
        )),
        0
    );
    assert_eq!(
        n(&format!(
            "SELECT COUNT(*) AS n FROM usage_daily WHERE tenant_id = '{t}'"
        )),
        1
    );
    assert_eq!(
        n(&format!(
            "SELECT COUNT(*) AS n FROM dlq_items WHERE id = 'dlq_gold{t}'"
        )),
        0
    );
    let staging = job_alarm(&jid)["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "staging")
        .cloned()
        .unwrap();
    assert!(
        staging["counts_json"]
            .as_str()
            .unwrap()
            .contains("\"staging_requeued\":"),
        "{staging}"
    );
    let audit = d1("SELECT COUNT(*) AS n FROM audit_log WHERE tenant_id IS NULL AND action = 'retention.purge' AND target_id = 'global'")[0]["n"]
        .as_i64()
        .unwrap();
    assert!(audit >= 8);
}

/// The nightly backup copies new objects with their metadata, never deletes, and a retention purge
/// removes the key from both buckets.
#[test]
fn backup_copy() {
    let _reset = fakes::Reset;
    let s = open_sender("backup copy", "live");
    let m = received(&s, "xia@backup.example", "backup me");
    let key = format!(
        "t/{}/i/{}/m/{}/raw.eml",
        s.tid,
        s.iid,
        m["id"].as_str().unwrap()
    );
    let (_, st) = run_job("backup", None, json!({}));
    assert_eq!(st, "completed");
    assert_eq!(r2_keys(&key, true), std::slice::from_ref(&key));
    let size = |backup: bool| {
        let mut b = json!({ "op": "head", "key": key });
        if backup {
            b["bucket"] = "backup".into();
        }
        hook("POST", "/__test/r2", Some(b)).body["size"].clone()
    };
    assert_eq!(size(true), size(false));
    fakes::advance(91 * 86_400_000);
    run_job("retention", Some(&s.tid), json!({}));
    assert!(r2_keys(&key, false).is_empty() && r2_keys(&key, true).is_empty());
}

/// The global job's `identity_keys` step: a `retiring` key past `verify_until` becomes `retired` and
/// keeps its row, so its thumbprint is never reused (Agent signing keys § 2).
#[test]
fn global_identity_keys() {
    let _reset = crate::fakes::Reset;
    let s = crate::assertions::signer("global_identity_keys");
    let first = crate::assertions::token(&s)["kid"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = crate::post(
        &format!("/v1/identities/{}/keys/rotate", s.iid),
        &s.key,
        json!({}),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    crate::fakes::advance(7 * 86_400_000 + 1_000);
    let (_, st) = run_job("retention", None, json!({ "global": true }));
    assert_eq!(st, "completed");
    let row = &d1(&format!(
        "SELECT status, retired_at FROM identity_keys WHERE id = '{first}'"
    ))[0];
    assert_eq!(row["status"], "retired");
    assert!(row["retired_at"].is_i64());
}
