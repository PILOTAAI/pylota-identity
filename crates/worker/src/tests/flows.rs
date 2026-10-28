//! Flows through the app on the fakes. Each mirrors the `it::` test of the same name, which runs the
//! same calls against workerd (`cargo xtask itest`).

use pylota_identity_platform::fakes::FakeClock;
use pylota_identity_platform::{Platform as _, RateLimiter as _};
use serde_json::{Value, json};

use super::Harness;

fn tenant(h: &Harness, key: &str, slug: &str) -> Value {
    let r = h.call(
        "POST",
        "/v1/tenants",
        Some(key),
        Some(json!({ "slug": slug, "name": slug, "mode": "live" })),
        &[],
    );
    assert_eq!(r.status, 201, "{}", r.body);
    r.body
}

#[test]
fn meta_routes_and_global_headers() {
    let h = Harness::with_vars(&[("PM_SECURITY_CONTACT", "security@pylotamail.test")]);
    let r = h.call("GET", "/health", None, None, &[]);
    assert_eq!(
        (r.status, r.body["status"].as_str(), r.body["env"].as_str()),
        (200, Some("ok"), Some("local"))
    );
    assert!(
        r.header("request-id")
            .is_some_and(|v| v.starts_with("req_"))
    );
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"));
    assert!(r.header("access-control-allow-origin").is_none() && r.header("set-cookie").is_none());
    let txt = h.call("GET", "/.well-known/security.txt", None, None, &[]);
    let t = txt.body.as_str().unwrap();
    assert!(t.starts_with("Contact: mailto:security@pylotamail.test\nExpires: "));
    assert!(t.contains("Canonical: https://api.pylotamail.test/.well-known/security.txt"));
    assert_eq!(
        h.call("GET", "/openapi.json", None, None, &[]).body["openapi"],
        "3.1.0"
    );
    let key = h.platform_key();
    let me = h.call("GET", "/v1/me", Some(&key), None, &[]);
    assert_eq!(
        (me.status, me.body["level"].as_str()),
        (200, Some("platform"))
    );
    assert_eq!(me.header("cache-control"), Some("no-store"));
    assert_eq!(me.header("ratelimit-limit"), Some("600"));
    serde_json::from_value::<pylota_identity_api_types::Me>(me.body).unwrap();
}

#[test]
fn unknown_route_and_method() {
    let h = Harness::new();
    let r = h.call("GET", "/v1/nothing-here", None, None, &[]);
    assert_eq!((r.status, r.code()), (404, "route_not_found"));
    let r = h.call("DELETE", "/v1/tenants", None, None, &[]);
    assert_eq!((r.status, r.code()), (405, "method_not_allowed"));
    assert_eq!(r.header("allow"), Some("POST, GET"));
    assert_eq!(
        h.call("GET", "/console/sign-in", None, None, &[]).status,
        404
    );
}

#[test]
fn unknown_key_uniform_and_status_after_secret_match() {
    let h = Harness::new();
    let key = h.platform_key();
    let strip = |mut v: Value| {
        v["error"]["request_id"] = Value::Null;
        v
    };
    let unknown = h.call(
        "GET",
        "/v1/me",
        Some("pmk_live_000000000000_0000000000000000000000000000000000000000000000000000"),
        None,
        &[],
    );
    let wrong_secret = format!("{}{}", &key[..22], "1".repeat(52));
    let wrong = h.call("GET", "/v1/me", Some(&wrong_secret), None, &[]);
    let malformed = h.call("GET", "/v1/me", Some("not-a-key"), None, &[]);
    let missing = h.call("GET", "/v1/me", None, None, &[]);
    assert_eq!(unknown.status, 401);
    assert_eq!(strip(unknown.body.clone()), strip(wrong.body));
    assert_eq!(strip(unknown.body.clone()), strip(malformed.body));
    assert_eq!(strip(unknown.body), strip(missing.body));

    // Revocation and expiry are reported only after the secret matched.
    let child = h.call(
        "POST",
        "/v1/keys",
        Some(&key),
        Some(json!({ "name": "c", "level": "platform", "permissions": ["audit:read"] })),
        &[],
    );
    assert_eq!(child.status, 201, "{}", child.body);
    let secret = child.body["secret"].as_str().unwrap().to_owned();
    assert_eq!(
        h.call(
            "DELETE",
            &format!("/v1/keys/{}", child.body["id"].as_str().unwrap()),
            Some(&key),
            None,
            &[]
        )
        .status,
        204
    );
    assert_eq!(
        h.call("GET", "/v1/me", Some(&secret), None, &[]).code(),
        "key_revoked"
    );
    let soon = pylota_identity_core::days::rfc3339(FakeClock::START_MS + 60_000);
    let exp = h.call("POST", "/v1/keys", Some(&key), Some(json!({ "name": "e", "level": "platform", "permissions": ["audit:read"], "expires_at": soon })), &[]);
    let exp_secret = exp.body["secret"].as_str().unwrap().to_owned();
    assert_eq!(
        h.call("GET", "/v1/me", Some(&exp_secret), None, &[]).status,
        200
    );
    h.app.p.clock.advance(61_000);
    assert_eq!(
        h.call("GET", "/v1/me", Some(&exp_secret), None, &[]).code(),
        "key_expired"
    );
}

