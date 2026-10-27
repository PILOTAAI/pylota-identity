//! Addresses: envelope normalisation, username validation, reserved names and confusable detection
//! ([Identities › Username validation], [Inbound › Steps]). A1, A3, A4 and A12.
//!
//! [Identities › Username validation]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md
//! [Inbound › Steps]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

#[rustfmt::skip]
pub(crate) mod confusables_table;

use unicode_normalization::UnicodeNormalization as _;

use confusables_table::{
    CASE_FOLDING, CONFUSABLES, DEFAULT_IGNORABLE, SCRIPT_SETS, SCRIPTS, STRONG_RTL,
};

/// Which reserved set applies (Identities › Reserved names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainClass {
    /// A username of the default tenant: it stands alone on the shared platform domain.
    Platform,
    /// Any other username, and a local part on a tenant's own domain.
    Tenant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressError {
    /// `400 address_reserved`
    Reserved,
    /// `400 address_unsupported`: an SMTPUTF8 local part.
    Unsupported,
    /// `400 address_invalid`
    Invalid,
    /// `400 local_part_too_long`
    LocalPartTooLong,
    /// `409 username_taken`
    Taken,
}

impl AddressError {
    pub const fn code(self) -> &'static str {
        match self {
            AddressError::Reserved => "address_reserved",
            AddressError::Unsupported => "address_unsupported",
            AddressError::Invalid => "address_invalid",
            AddressError::LocalPartTooLong => "local_part_too_long",
            AddressError::Taken => "username_taken",
        }
    }
}

/// `max_len` for a username.
pub const USERNAME_MAX: usize = 24;
/// `max_len` for an alias local part on a tenant domain.
pub const LOCAL_PART_MAX: usize = 40;
/// Username plus tenant suffix, leaving room for a thread token in 64 octets (A12).
pub const LOCAL_BUDGET: usize = 40;

/// RFC 2142 role names reserved on the platform domain only.
pub const ROLE_NAMES: &[&str] = &[
    "info",
    "marketing",
    "sales",
    "support",
    "noc",
    "security",
    "hostmaster",
    "usenet",
    "news",
    "webmaster",
    "www",
    "uucp",
    "ftp",
];

/// RFC 2142 names that must reach a person: reserved on every domain.
pub const PERSON_NAMES: &[&str] = &["postmaster", "abuse"];

/// Service and mail-system names: reserved on every domain.
pub const SERVICE_NAMES: &[&str] = &[
    "noreply",
    "no-reply",
    "donotreply",
    "do-not-reply",
    "mailer-daemon",
    "mailerdaemon",
    "mail-daemon",
    "bounce",
    "bounces",
    "root",
    "admin",
    "administrator",
    "sysadmin",
    "system",
    "daemon",
    "nobody",
    "null",
    "devnull",
    "listserv",
    "majordomo",
    "unsubscribe",
    "dmarc",
    "journal",
];

/// Platform-domain names whose mail is relayed to `PM_SECURITY_CONTACT` (Inbound › Steps, step 3).
pub const OPERATIONAL_NAMES: &[&str] = &[
    "postmaster",
    "abuse",
    "security",
    "hostmaster",
    "webmaster",
    "noc",
];

fn reserved_names(class: DomainClass) -> impl Iterator<Item = &'static str> {
    let roles: &[&str] = match class {
        DomainClass::Platform => ROLE_NAMES,
        DomainClass::Tenant => &[],
    };
    roles
        .iter()
        .chain(PERSON_NAMES)
        .chain(SERVICE_NAMES)
        .copied()
}

fn reserved_pattern(local: &str) -> bool {
    local.starts_with("owner-") || local.ends_with("-request") || local.starts_with("pm-")
}

/// Whether `local` (lower case) is reserved on this domain class, by name or pattern.
pub fn is_reserved(local: &str, class: DomainClass) -> bool {
    reserved_pattern(local) || reserved_names(class).any(|n| n == local)
}

