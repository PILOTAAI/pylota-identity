//! D1 changes followed by an object call (Design conventions § 9, J21).

use serde_json::json;

use crate::{alarm, cron, d1, fakes, hook, identity, indexed, patch, platform, tenant};

fn pending(tid: &str) -> i64 {
    d1(&format!(
        "SELECT COUNT(*) AS n FROM rpc_intents WHERE tenant_id = '{tid}'"
    ))[0]["n"]
        .as_i64()
        .unwrap()
}

/// A failed `Init` of a new tenant's `TenantQuota` and of a new identity's mailbox is re-sent by the
/// every-minute cron once the intent is due, and `identity.created` is emitted once.
#[test]
fn j21_init_retried() {
    let _reset = fakes::Reset;
    fakes::fault("do.call", "QUOTA", "error", 1);
    let p = platform();
    let t = tenant(&p, "j21");
    let tid = t["id"].as_str().unwrap();
    assert_eq!(pending(tid), 1);
    let qid = d1(&format!(
        "SELECT quota_do_id FROM tenants WHERE id = '{tid}'"
    ))[0]["quota_do_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let usage = || {
        let env = json!({ "v": 1, "tenant_id": tid, "identity_id": null, "request_id": "req_01ARYZ6S410000000000000000", "actor_key_id": null, "deadline_ms": 4_102_444_800_000_i64, "req": { "op": "get_usage" } });
        hook(
            "POST",
            "/__test/rpc",
            Some(json!({ "class": "quota", "object_id": qid, "envelope": env })),
        )
    };
    assert!(
        usage().body.get("err").is_some(),
        "the object has no owner before the retry"
    );
    // Not due for 60 seconds.
    cron("* * * * *");
    assert_eq!(pending(tid), 1);
    fakes::advance(61_000);
    cron("* * * * *");
    assert_eq!(pending(tid), 0);
    assert!(usage().body.get("ok").is_some());

    // The mailbox part: the identity is created, its Init waits, the cron delivers it once.
    fakes::fault("do.call", "MAILBOX", "error", 2);
    let idn = identity(&p, tid, "j21");
    let id = idn["id"].as_str().unwrap();
    assert_eq!(pending(tid), 1);
    let r = patch(
        &format!("/v1/identities/{id}"),
        &p,
        json!({ "display_name": "queued behind init" }),
    );
    assert_eq!(r.status, 200);
    assert_eq!(pending(tid), 2, "the update waits behind the init");
    fakes::advance(61_000);
    cron("* * * * *");
    assert_eq!(pending(tid), 0);
    cron("* * * * *");
    alarm(id);
    let types: Vec<String> = indexed(id).into_iter().map(|(t, _)| t).collect();
    assert_eq!(types, ["identity.created", "identity.updated"]);
}

/// With the mailbox unreachable, an update, a pause, a resume and a tenant suspension all commit; the
/// cron then emits each event once, in order, with the ID derived from its intent; a duplicate re-send
/// stores nothing.
#[test]
fn j21_identity_event_retried() {
    let _reset = fakes::Reset;
    let p = platform();
    let t = tenant(&p, "j21e");
    let tid = t["id"].as_str().unwrap();
    let idn = identity(&p, tid, "ev");
    let id = idn["id"].as_str().unwrap();
    let path = format!("/v1/identities/{id}");
    fakes::fault("do.call", "MAILBOX", "error", 100);
    assert_eq!(
        patch(&path, &p, json!({ "display_name": "one" })).status,
        200
    );
    assert_eq!(patch(&path, &p, json!({ "status": "paused" })).status, 200);
    assert_eq!(patch(&path, &p, json!({ "status": "active" })).status, 200);
    assert_eq!(
        patch(
            &format!("/v1/tenants/{tid}"),
            &p,
            json!({ "status": "suspended" })
        )
        .status,
        200
    );
    assert_eq!(pending(tid), 4);
    fakes::reset();
    fakes::advance(61_000);
    cron("* * * * *");
    assert_eq!(
        pending(tid),
        0,
        "{}",
        d1(&format!(
            "SELECT id, op, attempts, next_at, occurred_at, body_json FROM rpc_intents WHERE tenant_id = '{tid}'"
        ))
    );
    alarm(id);
    let events = indexed(id);
    let types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        types,
        [
            "identity.created",
            "identity.updated",
            "identity.paused",
            "identity.resumed",
            "identity.paused"
        ]
    );
    // A re-send of an event already stored: one row, nothing new indexed.
    let (_, updated_id) = &events[1];
    let tenant_row = d1(&format!(
        "SELECT mailbox_do_id FROM identities WHERE id = '{id}'"
    ));
    let mb = tenant_row[0]["mailbox_do_id"].as_str().unwrap();
    let env = json!({ "v": 1, "tenant_id": tid, "identity_id": id, "request_id": "req_01ARYZ6S410000000000000000", "actor_key_id": null, "deadline_ms": 4_102_444_800_000_i64,
        "req": { "op": "emit_event", "event_id": updated_id, "event_type": "identity.updated", "occurred_at": 1, "data": {} } });
    let r = hook(
        "POST",
        "/__test/rpc",
        Some(json!({ "class": "mailbox", "object_id": mb, "envelope": env })),
    );
    assert_eq!(
        r.body["ok"]["sequence"],
        serde_json::Value::Null,
        "{}",
        r.body
    );
    alarm(id);
    assert_eq!(indexed(id).len(), 5);
    patch(
        &format!("/v1/tenants/{tid}"),
        &p,
        json!({ "status": "active" }),
    );
}
