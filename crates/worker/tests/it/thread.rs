//! Threading in the mailbox (Threading § 3).

use crate::{inject, mail, mailbox_fixture, mailbox_query, wait_stored};

/// C1: `seq` is never reused, even after the newest thread is gone, so an old token never joins a
/// newer thread.
#[test]
fn seq_never_reused() {
    let (_, idn, addr, key) = mailbox_fixture("seq");
    assert_eq!(
        inject(
            "a@s.example",
            &addr,
            &mail("a@s.example", &addr, "One", "1")
        ),
        200
    );
    wait_stored(&idn, &key, "received", 1);
    assert_eq!(
        inject(
            "b@s.example",
            &addr,
            &mail("b@s.example", &addr, "Two", "2")
        ),
        200
    );
    wait_stored(&idn, &key, "received", 2);
    let max = mailbox_query(&idn, "SELECT MAX(seq) AS s FROM threads")[0]["s"]
        .as_i64()
        .unwrap();
    // Remove the newest thread as an erasure would (its message first).
    mailbox_query(
        &idn,
        &format!("DELETE FROM messages WHERE thread_seq = {max} RETURNING rowid"),
    );
    mailbox_query(
        &idn,
        &format!("DELETE FROM threads WHERE seq = {max} RETURNING seq"),
    );
    assert_eq!(
        inject(
            "c@s.example",
            &addr,
            &mail("c@s.example", &addr, "Three", "3")
        ),
        200
    );
    crate::eventually(30, "the third thread", || {
        let s = mailbox_query(&idn, "SELECT MAX(seq) AS s FROM threads")[0]["s"].as_i64();
        (s > Some(max - 1) && s != Some(max - 1)).then_some(())
    });
    let newest = mailbox_query(&idn, "SELECT MAX(seq) AS s FROM threads")[0]["s"]
        .as_i64()
        .unwrap();
    assert!(newest > max, "{newest} > {max}");
}

/// C7: a reply that drops our `Reply-To` token still joins through `In-Reply-To`, matched against the
/// Message-ID Cloudflare returned (strategy A).
#[test]
fn c7_reply_to_cloudflare_message_id() {
    let s = crate::send::open_sender("c7 cf id", "live");
    let mid = crate::send::send_ok(&s, &["jo@c7.example"], "Booking BK-77");
    let m = crate::send::wait_status(&s, &mid, "submitted");
    let rfc = m["rfc_message_id"].as_str().unwrap().to_owned();
    assert!(rfc.ends_with(s.addr.rsplit('@').next().unwrap()), "{rfc}");
    let raw = format!(
        "From: jo@c7.example\r\nTo: {}\r\nSubject: Re: Booking BK-77\r\nMessage-ID: <c7-{}@c7.example>\r\nIn-Reply-To: <{rfc}>\r\nDate: Mon, 26 Oct 2026 10:00:00 +0000\r\n\r\nThanks\r\n",
        s.addr,
        crate::slug("c7")
    );
    crate::inbound("jo@c7.example", &s.addr, &raw);
    let got = crate::eventually(30, "the reply", || {
        crate::messages_of(&s.iid, &s.key, "&direction=inbound")
            .into_iter()
            .next()
    });
    assert_eq!(got["thread_id"], m["thread_id"]);
}
