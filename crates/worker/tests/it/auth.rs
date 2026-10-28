//! Authentication (Security § 4, § 13).

use serde_json::json;

use crate::{call, fakes, get, mint, platform, post, tenant};

/// An unknown lookup, a wrong secret, a malformed key and no key get byte-identical bodies but for the
/// request ID.
#[test]
fn unknown_key_uniform() {
    let p = platform();
    let unknown = get(
        "/v1/me",
        "pmk_live_000000000000_0000000000000000000000000000000000000000000000000000",
    );
    let wrong = get("/v1/me", &format!("{}{}", &p[..22], "1".repeat(52)));
    let malformed = get("/v1/me", "pmk_live_short");
    let missing = call("GET", "/v1/me", None, None, &[]);
    assert_eq!((unknown.status, unknown.code()), (401, "unauthenticated"));
    for other in [&wrong, &malformed, &missing] {
        assert_eq!(other.without_request_id(), unknown.without_request_id());
    }
}

/// Revocation and expiry are reported only once the secret matched.
#[test]
fn status_after_secret_match() {
    let _reset = fakes::Reset;
    let p = platform();
    let (id, secret) = mint(
        &p,
        json!({ "name": "revoke-me", "level": "platform", "permissions": ["audit:read"] }),
    );
    assert_eq!(crate::delete(&format!("/v1/keys/{id}"), &p).status, 204);
    assert_eq!(get("/v1/me", &secret).code(), "key_revoked");
    let wrong_secret = format!("{}{}", &secret[..22], "1".repeat(52));
    assert_eq!(get("/v1/me", &wrong_secret).code(), "unauthenticated");
    let expires = chrono_like_in(120);
    let (_, exp) = mint(
        &p,
        json!({ "name": "expire-me", "level": "platform", "permissions": ["audit:read"], "expires_at": expires }),
    );
    assert_eq!(get("/v1/me", &exp).status, 200);
    fakes::advance(121_000);
    assert_eq!(get("/v1/me", &exp).code(), "key_expired");
}

/// RFC 3339 for now + `s` seconds (the Worker's clock is real time plus the fake offset, 0 here).
pub fn chrono_like_in(s: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    pylota_identity_core::days::rfc3339(now + s * 1000)
}

