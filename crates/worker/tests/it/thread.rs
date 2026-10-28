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
