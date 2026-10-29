//! JSON output of every command (CLI and setup § 3).

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

/// `cli::output::json_every_command` (FR-CLI-1): every command of the tree prints exactly one JSON
/// document with `--json`, a success or an error envelope, and nothing else on stdout.
#[test]
fn json_every_command() {
    let (cli, _fake) = generic();
    let i = ["--identity", "idn_00000000000000000000000001"];
    let t = ["--tenant", "ten_00000000000000000000000001"];
    let mut cases: Vec<Vec<&str>> = vec![
        vec!["config", "show"],
        vec!["config", "set", "tenant", "ten_00000000000000000000000001"],
        vec!["mcp", "config"],
        vec!["tenants", "create", "--slug", "acme", "--name", "Acme"],
        vec!["tenants", "list"],
        vec!["tenants", "get", "ten_00000000000000000000000001"],
        vec![
            "tenants",
            "update",
            "ten_00000000000000000000000001",
            "--name",
            "Acme",
        ],
        vec!["tenants", "policy", "ten_00000000000000000000000001"],
        vec!["tenants", "suspend", "ten_00000000000000000000000001"],
        vec!["tenants", "resume", "ten_00000000000000000000000001"],
        vec!["partners", "create", "--name", "P"],
        vec!["partners", "list"],
        vec!["partners", "get", "ptn_1"],
        vec!["partners", "update", "ptn_1", "--name", "Q"],
        vec!["partners", "delete", "ptn_1", "--yes"],
        vec![
            "keys",
            "create",
            "--level",
            "tenant",
            "--name",
            "k",
            "--permissions",
            "messages:read",
        ],
        vec!["keys", "list"],
        vec!["keys", "get", "key_1"],
        vec!["keys", "revoke", "key_1", "--yes"],
        vec!["keys", "rotate", "key_1"],
        vec!["keys", "rotate", "thread"],
        vec!["audit"],
        vec![
            "identities",
            "create",
            "--username",
            "bookings",
            "--display-name",
            "B",
        ],
        vec!["identities", "list"],
        vec!["identities", "get", "idn_00000000000000000000000001"],
        vec![
            "identities",
            "update",
            "idn_00000000000000000000000001",
            "--display-name",
            "B",
        ],
        vec!["identities", "pause", "idn_00000000000000000000000001"],
        vec!["identities", "resume", "idn_00000000000000000000000001"],
        vec!["addresses", "list"],
        vec!["addresses", "promote", "adr_1"],
        vec!["addresses", "retire", "adr_1"],
        vec!["addresses", "delete", "adr_1"],
        vec![
            "domains",
            "add",
            "acme.example",
            "--method",
            "cloudflare_zone",
        ],
        vec!["domains", "list"],
        vec!["domains", "get", "dom_1"],
        vec!["domains", "records", "dom_1"],
        vec!["domains", "verify", "dom_1"],
        vec!["domains", "health", "dom_1"],
        vec!["domains", "reprove", "dom_1"],
        vec!["domains", "probe", "dom_1"],
        vec!["domains", "remove", "dom_1", "--yes"],
        vec![
            "send",
            "--to",
            "a@example.org",
            "--subject",
            "s",
            "--text",
            "t",
        ],
        vec!["reply", "msg_1", "--text", "t"],
        vec!["reply-all", "msg_1", "--text", "t"],
        vec!["forward", "msg_1", "--to", "a@example.org"],
        vec!["cancel", "msg_1"],
        vec!["resolve", "msg_1", "--outcome", "sent"],
        vec!["threads", "list"],
        vec!["threads", "get", "thr_1"],
        vec!["threads", "label", "thr_1", "--add", "x"],
        vec!["threads", "hold", "thr_1", "--reason", "r"],
        vec!["threads", "unhold", "thr_1"],
        vec!["messages", "list"],
        vec!["messages", "get", "msg_1"],
        vec!["messages", "raw", "msg_1", "--out", "m.eml"],
        vec!["messages", "attachment", "msg_1", "att_1", "--out", "a.bin"],
        vec!["messages", "attachment-text", "msg_1", "att_1"],
        vec!["messages", "label", "msg_1", "--add", "x"],
        vec!["messages", "delete", "msg_1", "--yes"],
        vec!["messages", "related", "msg_1"],
        vec!["search", "invoice"],
        vec!["ask", "what?", "--no-stream"],
        vec!["wait", "--timeout", "1"],
        vec!["triage", "list"],
        vec!["triage", "rerun", "msg_1"],
        vec!["quarantine", "list"],
        vec!["quarantine", "release", "msg_1", "--reason", "r"],
        vec![
            "webhooks",
            "create",
            "--url",
            "https://h.example/w",
            "--events",
            "*",
        ],
        vec!["webhooks", "list"],
        vec!["webhooks", "get", "whk_1"],
        vec!["webhooks", "update", "whk_1", "--enable"],
        vec!["webhooks", "delete", "whk_1", "--yes"],
        vec!["webhooks", "rotate", "whk_1"],
        vec!["webhooks", "test", "whk_1"],
        vec!["webhooks", "deliveries", "whk_1"],
        vec!["webhooks", "replay", "whk_1", "--since", "2026-10-01"],
        vec!["suppressions", "list"],
        vec!["suppressions", "add", "a@example.org"],
        vec!["suppressions", "remove", "a@example.org"],
        vec!["lists", "list", "receive", "block"],
        vec!["lists", "add", "receive", "block", "@spam.example"],
        vec!["lists", "remove", "receive", "block", "@spam.example"],
        vec!["erasure", "create", "--scope", "tenant", "--reason", "r"],
        vec!["erasure", "get", "era_1"],
        vec!["erasure", "list"],
        vec!["export", "create", "--scope", "identity"],
        vec!["export", "get", "exp_1"],
        vec!["export", "delete", "exp_1"],
        // Errors: the CLI's own and the deployment commands without a deployment.
        vec!["webhooks", "verify", "--secret-env", "NOPE"],
        vec!["doctor"],
        vec!["deploy"],
        vec!["send", "--subject", "missing --to"],
    ];
    for c in &mut cases {
        c.push("--json");
        c.extend_from_slice(&i);
        c.extend_from_slice(&t);
    }
    for c in &cases {
        let r = cli.run(c);
        let docs: Vec<serde_json::Value> = serde_json::Deserializer::from_str(&r.out)
            .into_iter::<serde_json::Value>()
            .collect::<Result<_, _>>()
            .unwrap_or_else(|e| panic!("{c:?}: not JSON ({e}): {}", r.out));
        assert_eq!(
            docs.len(),
            1,
            "{c:?} printed {} documents: {}",
            docs.len(),
            r.out
        );
        assert!(
            r.out.ends_with('\n') && r.out.trim_end().lines().count() == 1,
            "{c:?}: {}",
            r.out
        );
        if r.code != 0 {
            assert!(
                docs[0]["error"]["code"].is_string(),
                "{c:?}: exit {} without an envelope",
                r.code
            );
        }
    }
}