fn in_ranges(c: char, ranges: &[(u32, u32)]) -> bool {
    let c = c as u32;
    ranges
        .binary_search_by(|&(a, b)| {
            if b < c {
                std::cmp::Ordering::Less
            } else if a > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn lookup<'t>(c: char, table: &'t [(char, &'t str)]) -> Option<&'t str> {
    table
        .binary_search_by_key(&c, |&(k, _)| k)
        .ok()
        .map(|i| table[i].1)
}

fn case_fold(s: &str) -> String {
    s.chars()
        .map(|c| lookup(c, CASE_FOLDING).map_or_else(|| c.to_string(), str::to_owned))
        .collect()
}

fn internal_skeleton(s: &str) -> String {
    let decomposed: String = s
        .nfd()
        .filter(|&c| !in_ranges(c, DEFAULT_IGNORABLE))
        .collect();
    let mapped: String = decomposed
        .chars()
        .map(|c| lookup(c, CONFUSABLES).map_or_else(|| c.to_string(), str::to_owned))
        .collect();
    mapped.nfd().collect()
}

/// The UTS #39 skeleton used to compare identifiers (Identities › Confusable detection): full case
/// folding, then `internalSkeleton` and lower-casing until stable, at most 3 rounds. Two strings are
/// confusable when their folds are equal.
pub fn fold(s: &str) -> String {
    let mut cur = case_fold(s);
    for _ in 0..3 {
        let next = case_fold(&internal_skeleton(&cur));
        if next == cur {
            break;
        }
        cur = next;
    }
    cur
}

/// Whether the resolved script set of `s` is empty (UTS #39 § 5.1, augmented sets; Common and
/// Inherited count as every script).
pub fn is_mixed_script(s: &str) -> bool {
    let mut resolved: Option<Vec<u8>> = None;
    for c in s.chars() {
        let cp = c as u32;
        let i = SCRIPTS.partition_point(|&(_, b, _)| b < cp);
        let set = SCRIPTS
            .get(i)
            .filter(|&&(a, _, _)| a <= cp)
            .map_or(0, |&(_, _, s)| s);
        if set == 0 {
            continue;
        }
        let scripts = SCRIPT_SETS[usize::from(set)];
        resolved = Some(match resolved {
            None => scripts.to_vec(),
            Some(r) => r.into_iter().filter(|x| scripts.contains(x)).collect(),
        });
        if resolved.as_ref().is_some_and(Vec::is_empty) {
            return true;
        }
    }
    false
}

/// Whether `s` holds a strong right-to-left character (bidi class R or AL).
pub fn has_strong_rtl(s: &str) -> bool {
    s.chars().any(|c| in_ranges(c, STRONG_RTL))
}

/// Validates a username or alias local part and returns its stored (lower-case) form, steps 1–7 of
/// Identities › Username validation. `existing` are the tenant's other non-deleted usernames.
pub fn validate_username(
    input: &str,
    class: DomainClass,
    max_len: usize,
    tenant_suffix: &str,
    existing: &[&str],
) -> Result<String, AddressError> {
    // 1. Confusable with a reserved name of this class.
    let folded = fold(input);
    if reserved_names(class).any(|n| fold(n) == folded) {
        return Err(AddressError::Reserved);
    }
    if !input.is_ascii() {
        // 2. Mixed script, or right-to-left.
        if is_mixed_script(input) || has_strong_rtl(input) {
            return Err(AddressError::Reserved);
        }
        // 3. Any other non-ASCII local part.
        return Err(AddressError::Unsupported);
    }
    // 4. Syntax.
    let local = input.to_ascii_lowercase();
    let b = local.as_bytes();
    let first_ok = b
        .first()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok = b
        .iter()
        .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'));
    if !first_ok || !rest_ok || b.len() > max_len || local.contains("..") || local.ends_with('.') {
        return Err(AddressError::Invalid);
    }
    // 5. Exact names and patterns.
    if is_reserved(&local, class) {
        return Err(AddressError::Reserved);
    }
    // 6. Room for a thread token.
    if local.len() + tenant_suffix.len() > LOCAL_BUDGET {
        return Err(AddressError::LocalPartTooLong);
    }
    // 7. Taken, exactly or by fold.
    if existing
        .iter()
        .any(|e| e.eq_ignore_ascii_case(&local) || fold(e) == folded)
    {
        return Err(AddressError::Taken);
    }
    Ok(local)
}

/// A normalised envelope recipient (Inbound › Steps, step 2).
#[derive(Debug, PartialEq, Eq)]
pub struct Envelope {
    /// Lower case, before the first `+`.
    pub base_local: String,
    /// After the first `+`; `None` when absent or empty.
    pub detail: Option<String>,
    /// IDNA A-label, lower case, no trailing dot.
    pub domain: String,
}

impl Envelope {
    /// `base_local@domain`.
    pub fn base(&self) -> String {
        format!("{}@{}", self.base_local, self.domain)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnvelopeError {
    /// No `@`, an empty part, a bad domain or more than 254 octets.
    Malformed,
    /// A non-ASCII local part: no such address can exist (FR-ADR-7), so the recipient is unknown.
    NonAsciiLocal,
}

/// Normalises an envelope recipient: trim and strip `<>`, split at the last `@`, A-label and
/// lower-case the domain, lower-case the local part (dots are significant), split at the first `+`.
pub fn parse_envelope(to: &str) -> Result<Envelope, EnvelopeError> {
    let t = to.trim();
    let t = t.strip_prefix('<').unwrap_or(t);
    let t = t.strip_suffix('>').unwrap_or(t).trim();
    let (local, domain) = t.rsplit_once('@').ok_or(EnvelopeError::Malformed)?;
    let domain = normalise_domain(domain).ok_or(EnvelopeError::Malformed)?;
    if local.is_empty() || local.len() > 64 {
        return Err(EnvelopeError::Malformed);
    }
    if !local.is_ascii() {
        return Err(EnvelopeError::NonAsciiLocal);
    }
    if local.bytes().any(|c| c <= b' ' || c == 0x7f) || local.len() + 1 + domain.len() > 254 {
        return Err(EnvelopeError::Malformed);
    }
    let local = local.to_ascii_lowercase();
    let (base_local, detail) = match local.split_once('+') {
        Some((b, d)) => (b.to_owned(), (!d.is_empty()).then(|| d.to_owned())),
        None => (local, None),
    };
    if base_local.is_empty() {
        return Err(EnvelopeError::Malformed);
    }
    Ok(Envelope {
        base_local,
        detail,
        domain,
    })
}

/// A domain name in stored form: no trailing dot, IDNA A-label, lower case, at most 253 octets. `None`
/// when it is not a valid host name.
pub fn normalise_domain(domain: &str) -> Option<String> {
    let d = domain.trim().strip_suffix('.').unwrap_or(domain.trim());
    if d.is_empty() || d.contains(['[', ']', ':', '/', '\\', '@', ' ']) {
        return None;
    }
    let ascii = idna::domain_to_ascii_strict(d).ok()?;
    let ok = !ascii.is_empty()
        && ascii.len() <= 253
        && ascii.contains('.')
        && ascii.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        });
    ok.then(|| ascii.to_ascii_lowercase())
}

/// The organisational (registrable) domain of a stored-form domain, by the Public Suffix List; the
/// domain itself when the list has no answer (a bare suffix, or an unlisted name).
pub fn org_domain(domain: &str) -> &str {
    psl::domain_str(domain).unwrap_or(domain)
}

/// The domain part of an address, after the last `@`.
pub fn domain_of(address: &str) -> &str {
    address.rsplit_once('@').map_or("", |(_, d)| d)
}

/// How the platform domain handles a `base_local` before the directory lookup (A4).
#[derive(Debug, PartialEq, Eq)]
pub enum RoleRoute {
    /// An operational name: relay to `PM_SECURITY_CONTACT`, or `550 5.1.1` when unset.
    SecurityContact,
    /// `postmaster@` or `abuse@` a tenant domain: relay to the tenant's owner contact.
    TenantOwner,
    /// Not a role address here: the directory lookup decides.
    Directory,
}

/// The A4 routing of role mail. Role names other than the operational ones need no special case: no
/// identity can own them on the platform domain, so the directory lookup rejects them `550 5.1.1`.
pub fn role_route(base_local: &str, class: DomainClass) -> RoleRoute {
    match class {
        DomainClass::Platform if OPERATIONAL_NAMES.contains(&base_local) => {
            RoleRoute::SecurityContact
        }
        DomainClass::Tenant if PERSON_NAMES.contains(&base_local) => RoleRoute::TenantOwner,
        _ => RoleRoute::Directory,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform(input: &str) -> Result<String, AddressError> {
        validate_username(input, DomainClass::Platform, USERNAME_MAX, "", &[])
    }

    fn tenant(input: &str) -> Result<String, AddressError> {
        validate_username(input, DomainClass::Tenant, LOCAL_PART_MAX, "", &[])
    }

    /// Covers: A1
    #[test]
    fn a1_case_and_dots() {
        assert_eq!(platform("Bookings.Acme").unwrap(), "bookings.acme");
        let e = parse_envelope(" <Bookings.Acme+T03K.9F2MQ7XA@Agents.EXAMPLE.> ").unwrap();
        assert_eq!(e.base_local, "bookings.acme");
        assert_eq!(e.detail.as_deref(), Some("t03k.9f2mq7xa"));
        assert_eq!(e.domain, "agents.example");
        // Dots are significant: no provider-style folding.
        assert_ne!(
            parse_envelope("b.ookings@x.example").unwrap().base(),
            parse_envelope("bookings@x.example").unwrap().base()
        );
        assert_eq!(parse_envelope("a+@x.example").unwrap().detail, None);
        assert_eq!(
            parse_envelope("a+b+c@x.example").unwrap().detail.as_deref(),
            Some("b+c")
        );
        assert_eq!(parse_envelope("a@b@x.example").unwrap().base_local, "a@b");
        assert_eq!(
            parse_envelope("a@bücher.example").unwrap().domain,
            "xn--bcher-kva.example"
        );
        for bad in [
            "",
            "a",
            "@x.example",
            "a@",
            "+t@x.example",
            "a@[127.0.0.1]",
            "a b@x.example",
        ] {
            assert_eq!(parse_envelope(bad), Err(EnvelopeError::Malformed), "{bad}");
        }
    }

    /// Covers: A1
    #[test]
    fn a1_syntax() {
        for bad in [
            ".a",
            "a.",
            "a..b",
            "-a",
            "_a",
            "a+b",
            "a b",
            "",
            &"a".repeat(25),
        ] {
            assert_eq!(platform(bad), Err(AddressError::Invalid), "{bad}");
        }
        assert_eq!(platform(&"a".repeat(24)).unwrap().len(), 24);
        assert_eq!(tenant(&"a".repeat(40)).unwrap().len(), 40);
        assert!(platform("a-b_c.d9").is_ok());
    }

    /// Covers: A3
    #[test]
    fn a3_smtputf8_refused() {
        assert_eq!(platform("josé"), Err(AddressError::Unsupported));
        assert_eq!(platform("用户"), Err(AddressError::Unsupported));
        assert_eq!(
            parse_envelope("josé@x.example"),
            Err(EnvelopeError::NonAsciiLocal)
        );
    }

    /// Covers: A4
    #[test]
    fn a4_reserved_and_confusable() {
        for name in [
            "postmaster",
            "abuse",
            "security",
            "support",
            "sales",
            "info",
            "marketing",
            "noreply",
            "mailer-daemon",
            "hostmaster",
            "webmaster",
            "owner-list",
            "list-request",
            "pm-probe",
            "pm-bounce",
            "journal",
        ] {
            assert_eq!(platform(name), Err(AddressError::Reserved), "{name}");
        }
        // rn for m, digit zero for o, Cyrillic а, an invisible joiner.
        assert_eq!(fold("rnailer-daemon"), fold("mailer-daemon"));
        assert_eq!(platform("rnailer-daemon"), Err(AddressError::Reserved));
        assert_eq!(platform("p0stmaster"), Err(AddressError::Reserved));
        assert_eq!(platform("Postmaster"), Err(AddressError::Reserved));
        assert_eq!(platform("аbuse"), Err(AddressError::Reserved));
        assert_eq!(platform("ab\u{200d}use"), Err(AddressError::Reserved));
        // Mixed scripts and right-to-left are reserved, a single other script is unsupported.
        assert!(is_mixed_script("pаypal"));
        assert!(!is_mixed_script("paypal"));
        assert!(!is_mixed_script("用户"));
        assert!(!is_mixed_script("ひらがな漢字"));
        assert_eq!(platform("pаypal"), Err(AddressError::Reserved));
        assert_eq!(platform("مرحبا"), Err(AddressError::Reserved));
        // Taken by exact match or by fold.
        assert_eq!(
            validate_username(
                "bookings",
                DomainClass::Tenant,
                USERNAME_MAX,
                "",
                &["Bookings"]
            ),
            Err(AddressError::Taken)
        );
        assert_eq!(
            validate_username("rnike", DomainClass::Tenant, USERNAME_MAX, "", &["mike"]),
            Err(AddressError::Taken)
        );
    }

    /// Covers: A4
    #[test]
    fn a4_role_names_by_domain() {
        for name in [
            "support",
            "sales",
            "info",
            "marketing",
            "security",
            "webmaster",
        ] {
            assert_eq!(platform(name), Err(AddressError::Reserved), "{name}");
            assert_eq!(tenant(name).as_deref(), Ok(name), "{name}");
        }
        for name in ["postmaster", "abuse", "noreply", "mailer-daemon", "pm-x"] {
            assert_eq!(tenant(name), Err(AddressError::Reserved), "{name}");
        }
        assert_eq!(
            role_route("security", DomainClass::Platform),
            RoleRoute::SecurityContact
        );
        assert_eq!(
            role_route("noc", DomainClass::Platform),
            RoleRoute::SecurityContact
        );
        assert_eq!(
            role_route("sales", DomainClass::Platform),
            RoleRoute::Directory
        );
        assert_eq!(
            role_route("abuse", DomainClass::Tenant),
            RoleRoute::TenantOwner
        );
        assert_eq!(
            role_route("security", DomainClass::Tenant),
            RoleRoute::Directory
        );
    }

    /// Covers: A12
    #[test]
    fn a12_local_part_budget() {
        let name = "a".repeat(24);
        assert_eq!(
            validate_username(
                &name,
                DomainClass::Tenant,
                USERNAME_MAX,
                &".".repeat(16),
                &[]
            ),
            Ok(name.clone())
        );
        assert_eq!(
            validate_username(
                &name,
                DomainClass::Tenant,
                USERNAME_MAX,
                &".".repeat(17),
                &[]
            ),
            Err(AddressError::LocalPartTooLong)
        );
    }

    #[test]
    fn org_domains() {
        assert_eq!(org_domain("mail.brightwell.example.co.uk"), "example.co.uk");
        assert_eq!(org_domain("a.b.example.com"), "example.com");
        assert_eq!(org_domain("example.com"), "example.com");
        assert_eq!(domain_of("ana@mail.example.com"), "mail.example.com");
    }

    #[test]
    fn unicode_table_is_current() {
        // The table is generated; spot checks against confusables.txt 18.0.0.
        assert_eq!(lookup('m', CONFUSABLES), Some("rn"));
        assert_eq!(lookup('\u{430}', CONFUSABLES), Some("a"));
        assert_eq!(lookup('ß', CASE_FOLDING), Some("ss"));
        assert!(in_ranges('\u{200b}', DEFAULT_IGNORABLE));
        assert!(in_ranges('\u{5d0}', STRONG_RTL));
        assert!(confusables_table::SCRIPT_NAMES.contains(&"Latn"));
    }
}
