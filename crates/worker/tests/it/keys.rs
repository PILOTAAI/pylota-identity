//! API keys (Security § 4.5–4.6, J6, J11, J20, W35).

use serde_json::json;

use crate::{delete, get, mint, owner_key, partner, platform, post, tenant};

/// A key never creates, reads, rotates or revokes a key wider than itself (J20).
#[test]
fn scope_exceeded() {
    let p = platform();
    let t = tenant(&p, "scope_exceeded");
    let tid = t["id"].as_str().unwrap();
    let (_, tk) = mint(
        &p,
        json!({ "name": "tk", "level": "tenant", "tenant_id": tid, "permissions": ["keys:manage", "identities:read"] }),
    );
    // Creation.
    for body in [
        json!({ "name": "x", "level": "tenant", "tenant_id": tid, "permissions": ["messages:read"] }),
        json!({ "name": "x", "level": "platform", "permissions": ["identities:read"] }),
    ] {
        assert_eq!(post("/v1/keys", &tk, body).code(), "key_scope_exceeded");
    }
    // Management of a wider key of the same tenant: readable by nobody narrower.
    let (wide, _) = mint(
        &p,
        json!({ "name": "wide", "level": "tenant", "tenant_id": tid, "permissions": ["keys:manage", "identities:read", "audit:read"] }),
    );
    assert_eq!(
        get(&format!("/v1/keys/{wide}"), &tk).code(),
        "key_scope_exceeded"
    );
    assert_eq!(
        post(&format!("/v1/keys/{wide}/rotate"), &tk, json!({})).code(),
        "key_scope_exceeded"
    );
    assert_eq!(
        delete(&format!("/v1/keys/{wide}"), &tk).code(),
        "key_scope_exceeded"
    );
    // A platform key ID is out of reach: 404 as for a missing one.
    let me = get("/v1/me", &p).body["key_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(get(&format!("/v1/keys/{me}"), &tk).code(), "key_not_found");
    assert_eq!(
        get("/v1/keys/key_01ARYZ6S410000000000000000", &tk).code(),
        "key_not_found"
    );
    // The list holds only keys that pass both rules.
    let listed = get("/v1/keys", &tk);
    let ids: Vec<&str> = listed.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&wide.as_str()));
}

/// Which levels can hold which permissions; refused permissions are 400 before any scope check.
#[test]
fn permission_level_rules() {
    let p = platform();
    let t = tenant(&p, "permission_level_rules");
    let tid = t["id"].as_str().unwrap();
    for (level, perm) in [
        ("tenant", "tenants:manage"),
        ("tenant", "partners:manage"),
        ("identity", "audit:read"),
        ("platform", "identities:sign"),
    ] {
        let mut body = json!({ "name": "x", "level": level, "permissions": [perm] });
        if level != "platform" {
            body["tenant_id"] = tid.into();
        }
        if level == "identity" {
            body["identity_id"] = "idn_01ARYZ6S410000000000000000".into();
        }
        let r = post("/v1/keys", &p, body);
        assert_eq!(
            r.details()["reason"],
            "permission_not_allowed_for_level",
            "{level} {perm}: {}",
            r.body
        );
    }
    assert_eq!(
        post(
            "/v1/keys",
            &p,
            json!({ "name": "x", "level": "platform", "permissions": [] })
        )
        .code(),
        "invalid_request"
    );
    // A platform key grants identities:sign without holding it.
    let r = post(
        "/v1/keys",
        &p,
        json!({ "name": "s", "level": "tenant", "tenant_id": tid, "permissions": ["identities:sign"] }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
}

/// W35, owner-only part: `tenants:erase` on a tenant key only from an owner's key.
#[test]
fn w35_tenant_erase_owner_only() {
    let p = platform();
    let t = tenant(&p, "w35");
    let tid = t["id"].as_str().unwrap();
    let body = json!({ "name": "e", "level": "tenant", "tenant_id": tid, "permissions": ["tenants:erase"] });
    assert_eq!(
        post("/v1/keys", &p, body.clone()).details()["reason"],
        "permission_owner_only"
    );
    let admin = post(
        "/v1/keys",
        &p,
        json!({ "name": "a", "level": "tenant", "tenant_id": tid, "permissions": ["keys:manage", "tenants:erase"] }),
    );
    assert_eq!(admin.details()["reason"], "permission_owner_only");
    let owner = owner_key(tid, &["keys:manage", "tenants:erase"]);
    let r = post("/v1/keys", &owner, body);
    assert_eq!(r.status, 201, "{}", r.body);
    // The minted key carries the owner's trace, so it may delegate further.
    let child = r.body["secret"].as_str().unwrap().to_owned();
    let _ = child;
}

/// J6: revoke and rotate are audited and found by `actor_key_id`.
#[test]
fn j6_revoke_rotate() {
    let p = platform();
    let me = get("/v1/me", &p).body["key_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (a, _) = mint(
        &p,
        json!({ "name": "a", "level": "platform", "permissions": ["audit:read"] }),
    );
    let (b, _) = mint(
        &p,
        json!({ "name": "b", "level": "platform", "permissions": ["audit:read"] }),
    );
    assert_eq!(
        post(
            &format!("/v1/keys/{a}/rotate"),
            &p,
            json!({ "overlap_hours": 0 })
        )
        .status,
        200
    );
    assert_eq!(delete(&format!("/v1/keys/{b}"), &p).status, 204);
    let rows = get(&format!("/v1/audit-events?actor_key_id={me}&limit=100"), &p);
    let actions: Vec<(String, String)> = rows.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["action"].as_str().unwrap().to_owned(),
                r["target_id"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    assert!(actions.contains(&("key.rotate".into(), a.clone())));
    assert!(actions.contains(&("key.revoke".into(), b.clone())));
    assert!(actions.contains(&("key.create".into(), a)));
}

/// J11: a partner key mints only tenant and identity keys of its own tenants, at most 10 partner keys
/// per partner, and never a partner or platform key.
#[test]
fn j11_partner_key_limits() {
    let p = platform();
    let (pid, pk) = partner("j11", &["tenants:manage", "keys:manage", "identities:read"]);
    let own = tenant(&pk, "j11_own");
    let other = tenant(&p, "j11_other");
    assert_eq!(post("/v1/keys", &pk, json!({ "name": "t", "level": "tenant", "tenant_id": own["id"], "permissions": ["identities:read"] })).status, 201);
    assert_eq!(post("/v1/keys", &pk, json!({ "name": "t", "level": "tenant", "tenant_id": other["id"], "permissions": ["identities:read"] })).code(), "key_scope_exceeded");
    let r = post(
        "/v1/keys",
        &pk,
        json!({ "name": "p", "level": "partner", "partner_id": pid, "permissions": ["identities:read"] }),
    );
    assert_eq!(r.code(), "key_scope_exceeded", "{}", r.body);
    for i in 0..9 {
        mint(
            &p,
            json!({ "name": format!("pk{i}"), "level": "partner", "partner_id": pid, "permissions": ["identities:read"] }),
        );
    }
    let r = post(
        "/v1/keys",
        &p,
        json!({ "name": "over", "level": "partner", "partner_id": pid, "permissions": ["identities:read"] }),
    );
    assert_eq!(
        (r.code(), r.details()["limit"].as_i64()),
        ("key_limit_reached", Some(10))
    );
}
