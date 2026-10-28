//! Entity IDs ([Data model › Conventions]): a type prefix, `_`, and a ULID (26 Crockford base32
//! characters, upper case). The time and the random bits come from the platform (`platform::Ids`), which
//! also keeps them monotonic; the format and the prefixes live in `api_types::ids`, shared with the
//! platform, and this module parses and checks.
//!
//! [Data model › Conventions]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/data-model.md

pub use pylota_identity_api_types::ids::{
    CROCKFORD, IdPrefix, MAX_TIME_MS, crockford_value, format,
};

/// A parsed ID.
#[derive(Debug, PartialEq, Eq)]
pub struct Id {
    pub prefix: IdPrefix,
    pub time_ms: u64,
    pub random: u128,
}

/// Parses `{prefix}_{ulid}` as stored: the prefix must be known, the ULID upper case and at most
/// `7ZZZZZZZZZZZZZZZZZZZZZZZZZ` (128 bits).
pub fn parse(id: &str) -> Option<Id> {
    let (p, ulid) = id.split_once('_')?;
    let prefix = *IdPrefix::ALL.iter().find(|x| x.as_str() == p)?;
    let bytes = ulid.as_bytes();
    if bytes.len() != 26 || bytes[0] > b'7' {
        return None;
    }
    let mut value: u128 = 0;
    for &c in bytes {
        if c.is_ascii_lowercase() {
            return None;
        }
        value = (value << 5) | u128::from(crockford_value(c)?);
    }
    Some(Id {
        prefix,
        time_ms: (value >> 80) as u64,
        random: value & ((1u128 << 80) - 1),
    })
}

/// Whether `id` is a well-formed ID with this prefix (path and body parameters).
pub fn is_id(id: &str, prefix: IdPrefix) -> bool {
    parse(id).is_some_and(|p| p.prefix == prefix)
}

/// A deterministic ID (Webhooks › Platform events, Design conventions § 9): the ULID time is `time_ms`
/// and its random part the first 10 bytes of `HMAC-SHA256(key, label)`, so every retry of the same
/// change gets the same ID.
pub fn derived(prefix: IdPrefix, time_ms: i64, key: &[u8; 32], label: &str) -> String {
    let mac = crate::keys::mac(key, label);
    let random = mac[..10]
        .iter()
        .fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
    format(prefix, u64::try_from(time_ms).unwrap_or(0), random)
}

/// The event ID of an `rpc_intents` row: [`derived`] over `"intent:" + intent_id`.
pub fn intent_event_id(key: &[u8; 32], intent_id: &str, occurred_at: i64) -> String {
    derived(
        IdPrefix::Event,
        occurred_at,
        key,
        &format!("intent:{intent_id}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_is_stable_and_keyed() {
        let k = [7u8; 32];
        let a = intent_event_id(&k, "req_1:idn_1:identity.updated", 1_469_918_176_385);
        assert_eq!(
            a,
            intent_event_id(&k, "req_1:idn_1:identity.updated", 1_469_918_176_385)
        );
        assert!(a.starts_with("evt_01ARYZ6S41"));
        assert_ne!(
            a,
            intent_event_id(
                &[8u8; 32],
                "req_1:idn_1:identity.updated",
                1_469_918_176_385
            )
        );
        assert_ne!(
            a,
            intent_event_id(&k, "req_2:idn_1:identity.updated", 1_469_918_176_385)
        );
        assert!(parse(&a).is_some());
    }

    #[test]
    fn ulid_spec_vector() {
        // The ULID spec's example time 1469918176385 encodes as 01ARYZ6S41.
        let id = format(IdPrefix::Message, 1_469_918_176_385, 0);
        assert_eq!(id, "msg_01ARYZ6S410000000000000000");
        let parsed = parse(&id).unwrap();
        assert_eq!(parsed.time_ms, 1_469_918_176_385);
        assert_eq!(parsed.random, 0);
    }

    #[test]
    fn round_trip_and_refusals() {
        let id = format(
            IdPrefix::Identity,
            1_793_138_400_000,
            0xdead_beef_cafe_f00d_1234,
        );
        assert!(is_id(&id, IdPrefix::Identity));
        assert!(!is_id(&id, IdPrefix::Message));
        assert_eq!(parse(&id).unwrap().random, 0xdead_beef_cafe_f00d_1234);
        assert_eq!(id.len(), 30);
        for bad in [
            "idn_01ARYZ6S41000000000000000",  // 25 characters
            "idn_81ARYZ6S410000000000000000", // above 128 bits
            "idn_01ARYZ6S41000000000000000I", // I is not Crockford
            "idn_01aryz6s410000000000000000", // stored upper case
            "zzz_01ARYZ6S410000000000000000", // unknown prefix
            "01ARYZ6S410000000000000000",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn prefixes_are_unique_three_letters() {
        let mut seen = std::collections::HashSet::new();
        for p in IdPrefix::ALL {
            assert_eq!(p.as_str().len(), 3);
            assert!(seen.insert(p.as_str()), "{}", p.as_str());
        }
        assert_eq!(seen.len(), 50);
    }
}