/// The old secret works during the overlap and stops after it; the new one works throughout.
#[test]
fn rotation_overlap() {
    let _reset = fakes::Reset;
    let p = platform();
    let (id, old) = mint(
        &p,
        json!({ "name": "rotate-me", "level": "platform", "permissions": ["audit:read"] }),
    );
    let r = post(
        &format!("/v1/keys/{id}/rotate"),
        &p,
        json!({ "overlap_hours": 1 }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let new = r.body["secret"].as_str().unwrap().to_owned();
    assert_eq!(&new[..22], &old[..22]);
    assert_eq!(get("/v1/me", &old).status, 200);
    assert_eq!(get("/v1/me", &new).status, 200);
    fakes::advance(3_600_001);
    assert_eq!(get("/v1/me", &old).code(), "unauthenticated");
    assert_eq!(get("/v1/me", &new).status, 200);
}

/// `last_used_at` is written at most once a minute per key.
#[test]
fn last_used_throttle() {
    let _reset = fakes::Reset;
    let p = platform();
    let (id, k) = mint(
        &p,
        json!({ "name": "used", "level": "platform", "permissions": ["keys:manage"] }),
    );
    let used = || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let v = get(&format!("/v1/keys/{id}"), &p).body["last_used_at"].clone();
            if !v.is_null() || std::time::Instant::now() > deadline {
                return v;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    get("/v1/me", &k);
    let first = used();
    assert!(first.is_string());
    fakes::advance(30_000);
    get("/v1/me", &k);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(used(), first);
    fakes::advance(31_000);
    get("/v1/me", &k);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while used() == first && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_ne!(used(), first);
}

/// A missing permission is 403 with `details.required`.
#[test]
fn missing_permission() {
    let p = platform();
    let t = tenant(&p, "missing_permission");
    let (_, tk) = mint(
        &p,
        json!({ "name": "t", "level": "tenant", "tenant_id": t["id"], "permissions": ["identities:read"] }),
    );
    let r = get("/v1/audit-events", &tk);
    assert_eq!((r.status, r.code()), (403, "permission_denied"));
    assert_eq!(r.details()["required"], json!(["audit:read"]));
}

/// J22: the tenant's keys share `RL_TENANT`; the tenant's active-key cap holds.
#[test]
fn j22_tenant_aggregate_limits() {
    let p = platform();
    let t = tenant(&p, "j22");
    let tid = t["id"].as_str().unwrap();
    let mut keys = Vec::new();
    for i in 0..100 {
        let r = post(
            "/v1/keys",
            &p,
            json!({ "name": format!("k{i}"), "level": "tenant", "tenant_id": tid, "permissions": ["identities:read"] }),
        );
        assert_eq!(r.status, 201, "{i}: {}", r.body);
        if i < 3 {
            keys.push(r.body["secret"].as_str().unwrap().to_owned());
        }
    }
    let r = post(
        "/v1/keys",
        &p,
        json!({ "name": "over", "level": "tenant", "tenant_id": tid, "permissions": ["identities:read"] }),
    );
    assert_eq!(
        (r.status, r.code(), r.details()["limit"].as_i64()),
        (422, "key_limit_reached", Some(100))
    );
    // RL_TENANT (1,800 a minute) counts the tenant's keys together: four keys at 450 requests each stay
    // under RL_API (600 per key) and use the tenant's budget, so the next request is refused for the tenant.
    let _ = keys;
    let t2 = tenant(&p, "j22_rate");
    let keys: Vec<String> = (0..4)
        .map(|i| mint(&p, json!({ "name": format!("r{i}"), "level": "tenant", "tenant_id": t2["id"], "permissions": ["identities:read"] })).1)
        .collect();
    let per_key = 1_800 / keys.len();
    crate::fresh_minute(50);
    let refused = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for k in &keys {
            let refused = &refused;
            s.spawn(move || {
                for _ in 0..per_key {
                    if get("/v1/me", k).status == 429 {
                        refused.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let r = get("/v1/me", &keys[0]);
    assert_eq!(
        (r.status, r.details()["bucket"].as_str()),
        (429, Some("tenant")),
        "refused early: {refused:?}"
    );
    assert_eq!(r.header("ratelimit-limit"), Some("1800"));
    assert!(r.header("retry-after").is_some());
}

/// F2: a key without `search:read` gets `403 permission_denied` naming it on every search route; with
/// it, the same requests answer.
#[test]
fn f2_permission() {
    let (tid, idn, _, _) = crate::mailbox_fixture("f2perm");
    let p = platform();
    let (_, without) = mint(
        &p,
        json!({ "name": "nosearch", "level": "tenant", "tenant_id": tid, "permissions": ["messages:read"] }),
    );
    let (_, with) = mint(
        &p,
        json!({ "name": "search", "level": "tenant", "tenant_id": tid, "permissions": ["search:read"] }),
    );
    let msg = "msg_01ARYZ6S41TSV4RRFFQ69G5FAV";
    for key in [&without, &with] {
        let rs = [
            post(
                &format!("/v1/identities/{idn}/search"),
                key,
                json!({ "q": "x", "mode": "keyword" }),
            ),
            post(
                &format!("/v1/tenants/{tid}/search"),
                key,
                json!({ "q": "x", "mode": "keyword" }),
            ),
            get(&format!("/v1/identities/{idn}/contacts"), key),
            get(&format!("/v1/identities/{idn}/messages/{msg}/related"), key),
        ];
        for r in rs {
            if key == &without {
                assert_eq!(
                    (r.status, r.code()),
                    (403, "permission_denied"),
                    "{}",
                    r.body
                );
                assert_eq!(r.details()["required"], json!(["search:read"]));
            } else {
                assert!(
                    r.status == 200 || r.code() == "message_not_found",
                    "{}",
                    r.body
                );
            }
        }
    }
}
