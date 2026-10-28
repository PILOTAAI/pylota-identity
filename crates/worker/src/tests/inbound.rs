//! M7 on fakes: a message from `email()` through `pm-inbound` into the mailbox, and the
//! `AttachmentText` job of `pm-index` (Inbound, Search § 6).

use pylota_identity_platform::QueueProducer as _;
use pylota_identity_platform::app::{EmailIn, EmailOutcome};
use pylota_identity_platform::buffer::JsBuffer;
use pylota_identity_platform::fakes::{Settled, block_on, incoming};
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{PlatformError, PlatformErrorKind};
use serde_json::{Value, json};

use super::Harness;

/// A tenant with one identity; returns `(tenant id, identity id, address, mailbox object ID)`.
fn setup(h: &Harness) -> (String, String, String, String) {
    crate::email::reset_isolate();
    h.sql(
        "INSERT INTO domains (id, tenant_id, name, kind, method, zone_id, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_changed_at, monitor_do_id, created_at, updated_at) VALUES ('dom_01ARYZ6S410000000000000000', NULL, 'pylotamail.test', 'platform', 'platform', 'z', 1, 'catch_all', 'routing', 'cloudflare', 'subaddress', 1, 1, 'healthy', 1, '', 1, 1)",
        vec![],
    );
    let p = h.platform_key();
    let t = h.call(
        "POST",
        "/v1/tenants",
        Some(&p),
        Some(json!({ "slug": "acme", "name": "Acme", "mode": "live" })),
        &[],
    );
    assert_eq!(t.status, 201, "{}", t.body);
    let tid = t.body["id"].as_str().unwrap().to_owned();
    let tk = h.seed_key(
        "tenant",
        &["identities:read", "identities:write"],
        Some(&tid),
        None,
        None,
        None,
    );
    let r = h.call(
        "POST",
        &format!("/v1/tenants/{tid}/identities"),
        Some(&tk),
        Some(json!({ "username": "bookings", "display_name": "Acme Bookings", "owner": { "name": "Sam", "email": "sam@acme.example" } })),
        &[],
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let idn = r.body["id"].as_str().unwrap().to_owned();
    let address = r.body["primary_address"].as_str().unwrap().to_owned();
    let mb = h.sql(
        "SELECT mailbox_do_id FROM identities WHERE id = ?1",
        vec![idn.clone().into()],
    )[0]["mailbox_do_id"]
        .as_str()
        .unwrap()
        .to_owned();
    (tid, idn, address, mb)
}

fn email(h: &Harness, from: &str, to: &str, raw: &str) -> EmailOutcome {
    block_on(crate::email::handle(
        h.app.p.as_ref(),
        &EmailIn {
            from: from.to_owned(),
            to: to.to_owned(),
            headers: Vec::new(),
            raw: JsBuffer::from_bytes(raw.as_bytes()),
            raw_size: raw.len() as u64,
        },
    ))
}

/// Delivers everything waiting in `q` to its consumer once; returns how each was settled.
fn drain(h: &Harness, q: QueueName) -> Vec<Settled> {
    let bodies = h.app.p.queues.take(q);
    let mut logs = Vec::new();
    let batch: Vec<_> = bodies
        .into_iter()
        .enumerate()
        .map(|(i, b)| {
            let (m, log) = incoming(&format!("m{i}"), 0, 1, b);
            logs.push(log);
            m
        })
        .collect();
    block_on(crate::consumers::run(h.app.p.as_ref(), q, false, batch));
    logs.iter().map(|l| l.get()).collect()
}

fn messages(h: &Harness, mb: &str) -> Vec<Value> {
    h.mailbox_sql(mb, "SELECT id, status, subject, is_bcc, is_primary_recipient, flags_json, raw_r2_key, thread_seq FROM messages ORDER BY rowid")
}

fn plain(to: &str, msg_id: &str, subject: &str, body: &str) -> String {
    format!(
        "From: Alex <alex@sender.example>\r\nTo: {to}\r\nSubject: {subject}\r\nMessage-ID: <{msg_id}>\r\nDate: Mon, 26 Oct 2026 10:00:00 +0000\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
    )
}

#[test]
fn email_to_mailbox_and_redelivery_deduped() {
    let h = Harness::new();
    let (_, _, address, mb) = setup(&h);
    let raw = plain(
        &address,
        "a1@sender.example",
        "Booking 4411",
        "Hello, is the car free on Friday?",
    );
    assert_eq!(
        email(&h, "alex@sender.example", &address, &raw),
        EmailOutcome::Accept
    );
    assert_eq!(drain(&h, QueueName::Inbound), vec![Settled::Acked]);
    let rows = messages(&h, &mb);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["subject"], "Booking 4411");
    assert_eq!(rows[0]["is_bcc"], 0);
    // An unauthenticated sender is quarantined by default (on_auth_fail).
    let status = rows[0]["status"].as_str().unwrap().to_owned();
    assert!(
        matches!(status.as_str(), "received" | "quarantined"),
        "{status}"
    );
    // The raw copy is in R2, and index jobs went to pm-index (none for attachment text).
    assert!(
        h.app
            .p
            .blobs
            .bytes(rows[0]["raw_r2_key"].as_str().unwrap())
            .is_some()
    );
    let jobs = h.app.p.queues.take(QueueName::Index);
    assert!(jobs.iter().any(|j| j["kind"] == "embed"), "{jobs:?}");
    assert!(!jobs.iter().any(|j| j["kind"] == "attachment_text"));
    // The same message again (a sender's retry): one row, and the second raw copy is deleted.
    assert_eq!(
        email(&h, "alex@sender.example", &address, &raw),
        EmailOutcome::Accept
    );
    assert_eq!(drain(&h, QueueName::Inbound), vec![Settled::Acked]);
    assert_eq!(messages(&h, &mb).len(), 1);
    let raws = h
        .app
        .p
        .blobs
        .keys()
        .into_iter()
        .filter(|k| k.ends_with("raw.eml"))
        .count();
    assert_eq!(raws, 1);
}