#[test]
fn rotation_overlap_and_last_used_throttle() {
    let h = Harness::new();
    let key = h.platform_key();
    let k = h.call(
        "POST",
        "/v1/keys",
        Some(&key),
        Some(json!({ "name": "r", "level": "platform", "permissions": ["audit:read"] })),
        &[],
    );
    let (id, old) = (
        k.body["id"].as_str().unwrap().to_owned(),
        k.body["secret"].as_str().unwrap().to_owned(),
    );
    let rot = h.call(
        "POST",
        &format!("/v1/keys/{id}/rotate"),
        Some(&key),
        Some(json!({ "overlap_hours": 1 })),
        &[],
    );
    assert_eq!(rot.status, 200, "{}", rot.body);
    let new = rot.body["secret"].as_str().unwrap().to_owned();
    assert_eq!(new[..22], old[..22], "the lookup stays");
    assert_ne!(new, old);
    assert_eq!(h.call("GET", "/v1/me", Some(&old), None, &[]).status, 200);
    assert_eq!(h.call("GET", "/v1/me", Some(&new), None, &[]).status, 200);
    h.app.p.clock.advance(3_600_001);
    assert_eq!(
        h.call("GET", "/v1/me", Some(&old), None, &[]).code(),
        "unauthenticated"
    );
    assert_eq!(h.call("GET", "/v1/me", Some(&new), None, &[]).status, 200);

    // last_used_at is written at most once a minute.
    let used = |h: &Harness| {
        h.sql(
            "SELECT last_used_at FROM api_keys WHERE id = ?1",
            vec![id.clone().into()],
        )[0]["last_used_at"]
            .as_i64()
    };
    let first = used(&h).unwrap();
    h.app.p.clock.advance(30_000);
    h.call("GET", "/v1/me", Some(&new), None, &[]);
    assert_eq!(used(&h), Some(first));
    h.app.p.clock.advance(31_000);
    h.call("GET", "/v1/me", Some(&new), None, &[]);
    assert_eq!(used(&h), Some(first + 61_000));
}

#[test]
fn missing_permission_and_level() {
    let h = Harness::new();
    let p = h.platform_key();
    let t = tenant(&h, &p, "acme");
    let tid = t["id"].as_str().unwrap();
    let tk = h.seed_key("tenant", &["identities:read"], Some(tid), None, None, None);
    let r = h.call("GET", "/v1/audit-events", Some(&tk), None, &[]);
    assert_eq!((r.status, r.code()), (403, "permission_denied"));
    assert_eq!(
        r.body["error"]["details"]["required"],
        json!(["audit:read"])
    );
    // A tenant key reads its own tenant without tenants:manage, and no other.
    assert_eq!(
        h.call("GET", &format!("/v1/tenants/{tid}"), Some(&tk), None, &[])
            .status,
        200
    );
    let other = tenant(&h, &p, "other");
    let r = h.call(
        "GET",
        &format!("/v1/tenants/{}", other["id"].as_str().unwrap()),
        Some(&tk),
        None,
        &[],
    );
    assert_eq!(r.code(), "permission_denied");
}

