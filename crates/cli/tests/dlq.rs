//! `idvy dlq` and `idvy jobs` against the generic API fake (CLI and setup § 13, § 19).

mod support;

use pylota_identity_core::keys::{Mode, generate};
use serde_json::json;
use support::{Cli, Fake};

fn platform() -> (Cli, std::sync::Arc<Fake>) {
    let (cli, fake) = Cli::with_fake();
    fake.with(|s| s.generic_api = true);
    cli.write_config(
        &format!(
            "[profiles.default]\nurl = \"{}\"\nkey = \"{}\"\n",
            fake.base,
            generate(Mode::Live, [6; 8], [6; 32])
        ),
        0o600,
    );
    (cli, fake)
}

fn item(id: &str) -> serde_json::Value {
    json!({ "id": id, "queue": "pm-index", "kind": "embed", "tenant_id": null,
            "first_seen_at": "2026-10-29T10:00:00Z", "redriven_at": null, "redrive_count": 0 })
}

/// `cli::dlq::j8_list_redrive` (J8, CLI and setup § 13): `dlq list` passes the filters and prints the
/// page; `dlq redrive` posts once per item with its own `Idempotency-Key`, reports a failed item and
/// continues, and exits with the first failure's code; `--queue` lists the open items first and needs
/// `--yes` without a terminal; naming neither is exit 2.
#[test]
fn j8_list_redrive() {
    let (cli, fake) = platform();
    fake.with(|s| {
        s.scripted.insert(
            "GET /v1/platform/dlq".into(),
            vec![(
                200,
                json!({ "data": [item("dlq_a"), item("dlq_b")], "next_cursor": null }),
            )],
        );
        s.scripted.insert(
            "POST /v1/platform/dlq/dlq_b/redrive".into(),
            vec![(404, json!("dlq_item_not_found"))],
        );
    });
    let r = cli.run(&["dlq", "list", "--queue", "pm-index", "--status", "open"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(
        r.out.contains("dlq_a") && r.out.contains("pm-index"),
        "{}",
        r.out
    );
    let (_, path, _) = fake.with(|s| s.api_requests.last().unwrap().clone());
    assert_eq!(path, "/v1/platform/dlq");

    let r = cli.run(&["dlq", "redrive", "dlq_a", "dlq_b", "dlq_c", "--json"]);
    assert_eq!(r.code, 5, "the first failure's exit code (404): {}", r.out);
    let doc = r.json();
    assert_eq!(doc["redriven"].as_array().unwrap().len(), 2);
    assert_eq!(
        doc["failed"],
        json!([{ "id": "dlq_b", "error": "dlq_item_not_found" }])
    );
    let posts: Vec<(String, Option<String>)> = fake.with(|s| {
        s.api_requests
            .iter()
            .filter(|(m, p, _)| m == "POST" && p.ends_with("/redrive"))
            .map(|(_, p, k)| (p.clone(), k.clone()))
            .collect()
    });
    assert_eq!(posts.len(), 3, "the rest continue after a failure");
    let keys: std::collections::BTreeSet<_> =
        posts.iter().map(|(_, k)| k.clone().unwrap()).collect();
    assert_eq!(keys.len(), 3, "each item has its own key");

    // `--queue`: the open items, confirmed with --yes.
    assert_eq!(cli.run(&["dlq", "redrive", "--queue", "pm-index"]).code, 2);
    fake.with(|s| {
        s.scripted.remove("POST /v1/platform/dlq/dlq_b/redrive");
    });
    let r = cli.run(&["dlq", "redrive", "--queue", "pm-index", "--yes", "--json"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(r.json()["redriven"].as_array().unwrap().len(), 2);
    assert!(r.err.contains("2 open items in pm-index"), "{}", r.err);
    assert_eq!(cli.run(&["dlq", "redrive"]).code, 2);
}

/// `cli::jobs::start_get` (J3, CLI and setup § 19): `jobs start` needs `--tenant`, resolves each
/// `--identity-ids` entry and sends `identity_ids`, `after` and `before`; `event_log_rebuild` sends
/// `kind` and `tenant_id` only and refuses the narrowing flags with exit 2; `jobs get` reads the job.
#[test]
fn start_get() {
    let (cli, fake) = platform();
    assert_eq!(
        cli.run(&["jobs", "start", "reparse"]).code,
        2,
        "--tenant is required"
    );
    let r = cli.run(&[
        "jobs",
        "start",
        "reparse",
        "--tenant",
        "ten_00000000000000000000000001",
        "--identity-ids",
        "idn_00000000000000000000000007",
        "--after",
        "2026-10-01T00:00:00Z",
        "--json",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    let body = fake.with(|s| s.api_bodies.last().unwrap().clone());
    assert_eq!(
        body,
        json!({ "kind": "reparse", "tenant_id": "ten_00000000000000000000000001",
                "identity_ids": ["idn_00000000000000000000000007"], "after": "2026-10-01T00:00:00Z", "before": null })
    );
    let r = cli.run(&[
        "jobs",
        "start",
        "event_log_rebuild",
        "--tenant",
        "ten_00000000000000000000000001",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        fake.with(|s| s.api_bodies.last().unwrap().clone()),
        json!({ "kind": "event_log_rebuild", "tenant_id": "ten_00000000000000000000000001" })
    );
    let r = cli.run(&[
        "jobs",
        "start",
        "event_log_rebuild",
        "--tenant",
        "ten_00000000000000000000000001",
        "--after",
        "2026-10-01T00:00:00Z",
    ]);
    assert_eq!(r.code, 2);
    assert_eq!(cli.run(&["jobs", "get", "job_1"]).code, 0);
    let (m, p, _) = fake.with(|s| s.api_requests.last().unwrap().clone());
    assert_eq!((m.as_str(), p.as_str()), ("GET", "/v1/platform/jobs/job_1"));
}
