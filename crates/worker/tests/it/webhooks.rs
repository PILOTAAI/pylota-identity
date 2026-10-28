//! Webhooks in workerd (Webhooks and events, build plan M8): endpoints, fan-out, signed delivery to the
//! fake receivers, retries, auto-disable, partner scope, the J13 hold, replay and J29.

use serde_json::{Value, json};

use crate::fakes::{self, RECEIVERS, Received};
use crate::{
    alarm, d1, eventually, get, identity, mint, partner, patch, platform, post, slug, tenant,
};

/// A receiver URL and its target as the fake server records it.
fn receiver(name: &str) -> (String, String) {
    let host = format!("{}{RECEIVERS}", slug(name));
    (format!("https://{host}/hook"), format!("{host}/hook"))
}

fn endpoint(path: &str, key: &str, url: &str, events: Value) -> Value {
    let r = post(path, key, json!({ "url": url, "events": events }));
    assert_eq!(r.status, 201, "{}", r.body);
    r.body
}

/// An `identity.created` event in `tenant_id`, dispatched by the mailbox's outbox now.
fn event(key: &str, tenant_id: &str, name: &str) -> String {
    let idn = identity(key, tenant_id, &slug(name)[..8]);
    let id = idn["id"].as_str().unwrap().to_owned();
    alarm(&id);
    id
}

fn wait_received(target: &str, n: usize) -> Vec<Received> {
    eventually(40, &format!("{n} request(s) to {target}"), || {
        let r = fakes::received(target);
        (r.len() >= n).then_some(r)
    })
}

fn deliveries(endpoint_id: &str) -> Vec<Value> {
    d1(&format!(
        "SELECT event_id, event_type, attempt, status, error, next_attempt_at, created_at FROM webhook_deliveries WHERE endpoint_id = '{endpoint_id}' ORDER BY attempt"
    ))
    .as_array()
    .cloned()
    .unwrap_or_default()
}

fn tenant_key(tid: &str, perms: &[&str]) -> String {
    mint(
        &platform(),
        json!({ "name": "k", "level": "tenant", "tenant_id": tid, "permissions": perms }),
    )
    .1
}

const MAIL: &[&str] = &[
    "webhooks:manage",
    "messages:read",
    "quarantine:review",
    "identities:write",
];

#[test]
fn j29_mail_events_need_messages_read() {
    let t = tenant(&platform(), "j29");
    let tid = t["id"].as_str().unwrap();
    let only = tenant_key(tid, &["webhooks:manage"]);
    let path = format!("/v1/tenants/{tid}/webhooks");
    let (url, target) = receiver("j29");
    for (events, required) in [
        (json!(["*"]), json!(["messages:read", "quarantine:review"])),
        (json!(["message.received"]), json!(["messages:read"])),
        (
            json!(["message.quarantined"]),
            json!(["messages:read", "quarantine:review"]),
        ),
    ] {
        let r = post(&path, &only, json!({ "url": url, "events": events }));
        assert_eq!(
            (r.status, r.code()),
            (403, "permission_denied"),
            "{}",
            r.body
        );
        assert_eq!(r.details()["required"], required);
    }
    let ok = endpoint(&path, &only, &url, json!(["identity.created"]));
    let id = ok["id"].as_str().unwrap();
    let r = patch(
        &format!("/v1/webhooks/{id}"),
        &only,
        json!({ "events": ["message.received"] }),
    );
    assert_eq!(r.status, 403);
    // A replay to an endpoint receiving mail events needs the mail permissions too.
    let full = tenant_key(tid, MAIL);
    let mail = endpoint(&path, &full, &url, json!(["message.received"]));
    let r = post(
        &format!("/v1/webhooks/{}/replay", mail["id"].as_str().unwrap()),
        &only,
        json!({ "since": "2026-01-01T00:00:00Z" }),
    );
    assert_eq!(r.status, 403);
    // A changed URL disables the endpoint until a test to it passes.
    let (url2, target2) = receiver("j29b");
    let r = patch(&format!("/v1/webhooks/{id}"), &only, json!({ "url": url2 }));
    assert_eq!(
        (
            r.body["enabled"].as_bool(),
            r.body["disabled_reason"].as_str()
        ),
        (Some(false), Some("url_changed"))
    );
    let r = patch(
        &format!("/v1/webhooks/{id}"),
        &only,
        json!({ "enabled": true }),
    );
    assert_eq!(r.status, 400);
    let r = post(&format!("/v1/webhooks/{id}/test"), &only, json!({}));
    assert_eq!(r.body["status"], "succeeded", "{}", r.body);
    assert_eq!(
        get(&format!("/v1/webhooks/{id}"), &only).body["enabled"],
        true
    );
    assert_eq!(fakes::received(&target2).len(), 1);
    assert!(fakes::received(&target).is_empty());
}

