//! Identities (Identities and domains § Tests; A5, A17, A18, FR-IDN-1, FR-IDN-4).

use serde_json::json;

use crate::{
    d1, default_tenant, delete, get, identity, mint, partner, patch, platform, post,
    system_identity, tenant,
};

/// FR-IDN-1: a replay returns `200` and the same identity; a changed body is `client_id_conflict`.
#[test]
fn client_id_idempotent() {
    let p = platform();
    let t = tenant(&p, "cid");
    let path = format!("/v1/tenants/{}/identities", t["id"].as_str().unwrap());
    let body = json!({ "username": "desk", "display_name": "Desk", "client_id": "op:desk" });
    let a = post(&path, &p, body.clone());
    assert_eq!(a.status, 201, "{}", a.body);
    let b = post(&path, &p, body);
    assert_eq!((b.status, &b.body["id"]), (200, &a.body["id"]));
    let c = post(
        &path,
        &p,
        json!({ "username": "desk", "display_name": "Other", "client_id": "op:desk" }),
    );
    assert_eq!((c.status, c.code()), (409, "client_id_conflict"));
}

/// A18: only a platform key creates identities in the default tenant.
#[test]
fn a18_default_tenant_platform_only() {
    let p = platform();
    let dt = default_tenant();
    let path = format!("/v1/tenants/{dt}/identities");
    let body = json!({ "username": "billing", "display_name": "Billing" });
    let (_, tk) = mint(
        &p,
        json!({ "name": "t", "level": "tenant", "tenant_id": dt, "permissions": ["identities:write", "identities:read"] }),
    );
    let (_, pk) = partner("a18", &["tenants:manage", "identities:write"]);
    for k in [&tk, &pk] {
        let r = post(&path, k, body.clone());
        assert_eq!(
            (r.status, r.code(), r.details()["reason"].as_str()),
            (403, "scope_denied", Some("default_tenant"))
        );
    }
    let sys = system_identity();
    let (_, ik) = mint(
        &p,
        json!({ "name": "i", "level": "identity", "tenant_id": dt, "identity_id": sys, "permissions": ["identities:read", "identities:write"] }),
    );
    assert_eq!(post(&path, &ik, body.clone()).code(), "scope_denied");
    assert!(
        d1(&format!(
            "SELECT id FROM identities WHERE tenant_id = '{dt}' AND username = 'billing'"
        ))
        .as_array()
        .unwrap()
        .is_empty()
    );
    let r = post(&path, &p, body);
    assert_eq!(
        (r.status, r.body["primary_address"].as_str()),
        (201, Some("billing@agents.example"))
    );
}

/// A17: the system identity and the default tenant are protected; a platform key may pause it.
#[test]
fn a17_system_protected() {
    let p = platform();
    let sys = system_identity();
    let dt = default_tenant();
    let audits = || {
        d1("SELECT COUNT(*) AS n FROM audit_log")[0]["n"]
            .as_i64()
            .unwrap()
    };
    let before = audits();
    let r = delete(&format!("/v1/identities/{sys}"), &p);
    assert_eq!((r.status, r.code()), (409, "system_protected"));
    let r = patch(
        &format!("/v1/tenants/{dt}"),
        &p,
        json!({ "status": "suspended" }),
    );
    assert_eq!((r.status, r.code()), (409, "system_protected"));
    assert_eq!(audits(), before, "nothing changes");
    assert!(
        d1(&format!(
            "SELECT id FROM erasure_requests WHERE identity_id = '{sys}'"
        ))
        .as_array()
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        get(&format!("/v1/tenants/{dt}"), &p).body["status"],
        "active"
    );
    // Never listed; reachable by a platform key only, which may pause and resume it.
    assert!(
        get("/v1/identities?limit=100", &p).body["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["id"] != sys.as_str())
    );
    let (_, tk) = mint(
        &p,
        json!({ "name": "t", "level": "tenant", "tenant_id": dt, "permissions": ["identities:read"] }),
    );
    assert_eq!(
        get(&format!("/v1/identities/{sys}"), &tk).code(),
        "identity_not_found"
    );
    assert_eq!(
        patch(
            &format!("/v1/identities/{sys}"),
            &p,
            json!({ "status": "paused" })
        )
        .body["status"],
        "paused"
    );
    assert_eq!(
        patch(
            &format!("/v1/identities/{sys}"),
            &p,
            json!({ "status": "active" })
        )
        .body["status"],
        "active"
    );
    // Its mailbox was minted by the cron, as an owner-initialised system mailbox.
    assert_ne!(
        d1(&format!(
            "SELECT mailbox_do_id FROM identities WHERE id = '{sys}'"
        ))[0]["mailbox_do_id"],
        ""
    );
}

/// A5: a deleted identity's address is never created again, by any tenant.
#[test]
fn a5_tombstone_blocks_reuse() {
    let p = platform();
    let t = tenant(&p, "a5");
    let tid = t["id"].as_str().unwrap();
    let idn = identity(&p, tid, "reuse");
    let id = idn["id"].as_str().unwrap();
    assert_eq!(delete(&format!("/v1/identities/{id}"), &p).status, 202);
    // While it is deleting, the username is still taken; once deleted (M14's job), the address is not
    // free either.
    let path = format!("/v1/tenants/{tid}/identities");
    assert_eq!(
        post(
            &path,
            &p,
            json!({ "username": "reuse", "display_name": "x" })
        )
        .code(),
        "username_taken"
    );
    d1(&format!(
        "UPDATE identities SET status = 'deleted' WHERE id = '{id}'"
    ));
    let r = post(
        &path,
        &p,
        json!({ "username": "reuse", "display_name": "x" }),
    );
    assert_eq!((r.status, r.code()), (409, "address_taken"));
    let hash = d1("SELECT address_hash, reason FROM address_tombstones");
    assert!(
        hash.as_array()
            .unwrap()
            .iter()
            .all(|h| h["address_hash"].as_str().unwrap().len() == 64)
    );
}

/// FR-IDN-4: a `deleting` and a `deleted` identity read with their status, refuse writes, and a second
/// `DELETE` answers the same request without starting a job.
#[test]
fn deleting_and_deleted_routes() {
    let p = platform();
    let t = tenant(&p, "del");
    let tid = t["id"].as_str().unwrap();
    for state in ["deleting", "deleted"] {
        let idn = identity(&p, tid, &format!("gone-{state}"));
        let id = idn["id"].as_str().unwrap();
        let path = format!("/v1/identities/{id}");
        let first = delete(&path, &p);
        assert_eq!(
            (first.status, first.body["scope"].as_str()),
            (202, Some("identity")),
            "{}",
            first.body
        );
        if state == "deleted" {
            d1(&format!(
                "UPDATE identities SET status = 'deleted' WHERE id = '{id}'"
            ));
        }
        let g = get(&path, &p);
        assert_eq!((g.status, g.body["status"].as_str()), (200, Some(state)));
        let r = patch(&path, &p, json!({ "display_name": "x" }));
        assert_eq!((r.status, r.code()), (404, "identity_not_found"));
        let jobs = || {
            d1(&format!(
                "SELECT COUNT(*) AS n FROM jobs WHERE params_json LIKE '%{id}%'"
            ))[0]["n"]
                .as_i64()
        };
        let n = jobs();
        let again = delete(&path, &p);
        assert_eq!((again.status, &again.body["id"]), (200, &first.body["id"]));
        assert_eq!(jobs(), n, "no new job");
        assert_eq!(
            d1(&format!(
                "SELECT status FROM jobs WHERE params_json LIKE '%{id}%'"
            ))[0]["status"],
            "queued"
        );
    }
}
