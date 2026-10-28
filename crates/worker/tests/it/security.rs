//! The cross-tenant attack suite and the security checks (Security § 5, § 8.5, § 15).

use serde_json::{Value, json};

use crate::{Resp, call, d1, get, hook, mint, partner, platform, post, tenant};

const MISSING_TENANT: &str = "ten_01ARYZ6S410000000000000000";
const MISSING_KEY: &str = "key_01ARYZ6S410000000000000000";
const MISSING_PARTNER: &str = "ptn_01ARYZ6S410000000000000000";
const MISSING_IDENTITY: &str = "idn_01ARYZ6S410000000000000000";
const MISSING_DOMAIN: &str = "dom_01ARYZ6S410000000000000000";

/// The victim's IDs, or the missing ones of the control call.
struct Target {
    tenant: String,
    key: String,
    partner: String,
    identity: String,
    address: String,
    domain: String,
    webhook: String,
}

/// One request of the matrix: method, path and body for a target.
type Case = fn(&Target) -> (&'static str, String, Option<Value>);

/// Every operation of the route table but the public ones, with the request that names the target.
const CASES: &[(&str, Case)] = &[
    ("getMe", |_| ("GET", "/v1/me".into(), None)),
    ("createPlatformWebhook", |_| {
        (
            "POST",
            "/v1/webhooks".into(),
            Some(
                json!({ "url": "https://own.hooks.example.com/a", "events": ["identity.created"] }),
            ),
        )
    }),
    ("listWebhooks", |_| ("GET", "/v1/webhooks".into(), None)),
    ("createTenant", |t| {
        (
            "POST",
            "/v1/tenants".into(),
            Some(json!({ "slug": "x", "name": "x", "mode": "live", "partner_id": t.partner })),
        )
    }),
    ("listTenants", |t| {
        ("GET", format!("/v1/tenants?partner_id={}", t.partner), None)
    }),
    ("getTenant", |t| {
        ("GET", format!("/v1/tenants/{}", t.tenant), None)
    }),
    ("updateTenant", |t| {
        (
            "PATCH",
            format!("/v1/tenants/{}", t.tenant),
            Some(json!({ "name": "pwned" })),
        )
    }),
    ("getTenantPolicy", |t| {
        ("GET", format!("/v1/tenants/{}/policy", t.tenant), None)
    }),
    ("updateTenantPolicy", |t| {
        (
            "PATCH",
            format!("/v1/tenants/{}/policy", t.tenant),
            Some(json!({ "max_recipients": 1 })),
        )
    }),
    ("createPartner", |_| {
        ("POST", "/v1/partners".into(), Some(json!({ "name": "x" })))
    }),
    ("listPartners", |_| ("GET", "/v1/partners".into(), None)),
    ("getPartner", |t| {
        ("GET", format!("/v1/partners/{}", t.partner), None)
    }),
    ("updatePartner", |t| {
        (
            "PATCH",
            format!("/v1/partners/{}", t.partner),
            Some(json!({ "name": "pwned" })),
        )
    }),
    ("deletePartner", |t| {
        ("DELETE", format!("/v1/partners/{}", t.partner), None)
    }),
    ("createKey", |t| {
        (
            "POST",
            "/v1/keys".into(),
            Some(
                json!({ "name": "x", "level": "tenant", "tenant_id": t.tenant, "permissions": ["identities:read"] }),
            ),
        )
    }),
    ("listKeys", |t| {
        ("GET", format!("/v1/keys?tenant_id={}", t.tenant), None)
    }),
    ("getKey", |t| ("GET", format!("/v1/keys/{}", t.key), None)),
    ("revokeKey", |t| {
        ("DELETE", format!("/v1/keys/{}", t.key), None)
    }),
    ("rotateKey", |t| {
        (
            "POST",
            format!("/v1/keys/{}/rotate", t.key),
            Some(json!({})),
        )
    }),
    ("listAuditEvents", |t| {
        (
            "GET",
            format!("/v1/audit-events?tenant_id={}", t.tenant),
            None,
        )
    }),
    ("rotateSigningKey", |_| {
        ("POST", "/v1/platform/keys/thread/rotate".into(), None)
    }),
    ("createIdentity", |t| {
        (
            "POST",
            format!("/v1/tenants/{}/identities", t.tenant),
            Some(json!({ "username": "intruder", "display_name": "x" })),
        )
    }),
    ("listTenantIdentities", |t| {
        ("GET", format!("/v1/tenants/{}/identities", t.tenant), None)
    }),
    ("listIdentities", |t| {
        (
            "GET",
            format!("/v1/identities?tenant_id={}", t.tenant),
            None,
        )
    }),
    ("lookupIdentity", |t| {
        (
            "GET",
            format!("/v1/identities/lookup?address={}", t.address),
            None,
        )
    }),
    ("getIdentity", |t| {
        ("GET", format!("/v1/identities/{}", t.identity), None)
    }),
    ("updateIdentity", |t| {
        (
            "PATCH",
            format!("/v1/identities/{}", t.identity),
            Some(json!({ "status": "paused" })),
        )
    }),
    ("deleteIdentity", |t| {
        ("DELETE", format!("/v1/identities/{}", t.identity), None)
    }),
    ("listAddresses", |t| {
        (
            "GET",
            format!("/v1/identities/{}/addresses", t.identity),
            None,
        )
    }),
    ("listTenantDomains", |t| {
        ("GET", format!("/v1/tenants/{}/domains", t.tenant), None)
    }),
    ("getDomain", |t| {
        ("GET", format!("/v1/domains/{}", t.domain), None)
    }),
    // The mail routes of M7: the identity is checked first, so a message or thread ID never matters.
    ("listThreads", |t| {
        (
            "GET",
            format!("/v1/identities/{}/threads", t.identity),
            None,
        )
    }),
    ("getThread", |t| {
        (
            "GET",
            format!("/v1/identities/{}/threads/{THREAD}", t.identity),
            None,
        )
    }),
    ("updateThread", |t| {
        (
            "PATCH",
            format!("/v1/identities/{}/threads/{THREAD}", t.identity),
            Some(json!({ "read": true })),
        )
    }),
    ("listMessages", |t| {
        (
            "GET",
            format!("/v1/identities/{}/messages", t.identity),
            None,
        )
    }),
    ("getMessage", |t| {
        (
            "GET",
            format!("/v1/identities/{}/messages/{MESSAGE}", t.identity),
            None,
        )
    }),
    ("updateMessage", |t| {
        (
            "PATCH",
            format!("/v1/identities/{}/messages/{MESSAGE}", t.identity),
            Some(json!({ "read": true })),
        )
    }),
    ("getMessageRaw", |t| {
        (
            "GET",
            format!("/v1/identities/{}/messages/{MESSAGE}/raw", t.identity),
            None,
        )
    }),
    ("getAttachment", |t| {
        (
            "GET",
            format!(
                "/v1/identities/{}/messages/{MESSAGE}/attachments/{ATTACHMENT}",
                t.identity
            ),
            None,
        )
    }),
    ("getAttachmentText", |t| {
        (
            "GET",
            format!(
                "/v1/identities/{}/messages/{MESSAGE}/attachments/{ATTACHMENT}/text",
                t.identity
            ),
            None,
        )
    }),
    ("releaseMessage", |t| {
        (
            "POST",
            format!("/v1/identities/{}/messages/{MESSAGE}/release", t.identity),
            Some(json!({ "reason": "r" })),
        )
    }),
    ("listQuarantine", |t| {
        (
            "GET",
            format!("/v1/identities/{}/quarantine", t.identity),
            None,
        )
    }),
    ("waitForMessage", |t| {
        (
            "GET",
            format!("/v1/identities/{}/wait?timeout=1", t.identity),
            None,
        )
    }),
    // Webhook endpoints (M8): a tenant's endpoint by ID, and its tenant's list and create.
    ("createTenantWebhook", |t| {
        (
            "POST",
            format!("/v1/tenants/{}/webhooks", t.tenant),
            Some(json!({ "url": "https://x.hooks.example.com/a", "events": ["identity.created"] })),
        )
    }),
    ("listTenantWebhooks", |t| {
        ("GET", format!("/v1/tenants/{}/webhooks", t.tenant), None)
    }),
    ("getWebhook", |t| {
        ("GET", format!("/v1/webhooks/{}", t.webhook), None)
    }),
    ("updateWebhook", |t| {
        (
            "PATCH",
            format!("/v1/webhooks/{}", t.webhook),
            Some(json!({ "description": "x" })),
        )
    }),
    ("deleteWebhook", |t| {
        ("DELETE", format!("/v1/webhooks/{}", t.webhook), None)
    }),
    ("rotateWebhookSecret", |t| {
        (
            "POST",
            format!("/v1/webhooks/{}/rotate-secret", t.webhook),
            Some(json!({ "overlap_hours": 0 })),
        )
    }),
    ("testWebhook", |t| {
        ("POST", format!("/v1/webhooks/{}/test", t.webhook), None)
    }),
    ("listWebhookDeliveries", |t| {
        (
            "GET",
            format!("/v1/webhooks/{}/deliveries", t.webhook),
            None,
        )
    }),
    ("replayWebhookEvents", |t| {
        (
            "POST",
            format!("/v1/webhooks/{}/replay", t.webhook),
            Some(json!({ "since": "2026-01-01T00:00:00Z" })),
        )
    }),
];

const MISSING_WEBHOOK: &str = "whk_01ARYZ6S41TSV4RRFFQ69G5FAV";
const THREAD: &str = "thr_01ARYZ6S41TSV4RRFFQ69G5FAV";
const MESSAGE: &str = "msg_01ARYZ6S41TSV4RRFFQ69G5FAV";
const ATTACHMENT: &str = "att_01ARYZ6S41TSV4RRFFQ69G5FAV";

/// Operations whose request names no other scope: the foreign key gets what it gets on its own scope,
/// checked by the listing assertions below instead.
const OWN_SCOPE: &[&str] = &[
    "getMe",
    "createPartner",
    "listPartners",
    "rotateSigningKey",
    "createPlatformWebhook",
    "listWebhooks",
];

fn send(key: &str, (method, path, body): (&'static str, String, Option<Value>)) -> Resp {
    call(method, &path, Some(key), body, &[])
}

fn victim_state(t: &Target) -> Value {
    let ten = d1(&format!(
        "SELECT name, policy_version, updated_at FROM tenants WHERE id = '{}'",
        t.tenant
    ));
    let key = d1(&format!(
        "SELECT revoked_at, lookup FROM api_keys WHERE id = '{}'",
        t.key
    ));
    let ptn = d1(&format!(
        "SELECT name, status FROM partners WHERE id = '{}'",
        t.partner
    ));
    let keys = d1(&format!(
        "SELECT COUNT(*) AS n FROM api_keys WHERE tenant_id = '{}'",
        t.tenant
    ));
    let idn = d1(&format!(
        "SELECT status, pause_reason, display_name, updated_at FROM identities WHERE tenant_id = '{}' ORDER BY id",
        t.tenant
    ));
    let adr = d1(&format!(
        "SELECT COUNT(*) AS n FROM addresses WHERE tenant_id = '{}'",
        t.tenant
    ));
    json!([ten, key, ptn, keys, idn, adr])
}

#[test]
fn route_table_complete() {
    let r = hook("GET", "/__test/routes", None);
    assert_eq!(r.status, 200, "{}", r.body);
    for route in r.body["routes"].as_array().unwrap() {
        let op = route["operation_id"].as_str().unwrap();
        if route["scope"] == "public" {
            continue;
        }
        assert!(
            CASES.iter().any(|(o, _)| *o == op),
            "{op} has no cross-tenant case"
        );
        assert!(
            !route["levels"].as_array().unwrap().is_empty(),
            "{op} has no key levels"
        );
        let perms = route["permissions"].as_array().unwrap().len()
            + route["foreign_permissions"].as_array().unwrap().len();
        assert!(
            perms > 0 || route["scope"] == "any_key",
            "{op} has no permission"
        );
    }
    // Without the token the hooks do not exist.
    assert_eq!(
        call("GET", "/__test/routes", None, None, &[]).code(),
        "route_not_found"
    );
}

/// Every route × each foreign key class: the response to a request naming the victim's tenant, key or
/// partner is the response to the same request naming a missing one, and nothing changes.
#[test]
fn cross_tenant_matrix() {
    let p = platform();
    let all = [
        "tenants:manage",
        "keys:manage",
        "policy:write",
        "audit:read",
        "identities:read",
        "identities:write",
        "erasure:manage",
        "domains:read",
        "messages:read",
        "messages:write",
        "quarantine:review",
        "attachments:read",
        "search:read",
        "webhooks:manage",
    ];
    let (victim_partner, vk) = partner("victim", &all);
    let vt = tenant(&vk, "victim");
    let vtid = vt["id"].as_str().unwrap().to_owned();
    let (vkey, _) = mint(
        &vk,
        json!({ "name": "v", "level": "tenant", "tenant_id": vtid, "permissions": ["identities:read"] }),
    );
    let vidn = crate::identity(&vk, &vtid, "victim");
    // A tenant domain of the victim, written as M13 will write one.
    let vdom = format!("dom_{}", &vtid[4..]);
    d1(&format!(
        "INSERT INTO domains (id, tenant_id, name, kind, method, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_changed_at, monitor_do_id, created_at, updated_at) VALUES ('{vdom}', '{vtid}', '{}.example', 'zone', 'cloudflare_zone', 1, 'catch_all', 'routing', 'cloudflare', 'subaddress', 1, 1, 'healthy', 1, 'x', 1, 1)",
        vtid.to_ascii_lowercase()
    ));
    let vwhk = post(
        &format!("/v1/tenants/{vtid}/webhooks"),
        &vk,
        json!({ "url": "https://victim.hooks.example.com/a", "events": ["identity.created"] }),
    );
    assert_eq!(vwhk.status, 201, "{}", vwhk.body);
    let victim = Target {
        tenant: vtid.clone(),
        key: vkey,
        partner: victim_partner,
        identity: vidn["id"].as_str().unwrap().into(),
        address: vidn["primary_address"].as_str().unwrap().into(),
        domain: vdom,
        webhook: vwhk.body["id"].as_str().unwrap().into(),
    };
    let missing = Target {
        tenant: MISSING_TENANT.into(),
        key: MISSING_KEY.into(),
        partner: MISSING_PARTNER.into(),
        identity: MISSING_IDENTITY.into(),
        address: "nobody.nowhere@agents.example".into(),
        domain: MISSING_DOMAIN.into(),
        webhook: MISSING_WEBHOOK.into(),
    };
    // Attackers: a tenant key of another tenant, and a partner key of another partner.
    let at = tenant(&p, "attacker");
    let (_, tenant_attacker) = mint(
        &p,
        json!({ "name": "a", "level": "tenant", "tenant_id": at["id"], "permissions": ["keys:manage", "policy:write", "audit:read", "identities:read", "identities:write", "erasure:manage", "domains:read", "messages:read", "messages:write", "quarantine:review", "attachments:read", "search:read", "webhooks:manage"] }),
    );
    let (_, partner_attacker) = partner("attacker", &all);
    tenant(&partner_attacker, "attacker_own");
    let before = victim_state(&victim);
    for (class, key) in [
        ("foreign_tenant", &tenant_attacker),
        ("foreign_partner", &partner_attacker),
    ] {
        for (op, case) in CASES {
            if OWN_SCOPE.contains(op) {
                continue;
            }
            let foreign = send(key, case(&victim));
            let control = send(key, case(&missing));
            assert!(
                matches!(foreign.status, 400 | 403 | 404),
                "{class} {op}: {} {}",
                foreign.status,
                foreign.body
            );
            if matches!(*op, "listTenants" | "createTenant") {
                // No victim ID in the response; the partner filter is ignored for a partner key.
                assert!(
                    !foreign.body.to_string().contains(&victim.tenant),
                    "{class} {op} leaked the tenant"
                );
                continue;
            }
            assert_eq!(
                foreign.without_request_id(),
                control.without_request_id(),
                "{class} {op}"
            );
        }
    }
    assert_eq!(victim_state(&victim), before, "no side effect");
    // The own-scope listings never show the victim.
    for key in [&tenant_attacker, &partner_attacker] {
        for path in [
            "/v1/keys",
            "/v1/audit-events",
            "/v1/tenants",
            "/v1/identities",
        ] {
            let body = get(path, key).body.to_string();
            assert!(
                !body.contains(&victim.tenant)
                    && !body.contains(&victim.key)
                    && !body.contains(&victim.identity),
                "{path} leaked the victim"
            );
        }
    }
}

/// A `tenant_id` in a body or query never widens a key's access.
#[test]
fn body_scope_ignored() {
    let p = platform();
    let a = tenant(&p, "bsi_a");
    let b = tenant(&p, "bsi_b");
    let (_, ak) = mint(
        &p,
        json!({ "name": "a", "level": "tenant", "tenant_id": a["id"], "permissions": ["keys:manage", "audit:read", "policy:write"] }),
    );
    let r = post(
        "/v1/keys",
        &ak,
        json!({ "name": "x", "level": "tenant", "tenant_id": b["id"], "permissions": ["audit:read"] }),
    );
    assert_eq!(r.code(), "key_scope_exceeded");
    let bid = b["id"].as_str().unwrap();
    assert_eq!(
        get(&format!("/v1/audit-events?tenant_id={bid}"), &ak).code(),
        "tenant_not_found"
    );
    assert_eq!(
        get(&format!("/v1/keys?tenant_id={bid}"), &ak).code(),
        "tenant_not_found"
    );
    let before = d1(&format!(
        "SELECT policy_json, policy_version FROM tenants WHERE id = '{bid}'"
    ));
    let r = crate::patch(
        &format!("/v1/tenants/{}/policy", a["id"].as_str().unwrap()),
        &ak,
        json!({ "tenant_id": bid, "max_recipients": 3 }),
    );
    assert_eq!(
        (r.status, r.details()["field"].as_str()),
        (403, Some("tenant_id")),
        "{}",
        r.body
    );
    assert_eq!(
        d1(&format!(
            "SELECT policy_json, policy_version FROM tenants WHERE id = '{bid}'"
        )),
        before
    );
}

/// A forged envelope reaching an object is refused by its owner check.
#[test]
fn rpc_owner_mismatch() {
    let p = platform();
    let t = tenant(&p, "rpc_owner");
    let tid = t["id"].as_str().unwrap();
    let qid = d1(&format!(
        "SELECT quota_do_id FROM tenants WHERE id = '{tid}'"
    ))[0]["quota_do_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let env = |tenant: &str| json!({ "v": 1, "tenant_id": tenant, "identity_id": null, "request_id": "req_01ARYZ6S410000000000000000", "actor_key_id": null, "deadline_ms": 4_102_444_800_000_i64, "req": { "op": "get_usage" } });
    let ok = hook(
        "POST",
        "/__test/rpc",
        Some(json!({ "class": "quota", "object_id": qid, "envelope": env(tid) })),
    );
    assert!(ok.body.get("ok").is_some(), "{}", ok.body);
    let forged = hook(
        "POST",
        "/__test/rpc",
        Some(json!({ "class": "quota", "object_id": qid, "envelope": env(MISSING_TENANT) })),
    );
    assert_eq!(
        forged.body["err"]["error"]["code"], "internal_error",
        "{}",
        forged.body
    );
}

#[test]
fn response_headers() {
    let p = platform();
    for (r, authed) in [
        (get("/v1/me", &p), true),
        (
            call(
                "GET",
                "/health",
                None,
                None,
                &[("origin", "https://evil.example")],
            ),
            false,
        ),
        (get("/v1/nope", &p), false),
    ] {
        assert!(
            r.header("request-id")
                .is_some_and(|v| v.starts_with("req_"))
        );
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(r.header("referrer-policy"), Some("no-referrer"));
        assert!(r.header("strict-transport-security").is_some());
        assert!(r.header("set-cookie").is_none());
        assert!(
            r.headers
                .iter()
                .all(|(n, _)| !n.starts_with("access-control-"))
        );
        if authed {
            assert_eq!(r.header("cache-control"), Some("no-store"));
            assert!(r.header("ratelimit-limit").is_some());
        }
    }
    let pre = call(
        "OPTIONS",
        "/v1/me",
        None,
        None,
        &[
            ("origin", "https://evil.example"),
            ("access-control-request-method", "GET"),
        ],
    );
    assert!(
        pre.headers
            .iter()
            .all(|(n, _)| !n.starts_with("access-control-"))
    );
}

#[test]
fn well_known_security_txt() {
    let r = call("GET", "/.well-known/security.txt", None, None, &[]);
    assert_eq!(r.status, 200);
    assert!(r.header("content-type").unwrap().starts_with("text/plain"));
    let text = r.body.as_str().unwrap();
    assert!(
        text.contains("Contact: mailto:security@agents.example\n"),
        "{text}"
    );
    assert!(
        text.contains("\nExpires: ")
            && text.contains("Canonical: https://localhost:8799/.well-known/security.txt")
    );
}