#[test]
fn unknown_recipient_rejected_and_bcc_flagged() {
    let h = Harness::new();
    let (_, _, address, mb) = setup(&h);
    assert_eq!(
        email(
            &h,
            "alex@sender.example",
            "nobody.acme@pylotamail.test",
            &plain("nobody.acme@pylotamail.test", "x@s", "Hi", "x")
        ),
        EmailOutcome::Reject {
            smtp: crate::email::REJECT_UNKNOWN.to_owned()
        }
    );
    // Delivered to the identity, but its address is in no To or Cc header: a BCC copy (A10).
    let raw = plain("someone@else.example", "b1@sender.example", "FYI", "copy");
    assert_eq!(
        email(&h, "alex@sender.example", &address, &raw),
        EmailOutcome::Accept
    );
    drain(&h, QueueName::Inbound);
    let rows = messages(&h, &mb);
    assert_eq!(rows[0]["is_bcc"], 1);
}

fn with_attachment(to: &str, filename: &str, ctype: &str, b64: &str) -> String {
    format!(
        "From: Alex <alex@sender.example>\r\nTo: {to}\r\nSubject: Documents\r\nMessage-ID: <att@sender.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n--b\r\nContent-Type: {ctype}\r\nContent-Disposition: attachment; filename=\"{filename}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{b64}\r\n--b--\r\n"
    )
}

fn attachments(h: &Harness, mb: &str) -> Vec<Value> {
    h.mailbox_sql(
        mb,
        "SELECT id, text_status, text_r2_key, text_pages FROM attachments ORDER BY rowid",
    )
}

#[test]
fn text_attachment_extracted_and_indexed() {
    let h = Harness::new();
    let (_, _, address, mb) = setup(&h);
    // "Invoice INV-88213 for the rental." in base64.
    let raw = with_attachment(
        &address,
        "note.txt",
        "text/plain",
        "SW52b2ljZSBJTlYtODgyMTMgZm9yIHRoZSByZW50YWwu",
    );
    email(&h, "alex@sender.example", &address, &raw);
    drain(&h, QueueName::Inbound);
    let a = attachments(&h, &mb);
    assert_eq!(a[0]["text_status"], "pending", "{a:?}");
    let jobs = h.app.p.queues.take(QueueName::Index);
    let text_jobs: Vec<Value> = jobs
        .into_iter()
        .filter(|j| j["kind"] == "attachment_text")
        .collect();
    assert_eq!(text_jobs.len(), 1);
    // Run the job: the attachment becomes ready, its text is in R2 and in the keyword index.
    for j in text_jobs {
        block_on(h.app.p.queues.send(QueueName::Index, &j, 0)).unwrap();
    }
    assert_eq!(drain(&h, QueueName::Index), vec![Settled::Acked]);
    let a = attachments(&h, &mb);
    assert_eq!(
        (a[0]["text_status"].as_str(), a[0]["text_pages"].as_i64()),
        (Some("ready"), Some(1))
    );
    let md = h
        .app
        .p
        .blobs
        .bytes(a[0]["text_r2_key"].as_str().unwrap())
        .unwrap();
    assert!(
        String::from_utf8(md)
            .unwrap()
            .starts_with("<!-- pm:page 1 -->\nInvoice INV-88213")
    );
    let hits = h.mailbox_sql(
        &mb,
        "SELECT rowid FROM fts WHERE fts MATCH 'attachments:rental'",
    );
    assert_eq!(hits.len(), 1);
    // An Embed job follows for the attachment chunks.
    assert!(
        h.app
            .p
            .queues
            .take(QueueName::Index)
            .iter()
            .any(|j| j["kind"] == "embed" && j["reason"] == "attachment_text")
    );
}

