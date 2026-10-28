//! Search cursors ([Search § 5.8]): `c_` ‖ kid ‖ base64url(payload ‖ tag), the payload the compact JSON
//! of [`CursorV1`] and the tag the first 16 bytes of `HMAC-SHA256(cursor key, "pm-cursor-v1\0" ‖
//! payload)`. A cursor pins `as_of` and the position of the last hit, and works for 24 hours.
//!
//! [Search § 5.8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use crate::cursor::CursorError;

const DOMAIN: &[u8] = b"pm-cursor-v1\0";
pub const LIFETIME_MS: i64 = 86_400_000;
/// At most this many `seen` entries are carried.
pub const MAX_SEEN: usize = 200;

/// The order key of a hit: `(score_q DESC, date DESC, ord DESC)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Boundary {
    pub score_q: u32,
    pub date: i64,
    pub ord: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorV1 {
    pub v: u8,
    pub as_of: i64,
    pub issued_at: i64,
    /// Hex of the first 16 bytes of SHA-256 over the canonical request.
    pub qh: String,
    pub last: Boundary,
    pub seen: Vec<u64>,
}

/// `round(score × 1,000,000)`.
pub fn score_q(score: f64) -> u32 {
    (score.clamp(0.0, 1.0) * 1_000_000.0).round() as u32
}

/// The `qh` of a canonical request (its parts joined with `\n`).
pub fn request_hash(parts: &[&str]) -> String {
    let h = Sha256::digest(parts.join("\n").as_bytes());
    h[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn tag(key: &[u8; 32], payload: &[u8]) -> [u8; 16] {
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("any key length");
    m.update(DOMAIN);
    m.update(payload);
    let full: [u8; 32] = m.finalize().into_bytes().into();
    full[..16].try_into().expect("16 bytes")
}

pub fn encode(kid: &str, key: &[u8; 32], c: &CursorV1) -> String {
    let mut bytes = serde_json::to_vec(c).expect("a cursor serialises");
    let t = tag(key, &bytes);
    bytes.extend_from_slice(&t);
    format!("c_{kid}{}", URL_SAFE_NO_PAD.encode(bytes))
}

/// Verifies a cursor against the keys that may still verify it, its lifetime and the request hash.
pub fn decode(
    token: &str,
    keys: &[(&str, &[u8; 32])],
    qh: &str,
    now_ms: i64,
) -> Result<CursorV1, CursorError> {
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
    let c: CursorV1 = serde_json::from_slice(payload).map_err(|_| CursorError::Invalid)?;
    if c.v != 1 {
        return Err(CursorError::Invalid);
    }
    if now_ms - c.issued_at > LIFETIME_MS {
        return Err(CursorError::Expired);
    }
    if c.qh != qh {
        return Err(CursorError::Invalid);
    }
    Ok(c)
}

/// Whether a hit with key `k` comes after the boundary on the next page: below `last`, or within the
/// slack band `eps` above it and not already seen.
pub fn after(c: &CursorV1, k: &Boundary, eps: u32) -> bool {
    let below = (k.score_q, k.date, k.ord) < (c.last.score_q, c.last.date, c.last.ord);
    below
        || (k.score_q <= c.last.score_q.saturating_add(eps)
            && !c.seen.contains(&k.ord)
            && k.ord != c.last.ord)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c() -> CursorV1 {
        CursorV1 {
            v: 1,
            as_of: 1_000,
            issued_at: 1_000,
            qh: request_hash(&["q", "hybrid"]),
            last: Boundary {
                score_q: 500_000,
                date: 10,
                ord: 7,
            },
            seen: vec![9],
        }
    }

    #[test]
    fn cursor_tamper() {
        let k = [3u8; 32];
        let old = [4u8; 32];
        let t = encode("1", &k, &c());
        let qh = c().qh;
        assert_eq!(decode(&t, &[("1", &k)], &qh, 2_000), Ok(c()));
        // The previous kid still verifies inside its window.
        let t0 = encode("0", &old, &c());
        assert!(decode(&t0, &[("1", &k), ("0", &old)], &qh, 2_000).is_ok());
        assert_eq!(
            decode(&t0, &[("1", &k)], &qh, 2_000),
            Err(CursorError::Invalid)
        );
        let mut b = URL_SAFE_NO_PAD.decode(&t[3..]).unwrap();
        b[3] ^= 1;
        let bad = format!("c_1{}", URL_SAFE_NO_PAD.encode(b));
        assert_eq!(
            decode(&bad, &[("1", &k)], &qh, 2_000),
            Err(CursorError::Invalid)
        );
        assert_eq!(
            decode(&t, &[("1", &k)], &qh, 1_000 + LIFETIME_MS + 1),
            Err(CursorError::Expired)
        );
        assert_eq!(
            decode(&t, &[("1", &k)], "other", 2_000),
            Err(CursorError::Invalid)
        );
    }

    #[test]
    fn slack_band() {
        let cur = c();
        assert!(after(
            &cur,
            &Boundary {
                score_q: 400_000,
                date: 0,
                ord: 1
            },
            0
        ));
        assert!(!after(
            &cur,
            &Boundary {
                score_q: 510_000,
                date: 0,
                ord: 1
            },
            0
        ));
        assert!(after(
            &cur,
            &Boundary {
                score_q: 510_000,
                date: 0,
                ord: 1
            },
            20_000
        ));
        assert!(
            !after(
                &cur,
                &Boundary {
                    score_q: 510_000,
                    date: 0,
                    ord: 9
                },
                20_000
            ),
            "seen"
        );
    }
}
