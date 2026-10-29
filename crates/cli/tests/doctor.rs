//! `idvy doctor` against the recorded fakes (CLI and setup § 10, Observability § 7.2).

mod support;

use serde_json::{Value, json};
use support::{Cli, MAIL_DOMAIN, setup_args};

fn checks(doc: &Value) -> Vec<(String, String, Option<String>)> {
    doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_owned(),
                c["status"].as_str().unwrap().to_owned(),
                c["fix"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

fn status(doc: &Value, name: &str) -> String {
    checks(doc)
        .into_iter()
        .find(|c| c.0 == name)
        .map(|c| c.1)
        .unwrap_or_default()
}

/// `cli::doctor::every_check_has_fix` (FR-OPS-3, M16): every check reports; each `fail` and `warn`
/// prints a fix; exit 0 with warnings only, 12 with a failure. Cloudflare-side problems are failing
/// checks, never exit 10.
#[test]
fn every_check_has_fix() {
    let (cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);

    let healthy = cli.run(&["doctor", "--json"]);
    assert_eq!(healthy.code, 0, "{}", healthy.out);
    let doc = healthy.json();
    let all = checks(&doc);
    assert_eq!(
        all.len(),
        pylota_identity_cli::commands::doctor::CHECKS.len() - 1,
        "every check but the mail test runs"
    );
    for (name, st, fix) in &all {
        assert!(
            ["pass", "warn", "skip"].contains(&st.as_str()),
            "{name} is {st}: {doc}"
        );
        if st == "warn" {
            assert!(
                fix.as_deref().is_some_and(|f| !f.is_empty()),
                "{name} warns without a fix"
            );
        }
    }
    assert_eq!(status(&doc, "dns.platform"), "pass");
    assert_eq!(
        status(&doc, "quota"),
        "warn",
        "PM_DAILY_SEND_QUOTA is unset"
    );

    // Break five things the checks watch.
    fake.with(|s| {
        let z = s.zone_id(MAIL_DOMAIN);
        s.catch_all.insert(z.clone(), json!({ "enabled": false, "actions": [] }));
        for sc in s.scripts.values_mut() {
            sc.secrets.remove("PM_HASH_KEY");
        }
        for sub in s.sending.get_mut(&z).unwrap() {
            sub["preview_enabled"] = json!(true);
        }
        s.queues.retain(|q| q["queue_name"] != "pm-index");
        s.db()
            .execute(
                "INSERT INTO dlq_items (id, queue, message_id, body_json, body_sha256, first_seen_at) VALUES ('dlq_1', 'pm-inbound', 'm1', '{}', 'h', 1)",
                [],
            )
            .unwrap();
    });
    let broken = cli.run(&["doctor", "--json"]);
    assert_eq!(broken.code, 12, "{}", broken.out);
    let doc = broken.json();
    for name in [
        "routing.catch_all",
        "secrets",
        "sending.domains",
        "bindings",
        "dlq",
    ] {
        assert_eq!(status(&doc, name), "fail", "{name}: {doc}");
    }
    for (name, st, fix) in checks(&doc) {
        if st == "fail" || st == "warn" {
            assert!(
                fix.as_deref().is_some_and(|f| !f.is_empty()),
                "{name} has no fix"
            );
        }
    }
    assert!(doc["summary"]["fail"].as_u64().unwrap() >= 5);

    // Human output: a fix line under each failure.
    let human = cli.run(&["doctor"]);
    assert_eq!(human.code, 12);
    let lines: Vec<&str> = human.out.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        if l.starts_with("fail") {
            assert!(
                lines
                    .get(i + 1)
                    .is_some_and(|n| n.trim_start().starts_with("fix:")),
                "{l}"
            );
        }
    }

    // Without Cloudflare credentials the Cloudflare checks fail; doctor still exits 12, never 10.
    let mut no_token = Cli::new(Some(fake.clone()));
    no_token.home = cli.home.clone();
    no_token.vars.remove("CLOUDFLARE_API_TOKEN");
    no_token
        .vars
        .insert("HOME".into(), cli.home.display().to_string());
    let r = no_token.run(&[
        "doctor", "--json", "--check", "secrets", "--check", "health",
    ]);
    assert_eq!(r.code, 12, "{}", r.out);
    assert_eq!(checks(&r.json()).len(), 2);
    assert_eq!(cli.run(&["doctor", "--check", "nope"]).code, 2);
}