#[test]
fn j4_retry_schedule() {
    let t = tenant(&platform(), "j4");
    let tid = t["id"].as_str().unwrap();
    let k = tenant_key(tid, MAIL);
    let (url, target) = receiver("j4");
    fakes::respond(&target, &[500]);
    let e = endpoint(
        &format!("/v1/tenants/{tid}/webhooks"),
        &k,
        &url,
        json!(["identity.created"]),
    );
    let id = e["id"].as_str().unwrap();
    event(&k, tid, "j4");
    // Thirteen attempts (the itest build queues each after one second, and records the scheduled
    // time), then dead.
    let rows = eventually(90, "13 attempts", || {
        let d = deliveries(id);
        (d.len() >= 13).then_some(d)
    });
    let schedule = [
        30, 120, 600, 1800, 3600, 7200, 14400, 28800, 43200, 43200, 43200, 68400,
    ];
    for (i, s) in schedule.iter().enumerate() {
        let r = &rows[i];
        assert_eq!(
            (r["attempt"].as_i64(), r["status"].as_str()),
            (Some(i as i64 + 1), Some("failed"))
        );
        let wait =
            (r["next_attempt_at"].as_i64().unwrap() - r["created_at"].as_i64().unwrap()) / 1000;
        let (lo, hi) = (
            (f64::from(*s) * 0.9).floor() as i64 - 1,
            (f64::from(*s) * 1.1).ceil() as i64 + 1,
        );
        assert!(
            (lo..=hi).contains(&wait),
            "attempt {}: {wait} s not within {lo}..{hi}",
            i + 1
        );
    }
    assert_eq!(
        (
            rows[12]["status"].as_str(),
            rows[12]["next_attempt_at"].as_i64()
        ),
        (Some("dead"), None)
    );
    assert_eq!(fakes::received(&target).len(), 13);
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert_eq!(deliveries(id).len(), 13, "nothing after the 13th attempt");
}

#[test]
fn disable_on_410() {
    let p = platform();
    let t = tenant(&p, "gone");
    let tid = t["id"].as_str().unwrap();
    let k = tenant_key(tid, MAIL);
    let (url, target) = receiver("gone");
    fakes::respond(&target, &[410]);
    let e = endpoint(
        &format!("/v1/tenants/{tid}/webhooks"),
        &k,
        &url,
        json!(["identity.created"]),
    );
    let id = e["id"].as_str().unwrap();
    event(&k, tid, "gone");
    eventually(30, "the endpoint disabled", || {
        (get(&format!("/v1/webhooks/{id}"), &k).body["disabled_reason"] == "failing").then_some(())
    });
    let ev = d1(&format!(
        "SELECT payload_json FROM event_index WHERE type = 'webhook.disabled' AND tenant_id = '{tid}'"
    ));
    let env: Value = serde_json::from_str(ev[0]["payload_json"].as_str().unwrap()).unwrap();
    assert_eq!(env["data"], json!({ "webhook_id": id, "reason": "gone" }));
    assert_eq!(fakes::received(&target).len(), 1);
}

