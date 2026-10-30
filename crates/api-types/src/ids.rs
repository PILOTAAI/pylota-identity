//! The ID format shared by the API and the platform ([Data model › Conventions]): a type prefix, `_`, and
//! a ULID (26 Crockford base32 characters, upper case). `platform::Ids` generates IDs with [`format`];
//! `core::ids` parses and checks them.
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

/// The time (Unix milliseconds) of a `{prefix}_{ulid}` ID, or `None` when it is not one.
pub fn time_of(id: &str) -> Option<u64> {
    let (_, ulid) = id.split_once('_')?;
    if ulid.len() != 26 {
        return None;
    }
    let mut value: u128 = 0;
    for c in ulid.bytes() {
        let d = CROCKFORD
            .iter()
            .position(|x| *x == c.to_ascii_uppercase())? as u128;
        value = (value << 5) | d;
    }
    Some((value >> 80) as u64)
}

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

#[cfg(test)]
mod time_tests {
    use super::*;

    #[test]
    fn time_of_reads_the_ulid_time() {
        let id = format(IdPrefix::Message, 1_793_000_000_123, 42);
        assert_eq!(time_of(&id), Some(1_793_000_000_123));
        assert_eq!(time_of("msg_short"), None);
    }
}
