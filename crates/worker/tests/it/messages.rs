//! The read path, labels and read state (Inbound › Read path and release, FR-IN-10).

use serde_json::{Value, json};

use crate::{fakes, get, inject, mail, mailbox_fixture, patch, post, wait_stored};

/// An OTP mail from a domain with no `wait`: quarantined `otp_unsolicited` (E5).
pub fn held(from: &str, to: &str, subject: &str) -> String {
    mail(from, to, subject, "Your verification code is 918273.")
}

fn threads(idn: &str, key: &str, q: &str) -> Vec<Value> {
    let r = get(&format!("/v1/identities/{idn}/threads?limit=100{q}"), key);
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["data"].as_array().cloned().unwrap_or_default()
}

#[test]
fn read_path_visible_threads() {
    let _reset = fakes::Reset;
    let (tid, idn, addr, key) = mailbox_fixture("readpath");
    assert_eq!(
        inject(
            "a@q.example",
            &addr,
            &held("Ann <a@q.example>", &addr, "Held one")
        ),
        200
    );
    let q = wait_stored(&idn, &key, "quarantined", 1);
    // The thread of a quarantined message is not listed.
    assert!(threads(&idn, &key, "").is_empty());
    let mid = q[0]["id"].as_str().unwrap();
    // Release needs PM_QUARANTINE_KEY_RELEASE (off here): the tenant opts in, a platform key releases.
    let p = crate::platform();
    let r = patch(
        &format!("/v1/tenants/{tid}/policy"),
        &p,
        json!({ "quarantine": { "key_release": true } }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let r = post(
        &format!("/v1/identities/{idn}/messages/{mid}/release"),
        &p,
        json!({ "reason": "expected" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let t = threads(&idn, &key, "");
    assert_eq!(t.len(), 1);
    assert_eq!(
        (t[0]["subject"].as_str(), t[0]["last_direction"].as_str()),
        (Some("Held one"), Some("inbound"))
    );
    assert_eq!(
        (
            t[0]["message_count"].as_i64(),
            t[0]["unread_count"].as_i64()
        ),
        (Some(1), Some(1))
    );
    assert!(
        t[0]["snippet"]
            .as_str()
            .unwrap()
            .contains("verification code")
    );
    assert_eq!(t[0]["participants"][0]["address"], "a@q.example");
    // include=headers reads the raw MIME.
    let thr = t[0]["id"].as_str().unwrap();
    let g = get(
        &format!("/v1/identities/{idn}/threads/{thr}?include=headers"),
        &key,
    );
    assert!(
        g.body["messages"][0]["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["name"] == "Message-ID")
    );
    // Cursors: bound to the filters and the key, expired after 24 hours.
    assert_eq!(
        inject(
            "b@q.example",
            &addr,
            &mail("b@q.example", &addr, "Second", "x")
        ),
        200
    );
    wait_stored(&idn, &key, "received", 2);
    let page = get(&format!("/v1/identities/{idn}/threads?limit=1"), &key);
    let cursor = page.body["next_cursor"].as_str().unwrap().to_owned();
    assert_eq!(
        get(
            &format!("/v1/identities/{idn}/threads?limit=1&cursor={cursor}"),
            &key
        )
        .status,
        200
    );
    assert_eq!(
        get(
            &format!("/v1/identities/{idn}/threads?limit=1&is_unread=true&cursor={cursor}"),
            &key
        )
        .status,
        400
    );
    assert_eq!(
        get(
            &format!("/v1/identities/{idn}/threads?limit=1&cursor={cursor}"),
            &p
        )
        .status,
        400
    );
    fakes::advance(25 * 3_600_000);
    let r = get(
        &format!("/v1/identities/{idn}/threads?limit=1&cursor={cursor}"),
        &key,
    );
    assert_eq!((r.status, r.code()), (410, "cursor_expired"));
}

#[test]
fn labels_and_read_state() {
    let (tid, idn, addr, key) = mailbox_fixture("labels");
    let first = mail("Ann <a@l.example>", &addr, "Labels", "first");
    assert_eq!(inject("a@l.example", &addr, &first), 200);
    let m = wait_stored(&idn, &key, "received", 1);
    let mid = m[0]["id"].as_str().unwrap().to_owned();
    let thr = m[0]["thread_id"].as_str().unwrap().to_owned();
    let r = patch(
        &format!("/v1/identities/{idn}/messages/{mid}"),
        &key,
        json!({ "labels_add": ["todo", "crm:x"], "read": true }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["labels"], json!(["crm:x", "todo"]));
    assert!(threads(&idn, &key, "&is_unread=true").is_empty());
    assert_eq!(threads(&idn, &key, "&label=todo").len(), 1);
    let r = patch(
        &format!("/v1/identities/{idn}/threads/{thr}"),
        &key,
        json!({ "labels_remove": ["todo"], "read": false }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        (r.body["unread_count"].as_i64(), r.body["labels"].clone()),
        (Some(1), json!(["crm:x"]))
    );
    // The 65th label.
    let many: Vec<String> = (0..63).map(|i| format!("l{i}")).collect();
    assert_eq!(
        patch(
            &format!("/v1/identities/{idn}/messages/{mid}"),
            &key,
            json!({ "labels_add": many })
        )
        .status,
        200
    );
    let r = patch(
        &format!("/v1/identities/{idn}/messages/{mid}"),
        &key,
        json!({ "labels_add": ["one-more"] }),
    );
    assert_eq!((r.status, r.code()), (400, "invalid_request"));
    // Archived until new visible mail arrives in the thread.
    assert_eq!(
        patch(
            &format!("/v1/identities/{idn}/threads/{thr}"),
            &key,
            json!({ "archived": true })
        )
        .status,
        200
    );
    assert!(threads(&idn, &key, "").is_empty());
    let id = first
        .lines()
        .find(|l| l.starts_with("Message-ID:"))
        .unwrap()["Message-ID: ".len()..]
        .to_owned();
    let reply = mail("Ann <a@l.example>", &addr, "Re: Labels", "second").replacen(
        "Date:",
        &format!("In-Reply-To: {id}\r\nDate:"),
        1,
    );
    assert_eq!(inject("a@l.example", &addr, &reply), 200);
    wait_stored(&idn, &key, "received", 2);
    assert_eq!(threads(&idn, &key, "")[0]["id"], thr.as_str());
    // A message that is not visible needs quarantine:review.
    assert_eq!(
        inject("z@l.example", &addr, &held("z@l.example", &addr, "Held")),
        200
    );
    let q = wait_stored(&idn, &key, "quarantined", 1);
    let hid = q[0]["id"].as_str().unwrap();
    let (_, plain) = crate::mint(
        &crate::platform(),
        json!({ "name": "plain", "level": "tenant", "tenant_id": tid, "permissions": ["messages:read", "messages:write"] }),
    );
    assert_eq!(
        patch(
            &format!("/v1/identities/{idn}/messages/{hid}"),
            &plain,
            json!({ "read": true })
        )
        .status,
        404
    );
    assert_eq!(
        patch(
            &format!("/v1/identities/{idn}/messages/{hid}"),
            &key,
            json!({ "read": true })
        )
        .status,
        200
    );
}

#[test]
fn thread_counts_visible_only() {
    let (_, idn, addr, key) = mailbox_fixture("counts");
    let first = mail("Ann <a@c.example>", &addr, "Counts", "first");
    assert_eq!(inject("a@c.example", &addr, &first), 200);
    wait_stored(&idn, &key, "received", 1);
    let id = first
        .lines()
        .find(|l| l.starts_with("Message-ID:"))
        .unwrap()["Message-ID: ".len()..]
        .to_owned();
    // A held reply in the same thread changes none of its visible columns.
    let reply = held("Ann <a@c.example>", &addr, "Re: Counts").replacen(
        "Date:",
        &format!("In-Reply-To: {id}\r\nDate:"),
        1,
    );
    assert_eq!(inject("a@c.example", &addr, &reply), 200);
    wait_stored(&idn, &key, "quarantined", 1);
    let t = threads(&idn, &key, "");
    assert_eq!(
        (
            t[0]["message_count"].as_i64(),
            t[0]["unread_count"].as_i64()
        ),
        (Some(1), Some(1))
    );
    assert!(t[0]["snippet"].as_str().unwrap().contains("first"));
    let thr = t[0]["id"].as_str().unwrap();
    let g = get(&format!("/v1/identities/{idn}/threads/{thr}"), &key);
    // With review the held message is in the thread's pages, never in its counts.
    assert_eq!(g.body["messages"].as_array().unwrap().len(), 2);
    assert_eq!(g.body["message_count"], 1);
}
