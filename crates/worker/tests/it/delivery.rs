//! `it::delivery` (Outbound › Delivery events): transitions, the G8 race and reconciliation, the abuse
//! pauses and first replies.

use serde_json::{Value, json};

use crate::send::{d1w, events_of, open_sender, send_ok, sender, sender_in, submit, wait_status};
use crate::{dkim, eventually, fakes, hook, mailbox_query, patch, platform, slug};

/// A Cloudflare Email Sending event of `kind` (`delivered`, `bounced`, `complained`, …).
pub fn cf_event(
    kind: &str,
    pid: &str,
    sender: &str,
    recipient: &str,
    subject: Option<&str>,
    at: &str,
) -> Value {
    let mut payload = json!({ "eventId": slug(&format!("{kind}{pid}{recipient}{at}")), "messageId": pid, "sender": sender,
        "recipient": recipient, "terminal": true, "subject": subject,
        "delivery": { "smtpStatusCode": if kind == "bounced" { 550 } else { 250 }, "smtpEnhancedStatusCode": if kind == "bounced" { "5.1.1" } else { "2.0.0" } } });
    if kind == "bounced" {
        payload["bounce"] = json!({ "type": "hard", "reason": "550 5.1.1 user unknown" });
    }
    json!({ "type": format!("cf.email.sending.message.{kind}"), "source": { "type": "email.sending" },
            "payload": payload, "metadata": { "eventTimestamp": at } })
}

/// Now as RFC 3339 on the Worker's clock (the fake offset included).
fn now_rfc3339(offset_ms: i64) -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        + offset_ms;
    pylota_identity_core::days::rfc3339(ms)
}

fn provider_id(iid: &str, mid: &str) -> String {
    mailbox_query(
        iid,
        &format!("SELECT provider_message_id FROM messages WHERE id = '{mid}'"),
    )[0]["provider_message_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn g6_hard_soft_complaint_late() {
    let s = open_sender("g6 events", "test");
    let hard = send_ok(&s, &["bounce@simulator.invalid"], "g6 hard");
    let soft = send_ok(&s, &["softbounce@simulator.invalid"], "g6 soft");
    let compl = send_ok(&s, &["complaint@simulator.invalid"], "g6 complaint");
    let m = wait_status(&s, &hard, "bounced");
    assert_eq!(m["deliveries"][0]["bounce_type"], "hard");
    let m = wait_status(&s, &soft, "bounced");
    assert_eq!(m["deliveries"][0]["bounce_type"], "soft");
    wait_status(&s, &compl, "complained");
    let sup = crate::d1(&format!(
        "SELECT address_hint, reason, expires_at FROM suppressions WHERE tenant_id = '{}' ORDER BY reason",
        s.tid
    ));
    let reasons: Vec<&str> = sup
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        ["complaint", "hard_bounce"],
        "a soft bounce never suppresses"
    );
    assert!(
        sup.as_array()
            .unwrap()
            .iter()
            .all(|r| r["expires_at"].is_null())
    );
    // A late bounce after delivered.
    let l = open_sender("g6 late", "live");
    let mid = send_ok(&l, &["late@example.net"], "g6 late");
    wait_status(&l, &mid, "submitted");
    let pid = provider_id(&l.iid, &mid);
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "delivered",
            &pid,
            &l.addr,
            "late@example.net",
            Some("g6 late"),
            &now_rfc3339(0),
        ),
    );
    wait_status(&l, &mid, "delivered");
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "bounced",
            &pid,
            &l.addr,
            "late@example.net",
            Some("g6 late"),
            &now_rfc3339(1000),
        ),
    );
    wait_status(&l, &mid, "bounced");
    assert_eq!(events_of(&l.iid, "message.bounced")[0]["suppressed"], true);
}

