//! The provider fakes of the domain tests (Testing § 6.1): the Cloudflare API of one account (zones, DNS
//! records, Email Routing, Email Sending, queues and event subscriptions), the SES v2 identity calls,
//! and RDAP. The DoH fake answers from this account's DNS records first, with per-test overrides:
//! a record hidden from DNS, an extra record, or a name that fails on one or both resolvers.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use serde_json::{Value, json};

/// The platform domain's zone, as `xtask itest` writes its row.
pub const PLATFORM_ZONE: &str = "00000000000000000000000000000001";
pub const NAME_SERVERS: [&str; 2] = ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"];
/// Email Routing's MX hosts and SPF include, as the fake's routing API lists them.
pub const ROUTING_MX: [(&str, u64); 3] = [
    ("route1.mx.cloudflare.net", 13),
    ("route2.mx.cloudflare.net", 86),
    ("route3.mx.cloudflare.net", 24),
];
const SPF: &str = "v=spf1 include:_spf.mx.cloudflare.net ~all";

#[derive(Clone, Debug)]
pub struct Rec {
    pub id: String,
    pub zone: String,
    pub rtype: String,
    pub name: String,
    pub content: String,
    pub priority: Option<u64>,
}

#[derive(Clone, Debug)]
struct Zone {
    id: String,
    name: String,
    status: String,
    name_servers: Vec<String>,
}

#[derive(Default)]
struct Cloud {
    next: u64,
    zones: Vec<Zone>,
    records: Vec<Rec>,
    /// Names with Email Routing on.
    routing: BTreeSet<String>,
    catch_all: BTreeMap<String, Value>,
    /// `(zone, rule id, address)`.
    rules: Vec<(String, String, String)>,
    /// `(zone, tag, name)`.
    sending: Vec<(String, String, String)>,
    subscriptions: Vec<(String, String)>,
    /// `(method, path substring, status, count)`.
    fail: Vec<(String, String, u16, u32)>,
    /// When a failure is answered: fill this zone with foreign sending domains up to this count.
    fill: Option<(String, usize)>,
    calls: Vec<(String, String)>,
    /// DNS view: records hidden by `(name, type)`, extra `(name, type, data)`, names failing on a
    /// resolver (`None`: both) with a DoH status, and names that answer with `cd=1`.
    hidden: BTreeSet<(String, String)>,
    extra: Vec<(String, String, String)>,
    failing: BTreeMap<String, (Option<String>, u64)>,
    bogus: BTreeSet<String>,
    /// SES identities by name.
    ses: BTreeMap<String, Value>,
    ses_calls: Vec<(String, String)>,
    /// RDAP answers by registrable domain.
    rdap: BTreeMap<String, Value>,
}

static CLOUD: Mutex<Option<Cloud>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Cloud) -> T) -> T {
    crate::fakes::start();
    let mut g = CLOUD.lock().unwrap();
    let c = g.get_or_insert_with(|| {
        let mut c = Cloud::default();
        c.zones.push(Zone {
            id: PLATFORM_ZONE.into(),
            name: "agents.example".into(),
            status: "active".into(),
            name_servers: NAME_SERVERS.iter().map(|s| (*s).to_owned()).collect(),
        });
        c.records.push(Rec {
            id: "platformdmarc".into(),
            zone: PLATFORM_ZONE.into(),
            rtype: "TXT".into(),
            name: "_dmarc.agents.example".into(),
            content: "v=DMARC1; p=quarantine".into(),
            priority: None,
        });
        c
    });
    f(c)
}

fn id(c: &mut Cloud) -> String {
    c.next += 1;
    format!("{:032x}", 0xC10D_0000_0000u64 + c.next)
}

/// An active zone of the account; answers its ID.
pub fn zone(name: &str) -> String {
    with(|c| {
        if let Some(z) = c.zones.iter().find(|z| z.name == name) {
            return z.id.clone();
        }
        let zid = id(c);
        c.zones.push(Zone {
            id: zid.clone(),
            name: name.into(),
            status: "active".into(),
            name_servers: NAME_SERVERS.iter().map(|s| (*s).to_owned()).collect(),
        });
        zid
    })
}