#[test]
fn b12_three_failed_conversions_unavailable() {
    let h = Harness::new();
    let (_, _, address, mb) = setup(&h);
    // A minimal PDF header, enough for the sniffer.
    let raw = with_attachment(
        &address,
        "scan.pdf",
        "application/pdf",
        "JVBERi0xLjQKJcOkw7zDtsOfCjEgMCBvYmoKPDw+PgplbmRvYmoKdHJhaWxlcgo8PD4+CiUlRU9GCg==",
    );
    email(&h, "alex@sender.example", &address, &raw);
    drain(&h, QueueName::Inbound);
    let mut job = h
        .app
        .p
        .queues
        .take(QueueName::Index)
        .into_iter()
        .find(|j| j["kind"] == "attachment_text")
        .expect("an attachment_text job");
    for attempt in 0..3 {
        assert_eq!(job["attempt"], attempt);
        h.app.p.ai.answer_markdown(Err(PlatformError::new(
            PlatformErrorKind::Unavailable,
            "AI",
            "down",
        )));
        block_on(h.app.p.queues.send(QueueName::Index, &job.clone(), 0)).unwrap();
        assert_eq!(drain(&h, QueueName::Index), vec![Settled::Acked]);
        if attempt < 2 {
            assert_eq!(attachments(&h, &mb)[0]["text_status"], "pending");
            job = h
                .app
                .p
                .queues
                .take(QueueName::Index)
                .into_iter()
                .find(|j| j["kind"] == "attachment_text")
                .expect("re-enqueued");
        }
    }
    assert_eq!(attachments(&h, &mb)[0]["text_status"], "unavailable");
    assert!(h.app.p.queues.take(QueueName::Index).is_empty());
}

#[test]
fn job_kinds_before_milestone_ack() {
    let h = Harness::new();
    for j in [
        json!({ "kind": "embed", "tenant_id": "t", "identity_id": "i", "message_id": "m" }),
        json!({ "kind": "triage", "tenant_id": "t", "identity_id": "i", "message_id": "m" }),
        json!({ "kind": "delete_vectors", "tenant_id": "t", "vector_ids": [] }),
        json!({ "kind": "reconcile", "tenant_id": "t", "identity_id": "i", "run_date": "2026-10-28" }),
    ] {
        block_on(h.app.p.queues.send(QueueName::Index, &j, 0)).unwrap();
    }
    assert_eq!(drain(&h, QueueName::Index), vec![Settled::Acked; 4]);
    assert!(h.app.p.queues.take(QueueName::Index).is_empty());
}

/// A tenant key with the read, write, review and wait permissions.
fn reader(h: &Harness, tid: &str) -> String {
    h.seed_key(
        "tenant",
        &[
            "messages:read",
            "messages:write",
            "quarantine:review",
            "attachments:read",
            "search:read",
        ],
        Some(tid),
        None,
        None,
        None,
    )
}

fn get(h: &Harness, key: &str, path: &str) -> super::Resp {
    h.call("GET", path, Some(key), None, &[])
}