#[test]
fn create_tenant_idempotent_replay_and_quota_init() {
    let h = Harness::new();
    let p = h.platform_key();
    let body = json!({ "slug": "acme", "name": "Acme", "mode": "live", "timezone": "Europe/London",
                       "owner": { "email": "Sam@Acme.example", "name": "Sam" }, "billing": { "mode": "exempt" } });
    let a = h.call(
        "POST",
        "/v1/tenants",
        Some(&p),
        Some(body.clone()),
        &[("idempotency-key", "k1")],
    );
    assert_eq!(a.status, 201, "{}", a.body);
    serde_json::from_value::<pylota_identity_api_types::Tenant>(a.body.clone()).unwrap();
    assert_eq!(a.body["address_suffix"], ".acme");
    let b = h.call(
        "POST",
        "/v1/tenants",
        Some(&p),
        Some(body.clone()),
        &[("idempotency-key", "k1")],
    );
    assert_eq!(
        (b.status, b.header("idempotent-replayed")),
        (201, Some("true"))
    );
    assert_eq!(a.body, b.body);
    let mut other = body.clone();
    other["slug"] = "acme2".into();
    assert_eq!(
        h.call(
            "POST",
            "/v1/tenants",
            Some(&p),
            Some(other),
            &[("idempotency-key", "k1")]
        )
        .code(),
        "idempotency_conflict"
    );
    // Stored, sending nothing: the owner, the billing account, and an Init that reached TenantQuota.
    let tid = a.body["id"].as_str().unwrap();
    assert_eq!(
        h.sql(
            "SELECT role FROM members WHERE tenant_id = ?1",
            vec![tid.into()]
        )[0]["role"],
        "owner"
    );
    assert_eq!(
        h.sql(
            "SELECT mode FROM billing_accounts WHERE tenant_id = ?1",
            vec![tid.into()]
        )[0]["mode"],
        "exempt"
    );
    assert!(
        h.sql("SELECT id FROM rpc_intents", vec![]).is_empty(),
        "the intent is deleted after Init"
    );
    assert!(h.app.p.mail.sent().is_empty());
    let objs = h.objects.borrow();
    assert_eq!(objs.len(), 1);
    // Duplicates.
    drop(objs);
    assert_eq!(
        h.call(
            "POST",
            "/v1/tenants",
            Some(&p),
            Some(json!({ "slug": "acme", "name": "x", "mode": "live" })),
            &[]
        )
        .code(),
        "slug_taken"
    );
    let r = h.call(
        "POST",
        "/v1/tenants",
        Some(&p),
        Some(json!({ "slug": "acrne", "name": "x", "mode": "live" })),
        &[],
    );
    assert_eq!(r.code(), "suffix_taken");
    let r = h.call(
        "POST",
        "/v1/tenants",
        Some(&p),
        Some(json!({ "slug": "paypa1", "name": "x", "mode": "live" })),
        &[],
    );
    assert_eq!(r.code(), "address_reserved");
    let r = h.call("POST", "/v1/tenants", Some(&p), Some(json!({ "slug": "zz", "name": "x", "mode": "live", "owner": { "email": "a@pylotamail.test" } })), &[]);
    assert_eq!(
        r.body["error"]["details"]["errors"][0]["path"],
        "owner.email"
    );
}

#[test]
fn j21_quota_init_retried_by_the_cron() {
    let h = Harness::new();
    let p = h.platform_key();
    // TenantQuota is down: the tenant is created and the intent stays.
    h.app.p.objects.on(
        pylota_identity_platform::durable::DoClass::Quota,
        |_, _| async {
            Err(pylota_identity_platform::PlatformError::unavailable(
                "QUOTA", "down",
            ))
        },
    );
    let t = tenant(&h, &p, "acme");
    assert_eq!(h.sql("SELECT op FROM rpc_intents", vec![]).len(), 1);
    // Back up: the next cron run re-sends Init and deletes the intent.
    let h2 = Harness::new();
    let objs = h2.objects.clone();
    let weak = std::rc::Rc::downgrade(&h.app.p);
    h.app.p.objects.on(
        pylota_identity_platform::durable::DoClass::Quota,
        move |id, env| {
            let sql = objs.borrow_mut().entry(id).or_default().clone();
            let p = weak.upgrade().unwrap();
            async move {
                Ok(crate::quota::Quota {
                    p: p.as_ref(),
                    sql: &sql,
                }
                .handle(&env))
            }
        },
    );
    // Not yet due: an intent waits 60 seconds before the cron re-sends it (Design conventions § 9).
    pylota_identity_platform::fakes::block_on(crate::crons::run(
        h.app.p.as_ref(),
        "* * * * *",
        "req_cron",
    ));
    assert_eq!(h.sql("SELECT id FROM rpc_intents", vec![]).len(), 1);
    h.app.p.clock.advance(60_000);
    pylota_identity_platform::fakes::block_on(crate::crons::run(
        h.app.p.as_ref(),
        "* * * * *",
        "req_cron",
    ));
    assert!(h.sql("SELECT id FROM rpc_intents", vec![]).is_empty());
    let q = h2.objects.borrow();
    let sql = q.values().next().unwrap();
    let owner: Vec<Value> =
        pylota_identity_platform::Sql::query(sql, "SELECT v FROM meta WHERE k = 'tenant_id'", &[])
            .unwrap();
    assert_eq!(owner[0]["v"], t["id"]);
}

