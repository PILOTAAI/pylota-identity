//! Partners and partner keys (FR-KEY-4, J10, J12, J13, J17, J18).

use serde_json::json;

use crate::{delete, get, partner, patch, platform, post, tenant};

#[test]
fn routes_and_audit() {
    let p = platform();
    let r = post(
        "/v1/partners",
        &p,
        json!({ "name": "RoutesCo", "default_billing_mode": "exempt", "max_tenants": 3, "ramp_exempt": true }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let id = r.body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        get(&format!("/v1/partners/{id}"), &p).body["ramp_exempt"],
        true
    );
    assert_eq!(
        patch(
            &format!("/v1/partners/{id}"),
            &p,
            json!({ "max_tenants": 4 })
        )
        .body["max_tenants"],
        4
    );
    assert!(
        get("/v1/partners?status=active", &p).body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == id.as_str())
    );
    assert_eq!(delete(&format!("/v1/partners/{id}"), &p).status, 204);
    let gone = get(&format!("/v1/partners/{id}"), &p);
    assert_eq!(
        (gone.body["status"].as_str(), gone.body["name"].as_str()),
        (Some("deleted"), Some(""))
    );
    assert_eq!(
        patch(&format!("/v1/partners/{id}"), &p, json!({ "name": "x" })).code(),
        "partner_not_found"
    );
    let audit = get(&format!("/v1/audit-events?target_id={id}"), &p);
    let actions: Vec<&str> = audit.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        ["partner.delete", "partner.update", "partner.create"]
    );
    // A partner key never reaches /v1/partners.
    let (_, pk) = partner("RoutesCo2", &["tenants:manage"]);
    assert_eq!(get("/v1/partners", &pk).code(), "permission_denied");
}

/// A partner key may lower a lower-only field to its ceiling, never above, and never a platform-only one.
#[test]
fn policy_caps_lower_only() {
    let (_, pk) = partner("caps", &["tenants:manage"]);
    let t = tenant(&pk, "caps");
    let path = format!("/v1/tenants/{}", t["id"].as_str().unwrap());
    assert_eq!(
        patch(
            &path,
            &pk,
            json!({ "policy": { "tenant_daily_send_cap": 100 } })
        )
        .status,
        200
    );
    let r = patch(
        &path,
        &pk,
        json!({ "policy": { "tenant_daily_send_cap": 5001 } }),
    );
    assert_eq!(
        (r.code(), r.details()["field"].as_str()),
        ("scope_denied", Some("tenant_daily_send_cap"))
    );
    let r = patch(
        &path,
        &pk,
        json!({ "policy": { "web_bot_auth": { "allowed": true } } }),
    );
    assert_eq!(
        (r.code(), r.details()["field"].as_str()),
        ("scope_denied", Some("web_bot_auth.allowed"))
    );
    assert_eq!(
        patch(
            &path,
            &pk,
            json!({ "policy": { "quarantine": { "key_release": true } } })
        )
        .status,
        200
    );
}

