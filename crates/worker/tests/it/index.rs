//! The `pm-index` consumer in an M7 build (Search § 6): attachment text, and the kinds later
//! milestones fill in.

use std::time::Duration;

use crate::{eventually, get, inject, mailbox_fixture, mailbox_query, wait_stored};

fn with_pdf(to: &str) -> String {
    format!(
        "From: a@pdf.example\r\nTo: {to}\r\nSubject: Scan\r\nMessage-ID: <{}@it.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n--b\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"scan.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQKJcOkw7zDtsOfCjEgMCBvYmoKPDw+PgplbmRvYmoKdHJhaWxlcgo8PD4+CiUlRU9GCg==\r\n--b--\r\n",
        crate::slug("pdf")
    )
}

/// B12: the converter fails on every attempt (workerd has no Workers AI): after the third the
/// attachment is `unavailable`, and still fetchable.
#[test]
fn b12_extraction_failure() {
    let (_, idn, addr, key) = mailbox_fixture("b12");
    assert_eq!(inject("a@pdf.example", &addr, &with_pdf(&addr)), 200);
    let m = wait_stored(&idn, &key, "received", 1);
    let mid = m[0]["id"].as_str().unwrap().to_owned();
    let att = eventually(40, "text_status unavailable", || {
        let rows = mailbox_query(&idn, "SELECT id, text_status FROM attachments");
        (rows[0]["text_status"] == "unavailable")
            .then(|| rows[0]["id"].as_str().unwrap().to_owned())
    });
    let r = get(
        &format!("/v1/identities/{idn}/messages/{mid}/attachments/{att}"),
        &key,
    );
    assert_eq!(r.status, 200);
    let t = get(
        &format!("/v1/identities/{idn}/messages/{mid}/attachments/{att}/text"),
        &key,
    );
    assert_eq!(t.body["status"], "unavailable");
}

/// `Embed` and `Triage` ack without work in M7: triage stays `pending`, nothing is dead-lettered.
#[test]
fn job_kinds_before_milestone() {
    let (_, idn, addr, key) = mailbox_fixture("kinds");
    assert_eq!(
        inject(
            "a@k.example",
            &addr,
            &crate::mail("a@k.example", &addr, "Kinds", "x")
        ),
        200
    );
    let m = wait_stored(&idn, &key, "received", 1);
    std::thread::sleep(Duration::from_secs(6));
    let full = get(
        &format!(
            "/v1/identities/{idn}/messages/{}",
            m[0]["id"].as_str().unwrap()
        ),
        &key,
    );
    assert_eq!(full.body["triage"]["status"], "pending", "{}", full.body);
    assert!(crate::fakes::queued("pm-index-dlq").is_empty());
}