#[test]
fn partners_routes_audit_and_suspension() {
    let h = Harness::new();
    let p = h.platform_key();
    let pt = h.call(
        "POST",
        "/v1/partners",
        Some(&p),
        Some(json!({ "name": "Pylota", "default_billing_mode": "exempt", "max_tenants": 2 })),
        &[],
    );
    assert_eq!(pt.status, 201, "{}", pt.body);
    serde_json::from_value::<pylota_identity_api_types::Partner>(pt.body.clone()).unwrap();
    let pid = pt.body["id"].as_str().unwrap().to_owned();
    let pk = h.call(
        "POST",
        "/v1/keys",
        Some(&p),
        Some(json!({ "name": "pk", "level": "partner", "partner_id": pid, "permissions": ["tenants:manage", "keys:manage", "audit:read"] })),
        &[],
    );
    assert_eq!(pk.status, 201, "{}", pk.body);
    let pk = pk.body["secret"].as_str().unwrap().to_owned();
    let t = tenant(&h, &pk, "op-one");
    assert_eq!(t["partner_id"], json!(pid));
    assert_eq!(
        h.sql("SELECT mode FROM billing_accounts", vec![])[0]["mode"],
        "exempt"
    );
    // Billing is platform-only; max_tenants holds.
    let r = h.call(
        "POST",
        "/v1/tenants",
        Some(&pk),
        Some(
            json!({ "slug": "x2", "name": "x", "mode": "live", "billing": { "mode": "metered" } }),
        ),
        &[],
    );
    assert_eq!(
        (r.code(), r.body["error"]["details"]["field"].as_str()),
        ("scope_denied", Some("billing"))
    );
    tenant(&h, &pk, "op-two");
    let r = h.call(
        "POST",
        "/v1/tenants",
        Some(&pk),
        Some(json!({ "slug": "op-three", "name": "x", "mode": "live" })),
        &[],
    );
    assert_eq!(
        (r.code(), r.body["error"]["details"]["max_tenants"].as_i64()),
        ("partner_tenant_limit", Some(2))
    );
    // A tenant of no partner answers the partner key as a missing one (J10).
    let foreign = tenant(&h, &p, "direct");
    let fid = foreign["id"].as_str().unwrap();
    let r = h.call("GET", &format!("/v1/tenants/{fid}"), Some(&pk), None, &[]);
    let missing = h.call(
        "GET",
        "/v1/tenants/ten_01ARYZ6S410000000000000000",
        Some(&pk),
        None,
        &[],
    );
    assert_eq!((r.status, r.code()), (404, "tenant_not_found"));
    assert_eq!(r.body["error"]["message"], missing.body["error"]["message"]);
    let list = h.call("GET", "/v1/tenants", Some(&pk), None, &[]);
    assert_eq!(list.body["data"].as_array().unwrap().len(), 2);
    // DELETE refuses while tenants remain.
    let r = h.call(
        "DELETE",
        &format!("/v1/partners/{pid}"),
        Some(&p),
        None,
        &[],
    );
    assert_eq!(
        (r.code(), r.body["error"]["details"]["tenants"].as_i64()),
        ("partner_has_tenants", Some(2))
    );
    // Suspension contains the partner's keys and its tenants' keys (J13).
    let tk = h.seed_key(
        "tenant",
        &["identities:read"],
        t["id"].as_str(),
        None,
        None,
        None,
    );
    assert_eq!(
        h.call(
            "PATCH",
            &format!("/v1/partners/{pid}"),
            Some(&p),
            Some(json!({ "status": "suspended" })),
            &[]
        )
        .status,
        200
    );
    assert_eq!(
        h.call("GET", "/v1/me", Some(&pk), None, &[]).code(),
        "partner_suspended"
    );
    assert_eq!(
        h.call("GET", "/v1/me", Some(&tk), None, &[]).code(),
        "partner_suspended"
    );
    let actions: Vec<String> = h
        .sql(
            "SELECT action FROM audit_log ORDER BY created_at, id",
            vec![],
        )
        .iter()
        .map(|r| r["action"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        actions.contains(&"partner.create".to_owned())
            && actions.contains(&"partner.update".to_owned())
    );
}

#[test]
fn partner_suspension_rules_j17() {
    let h = Harness::new();
    let p = h.platform_key();
    let pid = h
        .call(
            "POST",
            "/v1/partners",
            Some(&p),
            Some(json!({ "name": "P" })),
            &[],
        )
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let pk = h.call("POST", "/v1/keys", Some(&p), Some(json!({ "name": "pk", "level": "partner", "partner_id": pid, "permissions": ["tenants:manage"] })), &[]);
    let pk = pk.body["secret"].as_str().unwrap().to_owned();
    let t = tenant(&h, &pk, "acme");
    let path = format!("/v1/tenants/{}", t["id"].as_str().unwrap());
    let r = h.call(
        "PATCH",
        &path,
        Some(&p),
        Some(json!({ "status": "suspended" })),
        &[],
    );
    assert_eq!(
        (r.body["status"].as_str(), r.body["suspended_by"].as_str()),
        (Some("suspended"), Some("platform"))
    );
    // The partner's no-op suspension keeps the platform's, and it cannot lift it.
    let r = h.call(
        "PATCH",
        &path,
        Some(&pk),
        Some(json!({ "status": "suspended" })),
        &[],
    );
    assert_eq!(r.body["suspended_by"], "platform");
    let r = h.call(
        "PATCH",
        &path,
        Some(&pk),
        Some(json!({ "status": "active" })),
        &[],
    );
    assert_eq!(
        (r.code(), r.body["error"]["details"]["field"].as_str()),
        ("scope_denied", Some("status"))
    );
    // A ceiling: a platform value on a lower-only field bounds the partner.
    h.call(
        "PATCH",
        &path,
        Some(&p),
        Some(json!({ "status": "active", "policy": { "tenant_daily_send_cap": 300 } })),
        &[],
    );
    let r = h.call(
        "PATCH",
        &path,
        Some(&pk),
        Some(json!({ "policy": { "tenant_daily_send_cap": 301 } })),
        &[],
    );
    assert_eq!(r.code(), "scope_denied");
    let r = h.call(
        "PATCH",
        &path,
        Some(&pk),
        Some(json!({ "policy": { "tenant_daily_send_cap": 200 } })),
        &[],
    );
    assert_eq!(
        (r.status, r.body["policy"]["tenant_daily_send_cap"].as_i64()),
        (200, Some(200))
    );
}

#[test]
fn keys_scope_levels_and_limits() {
    let h = Harness::new();
    let p = h.platform_key();
    let t = tenant(&h, &p, "acme");
    let tid = t["id"].as_str().unwrap().to_owned();
    let tk = h.seed_key(
        "tenant",
        &["keys:manage", "identities:read"],
        Some(&tid),
        None,
        None,
        None,
    );
    let mint = |key: &str, body: Value| h.call("POST", "/v1/keys", Some(key), Some(body), &[]);
    // Permission checks come first (400), then scope (403).
    let r = mint(
        &tk,
        json!({ "name": "x", "level": "tenant", "tenant_id": tid, "permissions": ["tenants:manage"] }),
    );
    assert_eq!(
        r.body["error"]["details"]["reason"],
        "permission_not_allowed_for_level"
    );
    let r = mint(
        &tk,
        json!({ "name": "x", "level": "tenant", "tenant_id": tid, "permissions": ["messages:read"] }),
    );
    assert_eq!(r.code(), "key_scope_exceeded");
    let r = mint(
        &tk,
        json!({ "name": "x", "level": "platform", "permissions": ["identities:read"] }),
    );
    assert_eq!(r.code(), "key_scope_exceeded");
    let r = mint(
        &tk,
        json!({ "name": "x", "level": "tenant", "tenant_id": tid, "permissions": [] }),
    );
    assert_eq!(r.code(), "invalid_request");
    // tenants:erase only from an owner's key (W35, owner-only part).
    let r = mint(
        &p,
        json!({ "name": "x", "level": "tenant", "tenant_id": tid, "permissions": ["tenants:erase"] }),
    );
    assert_eq!(
        r.body["error"]["details"]["reason"],
        "permission_owner_only"
    );
    let owner = h.seed_key(
        "tenant",
        &["keys:manage", "tenants:erase"],
        Some(&tid),
        None,
        None,
        Some("owner"),
    );
    assert_eq!(mint(&owner, json!({ "name": "x", "level": "tenant", "tenant_id": tid, "permissions": ["tenants:erase"] })).status, 201);
    // A tenant key naming another tenant in the body gets key_scope_exceeded (body scope ignored).
    let other = tenant(&h, &p, "other");
    let r = mint(
        &tk,
        json!({ "name": "x", "level": "tenant", "tenant_id": other["id"], "permissions": ["identities:read"] }),
    );
    assert_eq!(r.code(), "key_scope_exceeded");
    // Granted within scope; provider-managed keys resist the tenant (W48); reach (J20).
    let ok = mint(
        &p,
        json!({ "name": "agent", "level": "tenant", "tenant_id": tid, "permissions": ["identities:read"] }),
    );
    assert_eq!(ok.status, 201);
    let kid = ok.body["id"].as_str().unwrap();
    let r = h.call("DELETE", &format!("/v1/keys/{kid}"), Some(&tk), None, &[]);
    assert_eq!(
        (r.code(), r.body["error"]["details"]["reason"].as_str()),
        ("key_scope_exceeded", Some("provider_managed"))
    );
    assert_eq!(
        h.call("GET", &format!("/v1/keys/{kid}"), Some(&tk), None, &[])
            .status,
        200
    );
    let pkid = h.call("GET", "/v1/me", Some(&p), None, &[]).body["key_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        h.call("GET", &format!("/v1/keys/{pkid}"), Some(&tk), None, &[])
            .code(),
        "key_not_found"
    );
    let listed = h.call("GET", "/v1/keys", Some(&tk), None, &[]);
    assert!(
        listed.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|k| k["tenant_id"] == json!(tid))
    );
    // The active key cap (J22).
    h.sql(
        "UPDATE api_keys SET created_at = created_at WHERE 0",
        vec![],
    );
    for i in 0..(crate::handlers::keys::TENANT_KEY_LIMIT - 4) {
        let r = mint(
            &p,
            json!({ "name": format!("k{i}"), "level": "tenant", "tenant_id": tid, "permissions": ["identities:read"] }),
        );
        assert_eq!(r.status, 201, "{i}: {}", r.body);
    }
    let r = mint(
        &p,
        json!({ "name": "over", "level": "tenant", "tenant_id": tid, "permissions": ["identities:read"] }),
    );
    assert_eq!((r.status, r.code()), (422, "key_limit_reached"));
    // Pages: 25 by default, then a cursor.
    let page = h.call(
        "GET",
        &format!("/v1/keys?tenant_id={tid}"),
        Some(&p),
        None,
        &[],
    );
    assert_eq!(page.body["data"].as_array().unwrap().len(), 25);
    let cursor = page.body["next_cursor"].as_str().unwrap().to_owned();
    let next = h.call(
        "GET",
        &format!("/v1/keys?tenant_id={tid}&cursor={cursor}"),
        Some(&p),
        None,
        &[],
    );
    assert_eq!(next.status, 200, "{}", next.body);
    assert_ne!(next.body["data"][0]["id"], page.body["data"][0]["id"]);
    let bad = h.call(
        "GET",
        &format!("/v1/keys?cursor={cursor}"),
        Some(&p),
        None,
        &[],
    );
    assert_eq!(
        bad.body["error"]["details"]["errors"][0]["path"], "cursor",
        "another filter"
    );
    // Revoke and rotate are audited.
    let actions = h.sql(
        "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'key.create'",
        vec![],
    );
    assert!(actions[0]["n"].as_i64().unwrap() > 90);
    let aud = h.call(
        "GET",
        &format!("/v1/audit-events?actor_key_id={pkid}&action=key.create&limit=2"),
        Some(&p),
        None,
        &[],
    );
    assert_eq!(aud.body["data"].as_array().unwrap().len(), 2);
}

