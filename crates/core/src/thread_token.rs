//! The thread token carried in the `Reply-To` sub-address ([Threading § 2]): `t{K}{S}.{M}`, where `K` is
//! the thread key's kid, `S` the thread seq and `M` the first 40 bits of an HMAC-SHA256 over a fixed
//! 46-byte layout. A token is a filing hint, never a credential.
//!
//! [Threading § 2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/threading.md

use hmac::{KeyInit as _, Mac as _};
use sha2::Sha256;

use crate::ids::crockford_value;

pub const TOKEN_VERSION: u8 = 0x01;

/// Crockford base32, lower case.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Seqs at or above 2^60 are never minted: they would not fit in 12 characters.
pub const MAX_SEQ: u64 = 1 << 60;

pub struct ThreadKey<'a> {
    /// One lower-case ASCII Crockford character (`signing_keys.kid`).
    pub kid: u8,
    pub key: &'a [u8; 32],
}

pub struct ThreadKeyring<'a> {
    /// The `signing_keys` row with `verify_until IS NULL`.
    pub current: ThreadKey<'a>,
    /// Older kids whose `verify_until` is still in the future.
    pub verifying: &'a [ThreadKey<'a>],
}

#[derive(Debug, PartialEq, Eq)]
pub enum TokenCheck {
    /// No detail part, or the detail does not look like a token.
    Absent,
    Valid {
        seq: u64,
        kid: u8,
        current: bool,
    },
    /// Looks like a token; unknown kid, or the MAC does not verify.
    Invalid,
    /// Looks like a token, not verified because a brute-force budget is spent; neither joins nor flags.
    Unchecked,
}

/// The 46 bytes the MAC covers (Threading § 2.2).
pub fn mac_input(kid: u8, identity_id: &str, seq: u64) -> [u8; 46] {
    let mut out = [0u8; 46];
    out[..6].copy_from_slice(b"pm-thr");
    out[6] = TOKEN_VERSION;
    out[7] = kid;
    // "idn_" + 26 characters; anything else is padded or cut so the layout stays fixed, and then never
    // verifies against a real identity.
    let id = identity_id.as_bytes();
    let n = id.len().min(30);
    out[8..8 + n].copy_from_slice(&id[..n]);
    out[38..].copy_from_slice(&seq.to_be_bytes());
    out
}

fn mac40(key: &[u8; 32], kid: u8, identity_id: &str, seq: u64) -> [u8; 8] {
    let mut mac =
        hmac::Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(&mac_input(kid, identity_id, seq));
    let tag = mac.finalize().into_bytes();
    let n = tag[..5].iter().fold(0u64, |n, &b| (n << 8) | u64::from(b));
    let mut out = [0u8; 8];
    for (i, c) in out.iter_mut().enumerate() {
        *c = ALPHABET[((n >> (35 - 5 * i)) & 31) as usize];
    }
    out
}

fn seq_chars(mut seq: u64) -> String {
    let mut digits = Vec::with_capacity(12);
    loop {
        digits.push(ALPHABET[(seq & 31) as usize]);
        seq >>= 5;
        if seq == 0 {
            break;
        }
    }
    digits.reverse();
    String::from_utf8(digits).expect("ASCII")
}

/// `t{K}{S}.{M}`, minted with the current key. `seq` must be between 1 and 2^60 − 1.
pub fn mint(ring: &ThreadKeyring<'_>, identity_id: &str, seq: u64) -> String {
    debug_assert!((1..MAX_SEQ).contains(&seq));
    let key = &ring.current;
    let m = mac40(key.key, key.kid, identity_id, seq);
    format!(
        "t{}{}.{}",
        char::from(key.kid),
        seq_chars(seq),
        std::str::from_utf8(&m).expect("ASCII")
    )
}

/// `bookings.acme+t03k.9f2mq7xa`.
pub fn reply_to_local(local: &str, token: &str) -> String {
    format!("{local}+{token}")
}

fn is_lower_crockford(c: u8) -> bool {
    ALPHABET.contains(&c)
}

