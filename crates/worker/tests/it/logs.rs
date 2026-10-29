//! I5: the Worker's captured output holds no message content and no clear address (Privacy § 12).

use crate::send::open_sender;
use crate::{inbound, mail};

/// I5: a canary body, subject and sender address never appear in the captured Worker log.
#[test]
fn i5_no_content_in_logs() {
    let s = open_sender("i5 logs", "live");
    let canary_from = "canary.sender.i5@logs.example";
    let body = "i5-canary-body-7f3a9c";
    let subject = "i5-canary-subject-2b81";
    let (o, _) = inbound(
        canary_from,
        &s.addr,
        &mail(canary_from, &s.addr, subject, body),
    );
    assert_eq!(o, "accepted");
    crate::wait_stored(&s.iid, &s.key, "received", 1);
    let r = crate::send::send(
        &s,
        serde_json::json!({ "to": [canary_from], "subject": subject, "text": body }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    std::thread::sleep(std::time::Duration::from_secs(3));
    let log = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/itest/worker.log"),
    )
    .expect("the captured Worker log");
    for canary in [canary_from, body, subject] {
        assert!(!log.contains(canary), "{canary} is in the log");
    }
}
