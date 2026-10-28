//! D1 changes followed by an object call (Design conventions § 9, J21).

use serde_json::json;

use crate::{cron, d1, fakes, hook, platform, tenant};

/// A failed `Init` of a new tenant's `TenantQuota` is re-sent by the every-minute cron.
#[test]
fn j21_init_retried() {
    let _reset = fakes::Reset;
    fakes::fault("do.call", "QUOTA", "error", 1);
    let t = tenant(&platform(), "j21");
    let tid = t["id"].as_str().unwrap();
    let pending = |tid: &str| {
        d1(&format!(
            "SELECT COUNT(*) AS n FROM rpc_intents WHERE tenant_id = '{tid}'"
        ))[0]["n"]
            .as_i64()
            .unwrap()
    };
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
    cron("* * * * *");
    assert_eq!(pending(tid), 0);
    let r = usage();
    assert!(r.body.get("ok").is_some(), "{}", r.body);
    cron("* * * * *");
    assert_eq!(pending(tid), 0);
}
