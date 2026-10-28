//! `it::testmode` (L1–L4): test tenants never reach a real transport.

use serde_json::json;

use crate::send::{events_of, open_sender, send, send_ok, wait_status};
use crate::{fakes, mailbox_query};

#[test]
fn l1_refuse_external() {
    let s = open_sender("l1 external", "test");
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "x", "text": "x" }),
    );
    assert_eq!(r.status, 403, "{}", r.body);
    assert_eq!(r.code(), "test_mode_recipient");
    assert!(
        fakes::mail_sent("jo@example.net")
            .iter()
            .all(|m| m["subject"] != "x" || m["from"] != s.addr)
    );
}

#[test]
fn l2_simulator_matrix() {
    let s = open_sender("l2 matrix", "test");
    for (local, status) in [
        ("delivered", "delivered"),
        ("bounce", "bounced"),
        ("softbounce", "bounced"),
        ("complaint", "complained"),
        ("deferred", "deferred"),
        ("reject", "rejected"),
        ("timeout", "uncertain"),
        ("anything", "delivered"),
    ] {
        let mid = send_ok(
            &s,
            &[&format!("{local}@simulator.invalid")],
            &format!("l2 {local}"),
        );
        let m = wait_status(&s, &mid, status);
        if !matches!(local, "reject" | "timeout") {
            assert_eq!(
                m["provider_message_id"].as_str().unwrap(),
                format!("test-{}", mid[4..].to_ascii_lowercase())
            );
        }
    }
    assert!(events_of(&s.iid, "message.delivered").len() >= 2);
}

#[test]
fn l3_loopback() {
    let s = open_sender("l3 loop", "test");
    let peer = crate::send::sender_in(
        &s.tid,
        "l3 peer",
        json!({ "require_known_recipient": false }),
    );
    let mid = send_ok(&s, &[&peer.addr], "l3 loopback");
    let m = wait_status(&s, &mid, "delivered");
    assert!(
        m["rfc_message_id"]
            .as_str()
            .unwrap()
            .ends_with(s.addr.rsplit('@').next().unwrap())
    );
    let got = crate::wait_stored(&peer.iid, &peer.key, "received", 1);
    assert_eq!(got[0]["subject"], "l3 loopback");
    assert!(
        got[0]["flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "loopback")
    );
    assert_eq!(
        mailbox_query(
            &s.iid,
            &format!("SELECT provider FROM messages WHERE id = '{mid}'")
        )[0]["provider"],
        "loopback"
    );
}

#[test]
fn l4_mode_binding() {
    // A tenant's mode decides the transport: a live tenant's send to the simulator domain goes to the
    // provider like any other address, and a test tenant never reaches the provider.
    let live = open_sender("l4 live", "live");
    send_ok(&live, &["delivered@simulator.invalid"], "l4 live");
    crate::eventually(30, "the provider", || {
        (!fakes::mail_sent("delivered@simulator.invalid").is_empty()).then_some(())
    });
    let test = open_sender("l4 test", "test");
    let before = fakes::mail_sent("delivered@simulator.invalid").len();
    let mid = send_ok(&test, &["delivered@simulator.invalid"], "l4 test");
    wait_status(&test, &mid, "delivered");
    assert_eq!(
        fakes::mail_sent("delivered@simulator.invalid").len(),
        before
    );
}