/// J11: a partner key never mints a partner key, its own partner's included.
#[test]
fn partner_key_mints_no_partner_key() {
    let h = Harness::new();
    let p = h.platform_key();
    let pid = h
        .call(
            "POST",
            "/v1/partners",
            Some(&p),
            Some(json!({ "name": "P" })),
            &[],
        )
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = json!({ "name": "pk", "level": "partner", "partner_id": pid, "permissions": ["keys:manage", "identities:read"] });
    let pk = h.call("POST", "/v1/keys", Some(&p), Some(body.clone()), &[]);
    let pk = pk.body["secret"].as_str().unwrap().to_owned();
    let r = h.call("POST", "/v1/keys", Some(&pk), Some(body), &[]);
    assert_eq!((r.status, r.code()), (403, "key_scope_exceeded"));
}

/// J26: a `null` resets a field to the deployment default and removes the writer's ceiling.
#[test]
fn policy_null_resets_to_default() {
    let h = Harness::new();
    let p = h.platform_key();
    let t = tenant(&h, &p, "acme");
    let path = format!("/v1/tenants/{}/policy", t["id"].as_str().unwrap());
    let set = h.call(
        "PATCH",
        &path,
        Some(&p),
        Some(json!({ "tenant_daily_send_cap": 10 })),
        &[],
    );
    assert_eq!(set.body["policy"]["tenant_daily_send_cap"], 10);
    let r = h.call(
        "PATCH",
        &path,
        Some(&p),
        Some(json!({ "tenant_daily_send_cap": null })),
        &[],
    );
    assert_eq!(
        (r.status, r.body["policy"]["tenant_daily_send_cap"].as_i64()),
        (200, Some(5000)),
        "{}",
        r.body
    );
}