/// Verifies the detail part of the envelope recipient, already lower-cased (Threading § 2.3).
pub fn verify(ring: &ThreadKeyring<'_>, identity_id: &str, detail: &str) -> TokenCheck {
    let b = detail.as_bytes();
    // ^t(K)(S{1,12})\.(M{8})$
    if b.len() < 12 || b[0] != b't' || b[b.len() - 9] != b'.' {
        return TokenCheck::Absent;
    }
    let kid = b[1];
    let s = &b[2..b.len() - 9];
    let m = &b[b.len() - 8..];
    if !(1..=12).contains(&s.len())
        || !is_lower_crockford(kid)
        || !s.iter().all(|&c| is_lower_crockford(c))
        || !m.iter().all(|&c| is_lower_crockford(c))
    {
        return TokenCheck::Absent;
    }
    if s[0] == b'0' {
        return TokenCheck::Invalid;
    }
    let seq = s.iter().fold(0u64, |n, &c| {
        (n << 5) | u64::from(crockford_value(c).expect("checked"))
    });
    if seq == 0 || seq >= MAX_SEQ {
        return TokenCheck::Invalid;
    }
    let (key, current) = if ring.current.kid == kid {
        (&ring.current, true)
    } else if let Some(k) = ring.verifying.iter().find(|k| k.kid == kid) {
        (k, false)
    } else {
        return TokenCheck::Invalid;
    };
    let expected = mac40(key.key, kid, identity_id, seq);
    let diff = expected
        .iter()
        .zip(m)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    if diff == 0 {
        TokenCheck::Valid { seq, kid, current }
    } else {
        TokenCheck::Invalid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const IDN: &str = "idn_01J9Z3K8V4QW7X2M5N6P8R0T1Y";
    const KEY0: [u8; 32] = [7; 32];
    const KEY1: [u8; 32] = [9; 32];

    fn ring<'a>(verifying: &'a [ThreadKey<'a>]) -> ThreadKeyring<'a> {
        ThreadKeyring {
            current: ThreadKey {
                kid: b'1',
                key: &KEY1,
            },
            verifying,
        }
    }

    #[test]
    fn a2_layout_vectors() {
        let input = mac_input(b'0', IDN, 115);
        assert_eq!(&input[..6], b"pm-thr");
        assert_eq!(input[6], 1);
        assert_eq!(input[7], b'0');
        assert_eq!(&input[8..38], IDN.as_bytes());
        assert_eq!(&input[38..], &[0, 0, 0, 0, 0, 0, 0, 115]);
        assert_eq!(seq_chars(115), "3k");
        assert_eq!(seq_chars(1), "1");
        assert_eq!(seq_chars(32), "10");
        assert_eq!(seq_chars(MAX_SEQ - 1), "zzzzzzzzzzzz");

        // HMAC-SHA256(key = 32 × 0x07, the 46 bytes above), computed independently with Python's
        // hmac module (2026-10-27): first five bytes 43 30 35 ec b2.
        let r = ThreadKeyring {
            current: ThreadKey {
                kid: b'0',
                key: &KEY0,
            },
            verifying: &[],
        };
        let token = mint(&r, IDN, 115);
        assert_eq!(token, "t03k.8cr3bv5j");
        assert_eq!(
            reply_to_local("bookings.acme", &token),
            "bookings.acme+t03k.8cr3bv5j"
        );
        assert_eq!(
            verify(&r, IDN, &token),
            TokenCheck::Valid {
                seq: 115,
                kid: b'0',
                current: true
            }
        );
    }

    #[test]
    fn a2_non_token_tags() {
        let r = ring(&[]);
        for detail in [
            "",
            "invoices",
            "t",
            "t03k",
            "T03K.9F2MQ7XA",
            "t0i3.9f2mq7xa",
            "t03k.9f2mq7x",
        ] {
            assert_eq!(verify(&r, IDN, detail), TokenCheck::Absent, "{detail}");
        }
        // Lower-cased first (address normalisation), it is token-shaped and fails the MAC.
        assert_eq!(verify(&r, IDN, "t13k.9f2mq7xa"), TokenCheck::Invalid);
        // Leading zero and zero seq.
        assert_eq!(verify(&r, IDN, "t103k.9f2mq7xa"), TokenCheck::Invalid);
        assert_eq!(verify(&r, IDN, "t10.9f2mq7xa"), TokenCheck::Invalid);
        // Thirteen seq characters do not match the pattern.
        assert_eq!(
            verify(&r, IDN, "t11234567890123.9f2mq7xa"),
            TokenCheck::Absent
        );
        // Twelve characters at or above 2^60.
        assert!(matches!(
            verify(&r, IDN, "t1z00000000000.9f2mq7xa"),
            TokenCheck::Absent | TokenCheck::Invalid
        ));
    }

    #[test]
    fn a2_rotated_kid() {
        let old = [ThreadKey {
            kid: b'0',
            key: &KEY0,
        }];
        let before = ThreadKeyring {
            current: ThreadKey {
                kid: b'0',
                key: &KEY0,
            },
            verifying: &[],
        };
        let token = mint(&before, IDN, 42);
        assert_eq!(
            verify(&ring(&old), IDN, &token),
            TokenCheck::Valid {
                seq: 42,
                kid: b'0',
                current: false
            }
        );
        assert_eq!(verify(&ring(&[]), IDN, &token), TokenCheck::Invalid);
        let unknown = format!("t2{}", &token[2..]);
        assert_eq!(verify(&ring(&old), IDN, &unknown), TokenCheck::Invalid);
    }

    #[test]
    fn a12_budget() {
        let local = "a".repeat(40);
        let token = mint(&ring(&[]), IDN, MAX_SEQ - 1);
        assert_eq!(reply_to_local(&local, &token).len(), 64);
    }

    proptest! {
        #[test]
        fn a2_round_trip(random in any::<u128>(), seq in 1u64..MAX_SEQ, kid in 0usize..32,
                         key in any::<[u8; 32]>(), bit in any::<prop::sample::Index>()) {
            let idn = crate::ids::format(crate::ids::IdPrefix::Identity, 1_793_138_400_000, random);
            let r = ThreadKeyring { current: ThreadKey { kid: ALPHABET[kid], key: &key }, verifying: &[] };
            let token = mint(&r, &idn, seq);
            prop_assert_eq!(verify(&r, &idn, &token),
                            TokenCheck::Valid { seq, kid: ALPHABET[kid], current: true });

            // A token for one identity never verifies for another.
            let other = crate::ids::format(crate::ids::IdPrefix::Identity, 1_793_138_400_000, random ^ 1);
            prop_assert_eq!(verify(&r, &other, &token), TokenCheck::Invalid);

            // Every single-bit flip of K, S or M (the bytes after "t", except the ".") fails.
            let mut bytes = token.into_bytes();
            let positions: Vec<usize> = (1..bytes.len()).filter(|&i| bytes[i] != b'.').collect();
            let i = positions[bit.index(positions.len())];
            for b in 0..8 {
                let mut flipped = bytes.clone();
                flipped[i] ^= 1 << b;
                if let Ok(s) = std::str::from_utf8(&flipped) {
                    let check = verify(&r, &idn, s);
                    prop_assert!(matches!(check, TokenCheck::Invalid | TokenCheck::Absent), "{s}: {check:?}");
                }
            }
            bytes.clear();
        }
    }
}
