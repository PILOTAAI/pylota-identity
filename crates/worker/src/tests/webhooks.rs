//! M8 on fakes: webhook endpoints, the fan-out and one attempt, retries, `410`, rotation and replay
//! (Webhooks).

use pylota_identity_core::standard_webhooks as sw;
use pylota_identity_platform::QueueProducer as _;
use pylota_identity_platform::dns::RType;
use pylota_identity_platform::fakes::{ScriptedResponse, Settled, block_on, incoming};
use pylota_identity_platform::queues::QueueName;
use serde_json::{Value, json};

use super::Harness;

const URL: &str = "https://hooks.receiver-example.com/mail";

/// A tenant, a tenant key with the webhook and mail permissions, and the receiver's DNS.
fn setup(h: &Harness) -> (String, String) {
    h.sql(
        "INSERT INTO domains (id, tenant_id, name, kind, method, zone_id, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_changed_at, monitor_do_id, created_at, updated_at) VALUES ('dom_01ARYZ6S410000000000000000', NULL, 'pylotamail.test', 'platform', 'platform', 'z', 1, 'catch_all', 'routing', 'cloudflare', 'subaddress', 1, 1, 'healthy', 1, '', 1, 1)",
        vec![],
    );
    let p = h.platform_key();
    let t = h.call(
        "POST",
        "/v1/tenants",
        Some(&p),
        Some(json!({ "slug": "acme", "name": "Acme", "mode": "live" })),
        &[],
    );
    let tid = t.body["id"].as_str().unwrap().to_owned();
    let k = h.seed_key(
        "tenant",
        &[
            "webhooks:manage",
            "messages:read",
            "quarantine:review",
            "identities:write",
            "identities:read",
        ],
        Some(&tid),
        None,
        None,
        None,
    );
    h.app.p.dns.set(
        None,
        "hooks.receiver-example.com",
        RType::A,
        &["93.184.216.34"],
    );
    h.app
        .p
        .dns
        .set(None, "hooks.receiver-example.com", RType::Aaaa, &[]);
    (tid, k)
}

fn create(h: &Harness, key: &str, tid: &str, body: Value) -> super::Resp {
    h.call(
        "POST",
        &format!("/v1/tenants/{tid}/webhooks"),
        Some(key),
        Some(body),
        &[],
    )
}

/// Delivers what waits in `pm-webhooks` once; returns how each message settled.
fn drain(h: &Harness) -> Vec<(Value, Settled)> {
    let bodies = h.app.p.queues.take(QueueName::Webhooks);
    let mut logs = Vec::new();
    let batch: Vec<_> = bodies
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let (m, log) = incoming(&format!("w{i}"), 0, 1, b.clone());
            logs.push(log);
            m
        })
        .collect();
    block_on(crate::consumers::run(
        h.app.p.as_ref(),
        QueueName::Webhooks,
        false,
        batch,
    ));
    bodies
        .into_iter()
        .zip(logs.iter().map(|l| l.get()))
        .collect()
}