#[test]
fn workspace_policy_j23_to_j27() {
    let h = Harness::new();
    let p = h.platform_key();
    let pid = h
        .call(
            "POST",
            "/v1/partners",
            Some(&p),
            Some(json!({ "name": "P" })),
            &[],
        )
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let pk = h.call("POST", "/v1/keys", Some(&p), Some(json!({ "name": "pk", "level": "partner", "partner_id": pid, "permissions": ["tenants:manage", "policy:write"] })), &[]);
    let pk = pk.body["secret"].as_str().unwrap().to_owned();
    let t = tenant(&h, &pk, "acme");
    let tid = t["id"].as_str().unwrap().to_owned();
    let path = format!("/v1/tenants/{tid}/policy");
    let wk = h.seed_key("tenant", &["policy:write"], Some(&tid), None, None, None);
    // J26: the partner's value bounds the workspace.
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&pk),
            Some(json!({ "tenant_daily_send_cap": 1000 })),
            &[]
        )
        .status,
        200
    );
    let r = h.call(
        "PATCH",
        &path,
        Some(&wk),
        Some(json!({ "tenant_daily_send_cap": 1001 })),
        &[],
    );
    assert_eq!(
        r.body["error"]["details"],
        json!({ "field": "tenant_daily_send_cap", "reason": "above_ceiling", "ceiling": 1000, "ceiling_source": "partner" })
    );
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&wk),
            Some(json!({ "tenant_daily_send_cap": 400 })),
            &[]
        )
        .status,
        200
    );
    assert_eq!(
        h.call(
            "PATCH",
            &path,
            Some(&wk),
            Some(json!({ "tenant_daily_send_cap": 1000 })),
            &[]
        )
        .status,
        200,
        "back up to the ceiling"
    );
    // J24: not writable.
    let r = h.call(
        "PATCH",
        &path,
        Some(&wk),
        Some(json!({ "quarantine": { "key_release": true } })),
        &[],
    );
    assert_eq!(
        r.body["error"]["details"],
        json!({ "field": "quarantine.key_release", "reason": "not_writable" })
    );
    // J25: a guard field only tightens from a key while key release is on... the test deployment's
    // PM_QUARANTINE_KEY_RELEASE defaults on, so turn it off.
    let h2 = Harness::with_vars(&[("PM_QUARANTINE_KEY_RELEASE", "off")]);
    let p2 = h2.platform_key();
    let t2 = tenant(&h2, &p2, "acme");
    let wk2 = h2.seed_key(
        "tenant",
        &["policy:write"],
        t2["id"].as_str(),
        None,
        None,
        None,
    );
    let path2 = format!("/v1/tenants/{}/policy", t2["id"].as_str().unwrap());
    let r = h2.call(
        "PATCH",
        &path2,
        Some(&wk2),
        Some(json!({ "quarantine": { "on_auth_fail": false } })),
        &[],
    );
    assert_eq!(
        (r.code(), r.body["error"]["details"]["reason"].as_str()),
        ("permission_denied", Some("person_required"))
    );
    assert_eq!(
        h2.call(
            "PATCH",
            &path2,
            Some(&wk2),
            Some(json!({ "screening": { "mode": "hold" } })),
            &[]
        )
        .status,
        200
    );
    let view = h2.call("GET", &path2, Some(&wk2), None, &[]);
    serde_json::from_value::<pylota_identity_api_types::TenantPolicyView>(view.body.clone())
        .unwrap();
    let guard = view.body["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["field"] == "quarantine.on_auth_fail")
        .unwrap()
        .clone();
    assert_eq!(guard["person_required_to_loosen"], true);
    // J27 and the audit row and event of every write.
    assert_eq!(view.body["policy_version"], 1);
    let audits = h2.sql(
        "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'tenant.policy_update'",
        vec![],
    );
    let events = h2.sql(
        "SELECT COUNT(*) AS n FROM event_index WHERE type = 'tenant.policy_updated'",
        vec![],
    );
    assert_eq!(
        (audits[0]["n"].as_i64(), events[0]["n"].as_i64()),
        (Some(1), Some(1))
    );
    // An identity key can never hold policy:write.
    let r = h.call("POST", "/v1/keys", Some(&p), Some(json!({ "name": "i", "level": "identity", "identity_id": "idn_01ARYZ6S410000000000000000", "permissions": ["policy:write"] })), &[]);
    assert_eq!(
        r.body["error"]["details"]["reason"],
        "permission_not_allowed_for_level"
    );
}