/// Removes a zone from the account (deleted or moved out of band).
pub fn drop_zone(name: &str) {
    with(|c| c.zones.retain(|z| z.name != name));
}

/// A DNS record in the account (a pre-existing MX, an SPF record, …).
pub fn record(zone_name: &str, rtype: &str, name: &str, content: &str, priority: Option<u64>) {
    let zid = zone(zone_name);
    with(|c| {
        let rid = id(c);
        c.records.push(Rec {
            id: rid,
            zone: zid,
            rtype: rtype.into(),
            name: name.into(),
            content: content.into(),
            priority,
        });
    });
}

/// The account's records at `name`.
pub fn records_at(name: &str) -> Vec<Rec> {
    with(|c| {
        c.records
            .iter()
            .filter(|r| r.name == name)
            .cloned()
            .collect()
    })
}

/// Hides a record type at `name` from DNS (the provider still has it), or shows it again.
pub fn hide(name: &str, rtype: &str, hidden: bool) {
    with(|c| {
        let k = (name.to_owned(), rtype.to_owned());
        if hidden {
            c.hidden.insert(k);
        } else {
            c.hidden.remove(&k);
        }
    });
}

/// An extra DNS answer at `name` that the provider does not have (`data` in DoH form).
pub fn dns_extra(name: &str, rtype: &str, data: &str) {
    with(|c| c.extra.push((name.into(), rtype.into(), data.into())));
}

pub fn dns_extra_clear(name: &str) {
    with(|c| c.extra.retain(|(n, _, _)| n != name));
}

/// Every query of `name` (`*.zone`: the zone and every name under it) fails on `resolver`
/// (`cloudflare-dns.com`, `dns.google`, or `None` for both) with DoH status `status` (2: SERVFAIL).
pub fn dns_fail(name: &str, resolver: Option<&str>, status: Option<u64>) {
    with(|c| match status {
        Some(st) => {
            c.failing
                .insert(name.into(), (resolver.map(str::to_owned), st));
        }
        None => {
            c.failing.remove(name);
        }
    });
}

/// `name` answers normally with `cd=1` while it fails (a broken DNSSEC chain).
pub fn dnssec_bogus(name: &str) {
    with(|c| {
        c.bogus.insert(name.into());
    });
}

/// The next `count` calls whose method is `method` and whose path contains `contains` answer
/// `status`.
pub fn fail(method: &str, contains: &str, status: u16, count: u32) {
    with(|c| c.fail.push((method.into(), contains.into(), status, count)));
}

/// With the next injected failure, the zone fills up to `n` mail domains (a limit reached meanwhile).
pub fn fill_on_failure(zone_name: &str, n: usize) {
    let zid = zone(zone_name);
    with(|c| c.fill = Some((zid, n)));
}

/// The calls so far: `(method, path)`.
pub fn calls() -> Vec<(String, String)> {
    with(|c| c.calls.clone())
}

pub fn has_rule(address: &str) -> bool {
    with(|c| c.rules.iter().any(|(_, _, a)| a == address))
}

pub fn sending_names() -> Vec<String> {
    with(|c| c.sending.iter().map(|(_, _, n)| n.clone()).collect())
}

pub fn catch_all(zone_id: &str) -> Value {
    with(|c| c.catch_all.get(zone_id).cloned().unwrap_or(Value::Null))
}

pub fn subscriptions() -> Vec<String> {
    with(|c| c.subscriptions.iter().map(|(_, d)| d.clone()).collect())
}

/// An Email Sending domain created outside this deployment.
pub fn foreign_sending(zone_name: &str, name: &str) {
    let zid = zone(zone_name);
    with(|c| {
        let tag = id(c);
        c.sending.push((zid, tag, name.into()));
    });
}

/// A catch-all at the zone's apex whose action is not the Worker.
pub fn foreign_catch_all(zone_name: &str) {
    let zid = zone(zone_name);
    with(|c| {
        c.catch_all.insert(
            zid,
            json!({ "enabled": true, "matchers": [{ "type": "all" }], "actions": [{ "type": "forward", "value": ["ops@elsewhere.example"] }] }),
        );
    });
}

