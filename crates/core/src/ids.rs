//! Entity IDs ([Data model › Conventions]): a type prefix, `_`, and a ULID (26 Crockford base32
//! characters, upper case). The time and the random bits come from the platform (`platform::Ids`), which
//! also keeps them monotonic; this module only formats and checks.
//!
//! [Data model › Conventions]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/data-model.md

/// Crockford base32, upper case (ULIDs). Thread tokens use the same alphabet in lower case.
pub const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The value of a Crockford character, either case; `None` for `I`, `L`, `O`, `U` and anything else.
pub fn crockford_value(c: u8) -> Option<u8> {
    let c = c.to_ascii_uppercase();
    CROCKFORD.iter().position(|&a| a == c).map(|i| i as u8)
}

macro_rules! prefixes {
    ($($variant:ident => $p:literal,)*) => {
        /// Every ID prefix of the data model.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum IdPrefix { $($variant,)* }

        impl IdPrefix {
            pub const ALL: &[IdPrefix] = &[$(IdPrefix::$variant,)*];

            /// The three letters before the underscore.
            pub const fn as_str(self) -> &'static str {
                match self { $(IdPrefix::$variant => $p,)* }
            }
        }
    };
}

prefixes! {
    Tenant => "ten", Identity => "idn", Address => "adr", Domain => "dom", Thread => "thr",
    Message => "msg", Attachment => "att", Audit => "aud", ConsoleUser => "usr", DeadLetter => "dlq",
    Probe => "prb", ServiceLedger => "sac", EventConsumer => "evc", ScreeningFeedback => "sfb",
    AgentSignup => "asu", PhoneRequest => "phr", ComplianceProfile => "cpp", A2p => "a2p",
    VoiceRoute => "vrt", PortIn => "pti", PhoneEvent => "pev", SmsConversation => "smc",
    VaultItem => "vit", VaultUse => "vus", VaultAudit => "vad", Webhook => "whk", Delivery => "dlv",
    Event => "evt", ApiKey => "key", Erasure => "era", Export => "exp", Job => "job", Request => "req",
    Invitation => "inv", Hold => "hld", Partner => "ptn", Draft => "drf", WebSocket => "wsc",
    Upload => "upl", PhoneNumber => "pnm", PhoneDraft => "pdr", ComplianceDocument => "cpd",
    VoiceConnector => "pvc", PortOut => "pto", PhoneCharge => "chg", Sms => "sms", Call => "cal",
    VaultGrant => "vgr", VaultConsumer => "vcn", VaultTicket => "vtk",
}

/// The largest ULID time: 48 bits of milliseconds.
pub const MAX_TIME_MS: u64 = (1 << 48) - 1;

/// `{prefix}_{ulid}` from a time in Unix milliseconds (48 bits) and 80 random bits (the low 80 bits of
/// `random`). Times above [`MAX_TIME_MS`] are clamped.
pub fn format(prefix: IdPrefix, time_ms: u64, random: u128) -> String {
    let value = (u128::from(time_ms.min(MAX_TIME_MS)) << 80) | (random & ((1u128 << 80) - 1));
    let mut out = String::with_capacity(30);
    out.push_str(prefix.as_str());
    out.push('_');
    for i in 0..26 {
        let shift = 125 - 5 * i;
        out.push(char::from(CROCKFORD[((value >> shift) & 31) as usize]));
    }
    out
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
