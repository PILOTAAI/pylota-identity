//! Addresses on tenant domains through workerd (Identities and domains › Addresses): a newer pending
//! address replaces the older (A11), promotion, retirement and rollback (FR-ADR-2–4), the platform
//! address kept as the fallback (A14), the retirement cron (FR-ADR-3) and replies from a retiring
//! address (C3).

use serde_json::{Value, json};

use crate::domains::{add, dmarc, list_zones, until, zone_name};
use crate::send::{Sender, open_sender};
use crate::{cloud, fakes, get, platform, post};

/// An open sender and a healthy apex `cloudflare_zone` domain in its tenant: `(sender, domain id, name)`.
pub fn on_domain(tag: &str) -> (Sender, String, String) {
    let s = open_sender(tag, "live");
    let zone = zone_name(tag);
    cloud::zone(&zone);
    list_zones(&s.tid, &[&zone]);
    dmarc(&zone, &zone);
    let r = add(
        &platform(),
        &s.tid,
        json!({ "name": zone, "method": "cloudflare_zone" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let id = r.body["id"].as_str().unwrap().to_owned();
    until(&platform(), &id, "healthy", 6);
    (s, id, zone)
}

pub fn create(s: &Sender, local: &str, domain_id: &str) -> crate::Resp {
    post(
        &format!("/v1/identities/{}/addresses", s.iid),
        &s.key,
        json!({ "local_part": local, "domain_id": domain_id }),
    )
}

pub fn addresses(s: &Sender) -> Vec<Value> {
    let r = get(&format!("/v1/identities/{}/addresses", s.iid), &s.key);
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["data"].as_array().cloned().unwrap_or_default()
}

pub fn by_address(s: &Sender, address: &str) -> Value {
    addresses(s)
        .into_iter()
        .find(|a| a["address"] == address)
        .unwrap_or(Value::Null)
}

pub fn promote(s: &Sender, id: &str, body: Value) -> crate::Resp {
    post(
        &format!("/v1/identities/{}/addresses/{id}/promote", s.iid),
        &s.key,
        body,
    )
}

fn retire(s: &Sender, id: &str, days: i64) -> crate::Resp {
    post(
        &format!("/v1/identities/{}/addresses/{id}/retire", s.iid),
        &s.key,
        json!({ "after_days": days }),
    )
}

/// A11: a second pending address on a domain replaces the first.
#[test]
fn a11_newer_pending_replaces() {
    let _reset = fakes::Reset;
    let s = open_sender("a11", "live");
    let zone = zone_name("a11");
    cloud::zone(&zone);
    let r = add(
        &platform(),
        &s.tid,
        json!({ "name": zone, "method": "cloudflare_zone" }),
    );
    let did = r.body["id"].as_str().unwrap().to_owned();
    let first = create(&s, "first", &did);
    assert_eq!(first.status, 201, "{}", first.body);
    assert_eq!(first.body["status"], "pending");
    let second = create(&s, "second", &did);
    assert_eq!(second.status, 201, "{}", second.body);
    let pending: Vec<Value> = addresses(&s)
        .into_iter()
        .filter(|a| a["status"] == "pending")
        .collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["address"], format!("second@{zone}"));
}

/// FR-ADR-2–4: promote, retire after a grace period, roll back by promoting the retiring address.
#[test]
fn promote_retire_rollback() {
    let _reset = fakes::Reset;
    let (s, did, zone) = on_domain("promote");
    let x = create(&s, "bookings", &did);
    assert_eq!(
        (x.status, x.body["status"].as_str()),
        (201, Some("active")),
        "{}",
        x.body
    );
    let xid = x.body["id"].as_str().unwrap().to_owned();
    let r = promote(&s, &xid, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["primary_address"], format!("bookings@{zone}"));
    let y = create(&s, "reservations", &did);
    let yid = y.body["id"].as_str().unwrap().to_owned();
    let r = promote(&s, &yid, json!({ "retire_previous_after_days": 10 }));
    assert_eq!(r.status, 200, "{}", r.body);
    let xr = by_address(&s, &format!("bookings@{zone}"));
    assert_eq!(
        (xr["status"].as_str(), xr["role"].as_str()),
        (Some("retiring"), Some("alias"))
    );
    assert!(xr["retire_at"].is_string());
    // Rollback: the retiring address is primary again, the other an active alias.
    let r = promote(&s, &xid, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    let xr = by_address(&s, &format!("bookings@{zone}"));
    let yr = by_address(&s, &format!("reservations@{zone}"));
    assert_eq!(
        (xr["role"].as_str(), xr["retire_at"].clone()),
        (Some("primary"), Value::Null)
    );
    assert_eq!(
        (yr["role"].as_str(), yr["status"].as_str()),
        (Some("alias"), Some("active"))
    );
    // Retire the alias now.
    let r = retire(&s, &yid, 0);
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (200, Some("retired")),
        "{}",
        r.body
    );
    let types: Vec<String> = crate::send::events(&s.iid)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    for t in [
        "identity.address_added",
        "identity.address_promoted",
        "identity.address_retired",
    ] {
        assert!(types.iter().any(|x| x == t), "{t}: {types:?}");
    }
}

/// A14: the platform address stays an active alias, cannot be retired or deleted, and promoting it
/// again rolls back.
#[test]
fn a14_platform_address_kept() {
    let _reset = fakes::Reset;
    let (s, did, zone) = on_domain("a14");
    let platform_addr = s.addr.clone();
    let x = create(&s, "desk", &did);
    let xid = x.body["id"].as_str().unwrap().to_owned();
    assert_eq!(promote(&s, &xid, json!({})).status, 200);
    let pa = by_address(&s, &platform_addr);
    assert_eq!(
        (pa["role"].as_str(), pa["status"].as_str()),
        (Some("alias"), Some("active"))
    );
    let pid = pa["id"].as_str().unwrap().to_owned();
    let r = retire(&s, &pid, 0);
    assert_eq!((r.status, r.code()), (409, "address_in_use"));
    let r = crate::delete(&format!("/v1/identities/{}/addresses/{pid}", s.iid), &s.key);
    assert_eq!((r.status, r.code()), (409, "address_in_use"));
    let r = retire(&s, &xid, 0);
    assert_eq!((r.status, r.code()), (409, "address_is_primary"));
    let r = promote(&s, &pid, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    let xr = by_address(&s, &format!("desk@{zone}"));
    assert_eq!(
        (
            xr["role"].as_str(),
            xr["status"].as_str(),
            xr["retire_at"].clone()
        ),
        (Some("alias"), Some("active"), Value::Null)
    );
    assert_eq!(by_address(&s, &platform_addr)["role"], "primary");
}

/// FR-ADR-3: `retire_at` reached → `retired` by the every-minute cron, with the event; inbound mail
/// to it gets `550 5.1.6`.
#[test]
fn retirement_cron() {
    let _reset = fakes::Reset;
    let (s, did, zone) = on_domain("retire cron");
    let x = create(&s, "old", &did);
    let xid = x.body["id"].as_str().unwrap().to_owned();
    let r = retire(&s, &xid, 1);
    assert_eq!(r.body["status"], "retiring");
    fakes::advance(2 * 86_400_000);
    crate::cron("* * * * *");
    let xr = by_address(&s, &format!("old@{zone}"));
    assert_eq!(xr["status"], "retired", "{xr}");
    assert!(crate::send::events_of(&s.iid, "identity.address_retired").len() == 1);
    fakes::advance(120_000);
    let to = format!("old@{zone}");
    let (outcome, smtp) = crate::inbound(
        "a@b.example",
        &to,
        &crate::mail("a@b.example", &to, "Late", "x"),
    );
    assert_eq!(outcome, "rejected", "{smtp}");
    assert!(smtp.starts_with("550 5.1.6"), "{smtp}");
}

/// C3: a reply on a thread where the counterparty wrote to a retiring address is sent from it.
#[test]
fn c3_reply_from_retiring() {
    let _reset = fakes::Reset;
    let (s, did, zone) = on_domain("c3");
    let x = create(&s, "old", &did);
    let xid = x.body["id"].as_str().unwrap().to_owned();
    assert_eq!(promote(&s, &xid, json!({})).status, 200);
    let old = format!("old@{zone}");
    let (outcome, _) = crate::inbound(
        "jo@example.net",
        &old,
        &crate::mail("jo@example.net", &old, "C3 thread", "Hello"),
    );
    assert_eq!(outcome, "accepted");
    let inbound = crate::eventually(30, "the message", || {
        crate::messages_of(&s.iid, &s.key, "")
            .into_iter()
            .find(|m| m["subject"] == "C3 thread")
    });
    let y = create(&s, "new", &did);
    let yid = y.body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        promote(&s, &yid, json!({ "retire_previous_after_days": 30 })).status,
        200
    );
    assert_eq!(by_address(&s, &old)["status"], "retiring");
    let mid = inbound["id"].as_str().unwrap();
    let r = crate::send::submit(
        &format!("/v1/identities/{}/messages/{mid}/reply", s.iid),
        &s.key,
        &crate::slug("c3 reply"),
        json!({ "text": "Thanks" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["from"]["address"], old);
    // A new thread sends from the primary.
    let n = crate::send::send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "C3 new", "text": "Hi" }),
    );
    assert_eq!(n.body["from"]["address"], format!("new@{zone}"));
}
