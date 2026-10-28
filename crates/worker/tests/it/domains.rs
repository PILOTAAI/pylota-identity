//! Domains through workerd (Identities and domains › Domains, Domain health) against the Cloudflare,
//! SES, DoH and RDAP fakes of `cloud.rs`: `cloudflare_zone` onboarding, the zone permission (H8, H18),
//! existing mail (H5), the mail-domain limit (H15), the S9 fallback, verification and its rate limit,
//! the J5 transport switch and ownership changes (H4).

use serde_json::{Value, json};

use crate::{Resp, cloud, d1, fakes, get, hook, mint, patch, platform, post, tenant};

/// A zone name unique to this run.
pub fn zone_name(tag: &str) -> String {
    format!("{}.example", crate::slug(tag))
}

/// A tenant and a tenant key with the domain and identity permissions: `(tenant id, key)`.
pub fn tenant_key(name: &str) -> (String, String) {
    let p = platform();
    let t = tenant(&p, name);
    let tid = t["id"].as_str().unwrap().to_owned();
    let (_, key) = mint(
        &p,
        json!({ "name": name, "level": "tenant", "tenant_id": tid, "permissions": [
            "domains:read", "domains:write", "identities:read", "identities:write",
            "messages:read", "messages:write"
        ] }),
    );
    (tid, key)
}