#[test]
fn j15_partner_scope_filter() {
    let p = platform();
    let perms = [
        "tenants:manage",
        "webhooks:manage",
        "messages:read",
        "quarantine:review",
        "identities:write",
    ];
    let (_, pk) = partner("j15p", &perms);
    let (_, qk) = partner("j15q", &perms);
    let tp = tenant(&pk, "j15_tp");
    let tq = tenant(&qk, "j15_tq");
    let tn = tenant(&p, "j15_none");
    let (pu, pt) = receiver("j15p");
    let (pu2, pt2) = receiver("j15p2");
    let (qu, qt) = receiver("j15q");
    let (tu, tt) = receiver("j15t");
    let pe = endpoint("/v1/webhooks", &pk, &pu, json!(["*"]));
    assert_eq!(
        (pe["scope"].as_str(), pe["partner_id"].is_string()),
        (Some("partner"), true)
    );
    endpoint("/v1/webhooks", &pk, &pu2, json!(["*"]));
    endpoint("/v1/webhooks", &qk, &qu, json!(["*"]));
    let tpid = tp["id"].as_str().unwrap();
    endpoint(
        &format!("/v1/tenants/{tpid}/webhooks"),
        &pk,
        &tu,
        json!(["*"]),
    );
    event(&pk, tpid, "j15a");
    event(&qk, tq["id"].as_str().unwrap(), "j15b");
    event(&p, tn["id"].as_str().unwrap(), "j15c");
    wait_received(&pt, 1);
    wait_received(&qt, 1);
    wait_received(&tt, 1);
    std::thread::sleep(std::time::Duration::from_secs(4));
    let tenant_of = |r: &Received| r.json()["tenant_id"].as_str().unwrap_or("").to_owned();
    assert!(fakes::received(&pt).iter().all(|r| tenant_of(r) == tpid));
    assert!(
        fakes::received(&qt)
            .iter()
            .all(|r| tenant_of(r) == tq["id"].as_str().unwrap())
    );
    assert!(fakes::received(&tt).iter().all(|r| tenant_of(r) == tpid));
    // Replay to P's endpoint selects P's tenants only.
    let r = post(
        &format!("/v1/webhooks/{}/replay", pe["id"].as_str().unwrap()),
        &pk,
        json!({ "since": "2026-01-01T00:00:00Z" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    std::thread::sleep(std::time::Duration::from_secs(4));
    assert!(fakes::received(&pt).iter().all(|r| tenant_of(r) == tpid));
    // A 410 on P's endpoint: webhook.disabled to P's other endpoint, never to Q's or the tenant's.
    fakes::respond(&pt, &[410]);
    event(&pk, tpid, "j15d");
    eventually(40, "webhook.disabled at P's other endpoint", || {
        fakes::received(&pt2)
            .iter()
            .any(|r| r.json()["type"] == "webhook.disabled")
            .then_some(())
    });
    std::thread::sleep(std::time::Duration::from_secs(3));
    for t in [&qt, &tt] {
        assert!(
            !fakes::received(t)
                .iter()
                .any(|r| r.json()["type"] == "webhook.disabled")
        );
    }
    // At most 20 partner endpoints.
    for i in 2..20 {
        endpoint(
            "/v1/webhooks",
            &pk,
            &receiver(&format!("j15n{i}")).0,
            json!(["identity.created"]),
        );
    }
    let r = post(
        "/v1/webhooks",
        &pk,
        json!({ "url": pu, "events": ["identity.created"] }),
    );
    assert_eq!((r.status, r.code()), (422, "webhook_limit_reached"));
}

#[test]
fn j13_held_while_partner_suspended() {
    let _reset = fakes::Reset;
    let p = platform();
    let perms = [
        "tenants:manage",
        "webhooks:manage",
        "messages:read",
        "quarantine:review",
        "identities:write",
    ];
    let (pid, pk) = partner("j13", &perms);
    let t = tenant(&pk, "j13");
    let tid = t["id"].as_str().unwrap();
    let (pu, pt) = receiver("j13p");
    let (tu, tt) = receiver("j13t");
    let pe = endpoint("/v1/webhooks", &pk, &pu, json!(["identity.created"]));
    let te = endpoint(
        &format!("/v1/tenants/{tid}/webhooks"),
        &pk,
        &tu,
        json!(["identity.created"]),
    );
    assert_eq!(
        patch(
            &format!("/v1/partners/{pid}"),
            &p,
            json!({ "status": "suspended" })
        )
        .status,
        200
    );
    event(&p, tid, "j13");
    let held = eventually(30, "two held deliveries", || {
        let h = d1(&format!(
            "SELECT endpoint_id FROM webhook_held WHERE partner_id = '{pid}'"
        ));
        (h.as_array().unwrap().len() >= 2).then_some(h)
    });
    assert_eq!(held.as_array().unwrap().len(), 2);
    for id in [&pe["id"], &te["id"]] {
        assert!(deliveries(id.as_str().unwrap()).is_empty());
    }
    assert!(fakes::received(&pt).is_empty() && fakes::received(&tt).is_empty());
    // Active again: the every-minute cron releases both.
    assert_eq!(
        patch(
            &format!("/v1/partners/{pid}"),
            &p,
            json!({ "status": "active" })
        )
        .status,
        200
    );
    crate::cron("* * * * *");
    wait_received(&pt, 1);
    wait_received(&tt, 1);
    assert!(
        d1(&format!(
            "SELECT 1 FROM webhook_held WHERE partner_id = '{pid}'"
        ))
        .as_array()
        .unwrap()
        .is_empty()
    );
    // Held past the replay window: dead with event_unavailable.
    assert_eq!(
        patch(
            &format!("/v1/partners/{pid}"),
            &p,
            json!({ "status": "suspended" })
        )
        .status,
        200
    );
    event(&p, tid, "j13late");
    eventually(30, "held again", || {
        (!d1(&format!(
            "SELECT 1 FROM webhook_held WHERE partner_id = '{pid}'"
        ))
        .as_array()
        .unwrap()
        .is_empty())
        .then_some(())
    });
    fakes::advance(31 * 86_400_000);
    crate::cron("* * * * *");
    eventually(30, "the stale hold recorded dead", || {
        d1(&format!("SELECT 1 FROM webhook_deliveries WHERE endpoint_id = '{}' AND error = 'event_unavailable'", pe["id"].as_str().unwrap()))
            .as_array()
            .filter(|a| !a.is_empty())
            .map(|_| ())
    });
}

#[test]
fn replay_by_ids_and_window() {
    let _reset = fakes::Reset;
    let t = tenant(&platform(), "replay");
    let tid = t["id"].as_str().unwrap();
    let k = tenant_key(tid, MAIL);
    let (url, target) = receiver("replay");
    let e = endpoint(
        &format!("/v1/tenants/{tid}/webhooks"),
        &k,
        &url,
        json!(["identity.created"]),
    );
    let id = e["id"].as_str().unwrap();
    event(&k, tid, "replay1");
    let first = wait_received(&target, 1);
    let ev = first[0].header("webhook-id").unwrap().to_owned();
    // By ID: delivered again with the same webhook-id.
    let r = post(
        &format!("/v1/webhooks/{id}/replay"),
        &k,
        json!({ "event_ids": [ev] }),
    );
    assert_eq!(r.body["queued"], 1, "{}", r.body);
    let again = wait_received(&target, 2);
    assert_eq!(again[1].header("webhook-id"), Some(ev.as_str()));
    // A webhook.test event is never replayed, also when named.
    let test = post(&format!("/v1/webhooks/{id}/test"), &k, json!({}));
    let r = post(
        &format!("/v1/webhooks/{id}/replay"),
        &k,
        json!({ "event_ids": [test.body["event_id"]] }),
    );
    assert_eq!(r.body["queued"], 0);
    // By window with status dead: only events whose deliveries died.
    fakes::respond(&target, &[500]);
    let r = post(
        &format!("/v1/webhooks/{id}/replay"),
        &k,
        json!({ "since": "2026-01-01T00:00:00Z", "status": "dead" }),
    );
    assert_eq!(r.body["queued"], 0);
    // 30 days from occurred_at: later, nothing can be replayed.
    fakes::advance(31 * 86_400_000);
    let r = post(
        &format!("/v1/webhooks/{id}/replay"),
        &k,
        json!({ "event_ids": [ev] }),
    );
    assert_eq!(r.body["queued"], 0);
}

#[test]
fn system_identity_events_platform_only() {
    let p = platform();
    let sys = crate::system_identity();
    let def = crate::default_tenant();
    let (pu, pt) = receiver("sysp");
    let (tu, tt) = receiver("syst");
    let pe = endpoint("/v1/webhooks", &p, &pu, json!(["identity.updated"]));
    let te = endpoint(
        &format!("/v1/tenants/{def}/webhooks"),
        &p,
        &tu,
        json!(["identity.updated"]),
    );
    assert_eq!(
        patch(
            &format!("/v1/identities/{sys}"),
            &p,
            json!({ "display_name": slug("sys") })
        )
        .status,
        200
    );
    alarm(&sys);
    let got = wait_received(&pt, 1);
    assert_eq!(got.last().unwrap().json()["identity_id"], sys.as_str());
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(fakes::received(&tt).is_empty());
    let r = post(
        &format!("/v1/webhooks/{}/replay", te["id"].as_str().unwrap()),
        &p,
        json!({ "since": "2026-01-01T00:00:00Z" }),
    );
    assert_eq!(r.body["queued"], 0, "{}", r.body);
    // Platform endpoints are shared by the suite: remove this one.
    assert_eq!(
        crate::delete(&format!("/v1/webhooks/{}", pe["id"].as_str().unwrap()), &p).status,
        204
    );
}

#[test]
fn deleted_identity_event_unavailable() {
    let t = tenant(&platform(), "gone_idn");
    let tid = t["id"].as_str().unwrap();
    let k = tenant_key(tid, MAIL);
    let (url, target) = receiver("goneidn");
    // The first attempt fails, so the second comes after the mailbox is erased.
    fakes::respond(&target, &[500, 200]);
    let e = endpoint(
        &format!("/v1/tenants/{tid}/webhooks"),
        &k,
        &url,
        json!(["identity.created"]),
    );
    let id = e["id"].as_str().unwrap();
    let idn = event(&k, tid, "goneidn");
    wait_received(&target, 1);
    crate::mailbox_query(
        &idn,
        "UPDATE meta SET v = '1' WHERE k = 'erased' RETURNING k",
    );
    let rows = eventually(30, "the attempt recorded dead", || {
        let d = deliveries(id);
        d.iter().any(|r| r["status"] == "dead").then_some(d)
    });
    let dead = rows.iter().find(|r| r["status"] == "dead").unwrap();
    assert_eq!(
        (dead["error"].as_str(), dead["attempt"].as_i64()),
        (Some("event_unavailable"), Some(2))
    );
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert_eq!(deliveries(id).len(), 2, "no retry after event_unavailable");
    assert_eq!(fakes::received(&target).len(), 1);
}

#[test]
fn rotation_two_signatures() {
    let t = tenant(&platform(), "rot");
    let tid = t["id"].as_str().unwrap();
    let k = tenant_key(tid, MAIL);
    let (url, target) = receiver("rot");
    let e = endpoint(
        &format!("/v1/tenants/{tid}/webhooks"),
        &k,
        &url,
        json!(["identity.created"]),
    );
    let id = e["id"].as_str().unwrap();
    let old = pylota_identity_core::standard_webhooks::secret_bytes(e["secret"].as_str().unwrap())
        .unwrap();
    let r = post(
        &format!("/v1/webhooks/{id}/rotate-secret"),
        &k,
        json!({ "overlap_hours": 1 }),
    );
    let new =
        pylota_identity_core::standard_webhooks::secret_bytes(r.body["secret"].as_str().unwrap())
            .unwrap();
    event(&k, tid, "rot");
    let got = wait_received(&target, 1);
    let req = &got[0];
    let content = pylota_identity_core::standard_webhooks::signed_content(
        req.header("webhook-id").unwrap(),
        req.header("webhook-timestamp").unwrap().parse().unwrap(),
        &req.body,
    );
    let sign = pylota_identity_core::standard_webhooks::sign_v1;
    assert_eq!(
        req.header("webhook-signature").unwrap(),
        format!("{} {}", sign(&new, &content), sign(&old, &content))
    );
}
