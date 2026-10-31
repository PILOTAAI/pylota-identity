//! I5: the Worker's captured output holds no message content and no clear address (Privacy § 12).

use crate::send::open_sender;
use crate::{inbound, mail};

/// I5: a canary body, subject and sender address never appear in the captured Worker log.
#[test]
fn i5_no_content_in_logs() {
    let _reset = crate::fakes::Reset;
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
    // A minted assertion and HTTP signature are never logged either (Agent signing keys § 10).
    let signer = crate::assertions::signer("i5 signing");
    let token = crate::assertions::token(&signer)["assertion"]
        .as_str()
        .unwrap()
        .to_owned();
    crate::fakes::set_config("PM_WEB_BOT_AUTH", Some("on"));
    crate::assertions::patch_policy(&signer.tid, true);
    let h = crate::http_signatures::sign(
        &signer,
        serde_json::json!({ "url": "https://www.brightwell.example/i5" }),
    );
    assert_eq!(h.status, 200, "{}", h.body);
    let signature = h.body["headers"]["Signature"].as_str().unwrap().to_owned();
    let token_sig = token.rsplit('.').next().unwrap().to_owned();
    let http_sig = signature
        .trim_start_matches("sig1=:")
        .trim_end_matches(':')
        .to_owned();
    std::thread::sleep(std::time::Duration::from_secs(3));
    let log = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/itest/worker.log"),
    )
    .expect("the captured Worker log");
    for canary in [
        canary_from,
        body,
        subject,
        token_sig.as_str(),
        http_sig.as_str(),
    ] {
        assert!(!log.contains(canary), "{canary} is in the log");
    }
}
