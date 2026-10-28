//! List cursors (REST API › Pagination): `c_` ‖ kid ‖ base64url(payload ‖ tag), where the payload is
//! `{"v":1,"r":"<operationId>","f":"<filter hash>","k":["<created_at>","<id>"],"t":<issued_at ms>}` and
//! the tag is the first 16 bytes of `HMAC-SHA256(cursor key, "pm-list-cursor-v1\0" ‖ payload)`. A cursor
//! works only for the list, filters and key that made it, and for 24 hours.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{KeyInit, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"pm-list-cursor-v1\0";
pub const LIFETIME_MS: i64 = 86_400_000;

fn tag(key: &[u8; 32], payload: &[u8]) -> [u8; 16] {
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("any key length");
    m.update(DOMAIN);
    m.update(payload);
    let full: [u8; 32] = m.finalize().into_bytes().into();
    full[..16].try_into().expect("16 bytes")
}

/// The first 16 bytes, hex, of SHA-256 over the parts joined with NUL.
pub fn filter_hash(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0]);
    }
    h.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A cursor for the next page after `(created_at, id)`.
pub fn encode(
    kid: &str,
    key: &[u8; 32],
    operation: &str,
    filter: &str,
    created_at: i64,
    id: &str,
    issued_at: i64,
) -> String {
    let payload = json!({ "v": 1, "r": operation, "f": filter, "k": [created_at.to_string(), id], "t": issued_at }).to_string();
    let mut bytes = payload.into_bytes();
    let t = tag(key, &bytes);
    bytes.extend_from_slice(&t);
    format!("c_{kid}{}", URL_SAFE_NO_PAD.encode(bytes))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorError {
    /// `400 invalid_request` (path `cursor`).
    Invalid,
    /// `410 cursor_expired`.
    Expired,
}

/// The keyset `(created_at, id)` of a cursor, checked against the keys (kid, key) that may verify it.
pub fn decode(
    token: &str,
    keys: &[(&str, &[u8; 32])],
    operation: &str,
    filter: &str,
    now_ms: i64,
) -> Result<(i64, String), CursorError> {
    let rest = token.strip_prefix("c_").ok_or(CursorError::Invalid)?;
    let (kid, data) = rest.split_at_checked(1).ok_or(CursorError::Invalid)?;
    let (_, key) = keys
        .iter()
        .find(|(k, _)| *k == kid)
        .ok_or(CursorError::Invalid)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(data)
        .map_err(|_| CursorError::Invalid)?;
    if bytes.len() < 17 {
        return Err(CursorError::Invalid);
    }
    let (payload, t) = bytes.split_at(bytes.len() - 16);
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(*key).expect("any key length");
    m.update(DOMAIN);
    m.update(payload);
    m.verify_truncated_left(t)
        .map_err(|_| CursorError::Invalid)?;
    let v: Value = serde_json::from_slice(payload).map_err(|_| CursorError::Invalid)?;
    if v["v"] != 1 || v["r"] != operation || v["f"] != filter {
        return Err(CursorError::Invalid);
    }
    let issued = v["t"].as_i64().ok_or(CursorError::Invalid)?;
    if now_ms - issued > LIFETIME_MS {
        return Err(CursorError::Expired);
    }
    let created = v["k"][0]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or(CursorError::Invalid)?;
    let id = v["k"][1].as_str().ok_or(CursorError::Invalid)?.to_owned();
    Ok((created, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_refusals() {
        let k = [5u8; 32];
        let f = filter_hash(&["key_1", "status=active"]);
        let c = encode("0", &k, "listTenants", &f, 10, "ten_1", 1_000);
        assert!(c.starts_with("c_0"));
        assert_eq!(
            decode(&c, &[("0", &k)], "listTenants", &f, 2_000),
            Ok((10, "ten_1".to_owned()))
        );
        assert_eq!(
            decode(&c, &[("0", &k)], "listKeys", &f, 2_000),
            Err(CursorError::Invalid)
        );
        assert_eq!(
            decode(
                &c,
                &[("0", &k)],
                "listTenants",
                &filter_hash(&["key_2"]),
                2_000
            ),
            Err(CursorError::Invalid)
        );
        assert_eq!(
            decode(&c, &[("1", &k)], "listTenants", &f, 2_000),
            Err(CursorError::Invalid)
        );
        assert_eq!(
            decode(&c, &[("0", &[6; 32])], "listTenants", &f, 2_000),
            Err(CursorError::Invalid)
        );
        assert_eq!(
            decode(&c, &[("0", &k)], "listTenants", &f, 1_000 + LIFETIME_MS + 1),
            Err(CursorError::Expired)
        );
        assert_eq!(
            decode("c_0!!", &[("0", &k)], "listTenants", &f, 0),
            Err(CursorError::Invalid)
        );
    }
}
