//! Idempotency keys of sends (CLI and setup § 4).

mod support;

use pylota_identity_core::keys::{Mode, generate};
use support::{Cli, Fake};

/// A CLI whose profile points at the fake's generic API.
fn generic() -> (Cli, std::sync::Arc<Fake>) {
    let (cli, fake) = Cli::with_fake();
    fake.with(|s| s.generic_api = true);
    cli.write_config(
        &format!(
            "[profiles.default]\nurl = \"{}\"\nkey = \"{}\"\n",
            fake.base,
            generate(Mode::Live, [5; 8], [5; 32])
        ),
        0o600,
    );
    (cli, fake)
}

/// `cli::http::send_idempotency_reuse` (FR-OUT-1): a send retried after a network error reuses the same
/// key, and a generated key is printed to stderr.
#[test]
fn send_idempotency_reuse() {
    let (cli, fake) = generic();
    let path = "/v1/identities/idn_00000000000000000000000001/messages";
    fake.with(|s| s.drop_next = Some(path.to_owned()));
    let r = cli.run(&[
        "send",
        "--identity",
        "idn_00000000000000000000000001",
        "--to",
        "a@example.org",
        "--subject",
        "s",
        "--text",
        "t",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    let keys: Vec<Option<String>> = fake.with(|s| {
        s.api_requests
            .iter()
            .filter(|(m, p, _)| m == "POST" && p == path)
            .map(|x| x.2.clone())
            .collect()
    });
    assert_eq!(keys.len(), 2, "one dropped attempt and one retry: {keys:?}");
    assert_eq!(keys[0], keys[1]);
    let k = keys[0].clone().unwrap();
    assert!(
        k.starts_with("idvy-") && r.err.contains(&format!("Idempotency-Key: {k}")),
        "{}",
        r.err
    );

    fake.with(|s| s.drop_next = Some(path.to_owned()));
    let r = cli.run(&[
        "send",
        "--identity",
        "idn_00000000000000000000000001",
        "--to",
        "a@example.org",
        "--subject",
        "s",
        "--text",
        "t",
        "--idempotency-key",
        "bk-2291-confirm",
    ]);
    assert_eq!(r.code, 0);
    let last: Vec<Option<String>> = fake.with(|s| {
        s.api_requests
            .iter()
            .rev()
            .take(2)
            .map(|x| x.2.clone())
            .collect()
    });
    assert_eq!(last, vec![Some("bk-2291-confirm".to_owned()); 2]);
}