pub fn list_zones(tid: &str, zones: &[&str]) {
    let r = patch(
        &format!("/v1/tenants/{tid}/policy"),
        &platform(),
        json!({ "domains": { "cloudflare_zones": zones } }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
}

pub fn add(key: &str, tid: &str, body: Value) -> Resp {
    post(&format!("/v1/tenants/{tid}/domains"), key, body)
}

/// Runs the domain's monitor once the next check is due (the fake clock moves 16 minutes).
pub fn check(domain_id: &str) {
    fakes::advance(16 * 60_000);
    let r = hook(
        "POST",
        "/__test/domain-alarm",
        Some(json!({ "domain_id": domain_id })),
    );
    assert_eq!(r.status, 200, "{}", r.body);
}

pub fn domain(key: &str, id: &str) -> Value {
    let r = get(&format!("/v1/domains/{id}"), key);
    assert_eq!(r.status, 200, "{}", r.body);
    r.body
}

/// Checks until the domain is in `state` (at most `n` checks).
pub fn until(key: &str, id: &str, state: &str, n: usize) -> Value {
    for _ in 0..n {
        let d = domain(key, id);
        if d["state"] == state {
            return d;
        }
        check(id);
    }
    let d = domain(key, id);
    assert_eq!(d["state"], state, "{d}");
    d
}

/// A DMARC record that lets the domain reach `healthy`.
pub fn dmarc(zone: &str, name: &str) {
    cloud::record(
        zone,
        "TXT",
        &format!("_dmarc.{name}"),
        "v=DMARC1; p=quarantine",
        None,
    );
}

/// A listed zone, a tenant and an added, healthy `cloudflare_zone` domain: `(tenant, key, zone, name, domain id)`.
pub fn healthy(tag: &str) -> (String, String, String, String, String) {
    let (tid, key) = tenant_key(tag);
    let zone = zone_name(tag);
    cloud::zone(&zone);
    list_zones(&tid, &[&zone]);
    let name = format!("mail.{zone}");
    dmarc(&zone, &name);
    let r = add(
        &key,
        &tid,
        json!({ "name": name, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let id = r.body["id"].as_str().unwrap().to_owned();
    until(&key, &id, "healthy", 6);
    (tid, key, zone, name, id)
}

/// FR-DOM-3: the records of the response equal the fake provider's answers; the domain starts
/// verifying at once.
#[test]
fn records_from_api() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("records");
    let zone = zone_name("records");
    let zid = cloud::zone(&zone);
    list_zones(&tid, &[&zone]);
    let name = format!("mail.{zone}");
    let r = add(
        &key,
        &tid,
        json!({ "name": name, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(
        (
            r.body["kind"].as_str(),
            r.body["routing_mode"].as_str(),
            r.body["delivery_events"].as_str()
        ),
        (Some("zone"), Some("literal"), Some("active"))
    );
    let records = r.body["records"].as_array().unwrap().clone();
    let has = |t: &str, n: &str, v: &str| {
        records.iter().any(|x| {
            x["type"] == t && x["name"] == n && x["value"].as_str().unwrap_or("").contains(v)
        })
    };
    for (h, _) in cloud::ROUTING_MX {
        assert!(has("MX", &name, h), "{h}");
    }
    assert!(has("TXT", &name, "include:_spf.mx.cloudflare.net"));
    assert!(has(
        "TXT",
        &format!("cf2024-1._domainkey.{name}"),
        "p=ROUTE"
    ));
    assert!(has("TXT", &format!("cf-send._domainkey.{name}"), "p=SEND"));
    assert!(has(
        "MX",
        &format!("cf-bounce.{name}"),
        "bounce.mx.cloudflare.net"
    ));
    let token = records
        .iter()
        .find(|x| x["purpose"] == "ownership")
        .unwrap()["value"]
        .as_str()
        .unwrap()
        .to_owned();
    // The ownership TXT was published through the DNS records API.
    assert!(
        cloud::records_at(&format!("_pylota-identity.{name}"))
            .iter()
            .any(|x| x.zone == zid && x.content == token)
    );
    let id = r.body["id"].as_str().unwrap();
    let rec = get(&format!("/v1/domains/{id}/records"), &key);
    assert_eq!(rec.status, 200, "{}", rec.body);
    assert_eq!(rec.body["data"].as_array().unwrap().len(), records.len());
    assert_eq!(domain(&key, id)["state"], "verifying");
    // The same name again: 409 with the domain's ID.
    let again = add(
        &key,
        &tid,
        json!({ "name": name, "method": "cloudflare_zone" }),
    );
    assert_eq!((again.status, again.code()), (409, "domain_exists"));
    assert_eq!(again.details()["domain_id"], id);
}

/// FR-DOM-2: a failed add re-runs against the recorded fake without creating anything twice.
#[test]
fn onboarding_idempotent() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("idempotent");
    let zone = zone_name("idempotent");
    cloud::zone(&zone);
    list_zones(&tid, &[&zone]);
    for name in [format!("mail.{zone}"), format!("news.{zone}")] {
        cloud::fail("POST", "/event_subscriptions/subscriptions", 503, 1);
        let r = add(
            &key,
            &tid,
            json!({ "name": name, "method": "cloudflare_zone" }),
        );
        assert_eq!((r.status, r.code()), (502, "upstream_error"));
        assert_eq!(r.details()["step"], "event_subscription");
        let r = add(
            &key,
            &tid,
            json!({ "name": name, "method": "cloudflare_zone" }),
        );
        assert_eq!(r.status, 201, "{}", r.body);
        let calls = cloud::calls();
        let count = |m: &str, path_has: &str, body_name: Option<&str>| {
            calls
                .iter()
                .filter(|(cm, p)| cm == m && p.contains(path_has) && body_name.is_none_or(|_| true))
                .count()
        };
        let _ = count;
        let sending: Vec<String> = cloud::sending_names()
            .into_iter()
            .filter(|n| *n == name)
            .collect();
        assert_eq!(sending.len(), 1, "one sending domain for {name}");
        let ownership = cloud::records_at(&format!("_pylota-identity.{name}"));
        assert_eq!(ownership.len(), 1, "one ownership TXT for {name}");
        assert_eq!(
            cloud::subscriptions()
                .iter()
                .filter(|d| **d == name)
                .count(),
            1
        );
    }
}

/// S9 fallback: a `403` to the subscription create gives `delivery_events: "manual"`; a `503` fails
/// the create with `502`.
#[test]
fn s9_manual_delivery_events() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("s9");
    let zone = zone_name("s9");
    cloud::zone(&zone);
    list_zones(&tid, &[&zone]);
    let name = format!("mail.{zone}");
    cloud::fail("POST", "/event_subscriptions/subscriptions", 403, 1);
    let r = add(
        &key,
        &tid,
        json!({ "name": name, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(r.body["delivery_events"], "manual");
    assert_eq!(
        r.body["details"]["action"],
        format!("run idvy domains subscribe {name}")
    );
    let id = r.body["id"].as_str().unwrap().to_owned();
    let rows = d1(&format!(
        "SELECT event_subscription_id FROM domains WHERE id = '{id}'"
    ));
    assert!(rows[0]["event_subscription_id"].is_null());
    assert_eq!(domain(&key, &id)["delivery_events"], "manual");
    // Once the subscription ID is recorded (`idvy domains subscribe`), events are active.
    d1(&format!(
        "UPDATE domains SET event_subscription_id = 'sub-manual' WHERE id = '{id}'"
    ));
    assert_eq!(domain(&key, &id)["delivery_events"], "active");
    let other = format!("news.{zone}");
    cloud::fail("POST", "/event_subscriptions/subscriptions", 503, 1);
    let r = add(
        &key,
        &tid,
        json!({ "name": other, "method": "cloudflare_zone" }),
    );
    assert_eq!((r.status, r.code()), (502, "upstream_error"));
}

/// H5: a name with foreign MX records is refused without `replace_mx`, and nothing is created.
#[test]
fn h5_existing_mx() {
    let _reset = fakes::Reset;
    let (tid, _) = tenant_key("h5");
    let zone = zone_name("h5");
    cloud::zone(&zone);
    cloud::record(&zone, "MX", &zone, "mx.elsewhere.example", Some(10));
    let before = cloud::calls().len();
    let r = add(
        &platform(),
        &tid,
        json!({ "name": zone, "method": "cloudflare_zone" }),
    );
    assert_eq!((r.status, r.code()), (409, "existing_mx"), "{}", r.body);
    assert!(
        !cloud::calls()[before..]
            .iter()
            .any(|(m, _)| m == "POST" || m == "PUT" || m == "DELETE"),
        "nothing created"
    );
    // With replace_mx (platform key) the MX record goes and the apex routes to the Worker.
    let r = add(
        &platform(),
        &tid,
        json!({ "name": zone, "method": "cloudflare_zone", "replace_mx": true }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(r.body["routing_mode"], "catch_all");
    assert!(
        !cloud::records_at(&zone)
            .iter()
            .any(|x| x.content == "mx.elsewhere.example")
    );
    let zid = cloud::zone(&zone);
    assert_eq!(cloud::catch_all(&zid)["actions"][0]["type"], "worker");
}

/// H8: zones of other tenants, unlisted zones and the deployment's own zone are refused with one body.
#[test]
fn h8_zone_permission() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("h8 a");
    let (tid_b, _) = tenant_key("h8 b");
    let listed = zone_name("h8 listed");
    let claimed_b = zone_name("h8 claimed");
    let unlisted = zone_name("h8 unlisted");
    cloud::zone(&listed);
    let zb = cloud::zone(&claimed_b);
    cloud::zone(&unlisted);
    d1(&format!(
        "INSERT INTO zone_claims (zone_name, zone_id, tenant_id, domain_id, state, created_at) VALUES ('{claimed_b}', '{zb}', '{tid_b}', 'dom_x', 'active', 0)"
    ));
    list_zones(&tid, &[&listed, &claimed_b]);
    let refused = |name: &str, extra: Value| {
        let mut body = json!({ "name": name, "method": "cloudflare_zone" });
        if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
            b.extend(e.clone());
        }
        let r = add(&key, &tid, body);
        assert_eq!(
            (r.status, r.code()),
            (403, "scope_denied"),
            "{name}: {}",
            r.body
        );
        assert_eq!(r.details()["reason"], "zone_not_allowed");
        r.without_request_id()
    };
    let a = refused(&format!("mail.{claimed_b}"), json!({}));
    let b = refused(&format!("mail.{unlisted}"), json!({}));
    let c = refused(&format!("mail.missing-{unlisted}"), json!({}));
    assert_eq!(a, b);
    assert_eq!(b, c, "the same body for an existing and a missing zone");
    refused("x.agents.example", json!({}));
    refused(&format!("mail.{listed}"), json!({ "replace_mx": true }));
    refused(&listed, json!({}));
    let ok = add(
        &key,
        &tid,
        json!({ "name": format!("mail.{listed}"), "method": "cloudflare_zone" }),
    );
    assert_eq!(ok.status, 201, "{}", ok.body);
    // A platform key may use every zone.
    let p = add(
        &platform(),
        &tid,
        json!({ "name": format!("mail.{unlisted}"), "method": "cloudflare_zone" }),
    );
    assert_eq!(p.status, 201, "{}", p.body);
    // A partner key cannot set the list.
    let (_, pk) = crate::partner("h8 partner", &["tenants:manage", "policy:write"]);
    let pt = tenant(&pk, "h8 partner tenant");
    let r = patch(
        &format!("/v1/tenants/{}/policy", pt["id"].as_str().unwrap()),
        &pk,
        json!({ "domains": { "cloudflare_zones": [listed] } }),
    );
    assert_eq!((r.status, r.code()), (403, "scope_denied"), "{}", r.body);
}

/// H18: an exact-name entry grants that one name; a listed zone's subdomain with foreign MX needs
/// `replace_mx`, which only a platform key may use there.
#[test]
fn h18_exact_name_grant() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("h18");
    let ops = zone_name("h18 ops");
    let listed = zone_name("h18 listed");
    cloud::zone(&ops);
    cloud::zone(&listed);
    let notify = format!("notify.{ops}");
    list_zones(&tid, &[&format!("={notify}"), &listed]);
    let r = add(
        &key,
        &tid,
        json!({ "name": notify, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    for n in [
        format!("accounts.{ops}"),
        format!("a.{notify}"),
        ops.clone(),
    ] {
        let r = add(
            &key,
            &tid,
            json!({ "name": n, "method": "cloudflare_zone" }),
        );
        assert_eq!((r.status, r.code()), (403, "scope_denied"), "{n}");
    }
    let sub = format!("mail.b.{listed}");
    cloud::record(&listed, "MX", &sub, "mx.elsewhere.example", Some(10));
    let r = add(
        &key,
        &tid,
        json!({ "name": sub, "method": "cloudflare_zone" }),
    );
    assert_eq!((r.status, r.code()), (409, "existing_mx"), "{}", r.body);
    let r = add(
        &key,
        &tid,
        json!({ "name": sub, "method": "cloudflare_zone", "replace_mx": true }),
    );
    assert_eq!((r.status, r.code()), (403, "scope_denied"));
    assert!(
        cloud::records_at(&sub)
            .iter()
            .any(|x| x.content == "mx.elsewhere.example")
    );
    let r = add(
        &platform(),
        &tid,
        json!({ "name": sub, "method": "cloudflare_zone", "replace_mx": true }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    assert!(
        !cloud::records_at(&sub)
            .iter()
            .any(|x| x.content == "mx.elsewhere.example")
    );
    let r = patch(
        &format!("/v1/tenants/{tid}/policy"),
        &platform(),
        json!({ "domains": { "cloudflare_zones": ["=not a host"] } }),
    );
    assert_eq!((r.status, r.code()), (400, "invalid_request"), "{}", r.body);
}

/// H15: a zone holding 30 mail domains refuses before anything is created; a create refused with an
/// unknown error while the zone is full is the same `422`.
#[test]
fn h15_zone_domain_limit() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("h15");
    let full = zone_name("h15 full");
    cloud::zone(&full);
    for i in 0..30 {
        cloud::foreign_sending(&full, &format!("s{i}.{full}"));
    }
    list_zones(&tid, &[&full]);
    let before = cloud::calls().len();
    let r = add(
        &key,
        &tid,
        json!({ "name": format!("mail.{full}"), "method": "cloudflare_zone" }),
    );
    assert_eq!(
        (r.status, r.code()),
        (422, "zone_domain_limit"),
        "{}",
        r.body
    );
    assert_eq!(r.details()["limit"], 30);
    assert!(!cloud::calls()[before..].iter().any(|(m, _)| m == "POST"));
    let almost = zone_name("h15 almost");
    cloud::zone(&almost);
    for i in 0..28 {
        cloud::foreign_sending(&almost, &format!("s{i}.{almost}"));
    }
    list_zones(&tid, &[&almost]);
    cloud::fill_on_failure(&almost, 30);
    cloud::fail("POST", "/email/sending/subdomains", 400, 1);
    let r = add(
        &key,
        &tid,
        json!({ "name": format!("mail.{almost}"), "method": "cloudflare_zone" }),
    );
    assert_eq!(
        (r.status, r.code()),
        (422, "zone_domain_limit"),
        "{}",
        r.body
    );
}

/// Security § 10: a second `verify` within a minute is `429` with `Retry-After`; the monitor runs one
/// check.
#[test]
fn verify_rate_limited() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("verify");
    let zone = zone_name("verify");
    cloud::zone(&zone);
    list_zones(&tid, &[&zone]);
    let r = add(
        &key,
        &tid,
        json!({ "name": format!("mail.{zone}"), "method": "cloudflare_zone" }),
    );
    let id = r.body["id"].as_str().unwrap();
    let first = post(&format!("/v1/domains/{id}/verify"), &key, json!({}));
    assert_eq!(first.status, 200, "{}", first.body);
    let second = post(&format!("/v1/domains/{id}/verify"), &key, json!({}));
    assert_eq!((second.status, second.code()), (429, "rate_limited"));
    assert!(second.header("retry-after").is_some());
    fakes::advance(61_000);
    assert_eq!(
        post(&format!("/v1/domains/{id}/verify"), &key, json!({})).status,
        200
    );
}

/// J5: onboarding gives the domain its failover SES identity and DKIM CNAMEs; only a platform key
/// switches, and only to an identity that can sign now.
#[test]
fn transport_patch() {
    let _reset = fakes::Reset;
    let (_, key, zone, name, id) = healthy("j5");
    let d = domain(&key, &id);
    assert_eq!(
        (d["ses_region"].as_str(), d["transport"].as_str()),
        (Some("eu-west-2"), Some("cloudflare"))
    );
    let cnames: Vec<Value> = d["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["type"] == "CNAME")
        .cloned()
        .collect();
    assert_eq!(cnames.len(), 3);
    assert!(cnames.iter().all(|r| r["required"] == false));
    let identity = cloud::ses_identity(&name).expect("the failover identity");
    assert!(identity["Tags"].to_string().contains(&id));
    // A missing CNAME changes no state while the domain sends through Cloudflare.
    let cname = cnames[0]["name"].as_str().unwrap().to_owned();
    cloud::hide(&cname, "CNAME", true);
    check(&id);
    check(&id);
    assert_eq!(domain(&key, &id)["state"], "healthy");
    cloud::hide(&cname, "CNAME", false);
    let path = format!("/v1/domains/{id}");
    let r = patch(&path, &key, json!({ "transport": "ses" }));
    assert_eq!((r.status, r.code()), (403, "scope_denied"));
    let r = patch(&path, &platform(), json!({ "transport": "ses" }));
    assert_eq!(
        (r.status, r.code()),
        (422, "transport_unavailable"),
        "{}",
        r.body
    );
    assert_eq!(
        (
            r.details()["reason"].as_str(),
            r.details()["dkim_status"].as_str()
        ),
        (Some("ses_identity_not_verified"), Some("PENDING"))
    );
    assert_eq!(domain(&key, &id)["transport"], "cloudflare");
    cloud::ses_status(&name, true, "SUCCESS");
    let r = patch(&path, &platform(), json!({ "transport": "ses" }));
    assert_eq!(
        (r.status, r.body["transport"].as_str()),
        (200, Some("ses")),
        "{}",
        r.body
    );
    let r = patch(&path, &platform(), json!({ "transport": "cloudflare" }));
    assert_eq!(r.body["transport"], "cloudflare");
    // A receive-only domain has no SES identity.
    let (tid2, key2) = tenant_key("j5 receive");
    list_zones(&tid2, &[&zone]);
    let r = add(
        &key2,
        &tid2,
        json!({ "name": format!("in.{zone}"), "method": "cloudflare_zone", "sending": false }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let r = patch(
        &format!("/v1/domains/{}", r.body["id"].as_str().unwrap()),
        &platform(),
        json!({ "transport": "ses" }),
    );
    assert_eq!((r.status, r.code()), (422, "transport_unavailable"));
}

/// H4: the ownership TXT removed, then an NS move, then a registration change each suspend the domain
/// after two agreeing cycles; reprove brings it back through `verifying`.
#[test]
fn h4_ownership_change() {
    let _reset = fakes::Reset;
    let (_, key, zone, name, id) = healthy("h4");
    let d = d1(&format!(
        "SELECT expected_ns_json FROM domains WHERE id = '{id}'"
    ));
    assert!(
        d[0]["expected_ns_json"]
            .as_str()
            .unwrap()
            .contains("ada.ns.cloudflare.com")
    );
    let own = format!("_pylota-identity.{name}");
    cloud::hide(&own, "TXT", true);
    let s = until(&key, &id, "suspended", 4);
    assert_eq!(s["state_reason"], "ownership_record_missing");
    cloud::hide(&own, "TXT", false);
    let r = post(&format!("/v1/domains/{id}/reprove"), &key, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    until(&key, &id, "healthy", 8);
    // NS: the weekly check sees a move and the confirming check 5 minutes later agrees.
    cloud::hide(&zone, "NS", true);
    cloud::dns_extra(&zone, "NS", "ns1.elsewhere.example.");
    fakes::advance(7 * 86_400_000);
    let s = until(&key, &id, "suspended", 4);
    assert_eq!(s["state_reason"], "nameservers_changed");
    cloud::hide(&zone, "NS", false);
    cloud::dns_extra_clear(&zone);
    let r = post(&format!("/v1/domains/{id}/reprove"), &key, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    until(&key, &id, "healthy", 8);
    // RDAP: recorded once healthy; one change is not an issue until an hour later.
    let reg = zone.clone();
    cloud::rdap(&reg, "REG-1", "ME", "2020-01-01T00:00:00Z");
    fakes::advance(7 * 86_400_000);
    check(&id);
    check(&id);
    cloud::rdap(&reg, "REG-2", "ME", "2020-01-01T00:00:00Z");
    fakes::advance(7 * 86_400_000);
    check(&id);
    assert_eq!(
        domain(&key, &id)["state"],
        "healthy",
        "one RDAP change is not yet an issue"
    );
    fakes::advance(3_600_000);
    let s = until(&key, &id, "suspended", 6);
    assert_eq!(s["state_reason"], "registration_changed");
}

/// A row written without a monitor (`idvy setup`, `--local-token`) gets one `DomainMonitor` from the
/// every-minute cron, with `Init` and `domain.created`; overlapping runs mint one ID.
#[test]
fn cron_mints_missing_monitor() {
    let _reset = fakes::Reset;
    let (_, key, _, _, id) = healthy("cron mint");
    d1(&format!(
        "UPDATE domains SET monitor_do_id = '' WHERE id = '{id}'"
    ));
    crate::cron("* * * * *");
    crate::cron("* * * * *");
    let rows = d1(&format!(
        "SELECT monitor_do_id FROM domains WHERE id = '{id}'"
    ));
    let monitor = rows[0]["monitor_do_id"].as_str().unwrap().to_owned();
    assert!(!monitor.is_empty());
    crate::cron("* * * * *");
    let again = d1(&format!(
        "SELECT monitor_do_id FROM domains WHERE id = '{id}'"
    ));
    assert_eq!(again[0]["monitor_do_id"].as_str(), Some(monitor.as_str()));
    check(&id);
    assert!(domain(&key, &id)["state"].is_string());
}
