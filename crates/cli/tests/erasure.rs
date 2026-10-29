//! `idvy erasure retry|reapply` against the generic API fake (CLI and setup § 19).

mod support;

use pylota_identity_core::keys::{Mode, generate};
use serde_json::json;
use support::Cli;

/// `cli::erasure::retry` (Privacy § 6.1): `erasure retry` reads the request and resubmits its tenant,
/// scope, identity and target with reason `retry:{era_id}`; a counterparty retry needs `--address`
/// (only the hash is stored), exit 2 without it; `erasure reapply` posts to the platform route.
#[test]
fn retry() {
    let (cli, fake) = Cli::with_fake();
    fake.with(|s| {
        s.generic_api = true;
        s.scripted.insert(
            "GET /v1/erasure-requests/era_msg".into(),
            vec![(200, json!({ "id": "era_msg", "tenant_id": "ten_1", "scope": "message", "identity_id": "idn_1",
                               "target_id": "msg_9", "status": "failed" }))],
        );
        s.scripted.insert(
            "GET /v1/erasure-requests/era_cp".into(),
            vec![(200, json!({ "id": "era_cp", "tenant_id": "ten_1", "scope": "counterparty", "identity_id": null,
                               "target_id": null, "status": "failed" }))],
        );
    });
    cli.write_config(
        &format!(
            "[profiles.default]\nurl = \"{}\"\nkey = \"{}\"\n",
            fake.base,
            generate(Mode::Live, [8; 8], [8; 32])
        ),
        0o600,
    );
    let r = cli.run(&["erasure", "retry", "era_msg", "--json"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let (m, p, k) = fake.with(|s| s.api_requests.last().unwrap().clone());
    assert_eq!((m.as_str(), p.as_str()), ("POST", "/v1/erasure-requests"));
    assert!(k.is_some(), "a generated Idempotency-Key");
    assert_eq!(
        fake.with(|s| s.api_bodies.last().unwrap().clone()),
        json!({ "tenant_id": "ten_1", "scope": "message", "reason": "retry:era_msg",
                "identity_id": "idn_1", "message_id": "msg_9" })
    );

    let n = fake.with(|s| s.api_requests.len());
    assert_eq!(cli.run(&["erasure", "retry", "era_cp"]).code, 2);
    assert_eq!(fake.with(|s| s.api_requests.len()), n + 1, "only the read");
    assert_eq!(
        cli.run(&["erasure", "retry", "era_cp", "--address", "jo@example.org"])
            .code,
        0
    );
    assert_eq!(
        fake.with(|s| s.api_bodies.last().unwrap().clone())["counterparty_address"],
        "jo@example.org"
    );

    assert_eq!(cli.run(&["erasure", "reapply", "era_msg"]).code, 0);
    let (m, p, _) = fake.with(|s| s.api_requests.last().unwrap().clone());
    assert_eq!(
        (m.as_str(), p.as_str()),
        ("POST", "/v1/platform/erasure-requests/era_msg/reapply")
    );
}
