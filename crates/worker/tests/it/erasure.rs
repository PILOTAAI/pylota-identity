//! `it::erasure` (I9): objects no row names are swept.

use serde_json::json;

use crate::send::{open_sender, send};
use crate::{alarm, fakes, hook};

fn exists(key: &str) -> bool {
    let r = hook(
        "POST",
        "/__test/r2",
        Some(json!({ "op": "head", "key": key })),
    );
    !r.body["size"].is_null()
}

#[test]
fn orphan_objects_swept() {
    let s = open_sender("i9 orphans", "live");
    let _f = fakes::Reset;
    // A submit killed after its sent copy leaves a compose marker and an object.
    fakes::fault_any("submit.after_put", "error", 1);
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "killed", "text": "x" }),
    );
    assert_eq!(r.status, 503, "{}", r.body);
    let prefix = format!("t/{}/i/{}/out/", s.tid, s.iid);
    let listed = hook(
        "POST",
        "/__test/r2",
        Some(json!({ "op": "list", "key": prefix })),
    );
    let key = listed.body["keys"][0].as_str().unwrap().to_owned();
    assert!(exists(&key));
    // An object no row names, older than 15 days by its ID.
    let old = pylota_identity_api_types::ids::format(
        pylota_identity_api_types::ids::IdPrefix::Message,
        1_700_000_000_000,
        7,
    );
    let stray = format!("t/{}/i/{}/m/{old}/raw.eml", s.tid, s.iid);
    assert_eq!(
        hook(
            "POST",
            "/__test/r2",
            Some(json!({ "op": "put", "key": stray, "body_base64": "eA==" }))
        )
        .status,
        200
    );
    // A send that is stored keeps its copy.
    let kept = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "kept", "text": "x" }),
    );
    assert_eq!(kept.status, 202);
    fakes::advance(2 * 86_400_000);
    alarm(&s.iid);
    assert!(
        !exists(&key),
        "the stale compose marker's object is deleted"
    );
    assert!(!exists(&stray), "an old orphan is deleted");
    let kept_key = format!("{prefix}{}.eml", kept.body["id"].as_str().unwrap());
    assert!(exists(&kept_key));
}
