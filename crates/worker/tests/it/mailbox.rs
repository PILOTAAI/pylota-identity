//! The `IdentityMailbox` object (Data model § 2, Design conventions § 4).

use crate::{alarm, fakes, hook, identity, indexed, platform, tenant};

/// J9: a mailbox whose `schema_version` is behind (as after a deploy, or a lost version write) runs
/// its idempotent migration again on wake and keeps working.
#[test]
fn j9_migration_on_wake() {
    let p = platform();
    let t = tenant(&p, "j9");
    let idn = identity(&p, t["id"].as_str().unwrap(), "j9");
    let id = idn["id"].as_str().unwrap();
    for name in ["after", "again"] {
        let r = hook(
            "POST",
            "/__test/mailbox-schema",
            Some(serde_json::json!({ "identity_id": id })),
        );
        assert!(r.body.get("ok").is_some(), "{}", r.body);
        // The next request re-runs v1 over the existing tables and is answered normally.
        let r = crate::patch(
            &format!("/v1/identities/{id}"),
            &p,
            serde_json::json!({ "display_name": name }),
        );
        assert_eq!(r.status, 200, "{}", r.body);
    }
    alarm(id);
    let types: Vec<String> = indexed(id).into_iter().map(|(t, _)| t).collect();
    assert_eq!(
        types,
        ["identity.created", "identity.updated", "identity.updated"]
    );
}

/// The outbox writes each event to `event_index` and one `Fanout` to `pm-webhooks`, pointers only.
#[test]
fn outbox_reaches_event_index_and_queue() {
    let p = platform();
    let t = tenant(&p, "outbox");
    let idn = identity(&p, t["id"].as_str().unwrap(), "outbox");
    let id = idn["id"].as_str().unwrap();
    alarm(id);
    let events = indexed(id);
    assert_eq!(events.len(), 1);
    let event_id = &events[0].1;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let q = fakes::queued("pm-webhooks");
        if let Some(m) = q.iter().find(|m| m["event_id"] == event_id.as_str()) {
            assert_eq!(
                (
                    m["kind"].as_str(),
                    m["owner_kind"].as_str(),
                    m["identity_id"].as_str()
                ),
                (Some("fanout"), Some("mailbox"), Some(id))
            );
            assert!(m.get("data").is_none(), "pointers only");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no Fanout for {event_id}: {q:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
