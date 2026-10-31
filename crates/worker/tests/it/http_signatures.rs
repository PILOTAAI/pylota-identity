//! Signed HTTP requests through workerd (Agent signing keys § 5; O9–O11, O13, FR-IDN-8).

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::{Value, json};

use crate::assertions::{Signer, patch_policy, signer};
use crate::{Resp, call, fakes, post};

pub fn sign(s: &Signer, body: Value) -> Resp {
    post(
        &format!("/v1/identities/{}/http-signatures", s.iid),
        &s.key,
        body,
    )
}

/// The directory's keys by kid: `x` as bytes.
pub fn directory_keys() -> Vec<(String, [u8; 32])> {
    let r = call(
        "GET",
        "/.well-known/http-message-signatures-directory",
        None,
        None,
        &[],
    );
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| {
            (
                k["kid"].as_str().unwrap().to_owned(),
                URL_SAFE_NO_PAD
                    .decode(k["x"].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap(),
            )
        })
        .collect()
}

/// Checks a `Signature` value against the base the verifier rebuilds.
pub fn verifies(x: &[u8; 32], base: &str, signature: &str) {
    let b64 = signature
        .split_once("=:")
        .and_then(|(_, r)| r.strip_suffix(':'))
        .unwrap();
    let sig: [u8; 64] = STANDARD.decode(b64).unwrap().try_into().unwrap();
    VerifyingKey::from_bytes(x)
        .unwrap()
        .verify_strict(base.as_bytes(), &Signature::from_bytes(&sig))
        .unwrap();
}

/// O9, O13: `PM_WEB_BOT_AUTH=off` is `422 web_bot_auth_disabled` and the directory `404 key_not_found`;
/// a tenant not opted in is `403 policy_denied`; nothing is signed.
#[test]
fn disabled_and_policy() {
    let _reset = fakes::Reset;
    let s = signer("disabled_and_policy");
    let body = json!({ "url": "https://www.brightwell.example/fleet" });
    fakes::set_config("PM_WEB_BOT_AUTH", Some("off"));
    let r = sign(&s, body.clone());
    assert_eq!((r.status, r.code()), (422, "web_bot_auth_disabled"));
    let d = call(
        "GET",
        "/.well-known/http-message-signatures-directory",
        None,
        None,
        &[],
    );
    assert_eq!((d.status, d.code()), (404, "key_not_found"));
    let rot = post(
        "/v1/platform/keys/web_bot_auth/rotate",
        &crate::platform(),
        json!({}),
    );
    assert_eq!((rot.status, rot.code()), (422, "web_bot_auth_disabled"));

    fakes::set_config("PM_WEB_BOT_AUTH", Some("on"));
    let r = sign(&s, body.clone());
    assert_eq!((r.status, r.code()), (403, "policy_denied"));
    patch_policy(&s.tid, true);
    let r = sign(&s, body);
    assert_eq!(r.status, 200, "{}", r.body);
    let h = &r.body["headers"];
    assert_eq!(h["From"], s.address);
    assert_eq!(
        h["Signature-Agent"],
        format!(
            "\"https://{}\"",
            std::env::var("PM_ITEST_URL")
                .unwrap()
                .trim_start_matches("https://")
        )
    );
    // The signature verifies with the directory's active key over the base a verifier rebuilds.
    let input = h["Signature-Input"].as_str().unwrap();
    let params = input.strip_prefix("sig1=").unwrap();
    assert!(params.starts_with("(\"@authority\" \"signature-agent\" \"from\");created="));
    assert!(params.contains(";alg=\"ed25519\"") && params.ends_with(";tag=\"web-bot-auth\""));
    let keyid = params
        .split(";keyid=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap();
    let keys = directory_keys();
    let (_, x) = keys.iter().find(|(k, _)| k == keyid).unwrap();
    let base = format!(
        "\"@authority\": www.brightwell.example\n\"signature-agent\": {}\n\"from\": {}\n\"@signature-params\": {params}",
        h["Signature-Agent"].as_str().unwrap(),
        s.address
    );
    verifies(x, &base, h["Signature"].as_str().unwrap());
}

/// O11: 29 and 301 seconds are refused; 30 and 300 are signed. O10: a non-ASCII component value is
/// refused, an IDN host becomes its A-label, and `@method` needs `method`.
#[test]
fn expiry_bounds() {
    let _reset = fakes::Reset;
    let s = signer("expiry_bounds");
    fakes::set_config("PM_WEB_BOT_AUTH", Some("on"));
    patch_policy(&s.tid, true);
    for (e, status) in [(29, 400), (30, 200), (300, 200), (301, 400)] {
        let r = sign(
            &s,
            json!({ "url": "https://www.brightwell.example/", "expires_in": e }),
        );
        assert_eq!(r.status, status, "{e}: {}", r.body);
    }
    let r = sign(&s, json!({ "url": "http://www.brightwell.example/" }));
    assert_eq!((r.status, r.code()), (400, "invalid_request"));
    let r = sign(
        &s,
        json!({ "url": "https://www.brightwell.example/", "components": ["@method"] }),
    );
    assert_eq!(
        (r.status, r.details()["errors"][0]["path"].as_str()),
        (400, Some("method"))
    );
    let r = sign(
        &s,
        json!({ "url": "https://Bücher.example/a?q=1", "method": "GET",
                "components": ["@method", "@path", "@query"] }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let input = r.body["headers"]["Signature-Input"].as_str().unwrap();
    assert!(input.starts_with(
        "sig1=(\"@authority\" \"signature-agent\" \"from\" \"@method\" \"@path\" \"@query\")"
    ));
    let r = sign(
        &s,
        json!({ "url": "https://www.brightwell.example/caf\u{e9}", "components": ["@path"] }),
    );
    assert_eq!(
        (r.status, r.details()["reason"].as_str()),
        (400, Some("non_ascii"))
    );
}
