//! Standard Webhooks signature verification ([Webhooks › Signing], spec v1.0.0): the receiver's half
//! of what the service signs. `idvy webhooks verify` uses it.
//!
//! [Webhooks › Signing]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{KeyInit as _, Mac as _};
use sha2::Sha256;

/// How far `webhook-timestamp` may be from `now` (seconds).
pub const TOLERANCE_S: i64 = 300;

/// Why a delivery did not verify.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// A required header is missing or not readable.
    MissingHeader(&'static str),
    /// The secret is not `whsec_` and base64.
    BadSecret,
    /// `webhook-timestamp` is more than five minutes from `now`.
    Timestamp,
    /// No `v1` signature matches.
    Signature,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::MissingHeader(h) => write!(f, "missing header {h}"),
            VerifyError::BadSecret => f.write_str("the secret is not a whsec_ secret"),
            VerifyError::Timestamp => f.write_str("the timestamp is outside the tolerance"),
            VerifyError::Signature => f.write_str("no signature matches"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Verifies one delivery: `secret` is the endpoint's `whsec_…` secret, `headers` the request's
/// headers (any case), `body` the raw body, `now_s` the receiver's Unix time. During a rotation
/// overlap the header carries two signatures; one match is enough.
pub fn verify(
    secret: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    now_s: i64,
) -> Result<(), VerifyError> {
    let header = |name: &'static str| {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim())
            .ok_or(VerifyError::MissingHeader(name))
    };
    let id = header("webhook-id")?;
    let ts: i64 = header("webhook-timestamp")?
        .parse()
        .map_err(|_| VerifyError::MissingHeader("webhook-timestamp"))?;
    let sigs = header("webhook-signature")?;
    let key = STANDARD
        .decode(
            secret
                .strip_prefix("whsec_")
                .ok_or(VerifyError::BadSecret)?,
        )
        .map_err(|_| VerifyError::BadSecret)?;
    if (now_s - ts).abs() > TOLERANCE_S {
        return Err(VerifyError::Timestamp);
    }
    let mut content = format!("{id}.{ts}.").into_bytes();
    content.extend_from_slice(body);
    for s in sigs.split(' ') {
        let Some(b64) = s.strip_prefix("v1,") else {
            continue;
        };
        let Ok(given) = STANDARD.decode(b64) else {
            continue;
        };
        let mut m =
            <hmac::Hmac<Sha256>>::new_from_slice(&key).map_err(|_| VerifyError::BadSecret)?;
        m.update(&content);
        if m.verify_slice(&given).is_ok() {
            return Ok(());
        }
    }
    Err(VerifyError::Signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(key: &[u8], id: &str, ts: i64, body: &[u8]) -> String {
        let mut m = <hmac::Hmac<Sha256>>::new_from_slice(key).unwrap();
        m.update(format!("{id}.{ts}.").as_bytes());
        m.update(body);
        format!("v1,{}", STANDARD.encode(m.finalize().into_bytes()))
    }

    /// Valid signatures, a rotation overlap, a tampered body and an old timestamp.
    #[test]
    fn verifies() {
        let (k1, k2) = (
            b"first-secret-bytes-0123456789ab".to_vec(),
            b"second-secret-bytes-0123456789a".to_vec(),
        );
        let s1 = format!("whsec_{}", STANDARD.encode(&k1));
        let s2 = format!("whsec_{}", STANDARD.encode(&k2));
        let body = br#"{"type":"message.received"}"#;
        let sig = format!(
            "{} {}",
            sign(&k1, "msg_1", 1000, body),
            sign(&k2, "msg_1", 1000, body)
        );
        let h = [
            ("Webhook-Id", "msg_1"),
            ("webhook-timestamp", "1000"),
            ("webhook-signature", sig.as_str()),
        ];
        assert_eq!(verify(&s1, &h, body, 1100), Ok(()));
        assert_eq!(verify(&s2, &h, body, 1100), Ok(()));
        assert_eq!(verify(&s1, &h, b"{}", 1100), Err(VerifyError::Signature));
        assert_eq!(
            verify(&s1, &h, body, 1000 + 301),
            Err(VerifyError::Timestamp)
        );
        assert_eq!(verify("nope", &h, body, 1000), Err(VerifyError::BadSecret));
        assert_eq!(
            verify(&s1, &h[..2], body, 1000),
            Err(VerifyError::MissingHeader("webhook-signature"))
        );
    }
}
