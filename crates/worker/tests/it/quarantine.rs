//! Quarantine: release (Inbound › Read path and release), and who may release (J14, J16).

use serde_json::{Value, json};

use crate::messages::held;
use crate::{
    d1, dkim, get, inject, mail, mailbox_fixture, mint, partner, patch, platform, post, wait_stored,
};

#[test]
fn release_transaction() {
    let (tid, idn, addr, key) = mailbox_fixture("release");
    // A signed code mail nobody waited for: held, with its match kept for the release.
    let raw = dkim::sign(&mail(
        "security@brightwell.example",
        &addr,
        "Your verification code",
        "Your verification code is 135 790.",
    ));
    assert_eq!(inject("bounce@brightwell.example", &addr, &raw), 200);
    let q = wait_stored(&idn, &key, "quarantined", 1);
    let mid = q[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(q[0]["quarantine_reason"], "otp_unsolicited");
    let p = platform();
    assert_eq!(
        patch(
            &format!("/v1/tenants/{tid}/policy"),
            &p,
            json!({ "quarantine": { "key_release": true } })
        )
        .status,
        200
    );
    let r = post(
        &format!("/v1/identities/{idn}/messages/{mid}/release"),
        &p,
        json!({ "reason": "expected code" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        (
            r.body["status"].as_str(),
            r.body["quarantine_reason"].clone()
        ),
        (Some("received"), Value::Null)
    );
    assert_eq!(r.body["triage"]["status"], "pending");
    let again = post(
        &format!("/v1/identities/{idn}/messages/{mid}/release"),
        &p,
        json!({ "reason": "again" }),
    );
    assert_eq!((again.status, again.code()), (409, "not_quarantined"));
    assert_eq!(again.details()["status"], "received");
    // The verification row: wait releases the code.
    let w = get(
        &format!(
            "/v1/identities/{idn}/wait?from=@brightwell.example&kind=verification&since=2000-01-01T00:00:00Z&timeout=3"
        ),
        &key,
    );
    assert_eq!(w.body["verification"]["code"], "135 790", "{}", w.body);
    // Events: quarantined, released, received (released: true), once each.
    crate::alarm(&idn);
    let types: Vec<String> = crate::indexed(&idn)
        .into_iter()
        .map(|(t, _)| t)
        // `message.triaged` follows the release asynchronously (M12).
        .filter(|t| t.starts_with("message.") && t != "message.triaged")
        .collect();
    assert_eq!(
        types,
        [
            "message.quarantined",
            "message.released",
            "message.received"
        ]
    );
    let rows = crate::mailbox_query(
        &idn,
        "SELECT payload_json FROM outbox WHERE type = 'message.received'",
    );
    let env: Value = serde_json::from_str(rows[0]["payload_json"].as_str().unwrap()).unwrap();
    assert_eq!(env["data"]["released"], true);
    let audit = d1(&format!(
        "SELECT actor_key_id FROM audit_log WHERE action = 'quarantine.release' AND target_id = '{mid}'"
    ));
    assert_eq!(audit.as_array().unwrap().len(), 1);
    // Thread recomputed: listed, one message.
    let t = get(&format!("/v1/identities/{idn}/threads"), &key);
    assert_eq!(t.body["data"][0]["message_count"], 1);
}

#[test]
fn j14_key_release_policy() {
    let (tid, _, _, _) = mailbox_fixture("j14");
    let body = json!({ "quarantine": { "key_release": true } });
    let (_, tk) = mint(
        &platform(),
        json!({ "name": "t", "level": "tenant", "tenant_id": tid, "permissions": ["quarantine:review"] }),
    );
    let r = patch(
        &format!("/v1/tenants/{tid}"),
        &tk,
        json!({ "policy": body }),
    );
    assert_eq!(r.status, 403, "{}", r.body);
    assert_eq!(
        patch(&format!("/v1/tenants/{tid}/policy"), &tk, body.clone()).code(),
        "permission_denied"
    );
    let (_, wk) = mint(
        &platform(),
        json!({ "name": "w", "level": "tenant", "tenant_id": tid, "permissions": ["policy:write"] }),
    );
    let r = patch(&format!("/v1/tenants/{tid}/policy"), &wk, body);
    assert_eq!((r.status, r.code()), (403, "scope_denied"));
    assert_eq!(
        (
            r.details()["field"].as_str(),
            r.details()["reason"].as_str()
        ),
        (Some("quarantine.key_release"), Some("not_writable"))
    );
    let pol = get(&format!("/v1/tenants/{tid}/policy"), &platform());
    assert_ne!(pol.body["quarantine"]["key_release"], true);
}

#[test]
fn j16_key_release_override() {
    let (pid, pk) = partner(
        "j16",
        &[
            "tenants:manage",
            "quarantine:review",
            "messages:read",
            "policy:write",
            "identities:write",
        ],
    );
    let t = post(
        "/v1/tenants",
        &pk,
        json!({ "slug": crate::slug("j16"), "name": "j16", "mode": "live" }),
    );
    assert_eq!(t.status, 201, "{}", t.body);
    let tid = t.body["id"].as_str().unwrap().to_owned();
    let _ = pid;
    let idn = crate::identity(&pk, &tid, "jsixteen");
    let id = idn["id"].as_str().unwrap().to_owned();
    let addr = idn["primary_address"].as_str().unwrap().to_owned();
    let (_, tk) = mint(
        &platform(),
        json!({ "name": "t", "level": "tenant", "tenant_id": tid, "permissions": ["quarantine:review", "messages:read"] }),
    );
    for i in 0..3 {
        assert_eq!(
            inject(
                "z@h.example",
                &addr,
                &held("z@h.example", &addr, &format!("Held {i}"))
            ),
            200
        );
    }
    let q = wait_stored(&id, &tk, "quarantined", 3);
    let ids: Vec<String> = q
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect();
    let release = |k: &str, mid: &str| {
        post(
            &format!("/v1/identities/{id}/messages/{mid}/release"),
            k,
            json!({ "reason": "r" }),
        )
    };
    // PM_QUARANTINE_KEY_RELEASE=off and no tenant opt-in: every key is refused.
    for k in [&platform(), &pk, &tk] {
        let r = release(k, &ids[0]);
        assert_eq!(
            (r.status, r.code()),
            (403, "permission_denied"),
            "{}",
            r.body
        );
    }
    // The tenant's partner opts in: a platform key and the partner's key release; a tenant key does not.
    assert_eq!(
        patch(
            &format!("/v1/tenants/{tid}/policy"),
            &pk,
            json!({ "quarantine": { "key_release": true } })
        )
        .status,
        200
    );
    assert_eq!(release(&tk, &ids[0]).status, 403);
    assert_eq!(release(&platform(), &ids[0]).status, 200);
    assert_eq!(release(&pk, &ids[1]).status, 200);
    let audit = d1(&format!(
        "SELECT actor_key_id FROM audit_log WHERE action = 'quarantine.release' AND tenant_id = '{tid}'"
    ));
    assert_eq!(audit.as_array().unwrap().len(), 2);
}
