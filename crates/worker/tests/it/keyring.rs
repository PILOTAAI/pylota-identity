//! The signing keyring (Security § 6.2).

use crate::{d1, get, platform, post};

/// Ten concurrent first uses of the cursor key leave one key, and every cursor minted in the race works.
#[test]
fn one_key_per_purpose_concurrent() {
    let p = platform();
    for i in 0..3 {
        assert_eq!(
            post(
                "/v1/partners",
                &p,
                serde_json::json!({ "name": format!("kr{i}") })
            )
            .status,
            201
        );
    }
    let cursors: Vec<String> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..10)
            .map(|_| {
                s.spawn(|| {
                    get("/v1/partners?limit=1", &platform()).body["next_cursor"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let rows = d1("SELECT purpose, COUNT(*) AS n FROM signing_keys GROUP BY purpose");
    for r in rows.as_array().unwrap() {
        if r["purpose"] == "cursor" {
            assert_eq!(r["n"], 1, "{rows}");
        }
    }
    for c in cursors {
        let r = get(&format!("/v1/partners?limit=1&cursor={c}"), &p);
        assert_eq!(r.status, 200, "{}", r.body);
    }
}
