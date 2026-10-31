//! The master-key rotation's Worker half against the workerd harness (Security § 6.2, build plan M17
//! Foundation): the two slots, the re-seal sweep over the sealed-column registry and the `master_key`
//! block of `GET /v1/platform/status`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pylota_identity_core::crypto;
use pylota_identity_core::sealed::SEALED;
use serde_json::{Value, json};

use crate::{cron, d1, fakes, get, identity, mint, platform, post, slug, tenant};

fn harness_key() -> [u8; 32] {
    STANDARD
        .decode(std::env::var("PM_ITEST_MASTER_KEY").unwrap())
        .unwrap()
        .try_into()
        .unwrap()
}

fn master_key() -> Value {
    let r = get("/v1/platform/status", &platform());
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["master_key"].clone()
}

/// Runs the every-minute cron until the sweep reports nothing left (at most `runs` times).
fn sweep(runs: usize) -> Value {
    for _ in 0..runs {
        cron("* * * * *");
        let mk = master_key();
        if mk["remaining"] == 0 {
            return mk;
        }
    }
    master_key()
}

/// `pm1|table|column|row id` envelope of `plain` under `key`, as SQL (`CAST(… AS BLOB)` for a blob).
fn sealed(
    key: &[u8; 32],
    table: &str,
    column: &str,
    rid: &str,
    plain: &[u8],
    blob: bool,
) -> String {
    let nonce = next_nonce();
    let env = crypto::seal(key, nonce, &crypto::aad(table, column, rid), plain);
    if blob {
        format!("CAST('{env}' AS BLOB)")
    } else {
        format!("'{env}'")
    }
}

/// A nonce never used before by this process (the test seals a handful of values).
fn next_nonce() -> [u8; 12] {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut b = [0u8; 12];
    b[..8].copy_from_slice(&n.to_be_bytes());
    b[8..].copy_from_slice(&std::process::id().to_be_bytes());
    b
}

/// Every non-null value of every registered column: `(table, column, row id, envelope)`.
fn all_sealed() -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    for s in SEALED {
        let rows = d1(&format!(
            "SELECT {} AS rid, CAST({} AS TEXT) AS env FROM {} WHERE {} IS NOT NULL",
            s.row_id, s.column, s.table, s.column
        ));
        for r in rows.as_array().unwrap() {
            out.push((
                s.table.to_owned(),
                s.column.to_owned(),
                r["rid"].as_str().unwrap().to_owned(),
                r["env"].as_str().unwrap().to_owned(),
            ));
        }
    }
    out
}