/// J10: another partner's tenant, or a tenant of no partner, answers as a missing one.
#[test]
fn j10_foreign_partner_not_found() {
    let p = platform();
    let (_, a) = partner("j10a", &["tenants:manage", "keys:manage", "audit:read"]);
    let (_, b) = partner("j10b", &["tenants:manage"]);
    let theirs = tenant(&b, "j10_theirs");
    let direct = tenant(&p, "j10_direct");
    let missing = get("/v1/tenants/ten_01ARYZ6S410000000000000000", &a);
    for t in [&theirs, &direct] {
        let id = t["id"].as_str().unwrap();
        for r in [
            get(&format!("/v1/tenants/{id}"), &a),
            patch(&format!("/v1/tenants/{id}"), &a, json!({ "name": "x" })),
        ] {
            assert_eq!(r.status, 404);
            assert_eq!(r.without_request_id(), missing.without_request_id());
        }
        assert_eq!(
            get(&format!("/v1/audit-events?tenant_id={id}"), &a).code(),
            "tenant_not_found"
        );
    }
    assert!(
        get("/v1/tenants", &a).body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// J12: a partner with tenants that are not erased is not deleted.
#[test]
fn j12_delete_with_tenants() {
    let (pid, pk) = partner("j12", &["tenants:manage"]);
    tenant(&pk, "j12");
    let r = delete(&format!("/v1/partners/{pid}"), &platform());
    assert_eq!(
        (r.code(), r.details()["tenants"].as_i64()),
        ("partner_has_tenants", Some(1))
    );
    assert_eq!(
        get(&format!("/v1/partners/{pid}"), &platform()).body["status"],
        "active"
    );
}

/// J13, authentication part: a suspended partner's keys and its tenants' keys get 403 on every route.
#[test]
fn j13_suspended_partner() {
    let p = platform();
    let (pid, pk) = partner("j13", &["tenants:manage", "keys:manage", "identities:read"]);
    let t = tenant(&pk, "j13");
    let (_, tk) = crate::mint(
        &pk,
        json!({ "name": "t", "level": "tenant", "tenant_id": t["id"], "permissions": ["identities:read"] }),
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
    for k in [&pk, &tk] {
        let r = get("/v1/me", k);
        assert_eq!((r.status, r.code()), (403, "partner_suspended"));
    }
    assert_eq!(
        get(&format!("/v1/tenants/{}", t["id"].as_str().unwrap()), &p).body["status"],
        "active"
    );
    patch(
        &format!("/v1/partners/{pid}"),
        &p,
        json!({ "status": "active" }),
    );
    assert_eq!(get("/v1/me", &tk).status, 200);
}

/// J17: operator enforcement stays: a partner cannot lift a platform suspension, and its suspension of a
/// platform-suspended tenant changes nothing.
#[test]
fn j17_operator_enforcement() {
    let p = platform();
    let (_, pk) = partner("j17", &["tenants:manage"]);
    let t = tenant(&pk, "j17");
    let path = format!("/v1/tenants/{}", t["id"].as_str().unwrap());
    assert_eq!(
        patch(&path, &p, json!({ "status": "suspended" })).body["suspended_by"],
        "platform"
    );
    assert_eq!(
        patch(&path, &pk, json!({ "status": "suspended" })).body["suspended_by"],
        "platform"
    );
    let r = patch(&path, &pk, json!({ "status": "active" }));
    assert_eq!(
        (r.code(), r.details()["field"].as_str()),
        ("scope_denied", Some("status"))
    );
    patch(
        &path,
        &p,
        json!({ "status": "active", "policy": { "identity_daily_send_cap": 50 } }),
    );
    assert_eq!(
        patch(
            &path,
            &pk,
            json!({ "policy": { "identity_daily_send_cap": 51 } })
        )
        .code(),
        "scope_denied"
    );
}

/// J18: `max_tenants` holds under concurrent creations; creations count in RL_PARTNER.
#[test]
fn j18_partner_limits() {
    let p = platform();
    let pt = post(
        "/v1/partners",
        &p,
        json!({ "name": "j18", "max_tenants": 2 }),
    );
    let pid = pt.body["id"].as_str().unwrap().to_owned();
    let (_, pk) = crate::mint(
        &p,
        json!({ "name": "pk", "level": "partner", "partner_id": pid, "permissions": ["tenants:manage"] }),
    );
    let results: Vec<u16> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let pk = pk.clone();
                s.spawn(move || post("/v1/tenants", &pk, json!({ "slug": crate::slug(&format!("j18_{i}")), "name": "x", "mode": "live" })).status)
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(
        results.iter().filter(|s| **s == 201).count(),
        2,
        "{results:?}"
    );
    assert!(
        results.iter().all(|s| matches!(s, 201 | 403)),
        "{results:?}"
    );
    // RL_PARTNER: 10 creations a minute across the partner's keys.
    let pt = post(
        "/v1/partners",
        &p,
        json!({ "name": "j18b", "max_tenants": 100 }),
    );
    let pid = pt.body["id"].as_str().unwrap().to_owned();
    let (_, pk) = crate::mint(
        &p,
        json!({ "name": "pk", "level": "partner", "partner_id": pid, "permissions": ["tenants:manage"] }),
    );
    let mut codes = Vec::new();
    crate::fresh_minute(15);
    for i in 0..11 {
        let r = post(
            "/v1/tenants",
            &pk,
            json!({ "slug": crate::slug(&format!("j18b_{i}")), "name": "x", "mode": "live" }),
        );
        codes.push((
            r.status,
            r.details()["bucket"].as_str().unwrap_or("").to_owned(),
        ));
    }
    assert_eq!(codes[10], (429, "partner_create".to_owned()), "{codes:?}");
}