#[test]
fn g8_race() {
    let s = open_sender("g8 race", "live");
    let _f = fakes::Reset;
    // An event before the provider ID is stored is retried, then applies.
    fakes::fault_any("queue.send", "error", 1);
    let mid = send_ok(&s, &["race@example.net"], "g8 race");
    let pid = format!(
        "<race-{}@{}>",
        slug("g8"),
        s.addr.rsplit('@').next().unwrap()
    );
    fakes::mail_script("race@example.net", vec![json!({ "messageId": pid })]);
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "delivered",
            &pid,
            &s.addr,
            "race@example.net",
            Some("g8 race"),
            &now_rfc3339(0),
        ),
    );
    std::thread::sleep(std::time::Duration::from_secs(3));
    crate::queue_send(
        "pm-outbound",
        json!({ "kind": "send", "v": 1, "tenant_id": s.tid, "identity_id": s.iid, "message_id": mid, "request_id": "req_g8" }),
    );
    wait_status(&s, &mid, "delivered");
    // An event older than 900 seconds for no message is orphaned (acked, nothing changes).
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "delivered",
            "<nobody@x.example>",
            &s.addr,
            "race@example.net",
            Some("x"),
            &now_rfc3339(-1_000_000),
        ),
    );
    // An event for an uncertain send within the window reconciles it.
    fakes::mail_script("unc@example.net", vec![json!({ "timeout": true })]);
    let u = send_ok(&s, &["unc@example.net"], "g8 uncertain");
    wait_status(&s, &u, "uncertain");
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "delivered",
            "<later@agents.example>",
            &s.addr,
            "unc@example.net",
            Some("g8 uncertain"),
            &now_rfc3339(1000),
        ),
    );
    let m = wait_status(&s, &u, "delivered");
    assert!(
        m["flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "reconciled")
    );
    assert_eq!(
        events_of(&s.iid, "message.reconciled")[0]["status"],
        "delivered"
    );
}

/// `n` outcomes recorded on a tenant's quota object directly (the windows the consumer feeds).
fn outcomes(tid: &str, identities: &[String], domain: &str, n: usize, outcome: &str) {
    let qid = crate::d1(&format!(
        "SELECT quota_do_id FROM tenants WHERE id = '{tid}'"
    ))[0]["quota_do_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    for i in 0..n {
        let r = hook(
            "POST",
            "/__test/rpc",
            Some(json!({ "class": "quota", "object_id": qid,
            "envelope": { "v": 1, "tenant_id": tid, "identity_id": null, "request_id": "req_q", "actor_key_id": null,
                "deadline_ms": 9_999_999_999_999i64,
                "req": { "op": "record_outcome", "identity_id": identities[i % identities.len()], "domain_id": domain,
                         "outcome": outcome, "at": at, "event_id": format!("seed-{tid}-{domain}-{outcome}-{i}"), "counted": true } } })),
        );
        assert_eq!(r.status, 200, "{}", r.body);
    }
}

