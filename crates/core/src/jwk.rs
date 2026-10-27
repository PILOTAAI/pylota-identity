//! Ed25519 public keys as JWKs, and their RFC 7638 thumbprints ([Agent signing keys §3]).
//!
//! The thumbprint is the `keyid` of every signature the deployment makes: Web Bot Auth requests, the key
//! directory's own signature and agent assertions.
//!
//! [Agent signing keys §3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest as _, Sha256};

/// The public JWK of an Ed25519 key, with its members in the lexicographic order RFC 7638 §3.2 requires
/// for the thumbprint input.
pub fn public_jwk(x: &[u8; 32]) -> String {
    format!(
        r#"{{"crv":"Ed25519","kty":"OKP","x":"{}"}}"#,
        URL_SAFE_NO_PAD.encode(x)
    )
}

/// RFC 7638 thumbprint of an Ed25519 public key (RFC 8037 §2): base64url, no padding, of the SHA-256 of
/// the canonical JWK.
pub fn thumbprint(x: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(public_jwk(x).as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8037 appendix A.3 (rfc-editor.org, read 2026-10-27).
    #[test]
    fn thumbprint_rfc8037_vector() {
        let x: [u8; 32] = URL_SAFE_NO_PAD
            .decode("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo")
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(
            public_jwk(&x),
            r#"{"crv":"Ed25519","kty":"OKP","x":"11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"}"#
        );
        assert_eq!(
            thumbprint(&x),
            "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
        );
    }
}
