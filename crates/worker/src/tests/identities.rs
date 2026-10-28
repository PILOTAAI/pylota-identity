//! M6 on fakes: identities, their mailbox, the outbox and the intents (Identities and domains, Design
//! conventions § 9, Webhooks › Transactional outbox).

use pylota_identity_api_types::Identity;
use pylota_identity_platform::fakes::block_on;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{Clock as _, Platform as _};
use serde_json::{Value, json};

use super::Harness;

/// The platform domain row setup writes, and a partner tenant with a key holding every identity
/// permission. Returns `(platform key, tenant id, tenant key)`.
fn setup(h: &Harness) -> (String, String, String) {
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
    assert_eq!(t.status, 201, "{}", t.body);
    let tid = t.body["id"].as_str().unwrap().to_owned();
    let tk = h.seed_key(
        "tenant",
        &[
            "identities:read",
            "identities:write",
            "erasure:manage",
            "domains:read",
        ],
        Some(&tid),
        None,
        None,
        None,
    );
    (p, tid, tk)
}

fn create(h: &Harness, key: &str, tid: &str, body: Value) -> super::Resp {
    h.call(
        "POST",
        &format!("/v1/tenants/{tid}/identities"),
        Some(key),
        Some(body),
        &[],
    )
}

fn mailbox_of(h: &Harness, id: &str) -> String {
    h.sql(
        "SELECT mailbox_do_id FROM identities WHERE id = ?1",
        vec![id.into()],
    )[0]["mailbox_do_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn outbox(h: &Harness, mailbox: &str) -> Vec<Value> {
    h.mailbox_sql(
        mailbox,
        "SELECT seq, event_id, type, payload_json, dispatched_at FROM outbox ORDER BY seq",
    )
}

#[test]
fn create_emits_created_through_the_outbox() {
    let h = Harness::new();
    let (_, tid, tk) = setup(&h);
    let r = create(
        &h,
        &tk,
        &tid,
        json!({ "username": "Bookings", "display_name": "Acme Car Hire", "owner": { "name": "Sam", "email": "Sam@Acme.example" } }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let idn: Identity = serde_json::from_value(r.body.clone()).unwrap();
    assert_eq!(r.body["username"], "bookings");
    assert_eq!(r.body["primary_address"], "bookings.acme@pylotamail.test");
    assert_eq!(r.body["owner"]["email"], "sam@acme.example");
    assert_eq!(idn.addresses.len(), 1);
    // The mailbox has its owner and identity.created at sequence 1; the intent is gone.
    let mb = mailbox_of(&h, &idn.id.0);
    let owner = h.mailbox_sql(&mb, "SELECT v FROM meta WHERE k = 'identity_id'");
    assert_eq!(owner[0]["v"], idn.id.0.as_str());
    let events = outbox(&h, &mb);
    assert_eq!(
        (
            events.len(),
            events[0]["type"].as_str(),
            events[0]["seq"].as_i64()
        ),
        (1, Some("identity.created"), Some(1))
    );
    let env: Value = serde_json::from_str(events[0]["payload_json"].as_str().unwrap()).unwrap();
    serde_json::from_value::<pylota_identity_api_types::IdentityCreatedData>(env["data"].clone())
        .unwrap();
    assert!(h.sql("SELECT id FROM rpc_intents", vec![]).is_empty());
    // The alarm writes event_index and a Fanout, then marks the row dispatched.
    h.mailbox_alarm(&mb);
    let idx = h.sql(
        "SELECT id, owner_kind, identity_id FROM event_index WHERE type = 'identity.created'",
        vec![],
    );
    assert_eq!(
        (idx.len(), idx[0]["owner_kind"].as_str()),
        (1, Some("mailbox"))
    );
    assert_eq!(idx[0]["id"], events[0]["event_id"]);
    let sent: Vec<Value> = h
        .app
        .p
        .queues
        .sent()
        .into_iter()
        .filter(|(q, _, _)| *q == QueueName::Webhooks)
        .map(|(_, b, _)| b)
        .collect();
    assert_eq!((sent.len(), sent[0]["kind"].as_str()), (1, Some("fanout")));
    assert!(outbox(&h, &mb)[0]["dispatched_at"].is_i64());
    // A repeated alarm sends nothing more.
    h.mailbox_alarm(&mb);
    assert_eq!(
        h.app
            .p
            .queues
            .sent()
            .iter()
            .filter(|(q, _, _)| *q == QueueName::Webhooks)
            .count(),
        1
    );
    // The inboxes hold was settled as used.
    let usage = h.objects.borrow().values().next().cloned().unwrap();
    let used: Vec<Value> = pylota_identity_platform::Sql::query(
        &usage,
        "SELECT used, held FROM allowances WHERE feature = 'inboxes'",
        &[],
    )
    .unwrap();
    assert_eq!(
        (used[0]["used"].as_i64(), used[0]["held"].as_i64()),
        (Some(1), Some(0))
    );
}

#[test]
fn client_id_and_username_rules() {
    let h = Harness::new();
    let (p, tid, tk) = setup(&h);
    let body = json!({ "username": "desk", "display_name": "Desk", "client_id": "acme:desk" });
    let a = create(&h, &tk, &tid, body.clone());
    assert_eq!(a.status, 201);
    let b = create(&h, &tk, &tid, body);
    assert_eq!((b.status, &b.body["id"]), (200, &a.body["id"]));
    let c = create(
        &h,
        &tk,
        &tid,
        json!({ "username": "desk2", "display_name": "Desk", "client_id": "acme:desk" }),
    );
    assert_eq!(c.code(), "client_id_conflict");
    for (name, code) in [
        ("postmaster", "address_reserved"),
        ("dеsk", "address_reserved"),
        ("a..b", "address_invalid"),
        ("DESK", "username_taken"),
        ("ünï", "address_unsupported"),
    ] {
        assert_eq!(
            create(
                &h,
                &tk,
                &tid,
                json!({ "username": name, "display_name": "x" })
            )
            .code(),
            code,
            "{name}"
        );
    }
    assert_eq!(
        create(
            &h,
            &tk,
            &tid,
            json!({ "username": "x1", "display_name": "x", "nope": 1 })
        )
        .code(),
        "invalid_request"
    );
    // A send cap above the tenant's needs a platform key.
    let r = create(
        &h,
        &tk,
        &tid,
        json!({ "username": "x2", "display_name": "x", "send_policy": { "daily_cap": 501 } }),
    );
    assert_eq!(
        (r.code(), r.body["error"]["details"]["field"].as_str()),
        ("scope_denied", Some("send_policy.daily_cap"))
    );
    assert_eq!(
        create(
            &h,
            &p,
            &tid,
            json!({ "username": "x2", "display_name": "x", "send_policy": { "daily_cap": 501 } })
        )
        .status,
        201
    );
    // A number in retention.message_hours needs erasure:manage.
    let narrow = h.seed_key(
        "tenant",
        &["identities:write"],
        Some(&tid),
        None,
        None,
        None,
    );
    let r = create(
        &h,
        &narrow,
        &tid,
        json!({ "username": "x3", "display_name": "x", "retention": { "message_hours": 24 } }),
    );
    assert_eq!(r.code(), "permission_denied");
}

#[test]
fn update_pause_resume_and_suspension_events_in_order() {
    let h = Harness::new();
    let (p, tid, tk) = setup(&h);
    let id = create(
        &h,
        &tk,
        &tid,
        json!({ "username": "ops", "display_name": "Ops" }),
    )
    .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/v1/identities/{id}");
    let r = h.call(
        "PATCH",
        &path,
        Some(&tk),
        Some(json!({ "display_name": "Ops desk" })),
        &[],
    );
    assert_eq!(
        (r.status, r.body["display_name"].as_str()),
        (200, Some("Ops desk"))
    );
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&tk),
            Some(json!({ "status": "paused" })),
            &[]
        )
        .body["pause_reason"],
        "manual"
    );
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&tk),
            Some(json!({ "status": "active" })),
            &[]
        )
        .body["status"],
        "active"
    );
    // Tenant suspension pauses it; a PATCH resume is refused; resuming the tenant resumes it.
    assert_eq!(
        h.call(
            "PATCH",
            &format!("/v1/tenants/{tid}"),
            Some(&p),
            Some(json!({ "status": "suspended" })),
            &[]
        )
        .status,
        200
    );
    let g = h.call("GET", &path, Some(&p), None, &[]);
    assert_eq!(
        (g.body["status"].as_str(), g.body["pause_reason"].as_str()),
        (Some("paused"), Some("tenant_suspended"))
    );
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&p),
            Some(json!({ "status": "active" })),
            &[]
        )
        .code(),
        "identity_paused"
    );
    assert_eq!(
        h.call(
            "PATCH",
            &format!("/v1/tenants/{tid}"),
            Some(&p),
            Some(json!({ "status": "active" })),
            &[]
        )
        .status,
        200
    );
    assert_eq!(
        h.call("GET", &path, Some(&p), None, &[]).body["status"],
        "active"
    );
    let mb = mailbox_of(&h, &id);
    let types: Vec<String> = outbox(&h, &mb)
        .iter()
        .map(|e| e["type"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        types,
        [
            "identity.created",
            "identity.updated",
            "identity.paused",
            "identity.resumed",
            "identity.paused",
            "identity.resumed"
        ]
    );
    let last: Value =
        serde_json::from_str(outbox(&h, &mb)[4]["payload_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        (last["data"]["reason"].as_str(), last["sequence"].as_i64()),
        (Some("tenant_suspended"), Some(5))
    );
    let upd: Value =
        serde_json::from_str(outbox(&h, &mb)[1]["payload_json"].as_str().unwrap()).unwrap();
    assert_eq!(upd["data"]["changed"], json!(["display_name"]));
    assert!(h.sql("SELECT id FROM rpc_intents", vec![]).is_empty());
}

#[test]
fn j21_event_intent_retried_once_in_order() {
    let h = Harness::new();
    let (_, tid, tk) = setup(&h);
    let id = create(
        &h,
        &tk,
        &tid,
        json!({ "username": "ops", "display_name": "Ops" }),
    )
    .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mb = mailbox_of(&h, &id);
    // The mailbox is unreachable: the PATCHes still commit, their intents wait.
    h.app.p.objects.on(
        pylota_identity_platform::durable::DoClass::Mailbox,
        |_, _| async {
            Err(pylota_identity_platform::PlatformError::unavailable(
                "MAILBOX", "down",
            ))
        },
    );
    let path = format!("/v1/identities/{id}");
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&tk),
            Some(json!({ "display_name": "A" })),
            &[]
        )
        .status,
        200
    );
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&tk),
            Some(json!({ "status": "paused" })),
            &[]
        )
        .status,
        200
    );
    assert_eq!(h.sql("SELECT id FROM rpc_intents", vec![]).len(), 2);
    // Back up: one cron run a minute later sends both, oldest first, and a repeat stores nothing.
    let boxes = h.mailboxes.clone();
    let weak = std::rc::Rc::downgrade(&h.app.p);
    h.app.p.objects.on(
        pylota_identity_platform::durable::DoClass::Mailbox,
        move |oid, env| {
            let state = boxes.borrow_mut().entry(oid.clone()).or_default().clone();
            let p = weak.upgrade().unwrap();
            async move {
                Ok(crate::mailbox::Mailbox {
                    p: p.as_ref(),
                    sql: &state.sql,
                    state: &state,
                    object_id: &oid,
                }
                .handle(&env)
                .await)
            }
        },
    );
    let intents = h.sql(
        "SELECT id, occurred_at FROM rpc_intents ORDER BY occurred_at",
        vec![],
    );
    h.app.p.clock.advance(60_000);
    block_on(crate::crons::run(h.app.p.as_ref(), "* * * * *", "req_cron"));
    assert!(h.sql("SELECT id FROM rpc_intents", vec![]).is_empty());
    let events = outbox(&h, &mb);
    let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(
        types,
        ["identity.created", "identity.updated", "identity.paused"]
    );
    // The event ID is derived from the intent.
    let want = pylota_identity_core::ids::intent_event_id(
        &h.app.p.secrets().hash_key,
        intents[0]["id"].as_str().unwrap(),
        intents[0]["occurred_at"].as_i64().unwrap(),
    );
    assert_eq!(events[1]["event_id"], want.as_str());
    // A duplicate re-send of the same event leaves one row.
    let env = crate::rpc::envelope_for(
        h.app.p.as_ref(),
        Some(&tid),
        Some(&id),
        "req_x",
        None,
        json!({ "op": "emit_event", "event_id": want, "event_type": "identity.updated", "occurred_at": h.app.p.as_ref().clock().now_ms(), "data": {} }),
    );
    let out: Value = block_on(pylota_identity_platform::ObjectClient::call::<_, Value>(
        &h.app.p.objects,
        pylota_identity_platform::durable::DoClass::Mailbox,
        &mb,
        &env,
    ))
    .map(|r| serde_json::to_value(r).unwrap())
    .unwrap();
    assert_eq!(out["ok"]["sequence"], Value::Null);
    assert_eq!(outbox(&h, &mb).len(), 3);
}

