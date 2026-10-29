//! `idvy domains subscribe` against the recorded fakes (CLI and setup § 18.4).

mod support;

use support::{Cli, setup_args};

/// A tenant domain of the default tenant on the account's zone `example.com`.
fn add_domain(fake: &support::Fake, id: &str, name: &str, sending: i64) {
    fake.with(|s| {
        let zone = s.zone_id("example.com");
        let c = s.db();
        let tenant: String = c
            .query_row("SELECT id FROM tenants WHERE address_suffix = ''", [], |r| r.get(0))
            .unwrap();
        c.execute(
            "INSERT INTO domains (id, tenant_id, name, kind, method, inbound, zone_id, is_apex, routing_mode, transport, \
             reply_token, receiving, sending, state, state_changed_at, monitor_do_id, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 'zone', 'cloudflare_zone', 'routing', ?4, ?5, 'catch_all', 'cloudflare', 'subaddress', 1, ?6, \
             'healthy', 1, 'monitor', 1, 1)",
            rusqlite::params![id, tenant, name, zone, i64::from(name == "example.com"), sending],
        )
        .unwrap();
    });
}

/// `cli::domains::subscribe_manual` (FR-DOM-3, spike S9 fallback): a `manual` domain gets one
/// subscription, created through Wrangler with the local token and recorded through D1; an existing
/// matching subscription is reused; a domain that is not `manual` is a no-op with exit 0; no local
/// token is exit 3.
#[test]
fn subscribe_manual() {
    let (cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);
    add_domain(&fake, "dom_000000000000000000000MANUAL", "example.com", 1);
    let subs = || fake.with(|s| s.subscriptions.len());
    let before = subs();

    let r = cli.run(&["domains", "subscribe", "example.com", "--json"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert_eq!(r.json()["delivery_events"], "active");
    assert_eq!(subs(), before + 1);
    let (sub, recorded) = fake.with(|s| {
        let sub = s.subscriptions.last().unwrap().clone();
        let recorded: Option<String> = s
            .db()
            .query_row("SELECT event_subscription_id FROM domains WHERE id = 'dom_000000000000000000000MANUAL'", [], |r| r.get(0))
            .unwrap();
        (sub, recorded)
    });
    assert_eq!(sub["source"]["domain"], "example.com");
    assert_eq!(sub["destination"]["queue"], "pm-delivery-events");
    assert_eq!(recorded.as_deref(), sub["id"].as_str());
    // The token reached Wrangler through its environment, never as an argument.
    let calls = fake.with(|s| s.runner_calls.clone());
    let create = calls
        .iter()
        .find(|c| c.args.iter().any(|a| a == "subscription"))
        .unwrap();
    assert!(
        create
            .env
            .iter()
            .any(|(k, v)| k == "CLOUDFLARE_API_TOKEN" && v == "cf-test-token")
    );
    assert!(!create.args.iter().any(|a| a.contains("cf-test-token")));

    // A re-run is a no-op: the domain is no longer manual.
    let r = cli.run(&["domains", "subscribe", "example.com"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("Nothing to do"), "{}", r.out);
    assert_eq!(subs(), before + 1);

    // A domain that does not send is a no-op too.
    add_domain(
        &fake,
        "dom_00000000000000000000NOSEND",
        "quiet.example.com",
        0,
    );
    let r = cli.run(&["domains", "subscribe", "quiet.example.com"]);
    assert_eq!(r.code, 0);
    assert_eq!(subs(), before + 1);

    // A subscription that already targets the zone and domain is reused (an interrupted run).
    add_domain(
        &fake,
        "dom_000000000000000000000REUSE",
        "news.example.com",
        1,
    );
    fake.with(|s| {
        let zone = s.zone_id("example.com");
        s.subscriptions.push(
            serde_json::json!({ "id": "sub-existing", "name": "pylota-identity news.example.com",
            "source": { "type": "email.sending", "zone_id": zone, "domain": "news.example.com" } }),
        );
    });
    let r = cli.run(&["domains", "subscribe", "news.example.com", "--json"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(subs(), before + 2, "reused, not created");

    // Without the local token: exit 3, nothing created.
    add_domain(
        &fake,
        "dom_0000000000000000000NOTOKEN",
        "shop.example.com",
        1,
    );
    let mut no_token = Cli::new(Some(fake.clone()));
    no_token.home = cli.home.clone();
    no_token
        .vars
        .insert("HOME".into(), cli.home.display().to_string());
    no_token.vars.remove("CLOUDFLARE_API_TOKEN");
    let r = no_token.run(&["domains", "subscribe", "shop.example.com"]);
    assert_eq!(r.code, 3, "{}", r.err);
    assert_eq!(subs(), before + 2);
}