#[test]
fn read_path_labels_and_read_state() {
    let h = Harness::new();
    let (tid, idn, address, _) = setup(&h);
    let k = reader(&h, &tid);
    email(
        &h,
        "alex@sender.example",
        &address,
        &plain(
            &address,
            "r1@sender.example",
            "Booking 4411",
            "Is the car free?",
        ),
    );
    drain(&h, QueueName::Inbound);
    let threads = get(&h, &k, &format!("/v1/identities/{idn}/threads"));
    assert_eq!(threads.status, 200, "{}", threads.body);
    let list = threads.body["data"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    let thr = list[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        (
            list[0]["message_count"].as_i64(),
            list[0]["unread_count"].as_i64()
        ),
        (Some(1), Some(1))
    );
    assert_eq!(list[0]["subject"], "Booking 4411");
    let t = get(
        &h,
        &k,
        &format!("/v1/identities/{idn}/threads/{thr}?include=headers"),
    );
    assert_eq!(t.status, 200, "{}", t.body);
    let msg = t.body["messages"][0].clone();
    let mid = msg["id"].as_str().unwrap().to_owned();
    assert!(
        msg["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "Subject")
    );
    assert!(msg.get("_raw_r2_key").is_none());
    assert_eq!(msg["text"], Value::Null, "text only with include=quoted");
    // Labels and read state on the message, seen on the thread.
    let r = h.call(
        "PATCH",
        &format!("/v1/identities/{idn}/messages/{mid}"),
        Some(&k),
        Some(json!({ "labels_add": ["crm:synced", "todo"], "read": true })),
        &[],
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["labels"], json!(["crm:synced", "todo"]));
    let bad = h.call(
        "PATCH",
        &format!("/v1/identities/{idn}/messages/{mid}"),
        Some(&k),
        Some(json!({ "labels_add": ["Bad"] })),
        &[],
    );
    assert_eq!(bad.status, 400);
    let threads = get(&h, &k, &format!("/v1/identities/{idn}/threads?label=todo"));
    assert_eq!(threads.body["data"][0]["unread_count"], 0);
    assert_eq!(
        threads.body["data"][0]["labels"],
        json!(["crm:synced", "todo"])
    );
    // Archive: gone from the default list, present with archived=true.
    let a = h.call(
        "PATCH",
        &format!("/v1/identities/{idn}/threads/{thr}"),
        Some(&k),
        Some(json!({ "archived": true })),
        &[],
    );
    assert_eq!(a.status, 200, "{}", a.body);
    assert!(
        get(&h, &k, &format!("/v1/identities/{idn}/threads")).body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        get(
            &h,
            &k,
            &format!("/v1/identities/{idn}/threads?archived=true")
        )
        .body["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // The raw MIME.
    let raw = get(&h, &k, &format!("/v1/identities/{idn}/messages/{mid}/raw"));
    assert_eq!(raw.status, 200);
}

#[test]
fn unsolicited_otp_quarantined_and_released() {
    let h = Harness::new();
    let (tid, idn, address, mb) = setup(&h);
    let k = reader(&h, &tid);
    let otp = plain(
        &address,
        "otp1@sender.example",
        "Your verification code",
        "Your verification code is 481207.",
    );
    email(&h, "alex@sender.example", &address, &otp);
    drain(&h, QueueName::Inbound);
    let rows = messages(&h, &mb);
    assert_eq!(rows[0]["status"], "quarantined", "{rows:?}");
    let mid = rows[0]["id"].as_str().unwrap().to_owned();
    // Not visible to lists or to a key without review; listed in quarantine.
    assert!(
        get(&h, &k, &format!("/v1/identities/{idn}/threads")).body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let plain_key = h.seed_key("tenant", &["messages:read"], Some(&tid), None, None, None);
    assert_eq!(
        get(
            &h,
            &plain_key,
            &format!("/v1/identities/{idn}/messages/{mid}")
        )
        .status,
        404
    );
    let held = get(
        &h,
        &plain_key,
        &format!("/v1/identities/{idn}/messages?status=quarantined"),
    );
    assert_eq!(
        (held.status, held.body["data"].as_array().unwrap().len()),
        (200, 0)
    );
    let q = get(&h, &k, &format!("/v1/identities/{idn}/quarantine"));
    assert_eq!(q.status, 200, "{}", q.body);
    assert_eq!(q.body["data"][0]["quarantine_reason"], "otp_unsolicited");
    // Release: visible, message.released then message.received, once.
    let rel = h.call(
        "POST",
        &format!("/v1/identities/{idn}/messages/{mid}/release"),
        Some(&k),
        Some(json!({ "reason": "expected" })),
        &[],
    );
    assert_eq!(rel.status, 200, "{}", rel.body);
    assert_eq!(rel.body["status"], "received");
    let again = h.call(
        "POST",
        &format!("/v1/identities/{idn}/messages/{mid}/release"),
        Some(&k),
        Some(json!({ "reason": "again" })),
        &[],
    );
    assert_eq!((again.status, again.code()), (409, "not_quarantined"));
    let types: Vec<String> = h
        .mailbox_sql(&mb, "SELECT type FROM outbox ORDER BY seq")
        .iter()
        .map(|r| r["type"].as_str().unwrap().to_owned())
        .collect();
    let n = types.len();
    assert_eq!(
        &types[n - 3..],
        &[
            "message.quarantined",
            "message.released",
            "message.received"
        ]
    );
    let threads = get(&h, &k, &format!("/v1/identities/{idn}/threads"));
    assert_eq!(threads.body["data"][0]["message_count"], 1);
    assert_eq!(
        h.sql(
            "SELECT action FROM audit_log WHERE action = 'quarantine.release'",
            vec![]
        )
        .len(),
        1
    );
}

#[test]
fn wait_registration_and_timeout() {
    let h = Harness::new();
    let (tid, idn, address, mb) = setup(&h);
    let k = reader(&h, &tid);
    // kind=verification needs from.
    let r = get(
        &h,
        &k,
        &format!("/v1/identities/{idn}/wait?kind=verification&timeout=1"),
    );
    assert_eq!(r.status, 400);
    // Nothing arrives: timed out, and the domain is registered.
    let r = get(
        &h,
        &k,
        &format!("/v1/identities/{idn}/wait?from=@sender.example&kind=verification&timeout=3"),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["timed_out"], true);
    let meta = h.mailbox_sql(&mb, "SELECT k, v FROM meta WHERE k LIKE 'wait:%'");
    assert_eq!(meta.len(), 1, "{meta:?}");
    assert_eq!(meta[0]["k"], "wait:sender.example", "{meta:?}");
    // An OTP from that domain now is solicited (E5): received, not quarantined.
    let otp = plain(
        &address,
        "otp2@sender.example",
        "Your verification code",
        "Your verification code is 481207.",
    );
    email(&h, "alex@sender.example", &address, &otp);
    drain(&h, QueueName::Inbound);
    assert_eq!(messages(&h, &mb)[0]["status"], "received");
    // wait finds it; the code is withheld because the mail is not authenticated (E4).
    let r = get(
        &h,
        &k,
        &format!(
            "/v1/identities/{idn}/wait?from=@sender.example&since=2000-01-01T00:00:00Z&timeout=2"
        ),
    );
    assert_eq!(r.body["timed_out"], false, "{}", r.body);
    assert_eq!(r.body["verification"], Value::Null);
    assert_eq!(r.body["message"]["subject"], "Your verification code");
    // subject_contains narrows.
    let r = get(
        &h,
        &k,
        &format!(
            "/v1/identities/{idn}/wait?subject_contains=invoice&since=2000-01-01T00:00:00Z&timeout=1"
        ),
    );
    assert_eq!(r.body["timed_out"], true);
}

#[test]
fn attachment_download_and_text() {
    let h = Harness::new();
    let (tid, idn, address, mb) = setup(&h);
    let k = reader(&h, &tid);
    let raw = with_attachment(
        &address,
        "note.txt",
        "text/plain",
        "SW52b2ljZSBJTlYtODgyMTMgZm9yIHRoZSByZW50YWwu",
    );
    email(&h, "alex@sender.example", &address, &raw);
    drain(&h, QueueName::Inbound);
    drain(&h, QueueName::Index);
    let mid = messages(&h, &mb)[0]["id"].as_str().unwrap().to_owned();
    let att = attachments(&h, &mb)[0]["id"].as_str().unwrap().to_owned();
    let r = get(
        &h,
        &k,
        &format!("/v1/identities/{idn}/messages/{mid}/attachments/{att}"),
    );
    assert_eq!(r.status, 200);
    assert!(
        r.header("content-disposition")
            .unwrap()
            .contains("note.txt")
    );
    let t = get(
        &h,
        &k,
        &format!("/v1/identities/{idn}/messages/{mid}/attachments/{att}/text"),
    );
    assert_eq!(t.status, 200, "{}", t.body);
    assert_eq!(t.body["status"], "ready");
    assert!(
        t.body["pages"][0]["text"]
            .as_str()
            .unwrap()
            .contains("INV-88213")
    );
}
