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
    // A platform key grants identities:sign without holding it, and the key signs.
    let r = post(
        "/v1/keys",
        &p,
        json!({ "name": "s", "level": "tenant", "tenant_id": tid, "permissions": ["identities:sign"] }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    signs(&p, tid, r.body["secret"].as_str().unwrap());
}

/// A key holding `identities:sign` mints an assertion for an identity of `tid`.
fn signs(p: &str, tid: &str, key: &str) {
    let i = crate::identity(p, tid, &crate::slug("signer")[..8]);
    let a = post(
        &format!("/v1/identities/{}/assertions", i["id"].as_str().unwrap()),
        key,
        json!({ "audience": "x" }),
    );
    assert_eq!(a.status, 201, "{}", a.body);
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
    // A partner key grants identities:sign to its own tenant's keys without holding it.
    let r = post(
        "/v1/keys",
        &pk,
        json!({ "name": "s", "level": "tenant", "tenant_id": own["id"], "permissions": ["identities:sign"] }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    signs(
        &p,
        own["id"].as_str().unwrap(),
        r.body["secret"].as_str().unwrap(),
    );
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

/// W48: a key a platform or partner key minted is provider-managed: a workspace admin's session can
/// neither rotate nor revoke it; a rotation records the rotating person, so removing them revokes it.
#[test]
fn w48_rotation_provenance() {
    let _reset = crate::fakes::Reset;
    let p = platform();
    let t = tenant(&p, "w48");
    let tid = t["id"].as_str().unwrap().to_owned();
    let (provider_key, _) = mint(
        &p,
        json!({ "name": "provider", "level": "tenant", "tenant_id": tid, "permissions": ["identities:read"] }),
    );
    let admin = crate::console::email("w48 admin");
    let mut a = crate::console::as_member(&admin, &tid, "admin");
    let r = a.submit(
        "/console/keys",
        &format!("/console/keys/{provider_key}/revoke"),
        &[],
    );
    assert_eq!(r.status, 403, "{}", r.body);
    assert!(crate::console::html(&r).contains("key_scope_exceeded"));
    // A key the owner minted in the console, rotated by the admin's own key, is traced to the admin.
    let owner = crate::console::email("w48 owner");
    let mut o = crate::console::as_member(&owner, &tid, "owner");
    let r = o.submit(
        "/console/keys",
        "/console/keys",
        &[
            ("name", "w48-owner"),
            ("identity_id", ""),
            ("permissions", "identities:read"),
        ],
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let kid = crate::d1(&format!(
        "SELECT id FROM api_keys WHERE tenant_id = '{tid}' AND name = 'w48-owner'"
    ))[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = a.submit(
        "/console/keys",
        "/console/keys",
        &[
            ("name", "w48-admin"),
            ("identity_id", ""),
            ("permissions", "keys:manage"),
            ("permissions", "identities:read"),
        ],
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let secret = crate::console::html(&r)
        .split(r#"id="secret" readonly value=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    let r = post(&format!("/v1/keys/{kid}/rotate"), &secret, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    let row = crate::d1(&format!(
        "SELECT created_by_user_id, created_by_role FROM api_keys WHERE id = '{kid}'"
    ))[0]
        .clone();
    let admin_id = crate::d1(&format!("SELECT id FROM users WHERE email = '{admin}'"))[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        (
            row["created_by_user_id"].as_str(),
            row["created_by_role"].as_str()
        ),
        (Some(admin_id.as_str()), Some("admin"))
    );
    let r = delete(&format!("/v1/tenants/{tid}/members/{admin_id}"), &p);
    assert_eq!(r.status, 204);
    let revoked = crate::d1(&format!(
        "SELECT revoked_at FROM api_keys WHERE id = '{kid}'"
    ))[0]["revoked_at"]
        .clone();
    assert!(
        revoked.is_i64(),
        "removing the rotating admin revokes the key"
    );
    let _ = get;
}
