//! Offline webhook verification (CLI and setup § 16).

mod support;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{KeyInit as _, Mac as _};
use support::{Cli, Key};

/// `cli::webhooks::verify` (FR-WH-2): valid signatures, a rotation overlap, a tampered body and an old
/// timestamp, from a headers file and from flags.
#[test]
fn verify() {
    let mut cli = Cli::new(None);
    let _ = Key::new(1);
    let sign = |k: &[u8], body: &[u8]| {
        let mut m = <hmac::Hmac<sha2::Sha256>>::new_from_slice(k).unwrap();
        m.update(b"msg_1.1000.");
        m.update(body);
        format!("v1,{}", STANDARD.encode(m.finalize().into_bytes()))
    };
    let (k1, k2) = (
        b"first-secret-bytes-0123456789ab".to_vec(),
        b"second-secret-bytes-0123456789a".to_vec(),
    );
    cli.vars.insert(
        "WEBHOOK_SECRET".into(),
        format!("whsec_{}", STANDARD.encode(&k2)),
    );
    let body = br#"{"id":"evt_1","type":"message.received"}"#;
    std::fs::write(cli.home.join("body.json"), body).unwrap();
    std::fs::write(
        cli.home.join("headers.txt"),
        format!(
            "Webhook-Id: msg_1\nwebhook-timestamp: 1000\nwebhook-signature: {} {}\n",
            sign(&k1, body),
            sign(&k2, body)
        ),
    )
    .unwrap();
    let base = [
        "webhooks",
        "verify",
        "--secret-env",
        "WEBHOOK_SECRET",
        "--headers",
        "headers.txt",
        "--body",
        "body.json",
    ];
    let mut ok = base.to_vec();
    ok.extend(["--now", "1100", "--json"]);
    let r = cli.run(&ok);
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(r.json()["event_id"], "evt_1");
    let mut old = base.to_vec();
    old.extend(["--now", "2000", "--json"]);
    let r = cli.run(&old);
    assert_eq!(r.code, 11);
    assert_eq!(
        r.json()["error"]["details"]["reason"],
        "timestamp_out_of_tolerance"
    );
    let r = cli.run_in(
        &[
            "webhooks",
            "verify",
            "--secret-env",
            "WEBHOOK_SECRET",
            "--id",
            "msg_1",
            "--timestamp",
            "1000",
            "--signature",
            &sign(&k2, body),
            "--now",
            "1000",
        ],
        br#"{"id":"evt_1","type":"tampered"}"#,
    );
    assert_eq!(r.code, 11);
    assert!(r.err.contains("no_matching_signature"), "{}", r.err);
}
