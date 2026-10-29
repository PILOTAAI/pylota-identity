//! `idvy setup` against the recorded Cloudflare fake (CLI and setup § 6).

mod support;

use std::collections::BTreeMap;

use serde_json::Value;
use support::{API_HOST, Cli, MAIL_DOMAIN, setup_args};

/// The status of each step of a JSON setup run.
fn statuses(doc: &Value) -> Vec<(String, String, String)> {
    doc["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["step"].as_str().unwrap().to_owned(),
                s["status"].as_str().unwrap().to_owned(),
                s["resource"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// `cli::setup::idempotent_rerun` (FR-OPS-1, M16): two runs against the recorded Cloudflare fake create
/// every resource once; the second run reports `exists` for every step and deploys nothing.
#[test]
fn idempotent_rerun() {
    let (cli, fake) = Cli::with_fake();
    let mut args = setup_args();
    args.push("--json");
    let first = cli.run(&args);
    assert_eq!(first.code, 0, "{}\n{}", first.out, first.err);
    let doc = first.json();
    assert_eq!(doc["platform_domain"], MAIL_DOMAIN);
    assert!(
        doc.get("secrets").is_none(),
        "secrets are printed only with --print-secrets"
    );
    let created = statuses(&doc).iter().filter(|s| s.1 == "created").count();
    assert!(created >= 20, "{:?}", statuses(&doc));

    let counts = |f: &support::Fake| {
        f.with(|s| {
            let deploys: u32 = s.scripts.values().map(|sc| sc.deploys).sum();
            BTreeMap::from([
                ("r2", s.buckets.len()),
                ("queues", s.queues.len()),
                ("indexes", s.indexes.len()),
                (
                    "metadata",
                    s.indexes
                        .values()
                        .map(|i| i["metadata"].as_array().unwrap().len())
                        .sum(),
                ),
                ("subscriptions", s.subscriptions.len()),
                ("sending", s.sending.values().map(Vec::len).sum()),
                (
                    "ownership",
                    s.records
                        .values()
                        .flatten()
                        .filter(|r| r["name"] == format!("_pylota-identity.{MAIL_DOMAIN}"))
                        .count(),
                ),
                ("deploys", deploys as usize),
                (
                    "secrets",
                    s.scripts.values().map(|sc| sc.secrets.len()).sum(),
                ),
                ("tenants", s.tenants.len()),
                ("dbs", s.dbs.len()),
            ])
        })
    };
    let after_first = counts(&fake);
    assert_eq!(after_first["dbs"], 1);
    assert_eq!(after_first["r2"], 1);
    assert_eq!(after_first["queues"], 12);
    assert_eq!(after_first["indexes"], 1);
    assert_eq!(after_first["metadata"], 8);
    assert_eq!(after_first["subscriptions"], 1);
    assert_eq!(after_first["sending"], 1);
    assert_eq!(after_first["ownership"], 1);
    assert_eq!(after_first["deploys"], 1);
    assert_eq!(after_first["secrets"], 3);
    assert_eq!(after_first["tenants"], 1);
    let rows = fake.with(|s| {
        let c = s.db();
        let n = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        (
            n("SELECT COUNT(*) FROM api_keys"),
            n("SELECT COUNT(*) FROM domains WHERE kind = 'platform'"),
            n("SELECT COUNT(*) FROM identities WHERE is_system = 1"),
            n("SELECT COUNT(*) FROM schema_migrations"),
        )
    });
    assert_eq!(rows, (1, 1, 1, 1));

    // The profile holds the URL, the account and a working platform key; the token is never stored.
    let config =
        std::fs::read_to_string(cli.home.join(".config/pylota-identity/config.toml")).unwrap();
    assert!(config.contains("account_id") && config.contains("pmk_live_"));
    assert!(!config.contains("cf-test-token"));
    // Generated secrets went to Wrangler on stdin only: never in an argument or a file.
    let (calls, secrets) = fake.with(|s| {
        (
            s.runner_calls.clone(),
            s.scripts
                .values()
                .flat_map(|sc| sc.secrets.values().cloned())
                .collect::<Vec<_>>(),
        )
    });
    for c in &calls {
        for secret in &secrets {
            assert!(
                !c.args.iter().any(|a| a.contains(secret.as_str())),
                "secret in an argument: {c:?}"
            );
        }
        assert!(c.env.iter().all(|(k, _)| k != "PM_KEY_PEPPER"));
    }
    let wrangler_toml = std::fs::read_to_string(cli.deploy_dir().join("wrangler.toml")).unwrap();
    for secret in &secrets {
        assert!(!wrangler_toml.contains(secret.as_str()) && !first.out.contains(secret.as_str()));
    }
    assert!(
        wrangler_toml.contains("PM_SIGNUP")
            && wrangler_toml.contains(&format!("PM_CONSOLE_HOST        = \"{API_HOST}\""))
    );
    assert!(pylota_identity_cli::bundle::render::var(&wrangler_toml, "PM_BILLING").is_none());
    assert_eq!(
        pylota_identity_cli::bundle::render::var(&wrangler_toml, "PM_SIGNUP").as_deref(),
        Some("closed")
    );

    let second = cli.run(&args);
    assert_eq!(second.code, 0, "{}\n{}", second.out, second.err);
    let again = statuses(&second.json());
    let changed: Vec<_> = again
        .iter()
        .filter(|s| s.1 != "exists" && s.0 != "mail_test")
        .collect();
    assert!(changed.is_empty(), "the second run changed {changed:?}");
    assert_eq!(
        counts(&fake),
        after_first,
        "the second run created a resource"
    );
    assert_eq!(
        std::fs::read_to_string(cli.deploy_dir().join("wrangler.toml")).unwrap(),
        wrangler_toml
    );
}

/// `cli::setup::h5_existing_mx`: foreign MX records stop setup (exit 10, `existing_mx`) before anything
/// is created; with `--replace-mx` they are deleted before routing is enabled.
#[test]
fn h5_existing_mx() {
    let (cli, fake) = Cli::with_fake();
    fake.with(|s| {
        let z = s.zone_id(MAIL_DOMAIN);
        s.records.entry(z).or_default().push(serde_json::json!({
            "id": "old-mx", "type": "MX", "name": MAIL_DOMAIN, "content": "mx.oldprovider.example", "priority": 10 }));
    });
    let mut args = setup_args();
    args.push("--json");
    let r = cli.run(&args);
    assert_eq!(r.code, 10, "{}", r.out);
    assert_eq!(r.json()["error"]["details"]["reason"], "existing_mx");
    assert!(fake.with(|s| s.dbs.is_empty() && s.queues.is_empty()));
    args.push("--replace-mx");
    let r = cli.run(&args);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(fake.with(|s| !s.records.values().flatten().any(|r| r["id"] == "old-mx")));
}

/// `cli::setup::not_apex`: a mail domain below a zone is refused with the zone's name.
#[test]
fn not_apex() {
    let (cli, _fake) = Cli::with_fake();
    let r = cli.run(&[
        "setup",
        "--domain",
        API_HOST,
        "--mail-domain",
        "mail.agents.example",
        "--owner-email",
        "sam@acmecarhire.example",
        "--yes",
    ]);
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.err.contains("its zone is agents.example"), "{}", r.err);
}
