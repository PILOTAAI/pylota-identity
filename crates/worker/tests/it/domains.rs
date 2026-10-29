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
    assert!(
        records.iter().all(|x| x["host"].is_string()),
        "every record has its host: {records:?}"
    );
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

/// The `from` of the last send to `to` with `subject`.
fn sent_from(to: &str, subject: &str) -> String {
    crate::eventually(30, subject, || {
        fakes::mail_sent(to)
            .into_iter()
            .rev()
            .find(|m| m["subject"] == subject)
            .and_then(|m| m["from"].as_str().map(str::to_owned))
    })
}

/// A sender whose primary is on a healthy domain: `(sender, domain id, zone, primary)`.
fn sender_on_domain(tag: &str) -> (crate::send::Sender, String, String, String) {
    let (s, did, zone) = crate::addresses::on_domain(tag);
    let x = crate::addresses::create(&s, "desk", &did);
    assert_eq!(x.status, 201, "{}", x.body);
    let xid = x.body["id"].as_str().unwrap().to_owned();
    assert_eq!(crate::addresses::promote(&s, &xid, json!({})).status, 200);
    (s, did, zone.clone(), format!("desk@{zone}"))
}

/// H1, FR-DOM-5, FR-DOM-6: a removed DKIM record makes the domain `failing` after two agreeing checks;
/// sends fall back to the platform address; recovery restores new threads, pinned threads stay.
#[test]
fn h1_failing_fallback() {
    let _reset = fakes::Reset;
    let (s, did, zone, primary) = sender_on_domain("h1");
    let rcpt = "h1@example.net";
    crate::send::send_ok(&s, &[rcpt], "H1 before");
    assert_eq!(sent_from(rcpt, "H1 before"), primary);
    let dkim = format!("cf-send._domainkey.{zone}");
    cloud::hide(&dkim, "TXT", true);
    let d = until(&platform(), &did, "failing", 4);
    assert_eq!(d["state_reason"], "dkim_missing");
    let h = get(&format!("/v1/domains/{did}/health"), &platform());
    assert_eq!(h.body["fallback_active"], true, "{}", h.body);
    let mid = crate::send::send_ok(&s, &[rcpt], "H1 during");
    assert_eq!(sent_from(rcpt, "H1 during"), s.addr);
    let m = crate::send::wait_status(&s, &mid, "submitted");
    assert!(
        m["flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "sent_via_fallback"),
        "{m}"
    );
    let thread = m["thread_id"].as_str().unwrap().to_owned();
    cloud::hide(&dkim, "TXT", false);
    until(&platform(), &did, "healthy", 4);
    crate::send::send_ok(&s, &[rcpt], "H1 after");
    assert_eq!(sent_from(rcpt, "H1 after"), primary);
    // The fallback thread stays pinned to the platform address.
    let r = crate::send::send(
        &s,
        json!({ "to": [rcpt], "subject": "H1 pinned", "text": "x", "thread_id": thread }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(sent_from(rcpt, "H1 pinned"), s.addr);
}

/// H17: while `delivery_events` is `manual`, sends leave from the platform address; once the
/// subscription is recorded, new threads send from the domain.
#[test]
fn h17_manual_events_fallback() {
    let _reset = fakes::Reset;
    let s = crate::send::open_sender("h17", "live");
    let zone = zone_name("h17");
    cloud::zone(&zone);
    dmarc(&zone, &zone);
    cloud::fail("POST", "/event_subscriptions/subscriptions", 403, 1);
    let r = add(
        &platform(),
        &s.tid,
        json!({ "name": zone, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.body["delivery_events"], "manual", "{}", r.body);
    let did = r.body["id"].as_str().unwrap().to_owned();
    until(&platform(), &did, "healthy", 6);
    let x = crate::addresses::create(&s, "desk", &did);
    let xid = x.body["id"].as_str().unwrap().to_owned();
    assert_eq!(crate::addresses::promote(&s, &xid, json!({})).status, 200);
    let rcpt = "h17@example.net";
    crate::send::send_ok(&s, &[rcpt], "H17 manual");
    assert_eq!(sent_from(rcpt, "H17 manual"), s.addr);
    d1(&format!(
        "UPDATE domains SET event_subscription_id = 'sub-h17' WHERE id = '{did}'"
    ));
    crate::send::send_ok(&s, &[rcpt], "H17 active");
    assert_eq!(sent_from(rcpt, "H17 active"), format!("desk@{zone}"));
}

/// H6: a literal rule that cannot be created leaves the address `pending` with `routing_rule_failed`;
/// the monitor retries it and activates the address with its rule.
#[test]
fn h6_rule_failure() {
    let _reset = fakes::Reset;
    let (_, key, _, name, did) = healthy("h6");
    let tid = domain(&key, &did)["tenant_id"].as_str().unwrap().to_owned();
    let s = crate::send::sender_in(&tid, "h6 in", json!({ "require_known_recipient": false }));
    cloud::fail("POST", "/email/routing/rules", 500, 1);
    let r = crate::addresses::create(&s, "help", &did);
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (201, Some("pending")),
        "{}",
        r.body
    );
    let address = format!("help@{name}");
    assert!(!cloud::has_rule(&address));
    check(&did);
    assert!(cloud::has_rule(&address));
    assert_eq!(
        crate::addresses::by_address(&s, &address)["status"],
        "active"
    );
}

/// H7: both resolvers failing change nothing for 6 hours; then `cd=1` decides `dns_unresolvable` (or
/// `dnssec_bogus` for a name that answers with validation off), both agree on `fail`, and two cycles
/// later the domain is `failing`.
#[test]
fn h7_dns_unresolvable() {
    let _reset = fakes::Reset;
    let (_, key, zone, _, did) = healthy("h7");
    cloud::dns_fail(&format!("*.{zone}"), None, Some(2));
    check(&did);
    fakes::advance(3 * 3_600_000);
    check(&did);
    assert_eq!(
        domain(&key, &did)["state"],
        "healthy",
        "no change before 6 hours"
    );
    fakes::advance(3 * 3_600_000);
    let d = until(&key, &did, "failing", 4);
    assert_eq!(d["state_reason"], "dns_unresolvable", "{d}");
    cloud::dns_fail(&format!("*.{zone}"), None, None);
    until(&key, &did, "healthy", 4);
    // A broken DNSSEC chain: the names answer with cd=1.
    let (_, key2, zone2, name2, did2) = healthy("h7 bogus");
    cloud::dns_fail(&format!("*.{zone2}"), None, Some(2));
    cloud::dnssec_bogus(&name2);
    check(&did2);
    fakes::advance(6 * 3_600_000 + 60_000);
    let d = until(&key2, &did2, "failing", 4);
    assert_eq!(d["state_reason"], "dnssec_bogus", "{d}");
    cloud::dns_fail(&format!("*.{zone2}"), None, None);
}

/// Puts the platform domain back as `xtask itest` left it.
struct PlatformRestore;

impl Drop for PlatformRestore {
    fn drop(&mut self) {
        cloud::hide("cf-send._domainkey.agents.example", "TXT", false);
        d1(
            "UPDATE domains SET state = 'healthy', state_reason = NULL, records_json = '[]', expected_ns_json = NULL WHERE kind = 'platform'",
        );
    }
}

/// H9: the platform domain becomes `failing` (a page, never `suspended`, also 14 days later); an NS
/// change counts as `fail`; reprove re-records its name servers; it cannot be deleted.
#[test]
fn h9_platform_domain_failing() {
    let _reset = fakes::Reset;
    let _restore = PlatformRestore;
    let p = platform();
    crate::cron("* * * * *");
    let row = d1("SELECT id, monitor_do_id FROM domains WHERE kind = 'platform'");
    let did = row[0]["id"].as_str().unwrap().to_owned();
    assert!(!row[0]["monitor_do_id"].as_str().unwrap_or("").is_empty());
    let dkim = "cf-send._domainkey.agents.example";
    cloud::record(
        "agents.example",
        "TXT",
        dkim,
        "v=DKIM1; k=rsa; p=PLATFORM",
        None,
    );
    let recs = json!([{ "type": "TXT", "name": dkim, "value": "v=DKIM1; k=rsa; p=PLATFORM", "purpose": "dkim", "required": true, "source": "sending" }]);
    d1(&format!(
        "UPDATE domains SET records_json = '{}', expected_ns_json = '[\"ada.ns.cloudflare.com\",\"bob.ns.cloudflare.com\"]' WHERE id = '{did}'",
        recs
    ));
    check(&did);
    cloud::hide(dkim, "TXT", true);
    let d = until(&p, &did, "failing", 4);
    assert_eq!(d["state_reason"], "dkim_missing");
    fakes::advance(15 * 86_400_000);
    check(&did);
    assert_eq!(domain(&p, &did)["state"], "failing", "never suspended");
    let r = crate::delete(&format!("/v1/domains/{did}"), &p);
    assert_eq!((r.status, r.code()), (403, "scope_denied"));
    cloud::hide(dkim, "TXT", false);
    until(&p, &did, "healthy", 4);
    // An NS change is a `fail`, never a suspension.
    cloud::hide("agents.example", "NS", true);
    cloud::dns_extra("agents.example", "NS", "ns1.elsewhere.example.");
    fakes::advance(8 * 86_400_000);
    let d = until(&p, &did, "failing", 4);
    assert_ne!(d["state"], "suspended");
    cloud::hide("agents.example", "NS", false);
    cloud::dns_extra_clear("agents.example");
    let r = post(&format!("/v1/domains/{did}/reprove"), &p, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    until(&p, &did, "healthy", 4);
}

/// The domain's state in D1.
fn state_of(id: &str) -> String {
    d1(&format!("SELECT state FROM domains WHERE id = '{id}'"))[0]["state"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

/// The domain's latest removal job.
fn removal_job(id: &str) -> String {
    crate::eventually(30, "the removal job", || {
        d1(&format!(
            "SELECT id FROM jobs WHERE kind = 'domain_remove' AND json_extract(params_json, '$.domain_id') = '{id}' ORDER BY created_at DESC, rowid DESC LIMIT 1"
        ))
        .get(0)
        .and_then(|r| r["id"].as_str().map(str::to_owned))
    })
}

/// Drives a job to its end; answers its status.
fn drive(job: &str) -> String {
    crate::eventually(120, "the removal to end", || {
        crate::erasure::job_alarm(job);
        let s = d1(&format!("SELECT status FROM jobs WHERE id = '{job}'"))[0]["status"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        matches!(s.as_str(), "completed" | "failed" | "canceled").then_some(s)
    })
}

/// `DELETE` the domain and drive its removal job: `(job id, status)`.
fn remove(key: &str, id: &str) -> (String, String) {
    let r = crate::delete(&format!("/v1/domains/{id}"), key);
    assert_eq!(r.status, 202, "{}", r.body);
    let job = removal_job(id);
    let st = drive(&job);
    (job, st)
}

/// The steps of a job: `(name, status, attempts, counts)`.
fn job_steps(job: &str) -> Vec<(String, String, i64, String)> {
    crate::erasure::job_alarm(job)["steps"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|s| {
            (
                s["name"].as_str().unwrap_or("").to_owned(),
                s["status"].as_str().unwrap_or("").to_owned(),
                s["attempts"].as_i64().unwrap_or(0),
                s["counts_json"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

/// Whether Email Routing's MX records are at `name`.
fn routed(name: &str) -> bool {
    cloud::records_at(name).iter().any(|r| r.rtype == "MX")
}

/// A healthy domain at `name` in an already listed zone.
fn healthy_at(key: &str, tid: &str, zone: &str, name: &str) -> String {
    dmarc(zone, name);
    let r = add(
        key,
        tid,
        json!({ "name": name, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let id = r.body["id"].as_str().unwrap().to_owned();
    until(key, &id, "healthy", 6);
    id
}

fn zone_wide_disables(since: usize) -> usize {
    cloud::calls()[since..]
        .iter()
        .filter(|(m, p)| m == "DELETE" && p.ends_with("/email/routing/dns"))
        .count()
}

/// Removing a domain with a failover SES identity deletes the identity and its DKIM CNAMEs; an
/// identity already gone at SES counts as done.
#[test]
fn remove_deletes_ses_identity() {
    let _reset = fakes::Reset;
    let (_, key, _, name, id) = healthy("remove ses");
    let identity = cloud::ses_identity(&name).expect("the failover identity");
    let tokens: Vec<String> = identity["DkimAttributes"]["Tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_owned())
        .collect();
    let cnames = |n: &str| {
        tokens
            .iter()
            .map(|t| cloud::records_at(&format!("{t}._domainkey.{n}")).len())
            .sum::<usize>()
    };
    assert_eq!(cnames(&name), 3);
    let (_, st) = remove(&key, &id);
    assert_eq!(st, "completed");
    assert_eq!(state_of(&id), "removed");
    assert!(cloud::ses_identity(&name).is_none());
    assert_eq!(cnames(&name), 0);
    // An identity deleted out of band: `NotFoundException` is done.
    let (_, key2, _, name2, id2) = healthy("remove ses gone");
    cloud::ses_forget(&name2);
    let (_, st) = remove(&key2, &id2);
    assert_eq!(st, "completed");
    assert_eq!(state_of(&id2), "removed");
}

/// H11: removal stays per name while other routing domains remain in the zone; the zone-wide disable
/// runs once, for the last routing domain at the apex; a gone zone skips the zone-bound steps.
#[test]
fn h11_remove_keeps_zone_routing() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("h11");
    let zone = zone_name("h11");
    cloud::zone(&zone);
    list_zones(&tid, &[&zone, &format!("={zone}")]);
    let (a, b) = (format!("a.{zone}"), format!("b.{zone}"));
    let apex = healthy_at(&key, &tid, &zone, &zone);
    let ida = healthy_at(&key, &tid, &zone, &a);
    let idb = healthy_at(&key, &tid, &zone, &b);
    assert!(routed(&zone) && routed(&a) && routed(&b));
    let since = cloud::calls().len();
    assert_eq!(remove(&key, &ida).1, "completed");
    assert!(!routed(&a), "a's routing records are gone");
    assert!(routed(&zone) && routed(&b), "the others keep theirs");
    assert_eq!(zone_wide_disables(since), 0);
    // The apex while a subdomain remains: still per name.
    assert_eq!(remove(&key, &apex).1, "completed");
    assert!(!routed(&zone) && routed(&b));
    assert_eq!(zone_wide_disables(since), 0);
    assert_eq!(remove(&key, &idb).1, "completed");
    assert_eq!(zone_wide_disables(since), 0);
    // The zone's last routing domain, at its apex: one zone-wide disable.
    let (tid2, key2) = tenant_key("h11 apex");
    let zone2 = zone_name("h11 apex");
    cloud::zone(&zone2);
    list_zones(&tid2, &[&format!("={zone2}")]);
    let only = healthy_at(&key2, &tid2, &zone2, &zone2);
    let since = cloud::calls().len();
    assert_eq!(remove(&key2, &only).1, "completed");
    assert_eq!(zone_wide_disables(since), 1);
    // A zone deleted out of band.
    let (_, key3, zone3, _, gone) = healthy("h11 gone");
    cloud::drop_zone(&zone3);
    let (job, st) = remove(&key3, &gone);
    assert_eq!(st, "completed");
    assert_eq!(state_of(&gone), "removed");
    let steps = job_steps(&job);
    for s in ["disable_sending", "delete_ownership_record", "delete_zone"] {
        let (_, status, _, counts) = steps.iter().find(|x| x.0 == s).unwrap();
        assert_eq!(status, "done", "{s}");
        assert!(counts.contains("skipped_zone_gone"), "{s}: {counts}");
    }
}

/// H12: a step failing 10 times fails the job; the domain stays `removing`; the daily run restarts
/// it from the failed step; `DELETE` restarts a failed job at once.
#[test]
fn h12_remove_failed_restart() {
    let _reset = fakes::Reset;
    let (_, key, _, name, id) = healthy("h12");
    cloud::fail("DELETE", "/email/sending/subdomains", 503, 1000);
    let (job, st) = remove(&key, &id);
    assert_eq!(st, "failed");
    assert_eq!(state_of(&id), "removing");
    let steps = job_steps(&job);
    let failed = steps.iter().find(|s| s.0 == "disable_sending").unwrap();
    assert_eq!(failed.2, 10, "{steps:?}");
    crate::eventually(30, "the domain_remove_failed alert", || {
        let log = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/itest/worker.log"),
        )
        .unwrap_or_default();
        log.lines()
            .any(|l| l.contains("\"domain_remove_failed\"") && l.contains(id.as_str()))
            .then_some(())
    });
    // The daily run (04:00 UTC) restarts it from `disable_sending` once the provider recovers.
    cloud::unfail();
    let since = cloud::calls().len();
    fakes::advance_to_minute(4 * 60);
    crate::cron("*/15 * * * *");
    let again = removal_job(&id);
    assert_ne!(again, job);
    assert_eq!(drive(&again), "completed");
    assert_eq!(state_of(&id), "removed");
    let calls = &cloud::calls()[since..];
    assert!(
        !calls.iter().any(|(_, p)| p.contains("/email/routing")),
        "earlier steps are not re-run: {calls:?}"
    );
    assert!(!cloud::sending_names().contains(&name));
    // `DELETE` on a `removing` domain whose job failed restarts it at once.
    let (_, key2, _, _, id2) = healthy("h12 delete");
    cloud::fail("DELETE", "/email/sending/subdomains", 503, 1000);
    let (first, st) = remove(&key2, &id2);
    assert_eq!(st, "failed");
    cloud::unfail();
    let r = crate::delete(&format!("/v1/domains/{id2}"), &key2);
    assert_eq!(r.status, 202, "{}", r.body);
    let second = removal_job(&id2);
    assert_ne!(second, first);
    assert_eq!(drive(&second), "completed");
    assert_eq!(state_of(&id2), "removed");
}

/// H13: a removed name added again gets a new row and monitor; the old monitor is retired; a retired
/// address stays with the old tenant; a zone with domains cannot leave the tenant's list.
#[test]
fn h13_readd_new_row() {
    let _reset = fakes::Reset;
    let (tid, key, zone, name, id) = healthy("h13");
    let s = crate::send::sender_in(&tid, "h13 a", json!({}));
    let x = crate::addresses::create(&s, "desk", &id);
    assert_eq!(x.status, 201, "{}", x.body);
    let xid = x.body["id"].as_str().unwrap().to_owned();
    d1(&format!(
        "UPDATE addresses SET status = 'retired', routing_rule_id = NULL WHERE id = '{xid}'"
    ));
    // The zone cannot leave A's list while A has a domain in it.
    let r = patch(
        &format!("/v1/tenants/{tid}/policy"),
        &platform(),
        json!({ "domains": { "cloudflare_zones": [] } }),
    );
    assert_eq!((r.status, r.code()), (409, "domain_in_use"), "{}", r.body);
    assert_eq!(r.details()["reason"], "zone_has_domains");
    assert_eq!(r.details()["domain_ids"], json!([id]));
    let old_monitor = d1(&format!(
        "SELECT monitor_do_id FROM domains WHERE id = '{id}'"
    ))[0]["monitor_do_id"]
        .clone();
    assert_eq!(remove(&key, &id).1, "completed");
    let r = hook(
        "POST",
        "/__test/domain-alarm",
        Some(json!({ "domain_id": id })),
    );
    assert!(
        r.body.to_string().contains("\"retired\":true"),
        "{}",
        r.body
    );
    // Tenant B adds the same name.
    let (tidb, keyb) = tenant_key("h13 b");
    list_zones(&tidb, &[&zone]);
    let r = add(
        &keyb,
        &tidb,
        json!({ "name": name, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let idb = r.body["id"].as_str().unwrap().to_owned();
    assert_ne!(idb, id);
    let row = d1(&format!(
        "SELECT monitor_do_id FROM domains WHERE id = '{idb}'"
    ));
    assert_ne!(row[0]["monitor_do_id"], old_monitor);
    until(&keyb, &idb, "healthy", 6);
    // A's retired address stays A's.
    let a = d1(&format!(
        "SELECT domain_id, status FROM addresses WHERE id = '{xid}'"
    ));
    assert_eq!(
        (a[0]["domain_id"].as_str(), a[0]["status"].as_str()),
        (Some(id.as_str()), Some("retired"))
    );
    let sb = crate::send::sender_in(&tidb, "h13 b", json!({}));
    let r = crate::addresses::create(&sb, "desk", &idb);
    assert_eq!((r.status, r.code()), (409, "address_taken"), "{}", r.body);
    // A's tenant erasure leaves B's domain alone.
    let e = crate::erasure::erase(
        &crate::erasure::eraser(&tid),
        json!({ "tenant_id": tid, "scope": "tenant" }),
    );
    assert_eq!(e.status, 202, "{}", e.body);
    let done = crate::erasure::run(e.body["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(domain(&keyb, &idb)["state"], "healthy");
}

/// H14: an unverified holder is evicted by a proved claim, a verified one never; unverified domains
/// expire after 14 days; a tenant holds 5 unverified domains at most.
#[test]
fn h14_unverified_claims() {
    let _reset = fakes::Reset;
    let (tida, keya) = tenant_key("h14 a");
    let zone = zone_name("h14");
    cloud::zone(&zone);
    list_zones(&tida, &[&zone]);
    let victim = format!("victim.{zone}");
    let own = format!("_pylota-identity.{victim}");
    cloud::hide(&own, "TXT", true);
    dmarc(&zone, &victim);
    let r = add(
        &keya,
        &tida,
        json!({ "name": victim, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let ida = r.body["id"].as_str().unwrap().to_owned();
    check(&ida);
    assert_ne!(domain(&keya, &ida)["state"], "healthy");
    let (tidb, keyb) = tenant_key("h14 b");
    list_zones(&tidb, &[&zone]);
    let r = add(
        &keyb,
        &tidb,
        json!({ "name": victim, "method": "cloudflare_zone" }),
    );
    assert_eq!((r.status, r.code()), (409, "domain_exists"), "{}", r.body);
    let claim = r.details()["claim"].clone();
    assert_eq!(claim["name"], own.as_str());
    // The same body for a verified holder, but for the claim value.
    let (_, _, czone, cname, cid) = healthy("h14 c");
    list_zones(&tidb, &[&zone, &czone]);
    let rc = add(
        &keyb,
        &tidb,
        json!({ "name": cname, "method": "cloudflare_zone" }),
    );
    assert_eq!((rc.status, rc.code()), (409, "domain_exists"));
    let strip = |mut b: Value| {
        b["error"]["details"]["claim"]["value"] = Value::Null;
        b["error"]["details"]["claim"]["name"] = Value::Null;
        b["error"]["request_id"] = Value::Null;
        b
    };
    assert_eq!(strip(r.body.clone()), strip(rc.body.clone()));
    // A verified holder is never evicted.
    cloud::dns_extra(
        &format!("_pylota-identity.{cname}"),
        "TXT",
        rc.details()["claim"]["value"].as_str().unwrap(),
    );
    let rc = add(
        &keyb,
        &tidb,
        json!({ "name": cname, "method": "cloudflare_zone", "claim": true }),
    );
    assert_eq!((rc.status, rc.code()), (409, "domain_exists"));
    assert_eq!(state_of(&cid), "healthy");
    // The claim TXT on both resolvers evicts A.
    cloud::dns_extra(&own, "TXT", claim["value"].as_str().unwrap());
    let r = add(
        &keyb,
        &tidb,
        json!({ "name": victim, "method": "cloudflare_zone", "claim": true }),
    );
    assert_eq!(
        (r.status, r.code()),
        (409, "domain_claim_pending"),
        "{}",
        r.body
    );
    assert_eq!(state_of(&ida), "removing");
    assert_eq!(drive(&removal_job(&ida)), "completed");
    assert_eq!(state_of(&ida), "removed");
    let removed = d1(&format!(
        "SELECT payload_json FROM event_index WHERE tenant_id = '{tida}' AND type = 'domain.removed'"
    ));
    assert!(removed.to_string().contains("evicted"), "{removed}");
    cloud::dns_extra_clear(&own);
    cloud::hide(&own, "TXT", false);
    let r = add(
        &keyb,
        &tidb,
        json!({ "name": victim, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    // 14 days unverified: removed with `unverified_expired`.
    let (tidd, keyd) = tenant_key("h14 d");
    let dzone = zone_name("h14 d");
    cloud::zone(&dzone);
    list_zones(&tidd, &[&dzone]);
    let names: Vec<String> = (0..6).map(|i| format!("n{i}.{dzone}")).collect();
    for n in &names {
        cloud::hide(&format!("_pylota-identity.{n}"), "TXT", true);
    }
    let mut ids = Vec::new();
    for n in &names[..5] {
        let r = add(
            &keyd,
            &tidd,
            json!({ "name": n, "method": "cloudflare_zone" }),
        );
        assert_eq!(r.status, 201, "{}", r.body);
        ids.push(r.body["id"].as_str().unwrap().to_owned());
    }
    let r = add(
        &keyd,
        &tidd,
        json!({ "name": names[5], "method": "cloudflare_zone" }),
    );
    assert_eq!(
        (r.status, r.code()),
        (422, "unverified_domain_limit"),
        "{}",
        r.body
    );
    fakes::advance(14 * 86_400_000);
    check(&ids[0]);
    assert_eq!(state_of(&ids[0]), "removing");
    let reason = d1(&format!(
        "SELECT state_reason FROM domains WHERE id = '{}'",
        ids[0]
    ));
    assert_eq!(reason[0]["state_reason"], "unverified_expired");
    assert_eq!(drive(&removal_job(&ids[0])), "completed");
    assert_eq!(state_of(&ids[0]), "removed");
}

/// H16: a failed add's journal objects are deleted by the hourly cleanup, with the journal row.
#[test]
fn h16_failed_add_cleanup() {
    let _reset = fakes::Reset;
    let (tid, key) = tenant_key("h16");
    let zone = zone_name("h16");
    cloud::zone(&zone);
    list_zones(&tid, &[&zone]);
    let name = format!("mail.{zone}");
    cloud::fail("POST", "/event_subscriptions/subscriptions", 503, 1);
    let r = add(
        &key,
        &tid,
        json!({ "name": name, "method": "cloudflare_zone" }),
    );
    assert_eq!((r.status, r.code()), (502, "upstream_error"), "{}", r.body);
    let rows = d1(&format!("SELECT id FROM domains WHERE name = '{name}'"));
    assert!(rows.as_array().unwrap().is_empty());
    let journal = |n: &str| {
        d1(&format!(
            "SELECT COUNT(*) AS n FROM domain_onboarding WHERE name = '{n}'"
        ))[0]["n"]
            .as_i64()
            .unwrap()
    };
    assert_eq!(journal(&name), 1);
    let own = format!("_pylota-identity.{name}");
    assert!(cloud::sending_names().contains(&name));
    assert_eq!(cloud::records_at(&own).len(), 1);
    assert!(cloud::ses_identity(&name).is_some());
    // Before the hour nothing is touched.
    fakes::advance_to_minute(fakes_minute_now() + 20);
    crate::cron("*/15 * * * *");
    assert_eq!(journal(&name), 1);
    fakes::advance(3_600_000);
    fakes::advance_to_minute((fakes_minute_now() / 60 + 1) * 60 % 1440);
    crate::cron("*/15 * * * *");
    assert_eq!(journal(&name), 0);
    assert!(!cloud::sending_names().contains(&name));
    assert!(cloud::records_at(&own).is_empty());
    assert!(cloud::ses_identity(&name).is_none());
}

/// The Worker's minute of the UTC day.
fn fakes_minute_now() -> i64 {
    fakes::worker_now().rem_euclid(86_400_000) / 60_000
}
