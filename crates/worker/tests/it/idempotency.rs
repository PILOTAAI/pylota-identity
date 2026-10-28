//! Idempotent replays (REST API › Idempotency).

use serde_json::json;

use crate::{call, mint, platform, tenant};

/// J19: a replay is per key, and a replay of a response that carried a secret returns none.
#[test]
fn j19_per_key_no_secret() {
    let p = platform();
    let t = tenant(&p, "j19");
    let tid = t["id"].as_str().unwrap();
    let (_, k1) = mint(
        &p,
        json!({ "name": "a", "level": "tenant", "tenant_id": tid, "permissions": ["keys:manage", "webhooks:manage"] }),
    );
    let (_, k2) = mint(
        &p,
        json!({ "name": "b", "level": "tenant", "tenant_id": tid, "permissions": ["keys:manage", "webhooks:manage"] }),
    );
    let idem = crate::slug("j19");
    let send = |key: &str, path: &str, body: serde_json::Value| {
        call(
            "POST",
            path,
            Some(key),
            Some(body),
            &[("idempotency-key", idem.as_str())],
        )
    };
    let key_body =
        json!({ "name": "c", "level": "tenant", "tenant_id": tid, "permissions": ["usage:read"] });
    let a = send(&k1, "/v1/keys", key_body.clone());
    assert_eq!(a.status, 201, "{}", a.body);
    assert!(a.body["secret"].is_string());
    let replay = send(&k1, "/v1/keys", key_body.clone());
    assert_eq!(replay.header("idempotent-replayed"), Some("true"));
    assert!(replay.body.get("secret").is_none());
    assert_eq!(replay.body["secret_replayed"], false);
    assert_eq!(replay.body["id"], a.body["id"]);
    // Another key with the same Idempotency-Key gets its own request, not the replay.
    let other = send(&k2, "/v1/keys", key_body);
    assert_eq!(other.status, 201);
    assert_ne!(other.body["id"], a.body["id"]);
    assert!(other.body["secret"].is_string());
    // Webhook creation and secret rotation.
    let whk = json!({ "url": "https://j19.hooks.example.com/a", "events": ["identity.created"] });
    let path = format!("/v1/tenants/{tid}/webhooks");
    let idem2 = crate::slug("j19w");
    let send2 = |path: &str, body: serde_json::Value| {
        call(
            "POST",
            path,
            Some(&k1),
            Some(body),
            &[("idempotency-key", idem2.as_str())],
        )
    };
    let w = send2(&path, whk.clone());
    assert!(w.body["secret"].is_string());
    let wr = send2(&path, whk);
    assert_eq!(
        (
            wr.body.get("secret").is_none(),
            wr.body["secret_replayed"].as_bool()
        ),
        (true, Some(false))
    );
    let rot_path = format!(
        "/v1/webhooks/{}/rotate-secret",
        w.body["id"].as_str().unwrap()
    );
    let idem3 = crate::slug("j19r");
    let rot = call(
        "POST",
        &rot_path,
        Some(&k1),
        Some(json!({ "overlap_hours": 0 })),
        &[("idempotency-key", idem3.as_str())],
    );
    assert!(rot.body["secret"].is_string(), "{}", rot.body);
    let rot2 = call(
        "POST",
        &rot_path,
        Some(&k1),
        Some(json!({ "overlap_hours": 0 })),
        &[("idempotency-key", idem3.as_str())],
    );
    assert_eq!(rot2.body["secret_replayed"], false);
}
