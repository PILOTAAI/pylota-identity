//! Compact JWS with EdDSA over Ed25519 (RFC 7515, RFC 8037 §3.1), for agent assertions ([Agent signing
//! keys §4]): signing with a seed, and verification with a public key that refuses every `alg` but
//! `EdDSA` and every `typ` but the one asked for.
//!
//! [Agent signing keys §4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use serde_json::Value;

/// The `typ` of an agent assertion.
pub const ASSERTION_TYP: &str = "agent-assertion+jwt";

/// `base64url(header) . base64url(payload)`, the JWS signing input.
pub fn signing_input(header: &str, payload: &[u8]) -> String {
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.as_bytes()),
        URL_SAFE_NO_PAD.encode(payload)
    )
}

/// The compact JWS of `payload` under `header` (a JSON object as text), signed with the 32-byte seed.
pub fn sign(header: &str, payload: &[u8], seed: &[u8; 32]) -> String {
    let input = signing_input(header, payload);
    let sig: Signature = SigningKey::from_bytes(seed).sign(input.as_bytes());
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.to_bytes()))
}

/// The public key of a seed.
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// The header of an agent assertion.
pub fn assertion_header(kid: &str) -> String {
    serde_json::json!({ "alg": "EdDSA", "typ": ASSERTION_TYP, "kid": kid }).to_string()
}

/// A JWKS entry: the public JWK with `kid`, `alg` and `use` (Agent signing keys §3.1).
pub fn published_jwk(x: &[u8; 32]) -> Value {
    serde_json::json!({
        "kty": "OKP", "crv": "Ed25519", "x": URL_SAFE_NO_PAD.encode(x),
        "kid": crate::jwk::thumbprint(x), "alg": "EdDSA", "use": "sig",
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Malformed,
    /// `alg` is not `EdDSA` (`none` included).
    Algorithm,
    /// `typ` is not the one asked for.
    Type,
    Signature,
}

/// The decoded header of a token, before any check.
pub fn header(token: &str) -> Result<Value, Error> {
    let h = token.split('.').next().ok_or(Error::Malformed)?;
    let bytes = URL_SAFE_NO_PAD.decode(h).map_err(|_| Error::Malformed)?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)
}

/// Verifies a compact JWS with `x` and returns `(header, payload)`. `typ` must equal `want_typ` when
/// given.
pub fn verify(token: &str, x: &[u8; 32], want_typ: Option<&str>) -> Result<(Value, Vec<u8>), Error> {
    let parts: Vec<&str> = token.split('.').collect();
    let [h, p, s] = parts.as_slice() else {
        return Err(Error::Malformed);
    };
    let head = header(token)?;
    if head["alg"] != "EdDSA" {
        return Err(Error::Algorithm);
    }
    if let Some(t) = want_typ
        && head["typ"] != t
    {
        return Err(Error::Type);
    }
    let payload = URL_SAFE_NO_PAD.decode(p).map_err(|_| Error::Malformed)?;
    let sig: [u8; 64] = URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| Error::Malformed)?
        .try_into()
        .map_err(|_| Error::Malformed)?;
    let key = VerifyingKey::from_bytes(x).map_err(|_| Error::Signature)?;
    key.verify_strict(format!("{h}.{p}").as_bytes(), &Signature::from_bytes(&sig))
        .map_err(|_| Error::Signature)?;
    Ok((head, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> [u8; 32] {
        URL_SAFE_NO_PAD
            .decode("nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A")
            .unwrap()
            .try_into()
            .unwrap()
    }

    /// RFC 8037 appendix A.4 (rfc-editor.org, read 2026-10-31): the JWS of "Example of Ed25519 signing"
    /// under `{"alg":"EdDSA"}` with the A.1 key.
    #[test]
    fn eddsa_rfc8037_vector() {
        let jws = sign(r#"{"alg":"EdDSA"}"#, b"Example of Ed25519 signing", &seed());
        assert_eq!(
            jws,
            "eyJhbGciOiJFZERTQSJ9.RXhhbXBsZSBvZiBFZDI1NTE5IHNpZ25pbmc.hgyY0il_MGCjP0JzlnLWG1PPOt7-09PGcvMg3AIbQR6dWbhijcNR4ki4iylGjg5BhVsPt9g7sVvpAr_MuM0KAg"
        );
        let x = public_key(&seed());
        assert_eq!(URL_SAFE_NO_PAD.encode(x), "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo");
        let (h, p) = verify(&jws, &x, None).unwrap();
        assert_eq!(h["alg"], "EdDSA");
        assert_eq!(p, b"Example of Ed25519 signing");
    }

    #[test]
    fn verify_refuses() {
        let x = public_key(&seed());
        let good = sign(&assertion_header("k"), b"{}", &seed());
        assert!(verify(&good, &x, Some(ASSERTION_TYP)).is_ok());
        assert_eq!(verify(&good, &x, Some("JWT")), Err(Error::Type));
        let none = format!("{}.e30.", URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#));
        assert_eq!(verify(&none, &x, None), Err(Error::Algorithm));
        let mut bad = good.clone();
        bad.replace_range(bad.len() - 4.., "AAAA");
        assert_eq!(verify(&bad, &x, None), Err(Error::Signature));
        let other = public_key(&[7; 32]);
        assert_eq!(verify(&good, &other, None), Err(Error::Signature));
        assert_eq!(verify("a.b", &x, None), Err(Error::Malformed));
        assert_eq!(published_jwk(&x)["kid"], "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k");
    }
}