/// Creates an identity, runs its mailbox alarm (the outbox), and fans out `identity.created`.
fn identity_event(h: &Harness, key: &str, tid: &str, name: &str) -> String {
    let r = h.call(
        "POST",
        &format!("/v1/tenants/{tid}/identities"),
        Some(key),
        Some(json!({ "username": name, "display_name": name })),
        &[],
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let idn = r.body["id"].as_str().unwrap().to_owned();
    let mb = h.sql(
        "SELECT mailbox_do_id FROM identities WHERE id = ?1",
        vec![idn.clone().into()],
    )[0]["mailbox_do_id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.mailbox_alarm(&mb);
    idn
}

#[test]
fn create_checks_url_permissions_and_limit() {
    let h = Harness::new();
    let (tid, k) = setup(&h);
    let r = create(&h, &k, &tid, json!({ "url": URL, "events": ["*"] }));
    assert_eq!(r.status, 201, "{}", r.body);
    assert!(r.body["secret"].as_str().unwrap().starts_with("whsec_"));
    assert_eq!(
        (r.body["scope"].as_str(), r.body["enabled"].as_bool()),
        (Some("tenant"), Some(true))
    );
    // J29: webhooks:manage alone does not reach mail events.
    let only = h.seed_key("tenant", &["webhooks:manage"], Some(&tid), None, None, None);
    let r = create(&h, &only, &tid, json!({ "url": URL, "events": ["*"] }));
    assert_eq!((r.status, r.code()), (403, "permission_denied"));
    assert_eq!(
        r.body["error"]["details"]["required"],
        json!(["messages:read", "quarantine:review"])
    );
    assert_eq!(
        create(
            &h,
            &only,
            &tid,
            json!({ "url": URL, "events": ["identity.created"] })
        )
        .status,
        201
    );
    // SSRF rules at create.
    for bad in [
        "http://hooks.receiver-example.com/",
        "https://127.0.0.1/x",
        "https://x.pylotamail.test/",
    ] {
        let r = create(
            &h,
            &k,
            &tid,
            json!({ "url": bad, "events": ["identity.created"] }),
        );
        assert_eq!(r.status, 400, "{bad}");
        assert_eq!(r.body["error"]["details"]["errors"][0]["path"], "url");
    }
    // At most 20 per tenant.
    for _ in 2..20 {
        assert_eq!(
            create(
                &h,
                &k,
                &tid,
                json!({ "url": URL, "events": ["identity.created"] })
            )
            .status,
            201
        );
    }
    let r = create(
        &h,
        &k,
        &tid,
        json!({ "url": URL, "events": ["identity.created"] }),
    );
    assert_eq!((r.status, r.code()), (422, "webhook_limit_reached"));
}

#[test]
fn fanout_delivers_signed_and_retries() {
    let h = Harness::new();
    let (tid, k) = setup(&h);
    let r = create(
        &h,
        &k,
        &tid,
        json!({ "url": URL, "events": ["identity.created"] }),
    );
    let secret = sw::secret_bytes(r.body["secret"].as_str().unwrap()).unwrap();
    let whk = r.body["id"].as_str().unwrap().to_owned();
    h.app.p.http.script(URL, ScriptedResponse::status(200));
    identity_event(&h, &k, &tid, "first");
    // The Fanout becomes one Deliver, which posts the signed envelope.
    let fan = drain(&h);
    assert!(
        fan.iter()
            .any(|(b, s)| b["kind"] == "fanout" && *s == Settled::Acked),
        "{fan:?}"
    );
    let del = drain(&h);
    assert_eq!(del.len(), 1, "{del:?}");
    let req = h
        .app
        .p
        .http
        .requests()
        .into_iter()
        .find(|r| r.url == URL)
        .expect("a delivery");
    let header = |n: &str| {
        req.headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    let body = req.body.clone().unwrap();
    let content = sw::signed_content(
        &header("webhook-id"),
        header("webhook-timestamp").parse().unwrap(),
        &body,
    );
    assert_eq!(header("webhook-signature"), sw::sign_v1(&secret, &content));
    let env: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(env["type"], "identity.created");
    let rows = h.sql(
        "SELECT status, attempt FROM webhook_deliveries WHERE endpoint_id = ?1",
        vec![whk.clone().into()],
    );
    assert_eq!(
        (rows[0]["status"].as_str(), rows[0]["attempt"].as_i64()),
        (Some("succeeded"), Some(1))
    );
    // A failing receiver: attempt 1 failed with its next time, attempt 2 queued.
    h.app.p.http.script(URL, ScriptedResponse::status(503));
    identity_event(&h, &k, &tid, "second");
    drain(&h);
    drain(&h);
    let failed = h.sql("SELECT status, error, next_attempt_at, created_at FROM webhook_deliveries WHERE endpoint_id = ?1 AND status = 'failed'", vec![whk.clone().into()]);
    assert_eq!(failed[0]["error"], "status_5xx");
    let wait =
        failed[0]["next_attempt_at"].as_i64().unwrap() - failed[0]["created_at"].as_i64().unwrap();
    assert!((27_000..=33_000).contains(&wait), "{wait}");
    let next = h.app.p.queues.sent();
    assert!(
        next.iter().any(|(q, b, delay)| *q == QueueName::Webhooks
            && b["attempt"] == 2
            && (27..=33).contains(delay)),
        "{next:?}"
    );
    // A 410 disables the endpoint at once and writes webhook.disabled.
    h.app.p.http.script(URL, ScriptedResponse::status(410));
    drain(&h);
    let e = h.sql(
        "SELECT enabled, disabled_reason FROM webhook_endpoints WHERE id = ?1",
        vec![whk.clone().into()],
    );
    assert_eq!(
        (e[0]["enabled"].as_i64(), e[0]["disabled_reason"].as_str()),
        (Some(0), Some("failing"))
    );
    let dis = h.sql(
        "SELECT payload_json FROM event_index WHERE type = 'webhook.disabled'",
        vec![],
    );
    let env: Value = serde_json::from_str(dis[0]["payload_json"].as_str().unwrap()).unwrap();
    assert_eq!(env["data"], json!({ "webhook_id": whk, "reason": "gone" }));
}

#[test]
fn rotation_two_signatures_and_replay() {
    let h = Harness::new();
    let (tid, k) = setup(&h);
    let r = create(
        &h,
        &k,
        &tid,
        json!({ "url": URL, "events": ["identity.created"] }),
    );
    let whk = r.body["id"].as_str().unwrap().to_owned();
    let old = sw::secret_bytes(r.body["secret"].as_str().unwrap()).unwrap();
    let rot = h.call(
        "POST",
        &format!("/v1/webhooks/{whk}/rotate-secret"),
        Some(&k),
        Some(json!({ "overlap_hours": 1 })),
        &[],
    );
    assert_eq!(rot.status, 200, "{}", rot.body);
    let new = sw::secret_bytes(rot.body["secret"].as_str().unwrap()).unwrap();
    h.app.p.http.script(URL, ScriptedResponse::status(200));
    let t = h.call(
        "POST",
        &format!("/v1/webhooks/{whk}/test"),
        Some(&k),
        None,
        &[],
    );
    assert_eq!(
        (t.status, t.body["status"].as_str()),
        (200, Some("succeeded")),
        "{}",
        t.body
    );
    let req = h
        .app
        .p
        .http
        .requests()
        .into_iter()
        .rfind(|r| r.url == URL)
        .unwrap();
    let header = |n: &str| {
        req.headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    let content = sw::signed_content(
        &header("webhook-id"),
        header("webhook-timestamp").parse().unwrap(),
        req.body.as_ref().unwrap(),
    );
    assert_eq!(
        header("webhook-signature"),
        format!(
            "{} {}",
            sw::sign_v1(&new, &content),
            sw::sign_v1(&old, &content)
        )
    );
    // Replay by window: the identity event delivered once is queued again, attempt 2, as a replay;
    // the webhook.test event never is.
    identity_event(&h, &k, &tid, "replayed");
    drain(&h);
    drain(&h);
    let r = h.call(
        "POST",
        &format!("/v1/webhooks/{whk}/replay"),
        Some(&k),
        Some(json!({ "since": "2000-01-01T00:00:00Z" })),
        &[],
    );
    assert_eq!(
        (r.status, r.body["queued"].as_i64()),
        (202, Some(1)),
        "{}",
        r.body
    );
    let q = h.app.p.queues.take(QueueName::Webhooks);
    assert_eq!(
        (
            q[0]["replay"].as_bool(),
            q[0]["attempt"].as_i64(),
            q[0]["event_type"].as_str()
        ),
        (Some(true), Some(2), Some("identity.created"))
    );
    let _ = block_on(h.app.p.queues.send(QueueName::Webhooks, &q[0], 0));
}
