//! The encryption envelope for secrets at rest ([Security §7.2]):
//! `pm1.{kid}.{nonce}.{ciphertext}`, AES-256-GCM under a master key, with associated data
//! `pm1|{table}|{column}|{row id}` so that a ciphertext copied into another row or column fails to open.
//! The nonce comes from the caller (`platform::Rng`): `core` reads no randomness.
//!
//! [Security §7.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// `pm1`.
pub const VERSION: &str = "pm1";

/// The key ID: the first 8 bytes of SHA-256 of the key, lower-case hex (16 characters).
pub fn kid(key: &[u8; 32]) -> String {
    let digest = Sha256::digest(key);
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// `pm1|{table}|{column}|{row id}`.
pub fn aad(table: &str, column: &str, row_id: &str) -> String {
    format!("{VERSION}|{table}|{column}|{row_id}")
}

/// Seals `plaintext` under `key` with a 12-byte `nonce` that the caller drew at random.
pub fn seal(key: &[u8; 32], nonce: [u8; 12], aad: &str, plaintext: &[u8]) -> String {
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));
    let ct = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .expect("AES-GCM encryption of a bounded value does not fail");
    format!(
        "{VERSION}.{}.{}.{}",
        kid(key),
        URL_SAFE_NO_PAD.encode(nonce),
        URL_SAFE_NO_PAD.encode(ct)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// Not a `pm1.` envelope.
    Malformed,
    /// Sealed under a key whose ID matches neither slot.
    UnknownKey,
    /// The tag does not verify: wrong key bytes, wrong AAD, or a changed ciphertext.
    Invalid,
}

/// The key ID an envelope names, without opening it (the master-key rotation counts by it).
pub fn envelope_kid(envelope: &str) -> Option<&str> {
    let mut parts = envelope.split('.');
    (parts.next()? == VERSION).then_some(())?;
    let kid = parts.next()?;
    (kid.len() == 16
        && kid
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
    .then_some(kid)
}

/// Opens an envelope with whichever of `keys` matches its key ID.
pub fn open(keys: &[&[u8; 32]], aad: &str, envelope: &str) -> Result<Vec<u8>, OpenError> {
    let parts: Vec<&str> = envelope.split('.').collect();
    let [version, kid_text, nonce, ct] = parts[..] else {
        return Err(OpenError::Malformed);
    };
    if version != VERSION || envelope_kid(envelope).is_none() {
        return Err(OpenError::Malformed);
    }
    let nonce = URL_SAFE_NO_PAD
        .decode(nonce)
        .map_err(|_| OpenError::Malformed)?;
    let ct = URL_SAFE_NO_PAD
        .decode(ct)
        .map_err(|_| OpenError::Malformed)?;
    let nonce = <[u8; 12]>::try_from(nonce).map_err(|_| OpenError::Malformed)?;
    if ct.len() < 16 {
        return Err(OpenError::Malformed);
    }
    let key = keys
        .iter()
        .find(|k| kid(k) == kid_text)
        .ok_or(OpenError::UnknownKey)?;
    Aes256Gcm::new(&Key::<Aes256Gcm>::from(**key))
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &ct,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| OpenError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: [u8; 32] = [7; 32];
    const B: [u8; 32] = [9; 32];

    /// Seal/open round trip; a wrong AAD, a wrong key or a flipped bit fails.
    /// Covers: SEC-3
    #[test]
    fn envelope_round_trip() {
        let aad_whk = aad("webhook_endpoints", "secret_enc", "whk_01J9");
        let env = seal(&A, [1; 12], &aad_whk, b"whsec_secret");
        assert!(env.starts_with(&format!("pm1.{}.", kid(&A))));
        assert_eq!(envelope_kid(&env), Some(kid(&A).as_str()));
        assert_eq!(open(&[&B, &A], &aad_whk, &env).unwrap(), b"whsec_secret");

        // Another row or column.
        let moved = aad("webhook_endpoints", "prev_secret_enc", "whk_01J9");
        assert_eq!(open(&[&A], &moved, &env), Err(OpenError::Invalid));
        assert_eq!(
            open(
                &[&A],
                &aad("webhook_endpoints", "secret_enc", "whk_02"),
                &env
            ),
            Err(OpenError::Invalid)
        );
        // A key with another ID, and the right ID with other bytes, both fail.
        assert_eq!(open(&[&B], &aad_whk, &env), Err(OpenError::UnknownKey));
        // Every single-character change of the ciphertext part fails.
        let cut = env.rfind('.').unwrap() + 1;
        for i in cut..env.len() {
            let mut bytes = env.clone().into_bytes();
            bytes[i] = if bytes[i] == b'A' { b'B' } else { b'A' };
            let changed = String::from_utf8(bytes).unwrap();
            assert!(open(&[&A], &aad_whk, &changed).is_err(), "position {i}");
        }
        for bad in [
            "",
            "pm1",
            "pm2.0000000000000000.AAAA.AAAA",
            "pm1.XYZ.AAAA.AAAA",
            &env.replace("pm1.", "pm1.."),
        ] {
            assert_eq!(
                open(&[&A], &aad_whk, bad),
                Err(OpenError::Malformed),
                "{bad}"
            );
        }
        assert_eq!(kid(&A).len(), 16);
        assert_ne!(kid(&A), kid(&B));
    }
}