/// SES identities: `(name, identity)`.
pub fn ses_identity(name: &str) -> Option<Value> {
    with(|c| c.ses.get(name).cloned())
}

/// Sets the SES fake's verification state of an identity.
pub fn ses_status(name: &str, verified: bool, dkim: &str) {
    with(|c| {
        if let Some(i) = c.ses.get_mut(name) {
            i["VerifiedForSendingStatus"] = verified.into();
            i["DkimAttributes"]["Status"] = dkim.into();
        }
    });
}

/// An SES identity tagged for another deployment.
pub fn foreign_ses_identity(name: &str) {
    with(|c| {
        c.ses.insert(name.into(), json!({ "IdentityType": "DOMAIN", "VerifiedForSendingStatus": true,
            "DkimAttributes": { "Status": "SUCCESS", "Tokens": ["f1", "f2", "f3"], "SigningHostedZone": "dkim.amazonses.com" },
            "Tags": [{ "Key": "pylota-identity:api-host", "Value": "other.example" }] }));
    });
}

/// SES calls so far: `(method, path)`.
pub fn ses_calls() -> Vec<(String, String)> {
    with(|c| c.ses_calls.clone())
}

/// The RDAP answer for a registrable domain (registrar handle, registrant handle, registration date).
pub fn rdap(domain: &str, registrar: &str, registrant: &str, date: &str) {
    with(|c| {
        c.rdap.insert(
            domain.into(),
            json!({ "objectClassName": "domain", "ldhName": domain,
                "entities": [
                    { "objectClassName": "entity", "handle": registrar, "roles": ["registrar"] },
                    { "objectClassName": "entity", "handle": registrant, "roles": ["registrant"] }
                ],
                "events": [{ "eventAction": "registration", "eventDate": date }] }),
        );
    });
}

fn envelope(result: Value) -> (u16, Value) {
    (
        200,
        json!({ "success": true, "errors": [], "messages": [], "result": result }),
    )
}

fn error(status: u16, code: i64) -> (u16, Value) {
    (
        status,
        json!({ "success": false, "errors": [{ "code": code, "message": "fake" }], "result": null }),
    )
}

fn rec_json(r: &Rec) -> Value {
    json!({ "id": r.id, "type": r.rtype, "name": r.name, "content": r.content, "priority": r.priority, "ttl": 1 })
}

fn query(path: &str) -> (String, BTreeMap<String, String>) {
    let (p, q) = path.split_once('?').unwrap_or((path, ""));
    let mut m = BTreeMap::new();
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            m.insert(k.to_owned(), decode(v));
        }
    }
    (p.to_owned(), m)
}