fn domain_of(addr: &str) -> String {
    crate::d1(&format!(
        "SELECT domain_id FROM addresses WHERE address = '{addr}'"
    ))[0]["domain_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn g12_tenant_domain_pause() {
    // The tenant: 500 outcomes over 30 identities with one complaint pause the tenant, no identity.
    let s = open_sender("g12 tenant", "live");
    let ids: Vec<String> = (0..30).map(|i| format!("idn_g12seed{i:02}")).collect();
    let platform_dom = domain_of(&s.addr);
    outcomes(&s.tid, &ids, &platform_dom, 499, "delivered");
    let mid = send_ok(&s, &["c@example.net"], "g12 complaint");
    wait_status(&s, &mid, "submitted");
    let pid = provider_id(&s.iid, &mid);
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "complained",
            &pid,
            &s.addr,
            "c@example.net",
            None,
            &now_rfc3339(0),
        ),
    );
    eventually(30, "the tenant pause", || {
        let t = crate::d1(&format!(
            "SELECT sending_paused_at FROM tenants WHERE id = '{}'",
            s.tid
        ));
        (!t[0]["sending_paused_at"].is_null()).then_some(())
    });
    let r = crate::send::send(
        &s,
        json!({ "to": ["x@example.net"], "subject": "x", "text": "x" }),
    );
    assert_eq!(r.code(), "sending_paused");
    assert_eq!(r.details()["scope"], "tenant");
    assert_eq!(
        crate::d1(&format!(
            "SELECT status FROM identities WHERE id = '{}'",
            s.iid
        ))[0]["status"],
        "active"
    );
    let audits = crate::d1(&format!(
        "SELECT COUNT(*) AS n FROM audit_log WHERE tenant_id = '{}' AND action = 'tenant.auto_pause'",
        s.tid
    ));
    assert_eq!(audits[0]["n"], 1);
    // Only a platform key resumes.
    let r = patch(
        &format!("/v1/tenants/{}", s.tid),
        &s.key,
        json!({ "sending_paused": false }),
    );
    assert_eq!(r.status, 403, "{}", r.body);
    let r = patch(
        &format!("/v1/tenants/{}", s.tid),
        &platform(),
        json!({ "sending_paused": false }),
    );
    assert_eq!(r.status, 200, "{}", r.body);

    // The domain: the same rates on one tenant domain pause that domain only.
    let d = open_sender("g12 domain", "live");
    let dom = format!("dom_{}", slug("g12 dom").to_ascii_uppercase());
    let name = format!("{}.example", slug("g12"));
    d1w(
        "INSERT INTO domains (id, tenant_id, name, kind, method, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_changed_at, records_json, monitor_do_id, event_subscription_id, created_at, updated_at) VALUES (?1, ?2, ?3, 'external', 'dns_records', 1, 'literal', 'ses', 'cloudflare', 'subaddress', 1, 1, 'healthy', 0, '[]', '', 'sub', 0, 0)",
        json!([dom, d.tid, name]),
    );
    let addr = format!("ops@{name}");
    d1w(
        "INSERT INTO addresses (id, address, local_part, domain_id, tenant_id, identity_id, role, status, created_at, updated_at) VALUES (?1, ?2, 'ops', ?3, ?4, ?5, 'alias', 'active', 0, 0)",
        json!([
            format!("adr_{}", slug("g12 adr").to_ascii_uppercase()),
            addr,
            dom,
            d.tid,
            d.iid
        ]),
    );
    let ids = vec![d.iid.clone()];
    outcomes(&d.tid, &ids, &dom, 499, "delivered");
    outcomes(&d.tid, &ids, &domain_of(&d.addr), 1000, "delivered");
    let r = crate::send::send(
        &d,
        json!({ "to": ["c@example.net"], "subject": "g12 d", "text": "x", "from_address": addr }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let mid = r.body["id"].as_str().unwrap().to_owned();
    wait_status(&d, &mid, "submitted");
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "complained",
            &provider_id(&d.iid, &mid),
            &addr,
            "c@example.net",
            None,
            &now_rfc3339(0),
        ),
    );
    eventually(30, "the domain pause", || {
        let t = crate::d1(&format!(
            "SELECT sending_paused_at FROM domains WHERE id = '{dom}'"
        ));
        (!t[0]["sending_paused_at"].is_null()).then_some(())
    });
    assert!(
        crate::d1(&format!(
            "SELECT sending_paused_at FROM tenants WHERE id = '{}'",
            d.tid
        ))[0]["sending_paused_at"]
            .is_null()
    );
    let r = crate::send::send(
        &d,
        json!({ "to": ["c@example.net"], "subject": "x", "text": "x", "from_address": addr }),
    );
    assert_eq!(r.code(), "sending_paused", "{}", r.body);
    assert_eq!(r.details()["scope"], "domain");
    // The platform domain is never paused.
    assert!(
        crate::d1(&format!(
            "SELECT sending_paused_at FROM domains WHERE id = '{}'",
            domain_of(&d.addr)
        ))[0]["sending_paused_at"]
            .is_null()
    );
}

