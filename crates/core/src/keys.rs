//! API key strings ([Security §4.1]): `pmk_{mode}_{lookup}_{secret}`, lower-case Crockford base32
//! (`0123456789abcdefghjkmnpqrstvwxyz`, no `i l o u`). The lookup carries 60 random bits, the secret
//! 32 random bytes; the stored hash is `hex(HMAC-SHA256(PM_KEY_PEPPER, whole string))`. Randomness and
//! the pepper are inputs.
//!
//! [Security §4.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use hmac::{KeyInit, Mac};
use sha2::Sha256;

/// The lower-case Crockford alphabet of key strings.
pub const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

pub const LOOKUP_LEN: usize = 12;
pub const SECRET_LEN: usize = 52;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Live,
    Test,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Live => "live",
            Mode::Test => "test",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "live" => Some(Mode::Live),
            "test" => Some(Mode::Test),
            _ => None,
        }
    }
}

/// A parsed key string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyParts<'a> {
    pub mode: Mode,
    pub lookup: &'a str,
}

fn encode_bits(bytes: &[u8], chars: usize) -> String {
    let mut out = String::with_capacity(chars);
    let (mut acc, mut bits, mut it) = (0u32, 0u32, bytes.iter());
    while out.len() < chars {
        if bits < 5 {
            acc = (acc << 8) | u32::from(*it.next().unwrap_or(&0));
            bits += 8;
        }
        bits -= 5;
        out.push(char::from(ALPHABET[((acc >> bits) & 31) as usize]));
    }
    out
}

/// A new key string from 8 random bytes for the lookup (60 bits used) and 32 for the secret.
pub fn generate(mode: Mode, lookup_random: [u8; 8], secret_random: [u8; 32]) -> String {
    format!(
        "pmk_{}_{}_{}",
        mode.as_str(),
        encode_bits(&lookup_random, LOOKUP_LEN),
        encode_bits(&secret_random, SECRET_LEN)
    )
}

fn in_alphabet(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| ALPHABET.contains(&b))
}

/// Parses a key string that matches `^pmk_(live|test)_[0-9a-hjkmnp-tv-z]{12}_[0-9a-hjkmnp-tv-z]{52}$`.
pub fn parse(token: &str) -> Option<KeyParts<'_>> {
    let rest = token.strip_prefix("pmk_")?;
    let (mode, rest) = rest.split_once('_')?;
    let (lookup, secret) = rest.split_once('_')?;
    (in_alphabet(lookup, LOOKUP_LEN) && in_alphabet(secret, SECRET_LEN)).then_some(KeyParts {
        mode: Mode::parse(mode)?,
        lookup,
    })
}

/// `HMAC-SHA256(pepper, token)`.
pub fn mac(pepper: &[u8; 32], token: &str) -> [u8; 32] {
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(pepper).expect("any key length");
    m.update(token.as_bytes());
    m.finalize().into_bytes().into()
}

/// The stored hash: lower-case hex of [`mac`].
pub fn hash(pepper: &[u8; 32], token: &str) -> String {
    mac(pepper, token)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Whether `token` matches a stored hex hash, in constant time (`Mac::verify_slice`). A malformed
/// stored value is compared against a fixed dummy, so it costs the same and never matches.
pub fn verify(pepper: &[u8; 32], token: &str, stored_hex: &str) -> bool {
    let stored = decode_hex(stored_hex);
    let ok = stored.is_some();
    let stored = stored.unwrap_or([0u8; 32]);
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(pepper).expect("any key length");
    m.update(token.as_bytes());
    m.verify_slice(&stored).is_ok() && ok
}

fn decode_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}


/// Key levels, `Identity < Tenant < Partner < Platform`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Identity,
    Tenant,
    Partner,
    Platform,
}

impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Level::Identity => "identity",
            Level::Tenant => "tenant",
            Level::Partner => "partner",
            Level::Platform => "platform",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "identity" => Level::Identity,
            "tenant" => Level::Tenant,
            "partner" => Level::Partner,
            "platform" => Level::Platform,
            _ => return None,
        })
    }
}

