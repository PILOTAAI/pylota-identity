//! Workspace policy (Workspace policy § 8, J23–J27). The itest deployment runs with
//! `PM_QUARANTINE_KEY_RELEASE=off`, as Cloud does.

use serde_json::{Value, json};

use crate::{Resp, d1, get, mint, partner, patch, platform, post, tenant};

fn tenant_key(tid: &str, permissions: &[&str]) -> String {
    mint(
        &platform(),
        json!({ "name": "wk", "level": "tenant", "tenant_id": tid, "permissions": permissions }),
    )
    .1
}

fn refused(r: &Resp) -> (u16, &str, Value) {
    (r.status, r.code(), r.details().clone())
}

fn stored(tid: &str) -> Value {
    let rows = d1(&format!(
        "SELECT policy_json, policy_version FROM tenants WHERE id = '{tid}'"
    ));
    json!({ "policy": serde_json::from_str::<Value>(rows[0]["policy_json"].as_str().unwrap()).unwrap(), "version": rows[0]["policy_version"] })
}

#[test]
fn j23_workspace_ceilings() {
    let t = tenant(&platform(), "j23");
    let tid = t["id"].as_str().unwrap();
    let wk = tenant_key(tid, &["policy:write"]);
    let path = format!("/v1/tenants/{tid}/policy");
    let lower = json!({ "tenant_daily_send_cap": 4000, "retention": { "raw_days": 30 }, "abuse": { "bounce_rate_pause": 0.02 }, "triage": { "enabled": false } });
    assert_eq!(patch(&path, &wk, lower).status, 200);
    let before = stored(tid);
    let r = patch(
        &path,
        &wk,
        json!({ "max_recipients": 5, "tenant_daily_send_cap": 5001 }),
    );
    assert_eq!(
        refused(&r),
        (
            403,
            "scope_denied",
            json!({ "field": "tenant_daily_send_cap", "reason": "above_ceiling", "ceiling": 5000, "ceiling_source": "deployment" })
        )
    );
    let r = patch(
        &path,
        &wk,
        json!({ "inbound": { "extract_image_text": true } }),
    );
    assert_eq!(r.details()["reason"], "above_ceiling");
    assert_eq!(
        stored(tid),
        before,
        "nothing is stored, also for the valid field"
    );
    // A platform ceiling.
    assert_eq!(
        patch(
            &path,
            &platform(),
            json!({ "identity_daily_send_cap": 200 })
        )
        .status,
        200
    );
    assert_eq!(
        patch(&path, &wk, json!({ "identity_daily_send_cap": 200 })).status,
        200
    );
    let r = patch(&path, &wk, json!({ "identity_daily_send_cap": 201 }));
    assert_eq!(
        (
            r.details()["ceiling"].as_i64(),
            r.details()["ceiling_source"].as_str()
        ),
        (Some(200), Some("platform"))
    );
    // Free fields.
    assert_eq!(
        patch(
            &path,
            &wk,
            json!({ "search": { "custom_refs": [] }, "retention": { "message_days": 400 } })
        )
        .status,
        200
    );
    let view = get(&path, &wk);
    let f = view.body["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["field"] == "identity_daily_send_cap")
        .unwrap()
        .clone();
    assert_eq!(
        (f["ceiling"].as_i64(), f["ceiling_source"].as_str()),
        (Some(200), Some("platform"))
    );
}

#[test]
fn j24_workspace_field_classes() {
    let t = tenant(&platform(), "j24");
    let tid = t["id"].as_str().unwrap();
    let wk = tenant_key(tid, &["policy:write"]);
    let path = format!("/v1/tenants/{tid}/policy");
    for (field, body) in [
        (
            "web_bot_auth.allowed",
            json!({ "web_bot_auth": { "allowed": true } }),
        ),
        (
            "domains.allow_create_zone",
            json!({ "domains": { "allow_create_zone": true } }),
        ),
        (
            "domains.cloudflare_zones",
            json!({ "domains": { "cloudflare_zones": ["agents.example"] } }),
        ),
        (
            "quarantine.key_release",
            json!({ "quarantine": { "key_release": true } }),
        ),
    ] {
        let r = patch(&path, &wk, body);
        assert_eq!(
            refused(&r),
            (
                403,
                "scope_denied",
                json!({ "field": field, "reason": "not_writable" })
            )
        );
    }
    let reader = tenant_key(tid, &["identities:read"]);
    assert_eq!(get(&path, &reader).code(), "permission_denied");
    assert_eq!(
        patch(&path, &reader, json!({ "max_recipients": 5 })).code(),
        "permission_denied"
    );
    let r = post(
        "/v1/keys",
        &platform(),
        json!({ "name": "i", "level": "identity", "tenant_id": tid, "identity_id": "idn_01ARYZ6S410000000000000000", "permissions": ["policy:write"] }),
    );
    assert_eq!(r.details()["reason"], "permission_not_allowed_for_level");
    let (_, other) = partner("j24_other", &["tenants:manage", "policy:write"]);
    assert_eq!(get(&path, &other).code(), "tenant_not_found");
    assert_eq!(stored(tid)["version"], 0);
}

#[test]
fn j25_guard_fields_need_person() {
    let (_, pk) = partner("j25", &["tenants:manage", "policy:write"]);
    let t = tenant(&pk, "j25");
    let tid = t["id"].as_str().unwrap();
    let wk = tenant_key(tid, &["policy:write"]);
    let path = format!("/v1/tenants/{tid}/policy");
    for (field, body) in [
        (
            "quarantine.on_auth_fail",
            json!({ "quarantine": { "on_auth_fail": false } }),
        ),
        (
            "quarantine.unsolicited_otp",
            json!({ "quarantine": { "unsolicited_otp": false } }),
        ),
        (
            "quarantine.spam_threshold",
            json!({ "quarantine": { "spam_threshold": 0.9 } }),
        ),
    ] {
        let r = patch(&path, &wk, body);
        assert_eq!(
            (
                r.status,
                r.code(),
                r.details()["field"].as_str(),
                r.details()["reason"].as_str()
            ),
            (
                403,
                "permission_denied",
                Some(field),
                Some("person_required")
            )
        );
    }
    // Tightening is accepted.
    assert_eq!(
        patch(
            &path,
            &wk,
            json!({ "quarantine": { "spam_threshold": 0.5 }, "send_allowlist_only": true })
        )
        .status,
        200
    );
    let r = patch(&path, &wk, json!({ "send_allowlist_only": false }));
    assert_eq!(r.details()["reason"], "person_required");
    let view = get(&path, &wk);
    assert!(
        view.body["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["field"] == "send_allowlist_only" && f["person_required_to_loosen"] == true)
    );
    // Under the tenant's key_release only a platform key or the tenant's own partner key loosens.
    assert_eq!(
        patch(
            &path,
            &platform(),
            json!({ "quarantine": { "key_release": true } })
        )
        .status,
        200
    );
    assert_eq!(
        patch(&path, &wk, json!({ "send_allowlist_only": false })).details()["reason"],
        "person_required"
    );
    assert_eq!(
        patch(
            &path,
            &pk,
            json!({ "send_allowlist_only": false, "quarantine": { "spam_threshold": 0.9 } })
        )
        .status,
        200
    );
    assert_eq!(stored(tid)["policy"]["send_allowlist_only"], false);
}

#[test]
fn j26_partner_ceiling() {
    let (_, pk) = partner("j26", &["tenants:manage", "policy:write"]);
    let r = post(
        "/v1/tenants",
        &pk,
        json!({ "slug": crate::slug("j26"), "name": "j26", "mode": "live", "policy": { "tenant_daily_send_cap": 1000 } }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let tid = r.body["id"].as_str().unwrap().to_owned();
    let wk = tenant_key(&tid, &["policy:write"]);
    let path = format!("/v1/tenants/{tid}/policy");
    assert_eq!(
        patch(&path, &wk, json!({ "tenant_daily_send_cap": 800 })).status,
        200
    );
    assert_eq!(
        patch(&path, &wk, json!({ "tenant_daily_send_cap": 1000 })).status,
        200
    );
    let r = patch(&path, &wk, json!({ "tenant_daily_send_cap": 1001 }));
    assert_eq!(
        (
            r.code(),
            r.details()["ceiling_source"].as_str(),
            r.details()["ceiling"].as_i64()
        ),
        ("scope_denied", Some("partner"), Some(1000))
    );
    // The partner itself is bounded by the deployment default only; its null removes its ceiling.
    assert_eq!(
        patch(&path, &pk, json!({ "tenant_daily_send_cap": 2000 })).status,
        200
    );
    assert_eq!(
        patch(&path, &wk, json!({ "tenant_daily_send_cap": 2001 })).details()["ceiling"],
        2000
    );
    // A partner's refusal names the field only (Workspace policy § 5).
    let r = patch(&path, &pk, json!({ "tenant_daily_send_cap": 5001 }));
    assert_eq!(
        (r.code(), r.details().clone()),
        ("scope_denied", json!({ "field": "tenant_daily_send_cap" }))
    );
    assert_eq!(
        patch(&path, &pk, json!({ "tenant_daily_send_cap": null })).status,
        200
    );
    assert_eq!(
        patch(&path, &wk, json!({ "tenant_daily_send_cap": 5000 })).status,
        200
    );
    // A tenant of no partner is never bound by partner ceilings.
    let direct = tenant(&platform(), "j26_direct");
    let did = direct["id"].as_str().unwrap();
    d1(&format!(
        "UPDATE tenants SET partner_ceilings_json = '{{\"tenant_daily_send_cap\":10}}' WHERE id = '{did}'"
    ));
    assert_eq!(
        patch(
            &format!("/v1/tenants/{did}/policy"),
            &tenant_key(did, &["policy:write"]),
            json!({ "tenant_daily_send_cap": 4000 })
        )
        .status,
        200
    );
}

#[test]
fn j27_concurrent_writes() {
    let (_, pk) = partner("j27", &["tenants:manage", "policy:write"]);
    let t = tenant(&pk, "j27");
    let tid = t["id"].as_str().unwrap().to_owned();
    let wk = tenant_key(&tid, &["policy:write"]);
    let path = format!("/v1/tenants/{tid}/policy");
    let writes = [
        (pk.clone(), json!({ "max_recipients": 7 })),
        (wk.clone(), json!({ "retention": { "raw_days": 20 } })),
        (pk.clone(), json!({ "identity_daily_send_cap": 300 })),
        (wk.clone(), json!({ "triage": { "enabled": false } })),
    ];
    let statuses: Vec<u16> = std::thread::scope(|s| {
        let hs: Vec<_> = writes
            .iter()
            .map(|(k, b)| s.spawn(|| patch(&path, k, b.clone()).status))
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(
        statuses.iter().all(|s| matches!(s, 200 | 503)),
        "{statuses:?}"
    );
    let ok = statuses.iter().filter(|s| **s == 200).count();
    assert!(ok >= 1);
    let now = stored(&tid);
    assert_eq!(now["version"].as_u64(), Some(ok as u64));
    let checks = [
        ("max_recipients", json!(7)),
        ("retention.raw_days", json!(20)),
        ("identity_daily_send_cap", json!(300)),
        ("triage.enabled", json!(false)),
    ];
    for (s, (field, want)) in statuses.iter().zip(checks.iter()) {
        let mut v = &now["policy"];
        for part in field.split('.') {
            v = &v[part];
        }
        if *s == 200 {
            assert_eq!(v, want, "{field} was lost");
        }
    }
    let audits = d1(&format!(
        "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'tenant.policy_update' AND target_id = '{tid}'"
    ));
    let events = d1(&format!(
        "SELECT COUNT(*) AS n FROM event_index WHERE type = 'tenant.policy_updated' AND tenant_id = '{tid}'"
    ));
    assert_eq!(
        (audits[0]["n"].as_u64(), events[0]["n"].as_u64()),
        (Some(ok as u64), Some(ok as u64))
    );
}

#[test]
fn policy_updated_event_and_audit() {
    let p = platform();
    let (_, pk) = partner("pu", &["tenants:manage", "policy:write"]);
    let t = tenant(&pk, "pu");
    let tid = t["id"].as_str().unwrap().to_owned();
    let count = |table: &str, col: &str, val: &str, scope: &str| -> u64 {
        d1(&format!(
            "SELECT COUNT(*) AS n FROM {table} WHERE {col} = '{val}' AND {scope} = '{tid}'"
        ))[0]["n"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(
        count("audit_log", "action", "tenant.policy_update", "target_id"),
        0,
        "creation writes neither"
    );
    assert_eq!(
        count("event_index", "type", "tenant.policy_updated", "tenant_id"),
        0
    );
    let wk = tenant_key(&tid, &["policy:write"]);
    assert_eq!(
        patch(
            &format!("/v1/tenants/{tid}"),
            &p,
            json!({ "policy": { "max_recipients": 9 } })
        )
        .status,
        200
    );
    assert_eq!(
        patch(
            &format!("/v1/tenants/{tid}"),
            &pk,
            json!({ "policy": { "max_recipients": 8 } })
        )
        .status,
        200
    );
    for k in [&p, &pk, &wk] {
        assert_eq!(
            patch(
                &format!("/v1/tenants/{tid}/policy"),
                k,
                json!({ "send_allowlist_only": true })
            )
            .status,
            200
        );
    }
    assert_eq!(
        count("audit_log", "action", "tenant.policy_update", "target_id"),
        5
    );
    assert_eq!(
        count("event_index", "type", "tenant.policy_updated", "tenant_id"),
        5
    );
    let rows = d1(&format!(
        "SELECT details_json FROM audit_log WHERE action = 'tenant.policy_update' AND target_id = '{tid}'"
    ));
    for r in rows.as_array().unwrap() {
        let d: Value = serde_json::from_str(r["details_json"].as_str().unwrap()).unwrap();
        assert!(d["fields"].is_array(), "{d}");
    }
}
