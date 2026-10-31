//! Identity keys through workerd (Agent signing keys § 2, § 3.1; O1–O3, O28, FR-IDN-6).

use serde_json::{Value, json};

use crate::assertions::{Signer, assert_as, jwks, signer, token, verified};
use crate::send::events_of;
use crate::{fakes, get, patch, platform, post};

fn keys(s: &Signer) -> Vec<Value> {
    let r = get(&format!("/v1/identities/{}/keys", s.iid), &s.key);
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["data"].as_array().cloned().unwrap_or_default()
}

fn kids(list: &[Value]) -> Vec<String> {
    list.iter()
        .map(|k| k["kid"].as_str().unwrap().to_owned())
        .collect()
}

fn rotate(s: &Signer) -> crate::Resp {
    post(
        &format!("/v1/identities/{}/keys/rotate", s.iid),
        &s.key,
        json!({}),
    )
}

fn revoke(s: &Signer, kid: &str) -> crate::Resp {
    post(
        &format!("/v1/identities/{}/keys/{kid}/revoke", s.iid),
        &s.key,
        json!({}),
    )
}

/// O2: the first signature creates the key; a rotation keeps the previous key in the JWKS until
/// `verify_until`, so a token signed before it still verifies; new tokens use the new key.
#[test]
fn lazy_create_and_rotate() {
    let _reset = fakes::Reset;
    let s = signer("lazy_create_and_rotate");
    assert!(keys(&s).is_empty());
    let (status, _, r) = jwks(&s.iid);
    assert_eq!((status, r.body["keys"].clone()), (200, json!([])));
    assert_eq!(r.header("content-type"), Some("application/jwk-set+json"));
    assert_eq!(r.header("cache-control"), Some("public, max-age=300"));

    let before = token(&s);
    let first = before["kid"].as_str().unwrap().to_owned();
    let ks = keys(&s);
    assert_eq!(
        (kids(&ks), &ks[0]["status"]),
        (vec![first.clone()], &json!("active"))
    );
    assert_eq!(ks[0]["public_jwk"]["kid"], first);
    assert!(ks[0].get("private_enc").is_none());
    let created = events_of(&s.iid, "identity.key_created");
    assert_eq!(created, vec![json!({ "identity_id": s.iid, "kid": first })]);

    let r = rotate(&s);
    assert_eq!(r.status, 200, "{}", r.body);
    let second = r.body["key"]["kid"].as_str().unwrap().to_owned();
    assert_ne!(second, first);
    assert_eq!(r.body["previous"]["kid"], first);
    assert_eq!(r.body["previous"]["status"], "retiring");
    assert!(r.body["previous"]["verify_until"].is_string());
    assert_eq!(
        events_of(&s.iid, "identity.key_rotated"),
        vec![json!({ "identity_id": s.iid, "kid": second, "previous_kid": first })]
    );
    let (_, published, _) = jwks(&s.iid);
    assert_eq!(kids(&published), vec![second.clone(), first.clone()]);
    // The token signed before the rotation still verifies; a new one uses the new key.
    verified(&s.iid, before["assertion"].as_str().unwrap());
    assert_eq!(token(&s)["kid"], second);

    // Seven days later the retiring key leaves the JWKS, before the retention job retires it.
    fakes::advance(7 * 86_400_000 + 1_000);
    let (_, published, _) = jwks(&s.iid);
    assert_eq!(kids(&published), vec![second]);
    assert!(
        keys(&s)
            .iter()
            .any(|k| k["kid"] == first && k["status"] == "retiring")
    );
}

/// O3: a revoked key is `retired` and absent from the next JWKS; revoking again changes nothing.
#[test]
fn revoke_removes_from_jwks() {
    let _reset = fakes::Reset;
    let s = signer("revoke_removes_from_jwks");
    let kid = token(&s)["kid"].as_str().unwrap().to_owned();
    let r = revoke(&s, &kid);
    assert_eq!(
        (r.status, &r.body["status"]),
        (200, &json!("retired")),
        "{}",
        r.body
    );
    assert!(r.body["retired_at"].is_string());
    let (_, published, _) = jwks(&s.iid);
    assert!(published.is_empty());
    assert_eq!(
        events_of(&s.iid, "identity.key_revoked"),
        vec![json!({ "identity_id": s.iid, "kid": kid })]
    );
    let again = revoke(&s, &kid);
    assert_eq!(
        (again.status, &again.body["status"]),
        (200, &json!("retired"))
    );
    assert_eq!(events_of(&s.iid, "identity.key_revoked").len(), 1);
    let unknown = revoke(&s, &"A".repeat(43));
    assert_eq!((unknown.status, unknown.code()), (404, "key_not_found"));
    // Another tenant's key on this identity's keys: the same 404 as a missing identity.
    let other = signer("revoke_removes_from_jwks_other");
    let r = get(&format!("/v1/identities/{}/keys", s.iid), &other.key);
    assert_eq!((r.status, r.code()), (404, "identity_not_found"));
}