fn decode(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(x) = u8::from_str_radix(&v[i + 1..i + 3], 16) {
                out.push(x);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The routing records Email Routing adds at `name` in zone `zid`.
fn routing_records(zid: &str, name: &str) -> Vec<Rec> {
    let mut v: Vec<Rec> = ROUTING_MX
        .iter()
        .map(|(h, p)| Rec {
            id: String::new(),
            zone: zid.into(),
            rtype: "MX".into(),
            name: name.into(),
            content: (*h).into(),
            priority: Some(*p),
        })
        .collect();
    v.push(Rec {
        id: String::new(),
        zone: zid.into(),
        rtype: "TXT".into(),
        name: name.into(),
        content: SPF.into(),
        priority: None,
    });
    v.push(Rec {
        id: String::new(),
        zone: zid.into(),
        rtype: "TXT".into(),
        name: format!("cf2024-1._domainkey.{name}"),
        content: format!("v=DKIM1; h=sha256; k=rsa; p=ROUTE{}", &zid[24..]),
        priority: None,
    });
    v
}

/// The records an Email Sending domain `name` with `tag` needs.
fn sending_records(zid: &str, tag: &str, name: &str) -> Vec<Rec> {
    vec![
        Rec {
            id: String::new(),
            zone: zid.into(),
            rtype: "MX".into(),
            name: format!("cf-bounce.{name}"),
            content: "bounce.mx.cloudflare.net".into(),
            priority: Some(10),
        },
        Rec {
            id: String::new(),
            zone: zid.into(),
            rtype: "TXT".into(),
            name: format!("cf-bounce.{name}"),
            content: SPF.into(),
            priority: None,
        },
        Rec {
            id: String::new(),
            zone: zid.into(),
            rtype: "TXT".into(),
            name: format!("cf-send._domainkey.{name}"),
            content: format!("v=DKIM1; k=rsa; p=SEND{}", &tag[24..]),
            priority: None,
        },
    ]
}

fn add_records(c: &mut Cloud, recs: Vec<Rec>) {
    for mut r in recs {
        if c.records.iter().any(|x| {
            x.zone == r.zone && x.name == r.name && x.rtype == r.rtype && x.content == r.content
        }) {
            continue;
        }
        r.id = id(c);
        c.records.push(r);
    }
}

/// A Cloudflare API call (`/client/v4/…`), or `None` for a path this fake does not own.
pub fn cf(method: &str, full: &str, body: &Value) -> Option<(u16, Value)> {
    let path = full.strip_prefix("/__http/api.cloudflare.com/client/v4")?;
    if path.contains("/email/sending/suppressions") {
        return None;
    }
    Some(with(|c| {
        c.calls.push((method.to_owned(), path.to_owned()));
        if let Some(f) = c
            .fail
            .iter_mut()
            .find(|(m, s, _, n)| m == method && path.contains(s.as_str()) && *n > 0)
        {
            f.3 -= 1;
            let status = f.2;
            if let Some((zone, n)) = c.fill.take() {
                while c.sending.iter().filter(|(z, _, _)| *z == zone).count() < n {
                    let tag = id(c);
                    let name = format!("filler-{tag}.example");
                    c.sending.push((zone.clone(), tag, name));
                }
            }
            return error(status, 1000);
        }
        let (p, q) = query(path);
        let parts: Vec<&str> = p.trim_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("GET", ["zones"]) => {
                let name = q.get("name").cloned().unwrap_or_default();
                envelope(Value::Array(
                    c.zones
                        .iter()
                        .filter(|z| z.name == name)
                        .map(|z| json!({ "id": z.id, "name": z.name, "status": z.status, "name_servers": z.name_servers, "created_on": "2026-01-01T00:00:00Z" }))
                        .collect(),
                ))
            }
            ("GET", ["zones", z]) => match c.zones.iter().find(|x| x.id == *z) {
                Some(x) => envelope(
                    json!({ "id": x.id, "name": x.name, "status": x.status, "name_servers": x.name_servers }),
                ),
                None => error(404, 1001),
            },
            (_, ["zones", z, ..]) if !c.zones.iter().any(|x| x.id == *z) => error(404, 1001),
            ("GET", ["zones", z, "dns_records"]) => {
                let name = q.get("name.exact").cloned().unwrap_or_default();
                let t = q.get("type").cloned();
                envelope(Value::Array(
                    c.records
                        .iter()
                        .filter(|r| {
                            r.zone == *z
                                && r.name == name
                                && t.as_ref().is_none_or(|t| &r.rtype == t)
                        })
                        .map(rec_json)
                        .collect(),
                ))
            }
            ("GET", ["zones", z, "dns_records", rid]) => {
                match c.records.iter().find(|r| r.zone == *z && r.id == *rid) {
                    Some(r) => envelope(rec_json(r)),
                    None => error(404, 81044),
                }
            }
            ("POST", ["zones", z, "dns_records"]) => {
                let r = Rec {
                    id: id(c),
                    zone: (*z).to_owned(),
                    rtype: body["type"].as_str().unwrap_or("").into(),
                    name: body["name"].as_str().unwrap_or("").to_ascii_lowercase(),
                    content: body["content"].as_str().unwrap_or("").into(),
                    priority: body["priority"].as_u64(),
                };
                let out = rec_json(&r);
                c.records.push(r);
                envelope(out)
            }
            ("DELETE", ["zones", z, "dns_records", rid]) => {
                let before = c.records.len();
                c.records.retain(|r| !(r.zone == *z && r.id == *rid));
                if c.records.len() < before {
                    envelope(json!({ "id": rid }))
                } else {
                    error(404, 81044)
                }
            }
            ("POST", ["zones", z, "email", "routing", "dns"]) => {
                let zone_name = c.zones.iter().find(|x| x.id == *z).unwrap().name.clone();
                let name = match body["name"].as_str() {
                    Some(n) if n == zone_name => return error(400, 2007),
                    Some(n) => n.to_owned(),
                    None => zone_name,
                };
                c.routing.insert(name.clone());
                let recs = routing_records(z, &name);
                add_records(c, recs);
                envelope(json!({ "enabled": true, "name": name }))
            }
            ("GET", ["zones", z, "email", "routing", "dns"]) => {
                let zone_name = c.zones.iter().find(|x| x.id == *z).unwrap().name.clone();
                let name = q.get("subdomain").cloned().unwrap_or(zone_name);
                envelope(Value::Array(
                    routing_records(z, &name).iter().map(rec_json).collect(),
                ))
            }
            ("PATCH", ["zones", _, "email", "routing", "dns"]) => {
                envelope(json!({ "name": body["name"] }))
            }
            ("DELETE", ["zones", z, "email", "routing", "dns"]) => {
                let zone_name = c.zones.iter().find(|x| x.id == *z).unwrap().name.clone();
                c.routing.retain(|n| n != &zone_name && !n.ends_with(&format!(".{zone_name}")));
                envelope(json!({ "enabled": false }))
            }
            ("PATCH", ["zones", _, "email", "routing"]) => envelope(body.clone()),
            ("GET", ["zones", z, "email", "routing", "rules", "catch_all"]) => envelope(
                c.catch_all
                    .get(*z)
                    .cloned()
                    .unwrap_or_else(|| json!({ "enabled": false, "matchers": [{ "type": "all" }], "actions": [{ "type": "drop" }] })),
            ),
            ("PUT", ["zones", z, "email", "routing", "rules", "catch_all"]) => {
                c.catch_all.insert((*z).to_owned(), body.clone());
                envelope(body.clone())
            }
            ("POST", ["zones", z, "email", "routing", "rules"]) => {
                let rid = id(c);
                let addr = body["matchers"][0]["value"].as_str().unwrap_or("").to_owned();
                c.rules.push(((*z).to_owned(), rid.clone(), addr));
                envelope(json!({ "id": rid, "tag": rid }))
            }
            ("GET", ["zones", z, "email", "routing", "rules", rid]) => {
                match c.rules.iter().find(|(zz, r, _)| zz == z && r == rid) {
                    Some((_, r, a)) => envelope(json!({ "id": r, "matchers": [{ "type": "literal", "field": "to", "value": a }] })),
                    None => error(404, 2020),
                }
            }
            ("DELETE", ["zones", z, "email", "routing", "rules", rid]) => {
                c.rules.retain(|(zz, r, _)| !(zz == z && r == rid));
                envelope(json!({ "id": rid }))
            }
            ("GET", ["zones", z, "email", "sending", "subdomains"]) => envelope(Value::Array(
                c.sending
                    .iter()
                    .filter(|(zz, _, _)| zz == z)
                    .map(|(_, t, n)| json!({ "tag": t, "name": n }))
                    .collect(),
            )),
            ("POST", ["zones", z, "email", "sending", "subdomains"]) => {
                let name = body["name"].as_str().unwrap_or("").to_owned();
                let tag = id(c);
                c.sending.push(((*z).to_owned(), tag.clone(), name.clone()));
                let recs = sending_records(z, &tag, &name);
                add_records(c, recs);
                envelope(json!({ "tag": tag, "name": name, "dkim_selector": "cf-send", "return_path_domain": format!("cf-bounce.{name}") }))
            }
            ("PATCH", ["zones", _, "email", "sending", "subdomains", _]) => envelope(body.clone()),
            ("GET", ["zones", z, "email", "sending", "subdomains", tag, "dns"]) => {
                match c.sending.iter().find(|(zz, t, _)| zz == z && t == tag) {
                    Some((_, t, n)) => envelope(Value::Array(
                        sending_records(z, t, n).iter().map(rec_json).collect(),
                    )),
                    None => error(404, 1002),
                }
            }
            ("GET", ["zones", z, "email", "sending", "subdomains", tag]) => {
                match c.sending.iter().find(|(zz, t, _)| zz == z && t == tag) {
                    Some((_, t, n)) => envelope(json!({ "tag": t, "name": n })),
                    None => error(404, 1002),
                }
            }
            ("DELETE", ["zones", z, "email", "sending", "subdomains", tag]) => {
                let names: Vec<String> = c
                    .sending
                    .iter()
                    .filter(|(zz, t, _)| zz == z && t == tag)
                    .map(|(_, _, n)| n.clone())
                    .collect();
                c.sending.retain(|(zz, t, _)| !(zz == z && t == tag));
                for n in names {
                    let bounce = format!("cf-bounce.{n}");
                    let dkim = format!("cf-send._domainkey.{n}");
                    c.records.retain(|r| r.name != bounce && r.name != dkim);
                }
                envelope(json!({ "tag": tag }))
            }
            ("GET", ["accounts", _, "queues"]) => envelope(json!([
                { "queue_id": "q0000000000000000000000000000001", "queue_name": "pm-delivery-events" },
                { "queue_id": "q0000000000000000000000000000002", "queue_name": "pm-index" }
            ])),
            ("POST", ["accounts", _, "event_subscriptions", "subscriptions"]) => {
                let sid = id(c);
                let domain = body["source"]["domain"].as_str().unwrap_or("").to_owned();
                c.subscriptions.push((sid.clone(), domain));
                envelope(json!({ "id": sid }))
            }
            ("DELETE", ["accounts", _, "event_subscriptions", "subscriptions", sid]) => {
                c.subscriptions.retain(|(s, _)| s != sid);
                envelope(json!({ "id": sid }))
            }
            _ => error(404, 7003),
        }
    }))
}

/// An SES v2 call (`email.{region}.amazonaws.com/v2/email/…`).
pub fn ses(method: &str, full: &str, body: &Value) -> Option<(u16, Value)> {
    let rest = full.strip_prefix("/__http/email.")?;
    let path = &rest[rest.find('/')?..];
    let path = path.strip_prefix("/v2/email/identities")?;
    Some(with(|c| {
        c.ses_calls.push((method.to_owned(), path.to_owned()));
        let name = decode(path.trim_start_matches('/').split('?').next().unwrap_or(""));
        match method {
            "POST" => {
                let name = body["EmailIdentity"].as_str().unwrap_or("").to_owned();
                if c.ses.contains_key(&name) {
                    return (
                        400,
                        json!({ "__type": "AlreadyExistsException", "message": "exists" }),
                    );
                }
                let n = c.ses.len();
                let tokens: Vec<String> = (0..3).map(|i| format!("tok{n}x{i}")).collect();
                let identity = json!({ "IdentityType": "DOMAIN", "VerifiedForSendingStatus": false,
                    "DkimAttributes": { "Status": "PENDING", "Tokens": tokens, "SigningHostedZone": "dkim.amazonses.com" },
                    "Tags": body["Tags"].clone() });
                c.ses.insert(name, identity.clone());
                (200, identity)
            }
            "GET" if name.is_empty() => (
                200,
                json!({ "EmailIdentities": c.ses.keys().map(|k| json!({ "IdentityName": k })).collect::<Vec<_>>() }),
            ),
            "GET" => match c.ses.get(&name) {
                Some(i) => (200, i.clone()),
                None => (
                    404,
                    json!({ "__type": "NotFoundException", "message": "missing" }),
                ),
            },
            "DELETE" => match c.ses.remove(&name) {
                Some(_) => (200, json!({})),
                None => (
                    404,
                    json!({ "__type": "NotFoundException", "message": "missing" }),
                ),
            },
            _ => (400, json!({ "message": "unsupported" })),
        }
    }))
}

/// RDAP: the IANA bootstrap file and the fake registry.
pub fn rdap_call(full: &str) -> Option<(u16, Value)> {
    if full.starts_with("/__http/data.iana.org/rdap/dns.json") {
        return Some((
            200,
            json!({ "version": "1.0", "services": [[["example"], ["https://rdap.registry.example/"]]] }),
        ));
    }
    let name = full.strip_prefix("/__http/rdap.registry.example/domain/")?;
    Some(with(|c| match c.rdap.get(name) {
        Some(v) => (200, v.clone()),
        None => (404, json!({ "errorCode": 404 })),
    }))
}

fn txt_data(t: &str) -> String {
    t.as_bytes()
        .chunks(255)
        .map(|c| format!("\"{}\"", String::from_utf8_lossy(c).replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn code(t: &str) -> u64 {
    match t {
        "A" => 1,
        "NS" => 2,
        "CNAME" => 5,
        "MX" => 15,
        "TXT" => 16,
        "DS" => 43,
        _ => 0,
    }
}

/// A DoH answer from the account, or `None` when `name` is under no fake zone and has no override.
pub fn dns(resolver: &str, name: &str, rtype: &str, cd: bool) -> Option<Value> {
    with(|c| {
        if name == "_spf.mx.cloudflare.net" && rtype == "TXT" {
            return Some(
                json!({ "Status": 0, "Answer": [{ "name": name, "type": 16, "TTL": 300,
                "data": txt_data("v=spf1 ip4:104.30.0.0/20 ~all") }] }),
            );
        }
        let under = c
            .zones
            .iter()
            .any(|z| name == z.name || name.ends_with(&format!(".{}", z.name)));
        let fail = c.failing.get(name).or_else(|| {
            c.failing
                .iter()
                .find(|(k, _)| {
                    k.strip_prefix("*.")
                        .is_some_and(|z| name == z || name.ends_with(&format!(".{z}")))
                })
                .map(|(_, v)| v)
        });
        if let Some((r, st)) = fail
            && r.as_deref().is_none_or(|r| r == resolver)
            && !(cd && c.bogus.contains(name))
        {
            return Some(json!({ "Status": st, "Answer": [] }));
        }
        let extra: Vec<&(String, String, String)> = c
            .extra
            .iter()
            .filter(|(n, t, _)| n == name && t == rtype)
            .collect();
        if !under && extra.is_empty() {
            return None;
        }
        let mut answers: Vec<Value> = Vec::new();
        let hidden = c.hidden.contains(&(name.to_owned(), rtype.to_owned()));
        if !hidden {
            if rtype == "NS" {
                if let Some(z) = c.zones.iter().find(|z| z.name == name) {
                    for ns in &z.name_servers {
                        answers.push(json!({ "name": name, "type": 2, "TTL": 300, "data": format!("{ns}.") }));
                    }
                }
            }
            for r in c
                .records
                .iter()
                .filter(|r| r.name == name && r.rtype == rtype)
            {
                let data = match rtype {
                    "TXT" => txt_data(&r.content),
                    "MX" => format!("{} {}.", r.priority.unwrap_or(10), r.content),
                    "CNAME" | "NS" => format!("{}.", r.content.trim_end_matches('.')),
                    _ => r.content.clone(),
                };
                answers
                    .push(json!({ "name": name, "type": code(rtype), "TTL": 300, "data": data }));
            }
        }
        for (_, _, d) in extra {
            let data = if rtype == "TXT" {
                txt_data(d)
            } else {
                d.clone()
            };
            answers.push(json!({ "name": name, "type": code(rtype), "TTL": 300, "data": data }));
        }
        if answers.is_empty() && !hidden {
            // Nothing here: the corpus zones may still answer.
            return None;
        }
        Some(json!({ "Status": 0, "AD": !cd, "Answer": answers }))
    })
}
