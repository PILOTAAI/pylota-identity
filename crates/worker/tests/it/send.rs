//! `it::send` (Outbound › Tests): the submit path, policy, composition, transports and outcomes.

use serde_json::{Value, json};

use crate::{
    call, eventually, fakes, get, hook, identity, mailbox_query, mint, patch, platform, post, slug,
};

/// A sending identity: tenant, identity, its address and a tenant key.
pub struct Sender {
    pub tid: String,
    pub iid: String,
    pub addr: String,
    pub key: String,
}

/// A tenant in `mode` with one identity (owner set, `send_policy` as given) and a tenant key.
pub fn sender(name: &str, mode: &str, send_policy: Value) -> Sender {
    let p = platform();
    let r = post(
        "/v1/tenants",
        &p,
        json!({ "slug": slug(name), "name": name, "mode": mode }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let tid = r.body["id"].as_str().unwrap().to_owned();
    sender_in(&tid, name, send_policy)
}

/// One more sending identity in `tid`.
pub fn sender_in(tid: &str, name: &str, send_policy: Value) -> Sender {
    let p = platform();
    let r = post(
        &format!("/v1/tenants/{tid}/identities"),
        &p,
        json!({ "username": &slug(name)[..10], "display_name": "Acme Bookings",
                "owner": { "name": "Ana Owner", "email": "ana@owner.example" }, "send_policy": send_policy }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let (_, key) = mint(
        &p,
        json!({ "name": name, "level": "tenant", "tenant_id": tid, "permissions": [
            "messages:read", "messages:write", "messages:send", "quarantine:review", "attachments:read",
            "identities:read", "identities:write", "suppressions:manage", "policy:write"
        ] }),
    );
    Sender {
        tid: tid.to_owned(),
        iid: r.body["id"].as_str().unwrap().to_owned(),
        addr: r.body["primary_address"].as_str().unwrap().to_owned(),
        key,
    }
}

/// A sender that may write to anyone (`require_known_recipient: false`).
pub fn open_sender(name: &str, mode: &str) -> Sender {
    sender(name, mode, json!({ "require_known_recipient": false }))
}

/// `POST {path}` with an `Idempotency-Key`.
pub fn submit(path: &str, key: &str, idem: &str, body: Value) -> crate::Resp {
    call(
        "POST",
        path,
        Some(key),
        Some(body),
        &[("idempotency-key", idem)],
    )
}

/// A send with a fresh key.
pub fn send(s: &Sender, body: Value) -> crate::Resp {
    let idem = slug(&format!("{}{}", body, crate::slug("k")));
    submit(
        &format!("/v1/identities/{}/messages", s.iid),
        &s.key,
        &idem,
        body,
    )
}

/// A plain send to `to`, which must be accepted; answers the message ID.
pub fn send_ok(s: &Sender, to: &[&str], subject: &str) -> String {
    let r = send(s, json!({ "to": to, "subject": subject, "text": "Hello" }));
    assert_eq!(r.status, 202, "{}", r.body);
    r.body["id"].as_str().unwrap().to_owned()
}

pub fn message(s: &Sender, mid: &str) -> Value {
    let r = get(&format!("/v1/identities/{}/messages/{mid}", s.iid), &s.key);
    assert_eq!(r.status, 200, "{}", r.body);
    r.body
}

/// Waits until the message has `status`.
pub fn wait_status(s: &Sender, mid: &str, status: &str) -> Value {
    eventually(40, &format!("{mid} {status}"), || {
        let m = message(s, mid);
        (m["status"] == status).then_some(m)
    })
}

/// The events of an identity's outbox: `(type, data)`, oldest first.
pub fn events(iid: &str) -> Vec<(String, Value)> {
    mailbox_query(iid, "SELECT type, payload_json FROM outbox ORDER BY seq")
        .iter()
        .map(|r| {
            let v: Value = serde_json::from_str(r["payload_json"].as_str().unwrap()).unwrap();
            (r["type"].as_str().unwrap().to_owned(), v["data"].clone())
        })
        .collect()
}

pub fn events_of(iid: &str, t: &str) -> Vec<Value> {
    events(iid)
        .into_iter()
        .filter(|(x, _)| x == t)
        .map(|(_, d)| d)
        .collect()
}

fn delivery(m: &Value, address: &str) -> Value {
    m["deliveries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["address"] == address)
        .cloned()
        .unwrap_or(Value::Null)
}

/// Runs one D1 statement through `/__test/d1`.
pub fn d1w(sql: &str, params: Value) -> Vec<Value> {
    let r = hook(
        "POST",
        "/__test/d1",
        Some(json!({ "sql": sql, "params": params })),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["rows"].as_array().cloned().unwrap_or_default()
}

/// Stores an inbound message from `from` to the sender and waits for it; answers its ID.
pub fn receive(s: &Sender, from: &str, subject: &str) -> String {
    let mut raw = crate::mail(from, &s.addr, subject, "Can you confirm?");
    if from.ends_with("@brightwell.example") {
        raw = crate::dkim::sign(&raw);
    }
    crate::inbound(from, &s.addr, &raw);
    eventually(30, "the inbound message", || {
        crate::messages_of(&s.iid, &s.key, "&direction=inbound")
            .into_iter()
            .find(|m| m["subject"] == subject)
            .map(|m| m["id"].as_str().unwrap().to_owned())
    })
}

#[test]
fn g1_same_key_same_body() {
    let s = open_sender("g1 same", "live");
    let path = format!("/v1/identities/{}/messages", s.iid);
    let body = json!({ "to": ["jo@example.net"], "subject": "BK-1", "text": "Hi" });
    let a = submit(&path, &s.key, "bk-2291-confirm", body.clone());
    assert_eq!(a.status, 202, "{}", a.body);
    assert_eq!(a.body["deduplicated"], false);
    // Key order and whitespace do not change the fingerprint.
    let b = submit(
        &path,
        &s.key,
        "bk-2291-confirm",
        json!({ "text": "Hi", "subject": "BK-1", "to": ["jo@example.net"] }),
    );
    assert_eq!(b.status, 202, "{}", b.body);
    assert_eq!(b.body["deduplicated"], true);
    assert_eq!(b.header("idempotent-replayed"), Some("true"));
    assert_eq!(b.body["id"], a.body["id"]);
    wait_status(&s, a.body["id"].as_str().unwrap(), "submitted");
    assert_eq!(
        fakes::mail_sent("jo@example.net")
            .iter()
            .filter(|m| m["subject"] == "BK-1")
            .count(),
        1
    );
}

#[test]
fn g1_same_key_different_body() {
    let s = open_sender("g1 diff", "live");
    let path = format!("/v1/identities/{}/messages", s.iid);
    let a = submit(
        &path,
        &s.key,
        "k-diff",
        json!({ "to": ["jo@example.net"], "subject": "A", "text": "x" }),
    );
    assert_eq!(a.status, 202, "{}", a.body);
    let b = submit(
        &path,
        &s.key,
        "k-diff",
        json!({ "to": ["jo@example.net"], "subject": "B", "text": "x" }),
    );
    assert_eq!(b.status, 409, "{}", b.body);
    assert_eq!(b.code(), "idempotency_conflict");
    assert_eq!(b.details()["original_message_id"], a.body["id"]);
}

#[test]
fn g1_in_flight() {
    // The first request waits on a held thread lock, so the same key arrives while it runs.
    let s = open_sender("g1 flight", "live");
    let m0 = receive(&s, "jo@example.net", "Flight");
    let _f = fakes::Reset;
    fakes::fault("queue.send", "Q_OUTBOUND", "error", 1);
    let first = submit(
        &format!("/v1/identities/{}/messages/{m0}/reply", s.iid),
        &s.key,
        "hold-lock",
        json!({ "text": "one" }),
    );
    assert_eq!(first.status, 202, "{}", first.body);
    let path = format!("/v1/identities/{}/messages/{m0}/reply", s.iid);
    let key = s.key.clone();
    let t = std::thread::spawn(move || submit(&path, &key, "same-key", json!({ "text": "two" })));
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let second = submit(
        &format!("/v1/identities/{}/messages/{m0}/reply", s.iid),
        &s.key,
        "same-key",
        json!({ "text": "two" }),
    );
    assert_eq!(second.status, 409, "{}", second.body);
    assert_eq!(second.code(), "request_in_progress");
    let waited = t.join().unwrap();
    assert_eq!(waited.code(), "thread_busy", "{}", waited.body);
}

#[test]
fn c4_thread_lock() {
    let s = open_sender("c4 lock", "live");
    let m0 = receive(&s, "jo@example.net", "Lock");
    let _f = fakes::Reset;
    fakes::fault("queue.send", "Q_OUTBOUND", "error", 1);
    let path = format!("/v1/identities/{}/messages/{m0}/reply", s.iid);
    let a = submit(&path, &s.key, "c4-a", json!({ "text": "first" }));
    assert_eq!(a.status, 202, "{}", a.body);
    let started = std::time::Instant::now();
    let b = submit(&path, &s.key, "c4-b", json!({ "text": "second" }));
    assert_eq!(b.status, 409, "{}", b.body);
    assert_eq!(b.code(), "thread_busy");
    assert!(started.elapsed().as_secs() >= 9, "the second waited");
    let ra = b.details()["retry_after"].as_i64().unwrap();
    assert!((1..=120).contains(&ra), "{ra}");
    // Nothing of the refused send was kept.
    assert_eq!(
        crate::messages_of(&s.iid, &s.key, "&direction=outbound").len(),
        1
    );
}

#[test]
fn cancel_queued() {
    let s = open_sender("cancel q", "live");
    let _f = fakes::Reset;
    fakes::fault("queue.send", "Q_OUTBOUND", "error", 1);
    let mid = send_ok(&s, &["jo@example.net"], "Cancel me");
    let r = post(
        &format!("/v1/identities/{}/messages/{mid}/cancel", s.iid),
        &s.key,
        json!({}),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["status"], "canceled");
    assert_eq!(events_of(&s.iid, "message.canceled").len(), 1);
    // The later queue message acks without sending.
    crate::queue_send(
        "pm-outbound",
        json!({ "kind": "send", "v": 1, "tenant_id": s.tid, "identity_id": s.iid, "message_id": mid, "request_id": "req_x" }),
    );
    std::thread::sleep(std::time::Duration::from_secs(6));
    assert!(
        fakes::mail_sent("jo@example.net")
            .iter()
            .all(|m| m["subject"] != "Cancel me")
    );
    // After transport: not cancelable.
    let sent = send_ok(&s, &["jo@example.net"], "Too late");
    wait_status(&s, &sent, "submitted");
    let r = post(
        &format!("/v1/identities/{}/messages/{sent}/cancel", s.iid),
        &s.key,
        json!({}),
    );
    assert_eq!(r.code(), "not_cancelable");
    // A suspended partner's queued message is cancelled by the consumer (J13).
    let (pid, pkey) = crate::partner(
        &slug("cancel partner"),
        &["tenants:manage", "identities:write", "keys:manage"],
    );
    let t = crate::tenant(&pkey, "cancel partner tenant");
    let ps = sender_in(
        t["id"].as_str().unwrap(),
        "cancel ps",
        json!({ "require_known_recipient": false }),
    );
    fakes::fault("queue.send", "Q_OUTBOUND", "error", 1);
    let pm = send_ok(&ps, &["jo@example.net"], "Partner held");
    assert_eq!(
        patch(
            &format!("/v1/partners/{pid}"),
            &platform(),
            json!({ "status": "suspended" })
        )
        .status,
        200
    );
    crate::queue_send(
        "pm-outbound",
        json!({ "kind": "send", "v": 1, "tenant_id": ps.tid, "identity_id": ps.iid, "message_id": pm, "request_id": "req_y" }),
    );
    // The partner's keys are refused while it is suspended: read the mailbox directly.
    let m = eventually(40, "the system cancel", || {
        let r = mailbox_query(
            &ps.iid,
            &format!("SELECT status FROM messages WHERE id = '{pm}'"),
        );
        (r[0]["status"] == "canceled").then(|| r[0].clone())
    });
    assert_eq!(m["status"], "canceled");
    let _ = patch(
        &format!("/v1/partners/{pid}"),
        &platform(),
        json!({ "status": "active" }),
    );
}

#[test]
fn a7_paused_refuses_send() {
    let s = open_sender("a7 paused", "live");
    let r = patch(
        &format!("/v1/identities/{}", s.iid),
        &s.key,
        json!({ "status": "paused" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "x", "text": "x" }),
    );
    assert_eq!(r.code(), "identity_paused", "{}", r.body);
    assert_eq!(r.details()["reason"], "manual");
}

#[test]
fn a8_owner_required() {
    let p = platform();
    let t = crate::tenant(&p, "a8 owner");
    let tid = t["id"].as_str().unwrap();
    let idn = identity(&p, tid, &slug("a8 owner")[..8]);
    let r = call(
        "POST",
        &format!("/v1/identities/{}/messages", idn["id"].as_str().unwrap()),
        Some(&p),
        Some(json!({ "to": ["jo@example.net"], "subject": "x", "text": "x" })),
        &[("idempotency-key", "a8")],
    );
    assert_eq!(r.status, 409, "{}", r.body);
    assert_eq!(r.code(), "identity_owner_required");
}

#[test]
fn a10_reply_all_excludes_bcc() {
    let s = open_sender("a10 bcc", "live");
    let raw = format!(
        "From: Jo <jo@example.net>\r\nTo: kim@example.net, {}\r\nCc: lee@example.net\r\nSubject: Team\r\nMessage-ID: <a10@it.example>\r\nDate: Mon, 26 Oct 2026 10:00:00 +0000\r\n\r\nHi all\r\n",
        s.addr
    );
    crate::inbound("jo@example.net", &s.addr, &raw);
    let mid = eventually(30, "inbound", || {
        crate::messages_of(&s.iid, &s.key, "&direction=inbound")
            .first()
            .map(|m| m["id"].as_str().unwrap().to_owned())
    });
    let r = submit(
        &format!("/v1/identities/{}/messages/{mid}/reply-all", s.iid),
        &s.key,
        "a10",
        json!({ "text": "Thanks" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let to: Vec<&str> = r.body["to"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["address"].as_str().unwrap())
        .collect();
    let cc: Vec<&str> = r.body["cc"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["address"].as_str().unwrap())
        .collect();
    assert_eq!(to, ["jo@example.net", "kim@example.net"]);
    assert_eq!(cc, ["lee@example.net"]);
    assert!(r.body["bcc"].as_array().unwrap().is_empty());
    assert!(!to.contains(&s.addr.as_str()));
}

#[test]
fn d6_exchange_cap() {
    let s = open_sender("d6 cap", "live");
    let mut last = None;
    for i in 0..3 {
        let m = receive(&s, "jo@example.net", &format!("Auto {i}"));
        // Every inbound joins one thread only through its own subject; reply into one thread instead.
        let target = last.clone().unwrap_or(m);
        let r = submit(
            &format!("/v1/identities/{}/messages/{target}/reply", s.iid),
            &s.key,
            &format!("d6-{i}"),
            json!({ "text": "Out of office", "kind": "auto_reply" }),
        );
        if i < 2 {
            assert_eq!(r.status, 202, "{}", r.body);
            last = Some(target);
        } else {
            assert_eq!(r.code(), "auto_reply_not_allowed", "{}", r.body);
        }
    }
}

#[test]
fn e2_require_known_recipient() {
    let s = sender("e2 known", "live", json!({}));
    let m0 = receive(&s, "jo@example.net", "Known?");
    // A first send to an address the identity only received from is suppressed, and stays so.
    for i in 0..2 {
        let r = send(
            &s,
            json!({ "to": ["jo@example.net"], "subject": format!("e2 {i}"), "text": "x" }),
        );
        assert_eq!(r.status, 202, "{}", r.body);
        assert_eq!(r.body["status"], "suppressed");
        assert_eq!(delivery(&r.body, "jo@example.net")["status"], "suppressed");
    }
    let sp = mailbox_query(&s.iid, "SELECT smtp_response FROM deliveries");
    assert!(
        sp.iter()
            .all(|r| r["smtp_response"] == "policy: unknown_recipient")
    );
    // A reply to a passing message reaches its reply target only when it authenticated; the itest
    // mail does not pass, so the reply is suppressed; with the setting off every recipient passes.
    let r = submit(
        &format!("/v1/identities/{}/messages/{m0}/reply", s.iid),
        &s.key,
        "e2-reply",
        json!({ "text": "Yes" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["status"], "suppressed");
    // A send-allow entry makes it known.
    let r = call(
        "PUT",
        &format!("/v1/tenants/{}/lists/send/allow/jo%40example.net", s.tid),
        Some(&s.key),
        Some(json!({})),
        &[],
    );
    assert!(r.status == 201 || r.status == 200, "{}", r.body);
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "e2 allowed", "text": "x" }),
    );
    assert_eq!(r.body["status"], "queued", "{}", r.body);
    // An earlier non-suppressed send makes a recipient known.
    let o = sender_in(
        &s.tid,
        "e2 open",
        json!({ "require_known_recipient": false }),
    );
    assert_eq!(
        send(
            &o,
            json!({ "to": ["kim@example.net"], "subject": "x", "text": "x" })
        )
        .body["status"],
        "queued"
    );
    // The system identity is created with false.
    let sys = crate::d1("SELECT send_policy_json FROM identities WHERE is_system = 1");
    assert!(
        sys[0]["send_policy_json"]
            .as_str()
            .unwrap()
            .contains("\"require_known_recipient\":false")
    );
}

#[test]
fn list_filters() {
    let s = open_sender("list filters", "live");
    let put = |path: &str| call("PUT", path, Some(&s.key), Some(json!({})), &[]).status;
    assert_eq!(
        put(&format!(
            "/v1/tenants/{}/lists/send/block/blocked%40example.net",
            s.tid
        )),
        201
    );
    let r = send(
        &s,
        json!({ "to": ["blocked@example.net", "jo@example.net"], "subject": "lf", "text": "x" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let rows = mailbox_query(
        &s.iid,
        "SELECT address, status, smtp_response FROM deliveries WHERE address = 'blocked@example.net'",
    );
    assert_eq!(rows[0]["smtp_response"], "policy: send_block");
    // A dry run reports the rule.
    let d = call(
        "POST",
        &format!("/v1/identities/{}/messages?dry_run=true", s.iid),
        Some(&s.key),
        Some(json!({ "to": ["blocked@example.net"], "subject": "lf", "text": "x" })),
        &[],
    );
    assert_eq!(d.code(), "recipient_blocked", "{}", d.body);
    assert_eq!(d.details()["scope"], "tenant");
    // send_allowlist_only without an allow entry; an @domain allow lets it through.
    assert_eq!(
        patch(
            &format!("/v1/tenants/{}/policy", s.tid),
            &s.key,
            json!({ "send_allowlist_only": true })
        )
        .status,
        200
    );
    assert_eq!(
        put(&format!(
            "/v1/tenants/{}/lists/send/allow/%40friends.example",
            s.tid
        )),
        201
    );
    let d = call(
        "POST",
        &format!("/v1/identities/{}/messages?dry_run=true", s.iid),
        Some(&s.key),
        Some(
            json!({ "to": ["jo@example.net", "kim@friends.example"], "subject": "lf", "text": "x" }),
        ),
        &[],
    );
    assert_eq!(d.status, 200, "{}", d.body);
    assert_eq!(d.body["recipients"][1]["status"], "queued");
    assert_eq!(d.body["recipients"][0]["reason"], "not_on_allowlist");
    let r = send(
        &s,
        json!({ "to": ["kim@friends.example", "jo@example.net"], "subject": "lf2", "text": "x" }),
    );
    assert_eq!(delivery(&r.body, "kim@friends.example")["status"], "queued");
    assert_eq!(delivery(&r.body, "jo@example.net")["status"], "suppressed");
}

#[test]
fn e3_caps() {
    let s = open_sender("e3 caps", "live");
    let r = send(
        &s,
        json!({ "to": (0..11).map(|i| format!("r{i}@example.net")).collect::<Vec<_>>(), "subject": "x", "text": "x" }),
    );
    assert_eq!(r.code(), "too_many_recipients", "{}", r.body);
    assert_eq!(
        patch(
            &format!("/v1/identities/{}", s.iid),
            &platform(),
            json!({ "send_policy": { "daily_cap": 2, "require_known_recipient": false } })
        )
        .status,
        200
    );
    for i in 0..2 {
        send_ok(&s, &["jo@example.net"], &format!("cap {i}"));
    }
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "cap 3", "text": "x" }),
    );
    assert_eq!(r.status, 429, "{}", r.body);
    assert_eq!(r.details()["cap"], "identity");
    assert!(r.details()["resets_at"].is_string());
    assert_eq!(events_of(&s.iid, "quota.warning").len(), 2, "80% and 100%");
    // The tenant cap.
    assert_eq!(
        patch(
            &format!("/v1/tenants/{}/policy", s.tid),
            &platform(),
            json!({ "tenant_daily_send_cap": 3 })
        )
        .status,
        200
    );
    let o = sender_in(
        &s.tid,
        "e3 other",
        json!({ "require_known_recipient": false }),
    );
    send_ok(&o, &["jo@example.net"], "tenant 3");
    let r = send(
        &o,
        json!({ "to": ["jo@example.net"], "subject": "tenant 4", "text": "x" }),
    );
    assert_eq!(r.details()["cap"], "tenant", "{}", r.body);
}

#[test]
fn e3_timezone_change_caps() {
    let s = open_sender("e3 tz", "live");
    let _f = fakes::Reset;
    assert_eq!(
        patch(
            &format!("/v1/tenants/{}", s.tid),
            &platform(),
            json!({ "timezone": "Europe/London" })
        )
        .status,
        200
    );
    assert_eq!(
        patch(
            &format!("/v1/tenants/{}/policy", s.tid),
            &platform(),
            json!({ "tenant_daily_send_cap": 5 })
        )
        .status,
        200
    );
    let mut sent = Vec::new();
    for i in 0..4 {
        sent.push(send_ok(&s, &["jo@example.net"], &format!("tz {i}")));
    }
    assert_eq!(
        patch(
            &format!("/v1/tenants/{}", s.tid),
            &platform(),
            json!({ "timezone": "Pacific/Auckland" })
        )
        .status,
        200
    );
    // The fifth still counts on the old day; the sixth is refused.
    sent.push(send_ok(&s, &["jo@example.net"], "tz 4"));
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "tz 5", "text": "x" }),
    );
    assert_eq!(r.details()["cap"], "tenant", "{}", r.body);
    // After the old zone's midnight the new zone applies and the count starts at 0.
    fakes::advance(26 * 3_600_000);
    for i in 0..5 {
        send_ok(&s, &["jo@example.net"], &format!("tz new {i}"));
    }
}

#[test]
fn e8_disclosure_footer() {
    let s = open_sender("e8 footer", "live");
    assert_eq!(
        patch(
            &format!("/v1/tenants/{}/policy", s.tid),
            &platform(),
            json!({ "ai_disclosure": { "mode": "footer", "text": "Written by an AI agent." } })
        )
        .status,
        200
    );
    send_ok(&s, &["e8@example.net"], "E8 footer");
    let m = eventually(30, "the send", || {
        fakes::mail_sent("e8@example.net")
            .into_iter()
            .find(|m| m["subject"] == "E8 footer")
    });
    assert!(
        m["text"]
            .as_str()
            .unwrap()
            .contains("Written by an AI agent.")
    );
    assert_eq!(
        patch(
            &format!("/v1/tenants/{}/policy", s.tid),
            &platform(),
            json!({ "ai_disclosure": { "mode": "header" } })
        )
        .status,
        200
    );
    send_ok(&s, &["e8@example.net"], "E8 header");
    let m = eventually(30, "the send", || {
        fakes::mail_sent("e8@example.net")
            .into_iter()
            .find(|m| m["subject"] == "E8 header")
    });
    assert!(
        m["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h[0] == "X-AI-Generated" && h[1] == "true")
    );
}

#[test]
fn c6_forward_keeps_refs() {
    let s = open_sender("c6 fwd", "live");
    let m0 = receive(&s, "accounts@brightwell.example", "Invoice 88213");
    let r = submit(
        &format!("/v1/identities/{}/messages/{m0}/forward", s.iid),
        &s.key,
        "c6",
        json!({ "to": ["fleet@example.net"], "text": "FYI" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["subject"], "Fwd: Invoice 88213");
    assert!(r.body["in_reply_to"].is_null());
    assert_eq!(r.body["thread_id"], message(&s, &m0)["thread_id"]);
    let m = eventually(30, "the forward", || {
        fakes::mail_sent("fleet@example.net").into_iter().next()
    });
    let text = m["text"].as_str().unwrap();
    assert!(
        text.contains("---------- Forwarded message ---------")
            && text.contains("accounts@brightwell.example")
    );
    assert!(
        m["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h[0] == "References")
    );
}

#[test]
fn g2_timeout_uncertain() {
    let s = open_sender("g2 timeout", "test");
    let mid = send_ok(&s, &["timeout@simulator.invalid"], "G2");
    wait_status(&s, &mid, "uncertain");
    assert_eq!(
        events_of(&s.iid, "message.uncertain")[0]["reason"],
        "transport_timeout"
    );
    std::thread::sleep(std::time::Duration::from_secs(4));
    assert_eq!(message(&s, &mid)["status"], "uncertain", "never resent");
    let r = post(
        &format!("/v1/identities/{}/messages/{mid}/resolve", s.iid),
        &s.key,
        json!({ "outcome": "sent" }),
    );
    assert_eq!(r.body["status"], "submitted", "{}", r.body);
    let again = post(
        &format!("/v1/identities/{}/messages/{mid}/resolve", s.iid),
        &s.key,
        json!({ "outcome": "sent" }),
    );
    assert_eq!(again.code(), "not_uncertain");
    let m2 = send_ok(&s, &["timeout@simulator.invalid"], "G2 b");
    wait_status(&s, &m2, "uncertain");
    let r = post(
        &format!("/v1/identities/{}/messages/{m2}/resolve", s.iid),
        &s.key,
        json!({ "outcome": "not_sent" }),
    );
    assert_eq!(r.body["status"], "failed");
    assert!(
        events_of(&s.iid, "message.failed")
            .iter()
            .any(|e| e["reason"] == "resolved_not_sent")
    );
}

#[test]
fn g2_delivery_failed_recipients() {
    let s = open_sender("g2 df", "live");
    let failed = json!({ "code": "E_DELIVERY_FAILED", "message": "recipient server rejection" });
    fakes::mail_script("df1@example.net", vec![failed.clone()]);
    let one = send_ok(&s, &["df1@example.net"], "df one");
    wait_status(&s, &one, "rejected");
    fakes::mail_script("df3a@example.net", vec![failed]);
    let three = send_ok(
        &s,
        &["df3a@example.net", "df3b@example.net", "df3c@example.net"],
        "df three",
    );
    wait_status(&s, &three, "uncertain");
    assert!(
        events_of(&s.iid, "message.uncertain")
            .iter()
            .any(|e| e["reason"] == "provider_outcome_unknown")
    );
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert_eq!(
        fakes::mail_sent("df3a@example.net").len(),
        1,
        "never resent"
    );
}

#[test]
fn g3_quota_backoff() {
    let s = open_sender("g3 quota", "live");
    let _f = fakes::Reset;
    for (who, code) in [
        ("q1@example.net", "E_DAILY_LIMIT_EXCEEDED"),
        ("q2@example.net", "E_RATE_LIMIT_EXCEEDED"),
    ] {
        fakes::mail_script(who, vec![json!({ "code": code, "message": "limit" })]);
        let mid = send_ok(&s, &[who], code);
        eventually(30, "a back-off", || {
            (fakes::mail_sent(who).len() >= 2).then_some(())
        });
        assert_eq!(message(&s, &mid)["status"], "queued");
        fakes::advance(25 * 3_600_000);
        wait_status(&s, &mid, "failed");
        assert!(
            events_of(&s.iid, "message.failed")
                .iter()
                .any(|e| e["message_id"] == mid && e["reason"] == "quota_exhausted")
        );
        fakes::reset();
    }
}

#[test]
fn g4_partial_suppression() {
    let s = open_sender("g4 partial", "live");
    // Our own suppression filters the recipient at submit.
    let r = post(
        &format!("/v1/tenants/{}/suppressions", s.tid),
        &s.key,
        json!({ "address": "gone@example.net" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let r = send(
        &s,
        json!({ "to": ["gone@example.net", "stay@example.net"], "subject": "g4", "text": "x" }),
    );
    assert_eq!(
        delivery(&r.body, "gone@example.net")["status"],
        "suppressed"
    );
    // A provider suppression is synced and the rest resent.
    fakes::cf_suppress("prov@example.net", "account");
    fakes::mail_script(
        "prov@example.net",
        vec![json!({ "code": "E_RECIPIENT_SUPPRESSED", "message": "suppressed" })],
    );
    let mid = send_ok(&s, &["prov@example.net", "ok@example.net"], "g4 provider");
    wait_status(&s, &mid, "submitted");
    let rows = mailbox_query(
        &s.iid,
        &format!(
            "SELECT d.address, d.status, d.smtp_response FROM deliveries d JOIN messages m ON m.rowid = d.message_rowid WHERE m.id = '{mid}' ORDER BY d.address"
        ),
    );
    assert_eq!(rows[0]["status"], "submitted");
    assert_eq!(rows[1]["status"], "suppressed");
    assert!(
        rows[1]["smtp_response"]
            .as_str()
            .unwrap()
            .starts_with("provider: suppressed")
    );
    let n = crate::d1(&format!(
        "SELECT COUNT(*) AS n FROM suppressions WHERE tenant_id = '{}' AND reason = 'provider'",
        s.tid
    ));
    assert_eq!(n[0]["n"], 1);
    assert_eq!(events_of(&s.iid, "suppression.created").len(), 1);
}

#[test]
fn g4_suppressed_after_submit() {
    let s = open_sender("g4 after", "live");
    let _f = fakes::Reset;
    fakes::fault("queue.send", "Q_OUTBOUND", "error", 1);
    let mid = send_ok(&s, &["late@example.net"], "g4 late");
    assert_eq!(
        post(
            &format!("/v1/tenants/{}/suppressions", s.tid),
            &s.key,
            json!({ "address": "late@example.net", "reason": "hard_bounce" })
        )
        .status,
        201
    );
    crate::queue_send(
        "pm-outbound",
        json!({ "kind": "send", "v": 1, "tenant_id": s.tid, "identity_id": s.iid, "message_id": mid, "request_id": "req_g4" }),
    );
    wait_status(&s, &mid, "suppressed");
    assert!(fakes::mail_sent("late@example.net").is_empty());
    // A provider suppression on the shared platform domain writes no row and no event.
    let domain = s.addr.rsplit('@').next().unwrap().to_owned();
    fakes::cf_suppress("shared@example.net", &domain);
    fakes::mail_script(
        "shared@example.net",
        vec![json!({ "code": "E_RECIPIENT_SUPPRESSED", "message": "s" })],
    );
    let m2 = send_ok(&s, &["shared@example.net"], "g4 shared");
    wait_status(&s, &m2, "suppressed");
    let rows = mailbox_query(
        &s.iid,
        "SELECT smtp_response FROM deliveries WHERE address = 'shared@example.net'",
    );
    assert_eq!(
        rows[0]["smtp_response"],
        "provider: suppressed on the shared domain"
    );
    assert!(events_of(&s.iid, "suppression.created").is_empty());
}

#[test]
fn g5_large_attachment() {
    let s = open_sender("g5 large", "live");
    let big = "////".repeat(1_300_000);
    let body = json!({ "to": ["big@example.net"], "subject": "Large", "text": "See attached",
        "attachments": [{ "filename": "report.bin", "content_type": "application/octet-stream", "content_base64": big }] });
    let r = send(&s, body.clone());
    assert_eq!(r.status, 413, "{}", r.body);
    assert_eq!(r.details()["reason"], "transport_limit");
    assert_eq!(
        patch(
            &format!("/v1/tenants/{}/policy", s.tid),
            &platform(),
            json!({ "large_attachments": "link" })
        )
        .status,
        200
    );
    let r = send(&s, body);
    assert_eq!(r.status, 202, "{}", r.body);
    let m = eventually(30, "the send", || {
        fakes::mail_sent("big@example.net").into_iter().next()
    });
    assert!(m["attachments"].as_array().unwrap().is_empty());
    let text = m["text"].as_str().unwrap();
    let url = text
        .split_whitespace()
        .find(|w| w.contains("/v1/links/"))
        .unwrap();
    let path = &url[url.find("/v1/links/").unwrap()..];
    let got = call("GET", path, None, None, &[]);
    assert_eq!(got.status, 200);
    assert_eq!(
        got.header("content-disposition")
            .map(|h| h.starts_with("attachment")),
        Some(true)
    );
    let bad = call("GET", &format!("{path}x"), None, None, &[]);
    assert_eq!(bad.code(), "attachment_not_found");
}

#[test]
fn g7_domain_states() {
    let s = open_sender("g7 domains", "live");
    // A tenant domain, pending: domain_not_ready.
    let dom = format!("dom_{}", slug("g7 dom").to_ascii_uppercase());
    let name = format!("{}.example", slug("g7"));
    d1w(
        "INSERT INTO domains (id, tenant_id, name, kind, method, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_changed_at, records_json, monitor_do_id, created_at, updated_at) VALUES (?1, ?2, ?3, 'external', 'dns_records', 1, 'literal', 'ses', 'cloudflare', 'subaddress', 1, 1, 'pending', 0, '[]', '', 0, 0)",
        json!([dom, s.tid, name]),
    );
    let addr = format!("bookings@{name}");
    d1w(
        "INSERT INTO addresses (id, address, local_part, domain_id, tenant_id, identity_id, role, status, created_at, updated_at) VALUES (?1, ?2, 'bookings', ?3, ?4, ?5, 'alias', 'active', 0, 0)",
        json!([
            format!("adr_{}", slug("g7 adr").to_ascii_uppercase()),
            addr,
            dom,
            s.tid,
            s.iid
        ]),
    );
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "g7", "text": "x", "from_address": addr }),
    );
    assert_eq!(r.code(), "domain_not_ready", "{}", r.body);
    // Failing: falls back to the platform address.
    d1w(
        "UPDATE domains SET state = 'failing', event_subscription_id = 'sub' WHERE id = ?1",
        json!([dom]),
    );
    let r = send(
        &s,
        json!({ "to": ["g7@example.net"], "subject": "g7 fallback", "text": "x", "from_address": addr }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let m = wait_status(&s, r.body["id"].as_str().unwrap(), "submitted");
    assert!(
        m["flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "sent_via_fallback")
    );
    assert_eq!(fakes::mail_sent("g7@example.net")[0]["from"], s.addr);
    // A retiring address only on a thread that uses it.
    d1w(
        "UPDATE addresses SET status = 'retiring' WHERE address = ?1",
        json!([addr]),
    );
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "g7 r", "text": "x", "from_address": addr }),
    );
    assert_eq!(
        r.details()["errors"][0]["path"],
        "from_address",
        "{}",
        r.body
    );
}

#[test]
fn g9_marketing_requirements() {
    let s = open_sender("g9 marketing", "live");
    let r = send(
        &s,
        json!({ "to": ["m@example.net"], "subject": "Offer", "text": "x", "kind": "marketing" }),
    );
    assert_eq!(r.code(), "marketing_requirements_missing");
    let r = send(
        &s,
        json!({ "to": ["m@example.net"], "subject": "Offer", "text": "x", "kind": "marketing",
        "unsubscribe": { "url": "https://acme.example/u/1" }, "consent": { "source": "signup", "at": "2026-10-01T00:00:00Z" } }),
    );
    assert_eq!(r.status, 422, "{}", r.body);
    assert_eq!(r.details()["reason"], "marketing_needs_ses");
}

#[test]
fn g9_marketing_transport_recheck() {
    // The `ses` transport is built in M13; until then a marketing message accepted on an `ses` domain
    // cannot exist, so the recheck is shown through fallback from a failing `ses`-transport domain.
    let s = open_sender("g9 recheck", "live");
    let dom = format!("dom_{}", slug("g9 dom").to_ascii_uppercase());
    let name = format!("{}.example", slug("g9"));
    d1w(
        "INSERT INTO domains (id, tenant_id, name, kind, method, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_changed_at, records_json, monitor_do_id, created_at, updated_at) VALUES (?1, ?2, ?3, 'external', 'dns_records', 1, 'literal', 'ses', 'smtp', 'subaddress', 1, 1, 'healthy', 0, '[]', '', 0, 0)",
        json!([dom, s.tid, name]),
    );
    let addr = format!("news@{name}");
    d1w(
        "INSERT INTO addresses (id, address, local_part, domain_id, tenant_id, identity_id, role, status, created_at, updated_at) VALUES (?1, ?2, 'news', ?3, ?4, ?5, 'alias', 'active', 0, 0)",
        json!([
            format!("adr_{}", slug("g9 adr").to_ascii_uppercase()),
            addr,
            dom,
            s.tid,
            s.iid
        ]),
    );
    let _f = fakes::Reset;
    fakes::fault("queue.send", "Q_OUTBOUND", "error", 1);
    let r = send(
        &s,
        json!({ "to": ["m@example.net"], "subject": "Offer", "text": "x", "kind": "marketing", "from_address": addr,
        "unsubscribe": { "url": "https://acme.example/u/1" }, "consent": { "source": "signup" } }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let mid = r.body["id"].as_str().unwrap().to_owned();
    d1w(
        "UPDATE domains SET state = 'failing' WHERE id = ?1",
        json!([dom]),
    );
    crate::queue_send(
        "pm-outbound",
        json!({ "kind": "send", "v": 1, "tenant_id": s.tid, "identity_id": s.iid, "message_id": mid, "request_id": "req_g9" }),
    );
    wait_status(&s, &mid, "rejected");
    assert!(
        events_of(&s.iid, "message.rejected")
            .iter()
            .any(|e| e["reason"] == "marketing_needs_ses")
    );
    assert!(fakes::mail_sent("m@example.net").is_empty());
}

#[test]
fn header_rules() {
    let s = open_sender("header rules", "live");
    let r = send(
        &s,
        json!({ "to": ["h@example.net"], "subject": "Hdr", "text": "x",
        "headers": { "X-Booking-Ref": "BK-1", "importance": "high", "keywords": "a" } }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let m = eventually(30, "the send", || {
        fakes::mail_sent("h@example.net").into_iter().next()
    });
    assert!(
        m["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h[0] == "Importance" && h[1] == "high")
    );
    for bad in [
        "X-Bad Name",
        "X-",
        "Importance ",
        "x-pylota-trace",
        "x-ai-generated",
    ] {
        let r = send(
            &s,
            json!({ "to": ["h@example.net"], "subject": "Hdr", "text": "x", "headers": { bad: "v" } }),
        );
        assert_eq!(r.code(), "header_not_allowed", "{bad}: {}", r.body);
    }
    for (n, v) in [
        ("Importance", "urgent"),
        ("priority", "high"),
        ("Sensitivity", "secret"),
    ] {
        let r = send(
            &s,
            json!({ "to": ["h@example.net"], "subject": "Hdr", "text": "x", "headers": { n: v } }),
        );
        assert_eq!(r.code(), "invalid_request", "{n}");
        assert_eq!(r.details()["errors"][0]["path"], format!("headers.{n}"));
    }
    let r = send(
        &s,
        json!({ "to": ["h@example.net"], "subject": "Hdr", "text": "x", "headers": { "Importance": "high", "importance": "low" } }),
    );
    assert_eq!(r.code(), "invalid_request");
    assert!(mailbox_query(&s.iid, "SELECT id FROM messages WHERE status = 'rejected'").is_empty());
}

#[test]
fn g10_provider_validation() {
    let s = open_sender("g10 validation", "live");
    for (i, code) in [
        "E_VALIDATION_ERROR",
        "E_FIELD_MISSING",
        "E_TOO_MANY_RECIPIENTS",
        "E_TOO_MANY_ATTACHMENTS",
        "E_CONTENT_TOO_LARGE",
        "E_HEADER_NOT_ALLOWED",
        "E_HEADER_USE_API_FIELD",
        "E_HEADER_VALUE_INVALID",
        "E_HEADER_VALUE_TOO_LONG",
        "E_HEADER_NAME_INVALID",
        "E_HEADERS_TOO_LARGE",
        "E_HEADERS_TOO_MANY",
    ]
    .iter()
    .enumerate()
    {
        let to = format!("v{i}@example.net");
        fakes::mail_script(&to, vec![json!({ "code": code, "message": "invalid" })]);
        let mid = send_ok(&s, &[&to], code);
        wait_status(&s, &mid, "rejected");
        assert_eq!(fakes::mail_sent(&to).len(), 1, "never retried");
    }
    assert!(
        events_of(&s.iid, "message.rejected")
            .iter()
            .all(|e| e["reason"] == "provider_validation")
    );
}

#[test]
fn g11_loopback() {
    // A live tenant's mail to a local address goes through the transport.
    let live = open_sender("g11 live", "live");
    let other = open_sender("g11 dest", "live");
    send_ok(&live, &[&other.addr], "g11 live");
    eventually(30, "the transport", || {
        (!fakes::mail_sent(&other.addr).is_empty()).then_some(())
    });
    // A test tenant reaches its own identity and a same-partner test tenant by loopback.
    let (_, pkey) = crate::partner(
        &slug("g11 partner"),
        &["tenants:manage", "identities:write", "keys:manage"],
    );
    let mk = |n: &str| {
        let r = post(
            "/v1/tenants",
            &pkey,
            json!({ "slug": slug(n), "name": n, "mode": "test" }),
        );
        assert_eq!(r.status, 201, "{}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    };
    let (ta, tb) = (mk("g11 a"), mk("g11 b"));
    let a = sender_in(&ta, "g11 sa", json!({ "require_known_recipient": false }));
    let a2 = sender_in(&ta, "g11 sa2", json!({ "require_known_recipient": false }));
    let b = sender_in(&tb, "g11 sb", json!({ "require_known_recipient": false }));
    let mid = send_ok(&a, &[&a2.addr, &b.addr], "g11 loop");
    wait_status(&a, &mid, "delivered");
    crate::wait_stored(&a2.iid, &a2.key, "received", 1);
    eventually(30, "b's copy", || {
        (!mailbox_query(
            &b.iid,
            "SELECT id FROM messages WHERE direction = 'inbound'",
        )
        .is_empty())
        .then_some(())
    });
    assert!(fakes::mail_sent(&a2.addr).is_empty());
    // An unrelated tenant's identity is refused and nothing is injected.
    let r = send(
        &a,
        json!({ "to": [other.addr], "subject": "x", "text": "x" }),
    );
    assert_eq!(r.code(), "test_mode_recipient");
}

#[test]
fn j28_transport_breaker() {
    let s = open_sender("j28 breaker", "live");
    let _f = fakes::Reset;
    d1w("DELETE FROM transport_breakers", json!([]));
    let ise = json!({ "code": "E_INTERNAL_SERVER_ERROR", "message": "temporarily unavailable" });
    let mut uncertain = Vec::new();
    for i in 0..3 {
        let to = format!("ise{i}@example.net");
        fakes::mail_script(&to, vec![ise.clone()]);
        let mid = send_ok(&s, &[&to], &format!("ise {i}"));
        wait_status(&s, &mid, "uncertain");
        uncertain.push(mid);
    }
    let row =
        crate::d1("SELECT state, open_count FROM transport_breakers WHERE scope = 'cloudflare'");
    assert_eq!(row[0]["state"], "open");
    // Held messages are neither claimed nor uncertain.
    let held: Vec<String> = (0..3)
        .map(|i| send_ok(&s, &[&format!("held{i}@example.net")], "held"))
        .collect();
    eventually(30, "a hold", || {
        mailbox_query(&s.iid, "SELECT k FROM meta WHERE k LIKE 'backoff:%'")
            .len()
            .ge(&3)
            .then_some(())
    });
    for h in &held {
        assert_eq!(message(&s, h)["status"], "queued");
    }
    // After open_until one probe is sent; accepted, it closes the breaker and the held mail leaves.
    fakes::advance(6 * 60_000);
    for h in &held {
        wait_status(&s, h, "submitted");
    }
    let row = crate::d1("SELECT state FROM transport_breakers WHERE scope = 'cloudflare'");
    assert_eq!(row[0]["state"], "closed");
}

#[test]
fn k3_failure_reason() {
    let s = open_sender("k3 reason", "test");
    let mid = send_ok(&s, &["reject@simulator.invalid"], "K3");
    wait_status(&s, &mid, "rejected");
    let e = events_of(&s.iid, "message.rejected");
    assert_eq!(e[0]["reason"], "provider_validation");
    assert!(
        e[0]["detail"]
            .as_str()
            .unwrap()
            .contains("E_SIMULATED_REJECT")
    );
}

#[test]
fn n13_loop_guard() {
    let s = open_sender("n13 loop", "live");
    let raw = format!(
        "From: bot@example.net\r\nTo: {}\r\nSubject: Ping\r\nMessage-ID: <n13@it.example>\r\nX-Pylota-Identity-Hop: 9\r\nDate: Mon, 26 Oct 2026 10:00:00 +0000\r\n\r\nping\r\n",
        s.addr
    );
    crate::inbound("bot@example.net", &s.addr, &raw);
    let mid = eventually(30, "inbound", || {
        crate::messages_of(&s.iid, &s.key, "&direction=inbound")
            .first()
            .map(|m| m["id"].as_str().unwrap().to_owned())
    });
    let r = submit(
        &format!("/v1/identities/{}/messages/{mid}/reply", s.iid),
        &s.key,
        "n13",
        json!({ "text": "pong" }),
    );
    assert_eq!(r.code(), "loop_detected", "{}", r.body);
    assert_eq!(r.details()["hop"], 10);
    let r = submit(
        &format!("/v1/identities/{}/messages/{mid}/forward", s.iid),
        &s.key,
        "n13f",
        json!({ "to": ["x@example.net"], "text": "fyi" }),
    );
    assert_eq!(r.code(), "loop_detected");
    send_ok(&s, &["bot@example.net"], "fresh");
}

#[test]
fn n35_reply_rate() {
    let s = open_sender("n35 rate", "live");
    let _f = fakes::Reset;
    let m0 = receive(&s, "responder@example.net", "Rate");
    for i in 0..25 {
        let r = submit(
            &format!("/v1/identities/{}/messages/{m0}/reply", s.iid),
            &s.key,
            &format!("n35-{i}"),
            json!({ "text": "again" }),
        );
        assert_eq!(r.status, 202, "{i}: {}", r.body);
    }
    let r = submit(
        &format!("/v1/identities/{}/messages/{m0}/reply", s.iid),
        &s.key,
        "n35-26",
        json!({ "text": "again" }),
    );
    assert_eq!(r.code(), "loop_detected", "{}", r.body);
    assert_eq!(r.details()["reason"], "thread_rate");
    let ra = r.details()["retry_after"].as_i64().unwrap();
    assert!(ra > 86_000 && ra <= 86_400, "{ra}");
    send_ok(&s, &["someone@example.net"], "fresh send");
    fakes::advance(24 * 3_600_000 + 60_000);
    let r = submit(
        &format!("/v1/identities/{}/messages/{m0}/reply", s.iid),
        &s.key,
        "n35-later",
        json!({ "text": "later" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
}

#[test]
fn sending_paused_refuses() {
    let s = open_sender("paused tenant", "live");
    d1w(
        "UPDATE tenants SET sending_paused_at = 1, sending_pause_reason = 'abuse_threshold' WHERE id = ?1",
        json!([s.tid]),
    );
    let r = send(
        &s,
        json!({ "to": ["bad address"], "subject": "x", "text": "x" }),
    );
    assert_eq!(
        r.code(),
        "sending_paused",
        "before any recipient check: {}",
        r.body
    );
    assert_eq!(r.details()["scope"], "tenant");
    let d = call(
        "POST",
        &format!("/v1/identities/{}/messages?dry_run=true", s.iid),
        Some(&s.key),
        Some(json!({ "to": ["jo@example.net"], "subject": "x", "text": "x" })),
        &[],
    );
    assert_eq!(d.code(), "sending_paused");
}

#[test]
fn stale_claim_outcome() {
    let s = open_sender("stale claim", "live");
    let _f = fakes::Reset;
    fakes::fault("queue.send", "Q_OUTBOUND", "error", 1);
    let mid = send_ok(&s, &["stale@example.net"], "Stale");
    // A claim taken by a consumer that died: resolved to uncertain by the claim alarm.
    let rpc = |req: Value| {
        crate::hook("POST", "/__test/rpc", Some(json!({ "class": "mailbox",
        "object_id": crate::d1(&format!("SELECT mailbox_do_id FROM identities WHERE id = '{}'", s.iid))[0]["mailbox_do_id"],
        "envelope": { "v": 1, "tenant_id": s.tid, "identity_id": s.iid, "request_id": "req_t", "actor_key_id": null,
            "deadline_ms": 9_999_999_999_999i64, "req": req } }))).body
    };
    let token = rpc(json!({ "op": "claim_transport", "message_id": mid }))["ok"]
        .as_str()
        .unwrap()
        .to_owned();
    let claimed_at = mailbox_query(
        &s.iid,
        &format!("SELECT v FROM meta WHERE k = 'claim:{mid}'"),
    )[0]["v"]
        .as_str()
        .unwrap()
        .split('|')
        .nth(1)
        .unwrap()
        .parse::<i64>()
        .unwrap();
    fakes::advance(6 * 60_000);
    crate::alarm(&s.iid);
    let m = message(&s, &mid);
    assert_eq!(m["status"], "uncertain");
    let rows = mailbox_query(
        &s.iid,
        "SELECT updated_at FROM deliveries WHERE address = 'stale@example.net'",
    );
    assert_eq!(rows[0]["updated_at"], claimed_at);
    // A late Unknown with the stale token changes nothing.
    let r = rpc(
        json!({ "op": "record_transport_outcome", "message_id": mid, "claim_token": token, "provider": "cloudflare",
        "result": { "kind": "unknown", "reason": "transport_timeout", "detail": "late" } }),
    );
    assert_eq!(r["ok"]["stale"], true);
    // A late Accepted is evidence: provider ID, submitted, reconciled.
    let r = rpc(
        json!({ "op": "record_transport_outcome", "message_id": mid, "claim_token": token, "provider": "cloudflare",
        "result": { "kind": "accepted", "provider_message_id": "<late@agents.example>", "rfc_message_id": null } }),
    );
    assert_eq!(r["ok"]["stale"], true);
    let m = message(&s, &mid);
    assert_eq!(m["status"], "submitted");
    assert!(
        m["flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "reconciled")
    );
    assert_eq!(events_of(&s.iid, "message.reconciled").len(), 1);
}

#[test]
fn dry_run_stores_nothing() {
    let s = open_sender("dry run", "live");
    let d = call(
        "POST",
        &format!("/v1/identities/{}/messages?dry_run=true", s.iid),
        Some(&s.key),
        Some(json!({ "to": ["jo@example.net"], "subject": "x", "text": "x" })),
        &[],
    );
    assert_eq!(d.status, 200, "{}", d.body);
    assert_eq!(d.body["would_send"], true);
    assert_eq!(d.body["recipients"][0]["status"], "queued");
    assert!(mailbox_query(&s.iid, "SELECT id FROM messages").is_empty());
}
