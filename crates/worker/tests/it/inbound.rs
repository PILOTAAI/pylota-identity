//! Inbound mail in workerd (Inbound, build plan M7): `email()`, the `pm-inbound` consumer and the
//! mailbox's ingest, observed through the read API.

use serde_json::Value;

use crate::{eventually, get, inbound, inject, mail, mailbox_fixture, messages_of, wait_stored};

/// B14: the same raw message delivered twice is stored once, with one `message.received`.
#[test]
fn b14_redelivery_deduped() {
    let (_, idn, addr, key) = mailbox_fixture("b14");
    let raw = mail(
        "Jo <jo@sender.example>",
        &addr,
        "B14 redelivery",
        "Hello twice.",
    );
    assert_eq!(inject("jo@sender.example", &addr, &raw), 200);
    wait_stored(&idn, &key, "received", 1);
    assert_eq!(inject("jo@sender.example", &addr, &raw), 200);
    std::thread::sleep(std::time::Duration::from_secs(4));
    assert_eq!(messages_of(&idn, &key, "").len(), 1);
    crate::alarm(&idn);
    let received = crate::indexed(&idn)
        .into_iter()
        .filter(|(t, _)| t == "message.received")
        .count();
    assert_eq!(received, 1);
}

/// A6: unknown and erased addresses get `550 5.1.1`, a retired one `550 5.1.6`; a suspended
/// tenant's mail is a temporary failure for 5 days, then `550 5.2.1`.
#[test]
fn a6_reject_codes() {
    let (tid, _, addr, _) = mailbox_fixture("a6");
    let raw = |to: &str| mail("jo@sender.example", to, "A6", "x");
    let unknown = "nobody-a6.unknown@agents.example";
    let (o, smtp) = inbound("jo@sender.example", unknown, &raw(unknown));
    assert_eq!((o.as_str(), &smtp[..9]), ("rejected", "550 5.1.1"));
    // The local endpoint reports setReject as 400.
    assert_eq!(inject("jo@sender.example", unknown, &raw(unknown)), 400);
    // Suspended: temporary failure, then 550 5.2.1 after 5 days.
    let p = crate::platform();
    let r = crate::patch(
        &format!("/v1/tenants/{tid}"),
        &p,
        serde_json::json!({ "status": "suspended" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let (o, _) = inbound("jo@sender.example", &addr, &raw(&addr));
    assert_eq!(o, "tempfail");
    let _reset = crate::fakes::Reset;
    crate::fakes::advance(5 * 86_400_000 + 60_000);
    let (o, smtp) = inbound("jo@sender.example", &addr, &raw(&addr));
    assert_eq!(
        (o.as_str(), &smtp[..9]),
        ("rejected", "550 5.2.1"),
        "{smtp}"
    );
}

/// A10: an envelope recipient missing from `To` and `Cc` is a BCC copy, flagged `bcc`.
#[test]
fn a10_bcc_copy_flagged() {
    let (_, idn, addr, key) = mailbox_fixture("a10");
    let raw = mail(
        "jo@sender.example",
        "someone@else.example",
        "A10 bcc",
        "copy",
    );
    assert_eq!(inject("jo@sender.example", &addr, &raw), 200);
    let m = wait_stored(&idn, &key, "received", 1);
    let flags: Vec<&str> = m[0]["flags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(flags.contains(&"bcc"), "{flags:?}");
    let full = get(
        &format!(
            "/v1/identities/{idn}/messages/{}",
            m[0]["id"].as_str().unwrap()
        ),
        &key,
    );
    assert_eq!(full.body["is_primary_recipient"], true);
}

fn id_of(raw: &str) -> String {
    raw.lines().find(|l| l.starts_with("Message-ID:")).unwrap()["Message-ID: ".len()..].to_owned()
}

fn reply_to(raw: &str, id: &str) -> String {
    raw.replacen("Date:", &format!("In-Reply-To: {id}\r\nDate:"), 1)
}

fn flags(m: &Value) -> Vec<String> {
    let mut f: Vec<String> = m["flags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    f.extend(
        m["trust"]["flags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned)),
    );
    f
}

fn full(idn: &str, key: &str, mid: &str) -> Value {
    get(&format!("/v1/identities/{idn}/messages/{mid}"), key).body
}

/// Every row of a mailbox's `messages`, whatever its status.
fn rows(idn: &str) -> Vec<Value> {
    crate::mailbox_query(
        idn,
        "SELECT id, status, flags_json, subject, is_primary_recipient, rfc_message_id, thread_seq FROM messages ORDER BY rowid",
    )
}

fn settle() {
    std::thread::sleep(std::time::Duration::from_secs(4));
}

/// A tagged address: `local+detail@domain`.
fn tagged(addr: &str, detail: &str) -> String {
    let (l, d) = addr.split_once('@').unwrap();
    format!("{l}+{detail}@{d}")
}

#[test]
fn a2_forged_token_ignored() {
    let (_, idn, addr, key) = mailbox_fixture("a2");
    let first = mail("a@a2.example", &addr, "A2 first", "x");
    assert_eq!(inject("a@a2.example", &addr, &first), 200);
    wait_stored(&idn, &key, "received", 1);
    // A forged token on a header reply: filed in the same identity, threaded by its headers.
    let to = tagged(&addr, "t03k.9f2mq7xa");
    let reply = reply_to(
        &mail("a@a2.example", &to, "Re: A2 first", "y"),
        &id_of(&first),
    );
    assert_eq!(inject("a@a2.example", &to, &reply), 200);
    let m = wait_stored(&idn, &key, "received", 2);
    assert_eq!(m[0]["thread_id"], m[1]["thread_id"]);
}

#[test]
fn a9_two_identities_two_copies() {
    let (tid, idn, addr, key) = mailbox_fixture("a9");
    let other = crate::identity(&crate::platform(), &tid, &crate::slug("a9b")[..8]);
    let (idn2, addr2) = (
        other["id"].as_str().unwrap().to_owned(),
        other["primary_address"].as_str().unwrap().to_owned(),
    );
    let raw = mail("a@a9.example", &format!("{addr}, {addr2}"), "A9 both", "x");
    assert_eq!(inject("a@a9.example", &addr, &raw), 200);
    assert_eq!(inject("a@a9.example", &addr2, &raw), 200);
    let one = wait_stored(&idn, &key, "received", 1);
    let two = wait_stored(&idn2, &key, "received", 1);
    let p1 = full(&idn, &key, one[0]["id"].as_str().unwrap())["is_primary_recipient"].as_bool();
    let p2 = full(&idn2, &key, two[0]["id"].as_str().unwrap())["is_primary_recipient"].as_bool();
    assert_eq!((p1, p2), (Some(true), Some(false)));
}

#[test]
fn b3_missing_message_id() {
    let (_, idn, addr, key) = mailbox_fixture("b3none");
    let raw = format!(
        "From: a@b3.example\r\nTo: {addr}\r\nSubject: No ID\r\nDate: Mon, 26 Oct 2026 10:00:00 +0000\r\n\r\nbody\r\n"
    );
    assert_eq!(inbound("a@b3.example", &addr, &raw).0, "accepted");
    let m = wait_stored(&idn, &key, "received", 1);
    assert!(
        full(&idn, &key, m[0]["id"].as_str().unwrap())["rfc_message_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
}

#[test]
fn b3_same_id_same_body() {
    let (_, idn, addr, key) = mailbox_fixture("b3same");
    let raw = mail("a@b3.example", &addr, "B3 same", "same body");
    assert_eq!(inject("a@b3.example", &addr, &raw), 200);
    wait_stored(&idn, &key, "received", 1);
    let relayed = format!(
        "Received: from relay.example by mx.example; Mon, 26 Oct 2026 10:00:01 +0000\r\n{raw}"
    );
    assert_eq!(inject("a@b3.example", &addr, &relayed), 200);
    settle();
    assert_eq!(rows(&idn).len(), 1);
}

#[test]
fn b3_same_id_different_body() {
    let (_, idn, addr, key) = mailbox_fixture("b3diff");
    let raw = mail("a@b3.example", &addr, "B3 diff", "first body");
    assert_eq!(inject("a@b3.example", &addr, &raw), 200);
    wait_stored(&idn, &key, "received", 1);
    assert_eq!(
        inject(
            "a@b3.example",
            &addr,
            &raw.replace("first body", "another body")
        ),
        200
    );
    let m = wait_stored(&idn, &key, "received", 2);
    assert!(
        m.iter()
            .any(|m| flags(m).iter().any(|f| f == "message_id_conflict")),
        "{m:?}"
    );
}

#[test]
fn c9_header_join_flagged() {
    let (_, idn, addr, key) = mailbox_fixture("c9");
    let first = mail("a@c9.example", &addr, "C9", "x");
    assert_eq!(inject("a@c9.example", &addr, &first), 200);
    wait_stored(&idn, &key, "received", 1);
    let stranger = reply_to(
        &mail("s@stranger.example", &addr, "Re: C9", "y"),
        &id_of(&first),
    );
    assert_eq!(inject("s@stranger.example", &addr, &stranger), 200);
    let m = wait_stored(&idn, &key, "received", 2);
    assert_eq!(m[0]["thread_id"], m[1]["thread_id"]);
    let joined = m.iter().find(|m| m["subject"] == "Re: C9").unwrap();
    assert!(
        flags(joined).iter().any(|f| f == "thread_join_unverified"),
        "{joined}"
    );
}

#[test]
fn c10_forged_participant_unverified() {
    let (_, idn, addr, key) = mailbox_fixture("c10");
    // An unauthenticated "participant" is not one: the forged reply's join is flagged.
    let first = mail("a@c10.example", &addr, "C10", "x");
    assert_eq!(inject("a@c10.example", &addr, &first), 200);
    wait_stored(&idn, &key, "received", 1);
    let forged = reply_to(
        &mail("a@c10.example", &addr, "Re: C10", "y"),
        &id_of(&first),
    );
    assert_eq!(inject("attacker@evil.example", &addr, &forged), 200);
    let m = wait_stored(&idn, &key, "received", 2);
    let joined = m.iter().find(|m| m["subject"] == "Re: C10").unwrap();
    assert!(flags(joined).iter().any(|f| f == "thread_join_unverified"));
    assert_eq!(joined["trust"]["known_sender"], false);
    // An authenticated participant's reply joins unflagged.
    let signed = crate::dkim::sign(&mail(
        "security@brightwell.example",
        &addr,
        "C10 signed",
        "z",
    ));
    assert_eq!(inject("bounce@brightwell.example", &addr, &signed), 200);
    wait_stored(&idn, &key, "received", 3);
    let reply = crate::dkim::sign(&reply_to(
        &mail("security@brightwell.example", &addr, "Re: C10 signed", "w"),
        &id_of(&signed),
    ));
    assert_eq!(inject("bounce@brightwell.example", &addr, &reply), 200);
    let m = wait_stored(&idn, &key, "received", 4);
    let ok = m.iter().find(|m| m["subject"] == "Re: C10 signed").unwrap();
    assert!(
        !flags(ok).iter().any(|f| f == "thread_join_unverified"),
        "{ok}"
    );
}

#[test]
fn d10_token_bruteforce() {
    let (_, idn, addr, key) = mailbox_fixture("d10");
    let to = tagged(&addr, "t03k.9f2mq7xa");
    for i in 0..12 {
        assert_eq!(
            inject(
                "x@d10.example",
                &to,
                &mail("x@d10.example", &to, &format!("D10 {i}"), "x")
            ),
            200
        );
    }
    let m = wait_stored(&idn, &key, "received", 12);
    let flagged = m
        .iter()
        .filter(|m| flags(m).iter().any(|f| f == "thread_join_unverified"))
        .count();
    // Ten failed checks per sender an hour are flagged; beyond them the token is not checked.
    assert_eq!(flagged, 10, "{m:?}");
}

#[test]
fn c11_token_flood_keeps_replies() {
    let (_, idn, addr, key) = mailbox_fixture("c11");
    let signed = crate::dkim::sign(&mail("security@brightwell.example", &addr, "C11", "x"));
    assert_eq!(inject("bounce@brightwell.example", &addr, &signed), 200);
    wait_stored(&idn, &key, "received", 1);
    let to = tagged(&addr, "t03k.9f2mq7xa");
    for i in 0..101 {
        let from = format!("j{i}@junk.example");
        assert_eq!(
            inject(&from, &to, &mail(&from, &to, &format!("junk {i}"), "x")),
            200
        );
    }
    wait_stored(&idn, &key, "received", 50);
    let reply = crate::dkim::sign(&reply_to(
        &mail("security@brightwell.example", &to, "Re: C11", "real"),
        &id_of(&signed),
    ));
    assert_eq!(inject("bounce@brightwell.example", &to, &reply), 200);
    let r = eventually(60, "the real reply", || {
        let m = crate::messages_of(&idn, &key, "&status=received");
        m.into_iter().find(|m| m["subject"] == "Re: C11")
    });
    assert!(
        !flags(&r).iter().any(|f| f == "thread_join_unverified"),
        "{r}"
    );
}

#[test]
fn d5_sender_throttle() {
    let (_, idn, addr, _) = mailbox_fixture("d5");
    for i in 0..61 {
        assert_eq!(
            inject(
                "one@d5.example",
                &addr,
                &mail("one@d5.example", &addr, &format!("D5 {i}"), "x")
            ),
            200
        );
    }
    let r = eventually(90, "61 rows", || {
        let r = rows(&idn);
        (r.len() >= 61).then_some(r)
    });
    let throttled = r.iter().filter(|m| m["status"] == "throttled").count();
    assert_eq!(throttled, 1, "{r:?}");
}

#[test]
fn d13_unsolicited_flood_caps() {
    let (_, idn, addr, key) = mailbox_fixture("d13");
    for i in 0..122 {
        let from = format!("f{i}@forged{i}.example");
        assert_eq!(
            inject(&from, &addr, &mail(&from, &addr, &format!("D13 {i}"), "x")),
            200
        );
    }
    let r = eventually(120, "122 rows", || {
        let r = rows(&idn);
        (r.len() >= 122).then_some(r)
    });
    assert_eq!(
        r.iter().filter(|m| m["status"] == "throttled").count(),
        2,
        "the 121st onwards"
    );
    // An authenticated correspondent in the same hour is not capped.
    let signed = crate::dkim::sign(&mail(
        "security@brightwell.example",
        &addr,
        "D13 known",
        "x",
    ));
    assert_eq!(inject("bounce@brightwell.example", &addr, &signed), 200);
    eventually(30, "the authenticated message", || {
        crate::messages_of(&idn, &key, "&status=received")
            .into_iter()
            .find(|m| m["subject"] == "D13 known")
    });
    // With `TenantQuota` unavailable, mail is still admitted.
    let _reset = crate::fakes::Reset;
    crate::fakes::fault("do.call", "QUOTA", "error", 1);
    let signed = crate::dkim::sign(&mail(
        "security@brightwell.example",
        &addr,
        "D13 quota down",
        "x",
    ));
    assert_eq!(inject("bounce@brightwell.example", &addr, &signed), 200);
    eventually(30, "the message admitted without TenantQuota", || {
        crate::messages_of(&idn, &key, "&status=received")
            .into_iter()
            .find(|m| m["subject"] == "D13 quota down")
    });
    // Throttled mail has no event.
    crate::alarm(&idn);
    let evented = crate::indexed(&idn)
        .into_iter()
        // `message.triaged` follows asynchronously (M12) and is not what this counts.
        .filter(|(t, _)| t.starts_with("message.") && t != "message.triaged")
        .count();
    assert_eq!(evented, 122);
}

#[test]
fn d4_backscatter_dropped() {
    let (_, idn, addr, _) = mailbox_fixture("d4");
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../conformance/corpus/dsn/d4_dsn_delayed_full.eml"),
    )
    .unwrap();
    assert_eq!(inject("mailer-daemon@example.com", &addr, &raw), 200);
    settle();
    settle();
    assert!(
        rows(&idn).is_empty(),
        "a DSN naming no message of ours is not stored"
    );
}

#[test]
fn e5_unsolicited_otp() {
    let (_, idn, addr, key) = mailbox_fixture("e5");
    let otp = |code: &str| {
        mail(
            "noreply@svc.example",
            &addr,
            "Your verification code",
            &format!("Your verification code is {code}."),
        )
    };
    assert_eq!(inject("noreply@svc.example", &addr, &otp("112 233")), 200);
    let q = wait_stored(&idn, &key, "quarantined", 1);
    assert_eq!(q[0]["quarantine_reason"], "otp_unsolicited");
    // With a wait for that domain: received.
    let r = get(
        &format!("/v1/identities/{idn}/wait?from=@svc.example&timeout=1"),
        &key,
    );
    assert_eq!(r.body["timed_out"], true);
    assert_eq!(inject("noreply@svc.example", &addr, &otp("445 566")), 200);
    wait_stored(&idn, &key, "received", 1);
}

#[test]
fn j1_r2_failure() {
    let _reset = crate::fakes::Reset;
    let (_, idn, addr, _) = mailbox_fixture("j1");
    crate::fakes::fault_any("r2.put", "error", 3);
    let (o, _) = inbound(
        "a@j1.example",
        &addr,
        &mail("a@j1.example", &addr, "J1", "x"),
    );
    assert_eq!(o, "tempfail");
    settle();
    assert!(rows(&idn).is_empty());
}

fn with_attachment(to: &str) -> String {
    format!(
        "From: a@j2.example\r\nTo: {to}\r\nSubject: J2\r\nMessage-ID: <{}@it.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n--b\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=\"note.txt\"\r\n\r\nInvoice INV-88213\r\n--b--\r\n",
        crate::slug("j2")
    )
}

#[test]
fn j2_retry_idempotent() {
    let _reset = crate::fakes::Reset;
    let (_, idn, addr, key) = mailbox_fixture("j2");
    crate::fakes::fault_any("inbound.after_ingest", "panic", 1);
    assert_eq!(inject("a@j2.example", &addr, &with_attachment(&addr)), 200);
    let m = wait_stored(&idn, &key, "received", 1);
    let mid = m[0]["id"].as_str().unwrap().to_owned();
    let att = m[0]["attachments"][0]["id"].as_str().unwrap().to_owned();
    eventually(30, "the attachment written by the retry", || {
        (get(
            &format!("/v1/identities/{idn}/messages/{mid}/attachments/{att}"),
            &key,
        )
        .status
            == 200)
            .then_some(())
    });
    assert_eq!(rows(&idn).len(), 1);
}

#[test]
fn j7_d1_transient() {
    let _reset = crate::fakes::Reset;
    let (_, idn, addr, key) = mailbox_fixture("j7");
    crate::fakes::fault("d1", "DB", "error", 1);
    let (o, _) = inbound(
        "a@j7.example",
        &addr,
        &mail("a@j7.example", &addr, "J7 staged", "x"),
    );
    assert_eq!(o, "accepted");
    let m = wait_stored(&idn, &key, "received", 1);
    assert_eq!(m[0]["subject"], "J7 staged");
    // Staged mail to an address that does not exist is dropped by the consumer.
    crate::fakes::fault("d1", "DB", "error", 1);
    let unknown = "nobody-j7.unknown@agents.example";
    assert_eq!(
        inbound(
            "a@j7.example",
            unknown,
            &mail("a@j7.example", unknown, "J7 unroutable", "x")
        )
        .0,
        "accepted"
    );
}

#[test]
fn j7_staging_outlives_dlq() {
    let _reset = crate::fakes::Reset;
    let (_, idn, addr, key) = mailbox_fixture("j7dlq");
    let before = crate::fakes::queued("pm-inbound-dlq").len();
    // D1 down for the handler and for every delivery of the staged pointer: it is dead-lettered. The
    // budget is large because every consumer running meanwhile (webhooks too) takes from it.
    crate::fakes::fault("d1", "DB", "error", 400);
    assert_eq!(
        inject(
            "a@j7.example",
            &addr,
            &mail("a@j7.example", &addr, "J7 redriven", "x")
        ),
        200
    );
    let pointer = eventually(120, "the dead-lettered pointer", || {
        crate::fakes::queued("pm-inbound-dlq")
            .into_iter()
            .skip(before)
            .find(|p| p["envelope_to"] == addr.as_str())
    });
    assert_eq!(pointer["kind"], "staged", "{pointer}");
    crate::fakes::reset();
    // Redriven 14 days later: the staged object is still there, and the message is stored.
    crate::fakes::advance(14 * 86_400_000);
    crate::queue_send("pm-inbound", pointer);
    let m = wait_stored(&idn, &key, "received", 1);
    assert_eq!(m[0]["subject"], "J7 redriven");
}

#[test]
fn panic_to_degraded() {
    let _reset = crate::fakes::Reset;
    let (_, idn, addr, key) = mailbox_fixture("panic");
    crate::fakes::fault_any("parse.panic", "panic", 2);
    assert_eq!(
        inject(
            "a@p.example",
            &addr,
            &mail("a@p.example", &addr, "Panics", "x")
        ),
        200
    );
    // A panic ends every request in flight in the isolate: give the two panicking attempts time.
    std::thread::sleep(std::time::Duration::from_secs(8));
    let m = eventually(90, "the degraded message", || {
        crate::messages_of(&idn, &key, "&status=received")
            .into_iter()
            .find(|m| m["subject"] == "Panics")
    });
    assert!(flags(&m).iter().any(|f| f == "parse_degraded"), "{m}");
    // A healthy message afterwards parses in full.
    assert_eq!(
        inject(
            "a@p.example",
            &addr,
            &mail("a@p.example", &addr, "Healthy", "x")
        ),
        200
    );
    let h = eventually(30, "the healthy message", || {
        crate::messages_of(&idn, &key, "&status=received")
            .into_iter()
            .find(|m| m["subject"] == "Healthy")
    });
    assert!(!flags(&h).iter().any(|f| f == "parse_degraded"));
}

/// NFR-REL-1 canary: under the J1, J2 and J7 faults, every message `email()` accepted is found
/// exactly once once the queues drain.
#[test]
fn nfr_rel1_no_loss_canary() {
    let _reset = crate::fakes::Reset;
    let (tid, idn, addr, key) = mailbox_fixture("canary");
    let send = |i: usize| {
        let raw = mail("a@canary.example", &addr, &format!("canary-{i}"), "x");
        inbound("a@canary.example", &addr, &raw).0
    };
    crate::fakes::fault_any("r2.put", "error", 1);
    assert_eq!(send(0), "accepted", "one failed put is retried in email()");
    // An address this isolate has not looked up yet, so its lookup reaches D1 and the mail is staged.
    let other = crate::identity(&crate::platform(), &tid, &crate::slug("canary2")[..8]);
    let oid = other["id"].as_str().unwrap().to_owned();
    let oaddr = other["primary_address"].as_str().unwrap().to_owned();
    crate::fakes::fault("d1", "DB", "error", 1);
    let staged = mail("a@canary.example", &oaddr, "canary-staged", "x");
    assert_eq!(inbound("a@canary.example", &oaddr, &staged).0, "accepted");
    assert_eq!(send(1), "accepted");
    crate::fakes::fault_any("inbound.after_ingest", "panic", 1);
    assert_eq!(send(2), "accepted");
    assert_eq!(send(3), "accepted");
    let m = wait_stored(&idn, &key, "received", 4);
    settle();
    let m2 = crate::messages_of(&idn, &key, "&status=received");
    assert_eq!(m.len(), 4);
    for i in 0..4 {
        assert_eq!(
            m2.iter()
                .filter(|m| m["subject"] == format!("canary-{i}").as_str())
                .count(),
            1
        );
    }
    let s = wait_stored(&oid, &key, "received", 1);
    assert_eq!(
        (s.len(), s[0]["subject"].as_str()),
        (1, Some("canary-staged"))
    );
}

/// A16: misses are cached, and above 20 misses a second on one domain unknown addresses get a
/// temporary failure; a known address still gets through.
#[test]
fn a16_dictionary_guard() {
    let (_, idn, addr, key) = mailbox_fixture("a16");
    // The known address is looked up, and cached, before the flood.
    let before = mail("x@a16.example", &addr, "A16 before", "x");
    assert_eq!(inbound("x@a16.example", &addr, &before).0, "accepted");
    let mut outcomes = Vec::new();
    for i in 0..50 {
        let to = format!("guess{i}-{}@agents.example", crate::slug("a16"));
        outcomes.push(
            inbound(
                "x@a16.example",
                &to,
                &mail("x@a16.example", &to, "guess", "x"),
            )
            .0,
        );
    }
    let rejected = outcomes.iter().filter(|o| *o == "rejected").count();
    let tempfail = outcomes.iter().filter(|o| *o == "tempfail").count();
    assert!(
        rejected >= 20 && tempfail > 0 && rejected + tempfail == 50,
        "{rejected} rejected, {tempfail} tempfail"
    );
    assert_eq!(
        inbound(
            "x@a16.example",
            &addr,
            &mail("x@a16.example", &addr, "A16 known", "x")
        )
        .0,
        "accepted"
    );
    wait_stored(&idn, &key, "received", 2);
    // The guard counts per second: the next tests start in a fresh one.
    std::thread::sleep(std::time::Duration::from_millis(1100));
}

/// D7: a receive-blocked sender's mail is stored `hidden` with `blocked_sender`; a suppressed sender's
/// mail is stored as usual with the flag `sender_suppressed`.
#[test]
fn d7_blocked_hidden() {
    let s = crate::send::open_sender("d7 blocked", "live");
    let r = crate::call(
        "PUT",
        &format!(
            "/v1/tenants/{}/lists/receive/block/spammer%40d7.example",
            s.tid
        ),
        Some(&s.key),
        Some(serde_json::json!({})),
        &[],
    );
    assert_eq!(r.status, 201, "{}", r.body);
    crate::inbound(
        "spammer@d7.example",
        &s.addr,
        &crate::mail("spammer@d7.example", &s.addr, "Blocked", "x"),
    );
    let rows = crate::eventually(30, "the hidden copy", || {
        let r = crate::mailbox_query(
            &s.iid,
            "SELECT status, quarantine_reason FROM messages WHERE direction = 'inbound'",
        );
        (!r.is_empty()).then_some(r)
    });
    assert_eq!(
        (
            rows[0]["status"].as_str(),
            rows[0]["quarantine_reason"].as_str()
        ),
        (Some("hidden"), Some("blocked_sender"))
    );
    assert!(
        crate::messages_of(&s.iid, &s.key, "").is_empty(),
        "never shown"
    );
    let r = crate::post(
        &format!("/v1/tenants/{}/suppressions", s.tid),
        &s.key,
        serde_json::json!({ "address": "gone@d7.example" }),
    );
    assert_eq!(r.status, 201);
    crate::inbound(
        "gone@d7.example",
        &s.addr,
        &crate::mail("gone@d7.example", &s.addr, "Suppressed", "x"),
    );
    let m = crate::wait_stored(&s.iid, &s.key, "received", 1);
    assert!(
        m[0]["flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "sender_suppressed")
    );
}

/// A receive-allow entry skips the spam rule.
#[test]
fn receive_allow_skips_spam() {
    let s = crate::send::open_sender("allow spam", "live");
    let r = crate::patch(
        &format!("/v1/tenants/{}/policy", s.tid),
        &crate::platform(),
        serde_json::json!({ "quarantine": { "spam_threshold": 0.0 } }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    crate::inbound(
        "a@spam.example",
        &s.addr,
        &crate::mail("a@spam.example", &s.addr, "Spammy", "x"),
    );
    crate::eventually(30, "the quarantined copy", || {
        (!crate::mailbox_query(
            &s.iid,
            "SELECT id FROM messages WHERE quarantine_reason = 'spam'",
        )
        .is_empty())
        .then_some(())
    });
    let r = crate::call(
        "PUT",
        &format!(
            "/v1/tenants/{}/lists/receive/allow/%40friend.example",
            s.tid
        ),
        Some(&s.key),
        Some(serde_json::json!({})),
        &[],
    );
    assert_eq!(r.status, 201);
    crate::inbound(
        "b@friend.example",
        &s.addr,
        &crate::mail("b@friend.example", &s.addr, "Friendly", "x"),
    );
    let m = crate::wait_stored(&s.iid, &s.key, "received", 1);
    assert_eq!(m[0]["subject"], "Friendly");
}