#[test]
fn g13_first_reply_outcomes_not_counted() {
    let s = sender("g13 first", "live", json!({}));
    let mut replies = Vec::new();
    for i in 0..11 {
        let from = format!("stranger{i}@brightwell.example");
        let raw = dkim::sign(&crate::mail(
            &from,
            &s.addr,
            &format!("Hello {i} {}", slug("g13")),
            "Please reply",
        ));
        crate::inbound(&from, &s.addr, &raw);
        let mid = eventually(30, "the inbound", || {
            crate::messages_of(&s.iid, &s.key, "&direction=inbound")
                .into_iter()
                .find(|m| m["from"]["address"] == from.as_str())
                .map(|m| m["id"].as_str().unwrap().to_owned())
        });
        let r = submit(
            &format!("/v1/identities/{}/messages/{mid}/reply", s.iid),
            &s.key,
            &format!("g13-{i}"),
            json!({ "text": "Thanks" }),
        );
        assert_eq!(r.status, 202, "{}", r.body);
        assert_eq!(r.body["status"], "queued", "rule 3 makes the sender known");
        replies.push((from, r.body["id"].as_str().unwrap().to_owned()));
    }
    let first = mailbox_query(
        &s.iid,
        "SELECT COUNT(*) AS n FROM deliveries WHERE first_reply = 1",
    );
    assert_eq!(first[0]["n"], 11);
    for (to, mid) in &replies {
        wait_status(&s, mid, "submitted");
        crate::queue_send(
            "pm-delivery-events",
            cf_event(
                "bounced",
                &provider_id(&s.iid, mid),
                &s.addr,
                to,
                Some("Re"),
                &now_rfc3339(0),
            ),
        );
    }
    for (_, mid) in &replies {
        wait_status(&s, mid, "bounced");
    }
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert_eq!(
        crate::d1(&format!(
            "SELECT status FROM identities WHERE id = '{}'",
            s.iid
        ))[0]["status"],
        "active"
    );
    let audits = crate::d1(&format!(
        "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'identity.auto_pause' AND target_id = '{}'",
        s.iid
    ));
    assert_eq!(audits[0]["n"], 0);
    let sup = crate::d1(&format!(
        "SELECT COUNT(*) AS n FROM suppressions WHERE tenant_id = '{}'",
        s.tid
    ));
    assert_eq!(sup[0]["n"], 11, "bounces still suppress");
    // A second message to one of them, now a contact, is not a first reply.
    let o = sender_in(
        &s.tid,
        "g13 other",
        json!({ "require_known_recipient": false }),
    );
    let _ = o;
    let r = crate::send::send(
        &s,
        json!({ "to": ["stranger0@brightwell.example"], "subject": "again", "text": "x" }),
    );
    assert_eq!(r.status, 202);
    let n = mailbox_query(
        &s.iid,
        "SELECT COUNT(*) AS n FROM deliveries WHERE first_reply = 1",
    );
    assert_eq!(n[0]["n"], 11);
}

#[test]
fn abuse_auto_pause() {
    let s = open_sender("abuse pause", "live");
    let ids = vec![s.iid.clone()];
    let qid = crate::d1(&format!(
        "SELECT quota_do_id FROM tenants WHERE id = '{}'",
        s.tid
    ))[0]["quota_do_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let _ = qid;
    // Ten bounces, one of them delivered twice: no pause.
    outcomes(&s.tid, &ids, "dom_none", 10, "bounced");
    outcomes(&s.tid, &ids, "dom_none", 10, "bounced");
    let mid = send_ok(&s, &["b@example.net"], "abuse");
    wait_status(&s, &mid, "submitted");
    crate::queue_send(
        "pm-delivery-events",
        cf_event(
            "bounced",
            &provider_id(&s.iid, &mid),
            &s.addr,
            "b@example.net",
            Some("abuse"),
            &now_rfc3339(0),
        ),
    );
    eventually(30, "the identity pause", || {
        (crate::d1(&format!(
            "SELECT status FROM identities WHERE id = '{}'",
            s.iid
        ))[0]["status"]
            == "paused")
            .then_some(())
    });
    assert_eq!(
        crate::d1(&format!(
            "SELECT pause_reason FROM identities WHERE id = '{}'",
            s.iid
        ))[0]["pause_reason"],
        "abuse_threshold"
    );
}