/// Every permission of the catalogue (REST API › Permissions).
pub const PERMISSIONS: &[&str] = &[
    "tenants:manage",
    "identities:read",
    "identities:write",
    "identities:sign",
    "domains:read",
    "domains:write",
    "messages:read",
    "messages:send",
    "messages:write",
    "attachments:read",
    "search:read",
    "search:agentic",
    "quarantine:review",
    "webhooks:read",
    "webhooks:manage",
    "events:read",
    "keys:manage",
    "erasure:manage",
    "tenants:erase",
    "suppressions:manage",
    "usage:read",
    "audit:read",
    "policy:write",
    "accounts:request",
    "accounts:approve",
    "phone:read",
    "phone:request",
    "phone:send",
    "phone:call",
    "phone:approve",
    "phone:manage",
    "phone:compliance",
    "vault:read",
    "vault:write",
    "vault:request",
    "vault:use",
    "vault:approve",
    "vault:admin",
    "drafts:read",
    "drafts:write",
    "drafts:approve",
    "members:read",
    "members:manage",
    "partners:manage",
    "platform:ops",
    "metrics:read",
];

/// Whether a key of `level` can hold `permission` (REST API › Permissions).
pub fn level_can_hold(level: Level, permission: &str) -> bool {
    match permission {
        "platform:ops" | "partners:manage" => level == Level::Platform,
        "tenants:manage" => level >= Level::Partner,
        "members:read"
        | "members:manage"
        | "suppressions:manage"
        | "audit:read"
        | "usage:read"
        | "policy:write"
        | "accounts:approve"
        | "drafts:approve"
        | "phone:approve"
        | "phone:manage"
        | "phone:compliance"
        | "vault:use"
        | "vault:approve"
        | "vault:admin"
        | "tenants:erase" => level >= Level::Tenant,
        "identities:sign" => level <= Level::Tenant,
        p => PERMISSIONS.contains(&p),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn regex_match(s: &str) -> bool {
        let re =
            regex::Regex::new("^pmk_(live|test)_[0-9a-hjkmnp-tv-z]{12}_[0-9a-hjkmnp-tv-z]{52}$")
                .unwrap();
        re.is_match(s)
    }

    proptest! {
        /// Generated keys match the regex; parsing rejects every other shape; the alphabets exclude
        /// `i l o u`.
        /// Covers: FR-KEY-2
        #[test]
        fn format_round_trip(lookup in any::<[u8; 8]>(), secret in any::<[u8; 32]>(), live in any::<bool>(), noise in "\\PC{0,80}") {
            let mode = if live { Mode::Live } else { Mode::Test };
            let key = generate(mode, lookup, secret);
            prop_assert!(regex_match(&key), "{}", key);
            let parts = parse(&key).unwrap();
            prop_assert_eq!(parts.mode, mode);
            prop_assert_eq!(parts.lookup, &key[9..21]);
            prop_assert!(!key[9..].contains(['i', 'l', 'o', 'u']));
            prop_assert_eq!(parse(&noise).is_some(), regex_match(&noise));
            let flipped = key.replacen("pmk_", "pmK_", 1);
            prop_assert!(parse(&flipped).is_none());
            prop_assert!(parse(&key[..key.len() - 1]).is_none());
        }
    }

    #[test]
    fn different_random_gives_different_lookups_and_hash_verifies() {
        let a = generate(Mode::Live, [0; 8], [0; 32]);
        let b = generate(Mode::Live, [0, 0, 0, 0, 0, 0, 0, 1 << 4], [0; 32]);
        assert_ne!(a[..21], b[..21]);
        let pepper = [3u8; 32];
        let h = hash(&pepper, &a);
        assert_eq!(h.len(), 64);
        assert!(verify(&pepper, &a, &h));
        assert!(!verify(&pepper, &b, &h));
        assert!(!verify(&[4; 32], &a, &h));
        assert!(!verify(&pepper, &a, "zz"));
        for bad in [
            "pmk_live_iiiiiiiiiiii_x",
            "pmk_prod_000000000000_0000",
            "Bearer pmk",
        ] {
            assert!(parse(bad).is_none());
        }
    }
}
