//! Signature and checksums of a release ([CLI and setup §8.2]): `SHA256SUMS.sig` is a prehashed minisign
//! signature of `SHA256SUMS` by one of the compiled-in keys, its trusted comment names the version, and
//! the tarball's SHA-256 matches its line. Any failure is exit 11; there is no flag to skip it.
//!
//! [CLI and setup §8.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#82-signature-and-checksums

use std::collections::BTreeMap;

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest as _, Sha256};

use crate::CliError;

/// The release signing keys (`current`, then `next`), base64 minisign public keys. The owner's keys
/// are added with the release pipeline (build plan M19); until then no download verifies, which is
/// what an unsigned project should do.
pub const RELEASE_KEYS: &[&str] = &[];

/// Verifies `sig` over `sums` with one of `keys`; returns the signing key's ID (hex).
pub fn signature(
    sums: &[u8],
    sig: &str,
    version: &str,
    keys: &[String],
) -> Result<String, CliError> {
    let bad = |why: &str| CliError::verification(format!("SHA256SUMS.sig: {why}"));
    let sig = Signature::decode(sig).map_err(|_| bad("not a minisign signature"))?;
    let mut wrong_key = true;
    for k in keys {
        let pk = PublicKey::from_base64(k).map_err(|_| bad("a compiled-in key is invalid"))?;
        match pk.verify(sums, &sig, false) {
            Ok(()) => {
                let want = format!("pylota-identity v{version}");
                if sig.trusted_comment() != want {
                    return Err(bad(&format!(
                        "the trusted comment is {:?}, not {want:?} (a signature of another release)",
                        sig.trusted_comment()
                    )));
                }
                return Ok(key_id(k));
            }
            Err(minisign_verify::Error::UnexpectedKeyId) => {}
            Err(_) => wrong_key = false,
        }
    }
    Err(bad(if wrong_key {
        "signed by a key this idvy does not trust"
    } else {
        "the signature does not verify"
    }))
}

/// The key ID of a base64 public key, as hex.
fn key_id(b64: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .ok()
        .and_then(|b| {
            b.get(2..10)
                .map(|id| id.iter().rev().map(|x| format!("{x:02X}")).collect())
        })
        .unwrap_or_default()
}

/// `SHA256SUMS`: lines `<64 lower-case hex>␠␠<filename>`, file names `^[A-Za-z0-9._-]{1,128}$`, no
/// duplicates.
pub fn sums(text: &[u8]) -> Result<BTreeMap<String, String>, CliError> {
    let bad = |why: String| CliError::verification(format!("SHA256SUMS: {why}"));
    let text = std::str::from_utf8(text).map_err(|_| bad("not UTF-8".into()))?;
    let mut out = BTreeMap::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let (hash, name) = line
            .split_once("  ")
            .ok_or_else(|| bad(format!("bad line {line:?}")))?;
        let hash_ok = hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let name_ok = (1..=128).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !hash_ok || !name_ok {
            return Err(bad(format!("bad line {line:?}")));
        }
        if out.insert(name.to_owned(), hash.to_owned()).is_some() {
            return Err(bad(format!("{name} is listed twice")));
        }
    }
    Ok(out)
}

/// The lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The file's SHA-256 must equal its line.
pub fn checksum(
    listed: &BTreeMap<String, String>,
    name: &str,
    bytes: &[u8],
) -> Result<String, CliError> {
    let want = listed
        .get(name)
        .ok_or_else(|| CliError::verification(format!("SHA256SUMS has no line for {name}")))?;
    let got = sha256_hex(bytes);
    if &got != want {
        return Err(CliError::verification(format!(
            "{name}: SHA-256 {got} does not match SHA256SUMS"
        )));
    }
    Ok(got)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sums_rules() {
        let h = "a".repeat(64);
        assert!(
            super::sums(format!("{h}  pylota-identity-worker-1.0.0.tar.gz\n").as_bytes()).is_ok()
        );
        assert!(super::sums(format!("{h}  ../evil\n").as_bytes()).is_err());
        assert!(super::sums(format!("{}  x\n", "A".repeat(64)).as_bytes()).is_err());
        assert!(super::sums(format!("{h}  x\n{h}  x\n").as_bytes()).is_err());
    }
}