/// O1: a paused identity cannot sign (`409`) and its JWKS is withdrawn (`404`), while its keys can
/// still be managed; a suspended tenant is `403 tenant_suspended`, checked first; a deleted identity is
/// `404` on both.
#[test]
fn paused_withdraws_jwks() {
    let _reset = fakes::Reset;
    let p = platform();
    let s = signer("paused_withdraws_jwks");
    token(&s);
    let r = patch(
        &format!("/v1/identities/{}", s.iid),
        &p,
        json!({ "status": "paused" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let r = assert_as(&s, json!({ "audience": "x" }));
    assert_eq!((r.status, r.code()), (409, "identity_paused"));
    let (status, _, r) = jwks(&s.iid);
    assert_eq!((status, r.code()), (404, "identity_not_found"));
    assert_eq!(rotate(&s).status, 200, "key management works while paused");
    let r = patch(
        &format!("/v1/identities/{}", s.iid),
        &p,
        json!({ "status": "active" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(jwks(&s.iid).0, 200);

    let r = patch(
        &format!("/v1/tenants/{}", s.tid),
        &p,
        json!({ "status": "suspended" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let r = assert_as(&s, json!({ "audience": "x" }));
    assert_eq!((r.status, r.code()), (403, "tenant_suspended"));
    assert_eq!(jwks(&s.iid).0, 404);
    let r = patch(
        &format!("/v1/tenants/{}", s.tid),
        &p,
        json!({ "status": "active" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);

    let unknown = jwks("idn_01ARYZ6S410000000000000000");
    assert_eq!((unknown.0, unknown.2.code()), (404, "identity_not_found"));
    crate::d1(&format!(
        "UPDATE identities SET status = 'deleting' WHERE id = '{}'",
        s.iid
    ));
    assert_eq!(
        assert_as(&s, json!({ "audience": "x" })).code(),
        "identity_not_found"
    );
    assert_eq!(jwks(&s.iid).0, 404);
    crate::d1(&format!(
        "UPDATE identities SET status = 'active' WHERE id = '{}'",
        s.iid
    ));
}

/// O28: concurrent first signatures and a `POST …/keys` leave one active key and one
/// `identity.key_created`; after the active key is revoked the next signature creates a new key, and a
/// rotation with no active key has no previous key.
#[test]
fn o28_concurrent_first_sign_and_revoke_active() {
    let _reset = fakes::Reset;
    let s = signer("o28");
    let path = format!("/v1/identities/{}/assertions", s.iid);
    let keys_path = format!("/v1/identities/{}/keys", s.iid);
    let tokens: Vec<String> = std::thread::scope(|sc| {
        let mut hs: Vec<_> = (0..10)
            .map(|_| sc.spawn(|| post(&path, &s.key, json!({ "audience": "x" }))))
            .collect();
        let create = sc.spawn(|| post(&keys_path, &s.key, json!({})));
        let mut out = Vec::new();
        for h in hs.drain(..) {
            let r = h.join().unwrap();
            assert_eq!(r.status, 201, "{}", r.body);
            out.push(r.body["assertion"].as_str().unwrap().to_owned());
        }
        let c = create.join().unwrap();
        assert!(matches!(c.status, 200 | 201), "{}", c.body);
        out
    });
    let ks = keys(&s);
    assert_eq!(
        ks.iter().filter(|k| k["status"] == "active").count(),
        1,
        "{ks:?}"
    );
    assert_eq!(ks.len(), 1, "the losers' keys were never stored: {ks:?}");
    let kid = ks[0]["kid"].as_str().unwrap().to_owned();
    assert_eq!(events_of(&s.iid, "identity.key_created").len(), 1);
    for t in &tokens {
        assert_eq!(
            pylota_identity_core::jwt::header(t).unwrap()["kid"],
            kid.as_str()
        );
        verified(&s.iid, t);
    }
    // The second POST …/keys answers the active key with 200.
    let again = post(&keys_path, &s.key, json!({}));
    assert_eq!((again.status, &again.body["kid"]), (200, &json!(kid)));

    assert_eq!(revoke(&s, &kid).status, 200);
    let next = token(&s)["kid"].as_str().unwrap().to_owned();
    assert_ne!(next, kid);
    assert_eq!(events_of(&s.iid, "identity.key_created").len(), 2);
    assert_eq!(revoke(&s, &next).status, 200);
    let r = rotate(&s);
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["previous"], Value::Null);
    assert_eq!(events_of(&s.iid, "identity.key_created").len(), 3);
    assert!(events_of(&s.iid, "identity.key_rotated").is_empty());
}