/// `it::secrets::master_key_rotation` (Security § 6.2): with a key in each slot and
/// `PM_MASTER_KEY_ACTIVE` switched to the new one, old and new ciphertexts open and new ones use the
/// new kid; `GET /v1/platform/status` reports `remaining` falling to 0, `activated_at` and then
/// `resealed_at`; a D1 state from before the rotation (restored from a snapshot) still opens and is
/// re-sealed; with only the active slot set, a ciphertext of the overwritten key reports
/// `secret_unavailable`; and the sweep re-seals every column of the sealed-column registry, one case
/// per registered column (the registry naming exactly the schema's sealed columns is
/// `core::sealed::registry_matches_schema`). The test rotates back to the harness key before it ends.
#[test]
fn master_key_rotation() {
    let _r = fakes::Reset;
    let a = harness_key();
    let k2 = [0x22u8; 32];
    let k3 = [0x33u8; 32];
    let (kid_a, kid2) = (crypto::kid(&a), crypto::kid(&k2));

    // One value in every registered column, sealed under the harness key.
    let t = tenant(&platform(), "master key");
    let tid = t["id"].as_str().unwrap().to_owned();
    let (_, key) = mint(
        &platform(),
        json!({ "name": "mk", "level": "tenant", "tenant_id": tid, "permissions": ["webhooks:manage", "identities:write"] }),
    );
    let (url, _) = crate::webhooks::receiver("master-key");
    let w = post(
        &format!("/v1/tenants/{tid}/webhooks"),
        &key,
        json!({ "url": url, "events": ["identity.created"], "headers": { "X-Gateway-Token": "gw-token-1" } }),
    );
    assert_eq!(w.status, 201, "{}", w.body);
    let wid = w.body["id"].as_str().unwrap().to_owned();
    let r = post(
        &format!("/v1/webhooks/{wid}/rotate-secret"),
        &key,
        json!({ "overlap_hours": 1 }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let idn = identity(&key, &tid, &slug("mk")[..8]);
    let iid = idn["id"].as_str().unwrap();
    let ik = format!("ik{}", slug("k"));
    let usr = format!("usr_{}", slug("u"));
    let state = format!("st{}", slug("o"));
    let wba = "W".repeat(43);
    let dom = d1("SELECT id FROM domains WHERE kind = 'platform' LIMIT 1")[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let now = fakes::worker_now();
    d1(&format!(
        "INSERT INTO identity_keys (id, identity_id, tenant_id, alg, public_jwk, private_enc, status, created_at) VALUES ('{ik}', '{iid}', '{tid}', 'EdDSA', '{{}}', {}, 'active', {now})",
        sealed(&a, "identity_keys", "private_enc", &ik, &[7; 32], true)
    ));
    d1(&format!(
        "INSERT INTO users (id, email, totp_sealed, recovery_codes_sealed, totp_pending_sealed, created_at) VALUES ('{usr}', '{usr}@people.example', {}, {}, {}, {now})",
        sealed(
            &a,
            "users",
            "totp_sealed",
            &usr,
            b"totp-secret-20-bytes",
            true
        ),
        sealed(&a, "users", "recovery_codes_sealed", &usr, b"[]", true),
        sealed(
            &a,
            "users",
            "totp_pending_sealed",
            &usr,
            b"pending-20-bytes....",
            true
        )
    ));
    d1(&format!(
        "INSERT INTO oauth_states (state_hash, cookie_hash, key_kid, provider, intent, pkce_sealed, created_at, expires_at) VALUES ('{state}', 'c', 'a', 'google', 'sign_in', {}, {now}, {})",
        sealed(&a, "oauth_states", "pkce_sealed", &state, b"verifier", true),
        now + 600_000
    ));
    d1(&format!(
        // Retiring, so it never competes with a deployment key another test created.
        "INSERT INTO signing_keys (purpose, kid, ciphertext, public_jwk, created_at, verify_until) VALUES ('web_bot_auth', '{wba}', {}, '{{}}', {now}, {})",
        sealed(
            &a,
            "signing_keys",
            "ciphertext",
            &format!("web_bot_auth:{wba}"),
            &[9; 32],
            true
        ),
        now + 86_400_000
    ));
    d1(&format!(
        "UPDATE domains SET smtp_sealed = {}, smtp_pending_sealed = {} WHERE id = '{dom}'",
        sealed(
            &a,
            "domains",
            "smtp_sealed",
            &dom,
            b"{\"host\":\"smtp.example\"}",
            true
        ),
        sealed(
            &a,
            "domains",
            "smtp_pending_sealed",
            &dom,
            b"{\"host\":\"smtp2.example\"}",
            true
        )
    ));
    let before = all_sealed();
    for s in SEALED {
        assert!(
            before
                .iter()
                .any(|(t, c, _, _)| t == s.table && c == s.column),
            "a case for {}.{}",
            s.table,
            s.column
        );
    }
    let plain = |table: &str, column: &str, rid: &str, env: &str, keys: &[&[u8; 32]]| {
        crypto::open(keys, &crypto::aad(table, column, rid), env)
    };
    let originals: Vec<Vec<u8>> = before
        .iter()
        .map(|(t, c, rid, env)| plain(t, c, rid, env, &[&a]).unwrap())
        .collect();

    // Before: slot a active, nothing left, activated and re-sealed under the harness key.
    let mk = sweep(3);
    assert_eq!(mk["active_slot"], "a");
    assert_eq!(mk["remaining"], 0);
    assert_eq!(mk["slots"][0]["kid"], kid_a.as_str());
    assert_eq!(mk["slots"][1]["present"], false);
    assert!(
        mk["activated_at"].is_string() && mk["resealed_at"].is_string(),
        "{mk}"
    );
    let secret_env = d1(&format!(
        "SELECT secret_enc FROM webhook_endpoints WHERE id = '{wid}'"
    ))[0]["secret_enc"]
        .as_str()
        .unwrap()
        .to_owned();

    // The rotation: the new key into slot b, then the selector.
    fakes::set_config("PM_MASTER_KEY_B", Some(&STANDARD.encode(k2)));
    fakes::set_config("PM_MASTER_KEY_ACTIVE", Some("b"));
    let mk = master_key();
    assert_eq!(mk["active_slot"], "b");
    assert_eq!(mk["slots"][1]["kid"], kid2.as_str());
    assert!(
        mk["remaining"].as_u64().unwrap() >= before.len() as u64,
        "{mk}"
    );
    assert!(
        mk["activated_at"].is_null() && mk["resealed_at"].is_null(),
        "{mk}"
    );
    // Old ciphertexts open; new ones use the new kid.
    let test = |id: &str| post(&format!("/v1/webhooks/{id}/test"), &key, json!({})).body;
    assert_eq!(test(&wid)["status"], "succeeded", "{}", test(&wid));
    let w2 = post(
        &format!("/v1/tenants/{tid}/webhooks"),
        &key,
        json!({ "url": url, "events": ["identity.created"] }),
    );
    let w2id = w2.body["id"].as_str().unwrap();
    let env2 = d1(&format!(
        "SELECT secret_enc FROM webhook_endpoints WHERE id = '{w2id}'"
    ))[0]["secret_enc"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(crypto::envelope_kid(&env2), Some(kid2.as_str()));

    // The sweep: every value re-sealed under the new key, opening to the same plaintext.
    let mk = sweep(10);
    assert_eq!(mk["remaining"], 0, "{mk}");
    assert!(
        mk["activated_at"].is_string() && mk["resealed_at"].is_string(),
        "{mk}"
    );
    assert!(mk["resealed_at"].as_str() >= mk["activated_at"].as_str());
    for ((t, c, rid, _), want) in before.iter().zip(&originals) {
        let env = all_sealed()
            .into_iter()
            .find(|(t2, c2, r2, _)| t2 == t && c2 == c && r2 == rid)
            .unwrap()
            .3;
        assert_eq!(crypto::envelope_kid(&env), Some(kid2.as_str()), "{t}.{c}");
        assert_eq!(&plain(t, c, rid, &env, &[&k2]).unwrap(), want, "{t}.{c}");
    }
    assert!(
        d1(&format!(
            "SELECT typeof(private_enc) AS t FROM identity_keys WHERE id = '{ik}'"
        ))[0]["t"]
            == "blob",
        "a blob column stays a blob"
    );
    assert_eq!(test(&wid)["status"], "succeeded");

    // A D1 state from before the rotation: still opens, and is re-sealed.
    let resealed_env = d1(&format!(
        "SELECT secret_enc FROM webhook_endpoints WHERE id = '{wid}'"
    ))[0]["secret_enc"]
        .as_str()
        .unwrap()
        .to_owned();
    let restore = |env: &str| {
        d1(&format!(
            "UPDATE webhook_endpoints SET secret_enc = '{env}' WHERE id = '{wid}'"
        ));
    };
    restore(&secret_env);
    assert_eq!(master_key()["remaining"], 1);
    assert_eq!(test(&wid)["status"], "succeeded");
    assert_eq!(sweep(3)["remaining"], 0);

    // Only the active slot holds a known key: the overwritten key's ciphertext does not open.
    fakes::set_config("PM_MASTER_KEY", Some(&STANDARD.encode(k3)));
    restore(&secret_env);
    // Past the 60-second cache of opened secrets, which still holds the earlier opening.
    fakes::advance(61_000);
    let r = test(&wid);
    assert_ne!(r["status"], "succeeded", "{r}");
    assert_eq!(r["error"], "secret_unavailable", "{r}");
    cron("* * * * *");
    assert_eq!(
        master_key()["remaining"],
        1,
        "an unopenable value stays counted"
    );

    // Back to the harness key, which the rest of the suite uses.
    restore(&resealed_env);
    fakes::set_config("PM_MASTER_KEY", Some(&STANDARD.encode(a)));
    fakes::set_config("PM_MASTER_KEY_ACTIVE", Some("a"));
    assert_eq!(sweep(10)["remaining"], 0);
    for (t, c, rid, env) in all_sealed() {
        assert_eq!(
            crypto::envelope_kid(&env),
            Some(kid_a.as_str()),
            "{t}.{c} {rid}"
        );
    }
    d1(&format!(
        "DELETE FROM identity_keys WHERE id = '{ik}'; DELETE FROM users WHERE id = '{usr}'; DELETE FROM oauth_states WHERE state_hash = '{state}'; \
         DELETE FROM signing_keys WHERE purpose = 'web_bot_auth' AND kid = '{wba}'; UPDATE domains SET smtp_sealed = NULL, smtp_pending_sealed = NULL WHERE id = '{dom}'"
    ));
}

/// A cursor of `GET /v1/tenants` (one tenant per page) and its kid.
fn tenants_cursor() -> (String, String) {
    let r = get("/v1/tenants?limit=1", &platform());
    assert_eq!(r.status, 200, "{}", r.body);
    let c = r.body["next_cursor"].as_str().unwrap().to_owned();
    let kid = c[2..3].to_owned();
    (c, kid)
}

fn page(cursor: &str) -> u16 {
    get(&format!("/v1/tenants?limit=1&cursor={cursor}"), &platform()).status
}

fn rotate(purpose: &str, revoke: bool) -> Value {
    let q = if revoke { "?revoke_previous=true" } else { "" };
    let r = post(
        &format!("/v1/platform/keys/{purpose}/rotate{q}"),
        &platform(),
        json!({}),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    r.body
}

/// `it::secrets::signing_key_rotation` (Security § 6.2): for `cursor`, a rotation makes new cursors
/// carry the new kid while old ones verify until `verify_until` (24 hours) and fail after it; with
/// `?revoke_previous=true` they fail at once and the response has `previous.revoked: true`. For
/// `thread` and `link` the response names the next kid and the previous key's window (90 and 7 days).
/// No response holds key material, and a 33rd rotation inside the window retires the oldest kid, so a
/// purpose never holds more than 32 keys.
#[test]
fn signing_key_rotation() {
    let _r = fakes::Reset;
    for _ in 0..2 {
        tenant(&platform(), "cursor pages");
    }
    let (old, kid0) = tenants_cursor();
    let r = rotate("cursor", false);
    assert_eq!(r["purpose"], "cursor");
    assert_eq!(r["previous"]["kid"], kid0.as_str());
    assert_eq!(r["previous"]["revoked"], false);
    let created =
        pylota_identity_core::days::parse_rfc3339(r["created_at"].as_str().unwrap()).unwrap();
    let until =
        pylota_identity_core::days::parse_rfc3339(r["previous"]["verify_until"].as_str().unwrap())
            .unwrap();
    assert_eq!(until - created, 86_400_000);
    let keys: Vec<&String> = r.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["created_at", "kid", "previous", "purpose"],
        "no key material"
    );
    let kid1 = r["kid"].as_str().unwrap().to_owned();
    assert_ne!(kid1, kid0);

    // Past the 60-second key cache: new cursors carry the new kid; the old one still verifies.
    fakes::advance(61_000);
    let (new, k) = tenants_cursor();
    assert_eq!(k, kid1);
    assert_eq!(page(&old), 200);
    assert_eq!(page(&new), 200);
    // After its window the old kid's cursors fail.
    fakes::advance(86_400_000);
    assert_ne!(page(&old), 200);

    // Revoked at once.
    let (fresh, _) = tenants_cursor();
    let r = rotate("cursor", true);
    assert_eq!(r["previous"]["revoked"], true);
    assert_eq!(r["previous"]["verify_until"], r["created_at"]);
    fakes::advance(61_000);
    assert_ne!(page(&fresh), 200);
    assert_eq!(tenants_cursor().1, r["kid"].as_str().unwrap());

    // Thread and link: the next kid and the previous window.
    for (purpose, window) in [("thread", 90 * 86_400_000i64), ("link", 7 * 86_400_000)] {
        let r = rotate(purpose, false);
        let c =
            pylota_identity_core::days::parse_rfc3339(r["created_at"].as_str().unwrap()).unwrap();
        let u = pylota_identity_core::days::parse_rfc3339(
            r["previous"]["verify_until"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(u - c, window, "{purpose}");
        assert_eq!(r["kid"].as_str().unwrap().len(), 1);
        assert!(!r.to_string().contains("ciphertext"));
    }

    // 33 rotations inside the window: never more than 32 keys of the purpose.
    for _ in 0..33 {
        rotate("cursor", false);
    }
    let n = d1("SELECT COUNT(*) AS n FROM signing_keys WHERE purpose = 'cursor'")[0]["n"]
        .as_u64()
        .unwrap();
    assert!(n <= 32, "{n} cursor keys");
    fakes::advance(61_000);
    tenants_cursor();
}

/// O8: assertions signed before and after a master-key rotation verify with the same published key;
/// the identity's sealed seed is re-sealed under the new key.
#[test]
fn rotate_master_reseals_identity_keys() {
    let _r = fakes::Reset;
    let a = harness_key();
    let k2 = [0x44u8; 32];
    let s = crate::assertions::signer("rotate_master_reseals_identity_keys");
    let before = crate::assertions::token(&s);
    let kid = before["kid"].as_str().unwrap().to_owned();
    fakes::set_config("PM_MASTER_KEY_B", Some(&STANDARD.encode(k2)));
    fakes::set_config("PM_MASTER_KEY_ACTIVE", Some("b"));
    assert_eq!(sweep(10)["remaining"], 0);
    let env = d1(&format!(
        "SELECT CAST(private_enc AS TEXT) AS env FROM identity_keys WHERE id = '{kid}'"
    ))[0]["env"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(crypto::envelope_kid(&env), Some(crypto::kid(&k2).as_str()));
    let after = crate::assertions::token(&s);
    assert_eq!(after["kid"], kid.as_str());
    for t in [&before, &after] {
        crate::assertions::verified(&s.iid, t["assertion"].as_str().unwrap());
    }
    fakes::set_config("PM_MASTER_KEY", Some(&STANDARD.encode(a)));
    fakes::set_config("PM_MASTER_KEY_ACTIVE", Some("a"));
    assert_eq!(
        sweep(10)["remaining"],
        0,
        "back to the harness key, which the suite uses"
    );
}
