//! `it::lists` (FR-OUT-16): the allow and block list routes.

use serde_json::json;

use crate::send::open_sender;
use crate::{call, get, mint, platform};

#[test]
fn entries_crud() {
    let s = open_sender("lists crud", "live");
    let base = format!("/v1/tenants/{}/lists", s.tid);
    for dir in ["receive", "send"] {
        for kind in ["allow", "block"] {
            for entry in ["Jo%40B%C3%BCcher.example", "%40Example.NET"] {
                let r = call(
                    "PUT",
                    &format!("{base}/{dir}/{kind}/{entry}"),
                    Some(&s.key),
                    Some(json!({ "note": "first" })),
                    &[],
                );
                assert_eq!(r.status, 201, "{}", r.body);
            }
            let r = get(&format!("{base}/{dir}/{kind}"), &s.key);
            let entries: Vec<&str> = r.body["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["entry"].as_str().unwrap())
                .collect();
            assert!(
                entries.contains(&"jo@xn--bcher-kva.example") && entries.contains(&"@example.net"),
                "{entries:?}"
            );
            let r = call(
                "PUT",
                &format!("{base}/{dir}/{kind}/%40example.net"),
                Some(&s.key),
                Some(json!({ "note": "second" })),
                &[],
            );
            assert_eq!(r.status, 200);
            assert_eq!(r.body["note"], "second");
            let r = get(&format!("{base}/{dir}/{kind}/%40example.net"), &s.key);
            assert_eq!(
                (r.body["scope"].as_str(), r.body["note"].as_str()),
                (Some("tenant"), Some("second"))
            );
            assert_eq!(
                call(
                    "DELETE",
                    &format!("{base}/{dir}/{kind}/%40example.net"),
                    Some(&s.key),
                    None,
                    &[]
                )
                .status,
                204
            );
            assert_eq!(
                get(&format!("{base}/{dir}/{kind}/%40example.net"), &s.key).code(),
                "list_entry_not_found"
            );
        }
    }
    // An identity key can never hold suppressions:manage.
    let (_, ik) = mint(
        &platform(),
        json!({ "name": "lists id", "level": "identity", "tenant_id": s.tid, "identity_id": s.iid,
        "permissions": ["messages:read"] }),
    );
    assert_eq!(
        get(&format!("{base}/send/allow"), &ik).code(),
        "permission_denied"
    );
    // Another tenant's key.
    let other = open_sender("lists other", "live");
    assert_eq!(
        get(&format!("{base}/send/allow"), &other.key).code(),
        "tenant_not_found"
    );
}