#[test]
fn rate_limited_and_aggregate_limits() {
    let h = Harness::new();
    let p = h.platform_key();
    let t = tenant(&h, &p, "acme");
    let tid = t["id"].as_str().unwrap().to_owned();
    let a = h.seed_key("tenant", &["identities:read"], Some(&tid), None, None, None);
    let b = h.seed_key("tenant", &["identities:read"], Some(&tid), None, None, None);
    h.app
        .p
        .limits
        .set_limit(pylota_identity_platform::ratelimit::RlBucket::Tenant, 3);
    for k in [&a, &b, &a] {
        assert_eq!(h.call("GET", "/v1/me", Some(k), None, &[]).status, 200);
    }
    let r = h.call("GET", "/v1/me", Some(&b), None, &[]);
    assert_eq!(
        (
            r.status,
            r.code(),
            r.body["error"]["details"]["bucket"].as_str()
        ),
        (429, "rate_limited", Some("tenant"))
    );
    assert_eq!(r.header("retry-after"), Some("60"));
    assert_eq!(r.header("ratelimit-limit"), Some("1800"));
    assert!(
        pylota_identity_platform::fakes::block_on(
            h.app
                .p
                .limits()
                .allow(pylota_identity_platform::ratelimit::RlBucket::Api, "x")
        )
        .unwrap()
    );
}

#[test]
fn signing_key_rotation_route() {
    let h = Harness::new();
    let p = h.platform_key();
    let r = h.call(
        "POST",
        "/v1/platform/keys/thread/rotate",
        Some(&p),
        None,
        &[],
    );
    assert_eq!(
        (
            r.status,
            r.body["kid"].as_str(),
            r.body["previous"]["kid"].as_str()
        ),
        (200, Some("1"), Some("0"))
    );
    let r = h.call(
        "POST",
        "/v1/platform/keys/link/rotate?revoke_previous=true",
        Some(&p),
        None,
        &[],
    );
    assert_eq!(r.body["previous"]["revoked"], true);
    assert_eq!(
        h.call("POST", "/v1/platform/keys/nope/rotate", Some(&p), None, &[])
            .code(),
        "invalid_request"
    );
    let _ = h.app.p.config();
}