#[test]
fn delete_tombstones_and_second_delete_answers_the_same_request() {
    let h = Harness::new();
    let (p, tid, tk) = setup(&h);
    let id = create(
        &h,
        &tk,
        &tid,
        json!({ "username": "gone", "display_name": "Gone" }),
    )
    .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/v1/identities/{id}");
    let d = h.call("DELETE", &path, Some(&tk), None, &[]);
    assert_eq!(
        (
            d.status,
            d.body["scope"].as_str(),
            d.body["status"].as_str()
        ),
        (202, Some("identity"), Some("queued"))
    );
    serde_json::from_value::<pylota_identity_api_types::ErasureRequest>(d.body.clone()).unwrap();
    let again = h.call("DELETE", &path, Some(&tk), None, &[]);
    assert_eq!((again.status, &again.body["id"]), (200, &d.body["id"]));
    assert_eq!(h.sql("SELECT id FROM jobs", vec![]).len(), 1);
    let g = h.call("GET", &path, Some(&tk), None, &[]);
    assert_eq!(
        (
            g.body["status"].as_str(),
            g.body["addresses"].as_array().map(Vec::len)
        ),
        (Some("deleting"), Some(0))
    );
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&tk),
            Some(json!({ "display_name": "x" })),
            &[]
        )
        .code(),
        "identity_not_found"
    );
    assert_eq!(
        h.sql("SELECT reason FROM address_tombstones", vec![])[0]["reason"],
        "deleted"
    );
    // A5: once deleted, the username is free but the address is not.
    h.sql(
        "UPDATE identities SET status = 'deleted' WHERE id = ?1",
        vec![id.clone().into()],
    );
    assert_eq!(
        create(
            &h,
            &p,
            &tid,
            json!({ "username": "gone", "display_name": "x" })
        )
        .code(),
        "address_taken"
    );
    // The inboxes allowance came back.
    let usage = h.objects.borrow().values().next().cloned().unwrap();
    let used: Vec<Value> = pylota_identity_platform::Sql::query(
        &usage,
        "SELECT used FROM allowances WHERE feature = 'inboxes'",
        &[],
    )
    .unwrap();
    assert_eq!(used[0]["used"].as_i64(), Some(0));
}

