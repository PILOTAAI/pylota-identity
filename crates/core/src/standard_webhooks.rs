//! Standard Webhooks signing ([Webhooks › Signing], spec v1.0.0): the signed content, the `v1,`
//! signature, the header with two signatures during a rotation overlap, and the `whsec_` secret form.
//!
//! [Webhooks › Signing]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{KeyInit as _, Mac as _};
use sha2::Sha256;

/// The secret prefix.
pub const PREFIX: &str = "whsec_";

/// `{webhook-id}.{webhook-timestamp}.{body}`.
pub fn signed_content(webhook_id: &str, timestamp_s: i64, body: &[u8]) -> Vec<u8> {
    let mut out = format!("{webhook_id}.{timestamp_s}.").into_bytes();
    out.extend_from_slice(body);
    out
}

/// `v1,` + standard base64 (padded) of HMAC-SHA256(secret, content).
pub fn sign_v1(secret: &[u8], content: &[u8]) -> String {
    let mut m = <hmac::Hmac<Sha256>>::new_from_slice(secret).expect("any key length");
    m.update(content);
    format!("v1,{}", STANDARD.encode(m.finalize().into_bytes()))
}

/// The `webhook-signature` header: the current secret's signature, then the previous one's during an
/// overlap, separated by one space.
pub fn signature_header(current: &[u8], previous: Option<&[u8]>, content: &[u8]) -> String {
    match previous {
        Some(p) => format!("{} {}", sign_v1(current, content), sign_v1(p, content)),
        None => sign_v1(current, content),
    }
}

/// `whsec_` + standard base64 of the secret bytes.
pub fn secret_string(bytes: &[u8]) -> String {
    format!("{PREFIX}{}", STANDARD.encode(bytes))
}

/// The secret bytes of a `whsec_…` string.
pub fn secret_bytes(secret: &str) -> Option<Vec<u8>> {
    STANDARD.decode(secret.strip_prefix(PREFIX)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vector of the Standard Webhooks reference library (`libraries/rust/src/lib.rs`, `test_sign`,
    /// read 2026-10-28 from github.com/standard-webhooks/standard-webhooks).
    #[test]
    fn standard_webhooks_vectors() {
        let secret = secret_bytes("whsec_C2FVsBQIhrscChlQIMV+b5sSYspob7oD").unwrap();
        let content = signed_content(
            "msg_27UH4WbU6Z5A5EzD8u03UvzRbpk",
            1_649_367_553,
            br#"{"email":"test@example.com","username":"test_user"}"#,
        );
        assert_eq!(
            sign_v1(&secret, &content),
            "v1,tZ1I4/hDygAJgO5TYxiSd6Sd0kDW6hPenDe+bTa3Kkw="
        );
        let both = signature_header(&[1; 32], Some(&secret), &content);
        let parts: Vec<&str> = both.split(' ').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1], "v1,tZ1I4/hDygAJgO5TYxiSd6Sd0kDW6hPenDe+bTa3Kkw=");
        assert_eq!(secret_bytes(&secret_string(&[7; 32])).unwrap(), vec![7; 32]);
    }
}
