//! The Web Bot Auth key directory through workerd (Agent signing keys § 3.2; O12).

use serde_json::json;

use crate::http_signatures::{directory_keys, verifies};
use crate::{call, fakes, platform, post};

/// O12: the directory is signed once per listed key over the request's `@authority`, with the
/// directory tag; a rotation keeps the previous key listed, and at most three keys are listed.
#[test]
fn directory_signed_per_key() {
    let _reset = fakes::Reset;
    fakes::set_config("PM_WEB_BOT_AUTH", Some("on"));
    let authority = std::env::var("PM_ITEST_URL")
        .unwrap()
        .trim_start_matches("https://")
        .to_owned();
    let check = |want: usize| {
        let r = call(
            "GET",
            "/.well-known/http-message-signatures-directory",
            None,
            None,
            &[],
        );
        assert_eq!(r.status, 200, "{}", r.body);
        assert_eq!(
            r.header("content-type"),
            Some("application/http-message-signatures-directory+json")
        );
        assert_eq!(r.header("cache-control"), Some("max-age=86400"));
        let keys = directory_keys();
        assert_eq!(keys.len(), want, "{}", r.body);
        let inputs: Vec<&str> = r.header("signature-input").unwrap().split(", ").collect();
        let sigs: Vec<&str> = r.header("signature").unwrap().split(", ").collect();
        assert_eq!((inputs.len(), sigs.len()), (want, want));
        for (i, (input, sig)) in inputs.iter().zip(&sigs).enumerate() {
            let label = format!("sig{}=", i + 1);
            let params = input.strip_prefix(&label).unwrap();
            assert!(
                params.starts_with("(\"@authority\";req);created="),
                "{params}"
            );
            assert!(params.ends_with(";tag=\"http-message-signatures-directory\""));
            let kid = params
                .split(";keyid=\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .unwrap();
            let (_, x) = keys.iter().find(|(k, _)| k == kid).unwrap();
            assert_eq!(kid.len(), 43);
            let base = format!("\"@authority\";req: {authority}\n\"@signature-params\": {params}");
            verifies(x, &base, sig);
        }
        keys
    };
    let n = directory_keys().len();
    let before = check(n)[0].0.clone();
    let r = post(
        "/v1/platform/keys/web_bot_auth/rotate",
        &platform(),
        json!({}),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["previous"]["kid"], before);
    let after = check((n + 1).min(3));
    assert_eq!(after[0].0, r.body["kid"].as_str().unwrap());
    assert!(
        after.iter().any(|(k, _)| *k == before),
        "the previous key stays listed"
    );
    for _ in 0..3 {
        let r = post(
            "/v1/platform/keys/web_bot_auth/rotate",
            &platform(),
            json!({}),
        );
        assert_eq!(r.status, 200, "{}", r.body);
    }
    check(3);
}