#[test]
fn scope_lookup_lists_and_platform_domain() {
    let h = Harness::new();
    let (p, tid, tk) = setup(&h);
    let id = create(
        &h,
        &tk,
        &tid,
        json!({ "username": "front", "display_name": "Front" }),
    )
    .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let other = h.call(
        "POST",
        "/v1/tenants",
        Some(&p),
        Some(json!({ "slug": "beta", "name": "Beta", "mode": "live" })),
        &[],
    );
    let ok = h.seed_key(
        "tenant",
        &["identities:read", "identities:write", "erasure:manage"],
        other.body["id"].as_str(),
        None,
        None,
        None,
    );
    let missing = h.call(
        "GET",
        "/v1/identities/idn_01ARYZ6S410000000000000000",
        Some(&ok),
        None,
        &[],
    );
    for (m, path) in [
        ("GET", format!("/v1/identities/{id}")),
        ("PATCH", format!("/v1/identities/{id}")),
        ("DELETE", format!("/v1/identities/{id}")),
        ("GET", format!("/v1/identities/{id}/addresses")),
    ] {
        let r = h.call(
            m,
            &path,
            Some(&ok),
            Some(json!({ "display_name": "x" })),
            &[],
        );
        assert_eq!(
            (r.status, r.code()),
            (404, "identity_not_found"),
            "{m} {path}"
        );
        let mut a = r.body.clone();
        let mut b = missing.body.clone();
        a["error"]["request_id"] = Value::Null;
        b["error"]["request_id"] = Value::Null;
        assert_eq!(a, b);
    }
    let lk = h.call(
        "GET",
        "/v1/identities/lookup?address=FRONT.acme@pylotamail.test",
        Some(&tk),
        None,
        &[],
    );
    assert_eq!(lk.body["id"], id.as_str());
    assert_eq!(
        h.call(
            "GET",
            "/v1/identities/lookup?address=front.acme@pylotamail.test",
            Some(&ok),
            None,
            &[]
        )
        .code(),
        "identity_not_found"
    );
    assert_eq!(
        h.call("GET", "/v1/identities", Some(&ok), None, &[]).body["data"],
        json!([])
    );
    assert_eq!(
        h.call("GET", "/v1/identities", Some(&tk), None, &[]).body["data"][0]["id"],
        id.as_str()
    );
    let list = h.call(
        "GET",
        &format!("/v1/tenants/{tid}/identities?limit=1"),
        Some(&tk),
        None,
        &[],
    );
    assert_eq!(list.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1/identities/{id}/addresses"),
            Some(&tk),
            None,
            &[]
        )
        .body["data"][0]["role"],
        "primary"
    );
    // The platform domain is visible to every key, with tenant_id null.
    let d = h.call(
        "GET",
        "/v1/domains/dom_01ARYZ6S410000000000000000",
        Some(&tk),
        None,
        &[],
    );
    assert_eq!((d.status, &d.body["tenant_id"]), (200, &Value::Null));
    serde_json::from_value::<pylota_identity_api_types::Domain>(d.body).unwrap();
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1/tenants/{tid}/domains"),
            Some(&tk),
            None,
            &[]
        )
        .body["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn system_identity_minted_hidden_and_protected() {
    let h = Harness::new();
    let (p, _, _) = setup(&h);
    let def = h.call(
        "POST",
        "/v1/tenants",
        Some(&p),
        Some(json!({ "slug": "default", "name": "Default", "mode": "live", "address_suffix": "" })),
        &[],
    );
    assert_eq!(def.status, 201, "{}", def.body);
    let dt = def.body["id"].as_str().unwrap().to_owned();
    h.sql(
        "INSERT INTO identities (id, tenant_id, username, display_name, owner_name, owner_email, send_policy_json, mailbox_do_id, is_system, created_at, updated_at) VALUES ('idn_01ARYZ6S41SYSTEM000000000', ?1, 'no-reply', 'Pylota Identity', 'Operator', 'postmaster@pylotamail.test', '{\"daily_cap\":50000,\"require_known_recipient\":false}', '', 1, 1, 1)",
        vec![dt.clone().into()],
    );
    h.sql("INSERT INTO addresses (id, address, local_part, domain_id, tenant_id, identity_id, role, status, created_at, updated_at) VALUES ('adr_01ARYZ6S41SYSTEM000000000', 'no-reply@pylotamail.test', 'no-reply', 'dom_01ARYZ6S410000000000000000', ?1, 'idn_01ARYZ6S41SYSTEM000000000', 'primary', 'active', 1, 1)", vec![dt.clone().into()]);
    block_on(crate::crons::run(h.app.p.as_ref(), "* * * * *", "req_cron"));
    let mb = mailbox_of(&h, "idn_01ARYZ6S41SYSTEM000000000");
    assert!(!mb.is_empty());
    assert_eq!(
        h.mailbox_sql(&mb, "SELECT v FROM meta WHERE k = 'is_system'")[0]["v"],
        "1"
    );
    // A second run mints nothing more.
    block_on(crate::crons::run(
        h.app.p.as_ref(),
        "* * * * *",
        "req_cron2",
    ));
    assert_eq!(mailbox_of(&h, "idn_01ARYZ6S41SYSTEM000000000"), mb);
    // Never listed; a tenant key of the default tenant gets 404; A18 and A17.
    let dk = h.seed_key(
        "tenant",
        &["identities:read", "identities:write", "erasure:manage"],
        Some(&dt),
        None,
        None,
        None,
    );
    assert_eq!(
        h.call("GET", "/v1/identities", Some(&p), None, &[]).body["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["is_system"] == 1 || i["username"] == "no-reply")
            .count(),
        0
    );
    assert_eq!(
        h.call(
            "GET",
            "/v1/identities/idn_01ARYZ6S41SYSTEM000000000",
            Some(&dk),
            None,
            &[]
        )
        .code(),
        "identity_not_found"
    );
    let r = create(
        &h,
        &dk,
        &dt,
        json!({ "username": "billing", "display_name": "x" }),
    );
    assert_eq!(
        (r.code(), r.body["error"]["details"]["reason"].as_str()),
        ("scope_denied", Some("default_tenant"))
    );
    assert_eq!(
        create(
            &h,
            &p,
            &dt,
            json!({ "username": "billing", "display_name": "x" })
        )
        .body["primary_address"],
        "billing@pylotamail.test"
    );
    assert_eq!(
        h.call(
            "DELETE",
            "/v1/identities/idn_01ARYZ6S41SYSTEM000000000",
            Some(&p),
            None,
            &[]
        )
        .code(),
        "system_protected"
    );
    assert_eq!(
        h.call(
            "PATCH",
            &format!("/v1/tenants/{dt}"),
            Some(&p),
            Some(json!({ "status": "suspended" })),
            &[]
        )
        .code(),
        "system_protected"
    );
    let paused = h.call(
        "PATCH",
        "/v1/identities/idn_01ARYZ6S41SYSTEM000000000",
        Some(&p),
        Some(json!({ "status": "paused" })),
        &[],
    );
    assert_eq!(paused.body["status"], "paused");
}
