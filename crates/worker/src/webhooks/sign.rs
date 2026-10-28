//! Delivery signing (Webhooks › Signing): the Standard Webhooks primitives of
//! `core::standard_webhooks`, and the request headers of one attempt.

pub use pylota_identity_core::standard_webhooks::{
    secret_bytes, secret_string, sign_v1, signature_header, signed_content,
};

/// The fixed headers of an attempt, after the endpoint's custom headers so they cannot override them.
pub fn headers(
    custom: &[(String, String)],
    event_id: &str,
    timestamp_s: i64,
    signature: &str,
    version: &str,
) -> Vec<(String, String)> {
    let mut h: Vec<(String, String)> = custom.to_vec();
    for (n, v) in [
        ("content-type", "application/json".to_owned()),
        (
            "user-agent",
            format!("PylotaIdentity/{version} (+https://github.com/PILOTAAI/pylota-identity)"),
        ),
        ("webhook-id", event_id.to_owned()),
        ("webhook-timestamp", timestamp_s.to_string()),
        ("webhook-signature", signature.to_owned()),
    ] {
        h.retain(|(k, _)| !k.eq_ignore_ascii_case(n));
        h.push((n.to_owned(), v));
    }
    h
}
