//! Tenants (FR-TEN-1/2/3).

use serde_json::json;

use crate::{call, d1, platform, slug};

/// The same Idempotency-Key and body replays the stored response; another body is a conflict; `owner`
/// and `billing` are stored and send nothing.
#[test]
fn create_idempotent_replay() {
    let p = platform();
    let s = slug("create_idempotent_replay");
    let body = json!({ "slug": s, "name": "Acme", "mode": "live", "timezone": "Europe/London",
                       "owner": { "email": format!("{s}@owner.example") }, "billing": { "mode": "exempt" } });
    let idem = [("idempotency-key", s.as_str())];
    let a = call("POST", "/v1/tenants", Some(&p), Some(body.clone()), &idem);
    assert_eq!(a.status, 201, "{}", a.body);
    let b = call("POST", "/v1/tenants", Some(&p), Some(body.clone()), &idem);
    assert_eq!(
        (b.status, b.header("idempotent-replayed")),
        (201, Some("true"))
    );
    assert_eq!(a.body, b.body);
    let mut other = body;
    other["name"] = "Changed".into();
    assert_eq!(
        call("POST", "/v1/tenants", Some(&p), Some(other), &idem).code(),
        "idempotency_conflict"
    );
    let tid = a.body["id"].as_str().unwrap();
    assert_eq!(
        d1(&format!(
            "SELECT mode FROM billing_accounts WHERE tenant_id = '{tid}'"
        ))[0]["mode"],
        "exempt"
    );
    assert_eq!(
        d1(&format!(
            "SELECT role FROM members WHERE tenant_id = '{tid}'"
        ))[0]["role"],
        "owner"
    );
    // Init reached the tenant's TenantQuota: its intent is gone.
    assert_eq!(
        d1(&format!(
            "SELECT COUNT(*) AS n FROM rpc_intents WHERE tenant_id = '{tid}'"
        ))[0]["n"],
        0
    );
}

/// D12: a suffix folding to a reserved name is `address_reserved`; one folding to another tenant's
/// suffix is `suffix_taken`; a distinct one is accepted.
#[test]
fn d12_suffix_confusable_refused() {
    let p = crate::platform();
    let base = crate::slug("d12");
    let create = |suffix: &str| {
        crate::post(
            "/v1/tenants",
            &p,
            serde_json::json!({ "slug": crate::slug(suffix), "name": "d12", "mode": "live", "address_suffix": suffix }),
        )
    };
    for s in [".stripe", ".str1pe", ".p0stmaster"] {
        let r = create(s);
        assert_eq!(
            (r.status, r.code()),
            (400, "address_reserved"),
            "{s}: {}",
            r.body
        );
        assert_eq!(r.details()["field"], "address_suffix");
    }
    let mine = format!(".m{}", &base[1..7]);
    assert_eq!(create(&mine).status, 201);
    let look = mine.replacen('m', "rn", 1);
    let r = create(&look);
    assert_eq!(
        (r.status, r.code()),
        (409, "suffix_taken"),
        "{look}: {}",
        r.body
    );
    assert_eq!(create(&format!(".d{}", &base[1..7])).status, 201);
}
