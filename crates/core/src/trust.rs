//! Trust signals of an inbound message ([Inbound › Trust signals], FR-IN-4): `known_sender`,
//! `shared_domain_sender` (D12), `display_name_spoof` and `lookalike_domain` (D2), and
//! `reply_to_mismatch` (D3), with `arc_rescued` treated as in [Parity additions § 6.5] (Z42).
//!
//! Contacts and tenant facts are inputs; `ingest` reads them from the mailbox and D1.
//!
//! [Inbound › Trust signals]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
//! [Parity additions § 6.5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/parity-additions.md

use serde::Serialize;

use crate::address::{domain_of, fold, org_domain};
use crate::auth::Verdict;
use crate::mime::Mailbox;
use crate::reply::reply_to_mismatch;

/// A row of the mailbox's `contacts`.
#[derive(Clone, Copy, Debug)]
pub struct Contact<'a> {
    /// Normalised address.
    pub address: &'a str,
    /// Display name; empty when unknown.
    pub name: &'a str,
    pub outbound_count: u64,
}

/// A row of `SELECT name, kind FROM domains WHERE (tenant_id = ?1 OR kind = 'platform') …`.
#[derive(Clone, Copy, Debug)]
pub struct TenantDomain<'a> {
    pub name: &'a str,
    /// `kind = 'platform'`: feeds the look-alike checks, never `known_sender`.
    pub platform: bool,
}

pub struct TrustInput<'a> {
    /// The single normalised `From` mailbox.
    pub from: &'a Mailbox,
    /// Normalised `Reply-To` addresses.
    pub reply_to: &'a [String],
    pub verdict: Verdict,
    /// `auth_json.dmarc.aligned_by` is `dkim` or `spf`.
    pub aligned: bool,
    /// The effective receive decision for the sender is `allow`.
    pub receive_allow: bool,
    /// `PM_PLATFORM_DOMAIN`.
    pub platform_domain: &'a str,
    /// The sender is an address of the receiving identity's own tenant.
    pub sender_in_tenant: bool,
    /// The sender is an address of the receiving identity.
    pub sender_is_identity: bool,
    pub tenant_domains: &'a [TenantDomain<'a>],
    /// `tenants.address_suffix`, for example `.acme`; empty for the default tenant.
    pub tenant_suffix: &'a str,
    pub tenant_name: &'a str,
    pub identity_name: &'a str,
    pub contacts: &'a [Contact<'a>],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TrustSignals {
    pub known_sender: bool,
    pub shared_domain_sender: bool,
    pub display_name_spoof: bool,
    pub lookalike_domain: bool,
    pub reply_to_mismatch: bool,
}

/// C10: `pass`, `arc_rescued`, or an aligned DKIM or SPF pass. Rules that trust an address the identity
/// knows apply only to an authenticated sender.
pub fn authenticated(verdict: Verdict, aligned: bool) -> bool {
    matches!(verdict, Verdict::Pass | Verdict::ArcRescued) || aligned
}

pub fn signals(i: &TrustInput<'_>) -> TrustSignals {
    TrustSignals {
        known_sender: known_sender(i),
        shared_domain_sender: on_platform(i) && !i.sender_in_tenant,
        display_name_spoof: display_name_spoof(i),
        lookalike_domain: lookalike_domain(i),
        reply_to_mismatch: reply_to_mismatch(&i.from.address, i.reply_to),
    }
}

fn on_platform(i: &TrustInput<'_>) -> bool {
    domain_of(&i.from.address).eq_ignore_ascii_case(i.platform_domain.trim_end_matches('.'))
}

fn written_to<'a>(contacts: &'a [Contact<'a>]) -> impl Iterator<Item = &'a Contact<'a>> {
    contacts.iter().filter(|c| c.outbound_count > 0)
}

/// FR-IN-4: authenticated, and written to, or receive-allowed, or (verdict `pass` only) on one of the
/// tenant's own domains. The platform domain never gives domain-level trust (D12).
pub fn known_sender(i: &TrustInput<'_>) -> bool {
    if !authenticated(i.verdict, i.aligned) {
        return false;
    }
    let from = i.from.address.as_str();
    let domain = domain_of(from);
    let own_domain = i.verdict == Verdict::Pass
        && !on_platform(i)
        && i.tenant_domains
            .iter()
            .any(|d| !d.platform && d.name.eq_ignore_ascii_case(domain));
    i.receive_allow || own_domain || written_to(i.contacts).any(|c| c.address == from)
}

/// The fold of a display name, with whitespace runs collapsed.
fn name_fold(s: &str) -> String {
    fold(&s.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn looks_like_address(t: &str) -> bool {
    t.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty() && domain.contains('.') && domain.split('.').all(|l| !l.is_empty())
    })
}

/// D2: an address-like token other than `From`; the fold of a written-to contact's name with another
/// address; or the identity's display name or the tenant name while the sender is not this identity.
pub fn display_name_spoof(i: &TrustInput<'_>) -> bool {
    let name = i.from.name.trim();
    if name.is_empty() {
        return false;
    }
    let from = i.from.address.as_str();
    let token = name
        .split(|c: char| c.is_whitespace() || "<>()[]\"',;:".contains(c))
        .any(|t| looks_like_address(t) && !t.trim_end_matches('.').eq_ignore_ascii_case(from));
    if token {
        return true;
    }
    let f = name_fold(name);
    let contact = written_to(i.contacts)
        .any(|c| !c.name.trim().is_empty() && name_fold(c.name) == f && c.address != from);
    let own = !i.sender_is_identity
        && [i.identity_name, i.tenant_name]
            .iter()
            .any(|n| !n.trim().is_empty() && name_fold(n) == f);
    contact || own
}

/// The fold of a domain in its Unicode form, so an IDNA A-label compares by what it shows.
fn domain_fold(d: &str) -> String {
    fold(&idna::domain_to_unicode(d).0)
}

/// The label left of the public suffix: `brightwell` for `agents.brightwell.example`.
fn registrable_label(d: &str) -> &str {
    org_domain(d).split('.').next().unwrap_or("")
}

/// D2: the fold of the sender's registrable domain equals the fold of a tenant domain (or its
/// registrable domain) or a written-to contact's domain, while the domains differ. On the platform
/// domain, the local part's tenant suffix is also compared with the receiving tenant's suffix and the
/// registrable labels of its own domains (D12).
pub fn lookalike_domain(i: &TrustInput<'_>) -> bool {
    let sender = domain_of(&i.from.address).to_ascii_lowercase();
    let reg = org_domain(&sender);
    let reg_fold = domain_fold(reg);
    let candidates = i
        .tenant_domains
        .iter()
        .map(|d| d.name)
        .chain(written_to(i.contacts).map(|c| domain_of(c.address)));
    for cand in candidates {
        let cand = cand.trim_end_matches('.').to_ascii_lowercase();
        for x in [cand.as_str(), org_domain(&cand)] {
            if x != reg && domain_fold(x) == reg_fold {
                return true;
            }
        }
    }
    if !on_platform(i) || i.sender_in_tenant {
        return false;
    }
    let local = i.from.address.rsplit_once('@').map_or("", |(l, _)| l);
    let Some((_, suffix)) = local.rsplit_once('.') else {
        return false;
    };
    let own = i.tenant_suffix.trim_start_matches('.');
    if suffix.is_empty() || suffix == own {
        return false;
    }
    let f = fold(suffix);
    (!own.is_empty() && fold(own) == f)
        || i.tenant_domains
            .iter()
            .filter(|d| !d.platform)
            .any(|d| fold(registrable_label(&d.name.to_ascii_lowercase())) == f)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLATFORM: &str = "agents.example";

    fn mailbox(name: &str, address: &str) -> Mailbox {
        Mailbox {
            address: address.into(),
            name: name.into(),
        }
    }

    struct Facts<'a> {
        domains: Vec<TenantDomain<'a>>,
        contacts: Vec<Contact<'a>>,
    }

    fn facts() -> Facts<'static> {
        Facts {
            domains: vec![
                TenantDomain {
                    name: "brightwell.example",
                    platform: false,
                },
                TenantDomain {
                    name: PLATFORM,
                    platform: true,
                },
            ],
            contacts: vec![
                Contact {
                    address: "ana@amerigo.example",
                    name: "Ana Pereira",
                    outbound_count: 2,
                },
                Contact {
                    address: "old@modern.example",
                    name: "Quiet Person",
                    outbound_count: 0,
                },
            ],
        }
    }

    fn input<'a>(from: &'a Mailbox, f: &'a Facts<'a>) -> TrustInput<'a> {
        TrustInput {
            from,
            reply_to: &[],
            verdict: Verdict::Pass,
            aligned: true,
            receive_allow: false,
            platform_domain: PLATFORM,
            sender_in_tenant: false,
            sender_is_identity: false,
            tenant_domains: &f.domains,
            tenant_suffix: ".acme",
            tenant_name: "Acme Car Hire",
            identity_name: "Bookings Desk",
            contacts: &f.contacts,
        }
    }

    /// Covers: D2
    #[test]
    fn d2_display_name_spoof() {
        let f = facts();
        let spoof = |name: &str, address: &str| {
            let m = mailbox(name, address);
            display_name_spoof(&input(&m, &f))
        };
        // An address-like token that is not the From address.
        assert!(spoof("ana@amerigo.example", "pay@evil.example"));
        assert!(spoof("Ana (ana@amerigo.example)", "pay@evil.example"));
        assert!(!spoof("ana@amerigo.example", "ana@amerigo.example"));
        assert!(!spoof("Ana at amerigo", "pay@evil.example"));
        // A written-to contact's name with another address, also through confusables (Cyrillic А).
        assert!(spoof("Ana Pereira", "ana@evil.example"));
        assert!(spoof("\u{410}na  Pereira", "ana@evil.example"));
        assert!(spoof("ana pereira", "ana@evil.example"));
        assert!(!spoof("Ana Pereira", "ana@amerigo.example"));
        // A contact never written to does not count.
        assert!(!spoof("Quiet Person", "q@evil.example"));
        // The identity's display name or the tenant name, unless the sender is this identity.
        assert!(spoof("Bookings Desk", "x@evil.example"));
        assert!(spoof("Acme Car HIre", "x@evil.example"));
        let m = mailbox("Bookings Desk", "bookings.acme@agents.example");
        let mut i = input(&m, &f);
        i.sender_is_identity = true;
        assert!(!display_name_spoof(&i));
        // No display name, or an unrelated one.
        assert!(!spoof("", "x@evil.example"));
        assert!(!spoof("Bob", "x@evil.example"));
    }

    /// Covers: D2
    #[test]
    fn d2_lookalike_domain() {
        let f = facts();
        let look = |address: &str| {
            let m = mailbox("", address);
            lookalike_domain(&input(&m, &f))
        };
        // The fold of the registrable domain equals a tenant domain's while the domains differ.
        assert!(look("x@brightwe11.example"));
        assert!(look("x@mail.brightwe11.example"));
        let cyrillic_e = idna::domain_to_ascii("brightw\u{435}ll.example").unwrap();
        assert!(cyrillic_e.starts_with("xn--"));
        assert!(look(&format!("x@{cyrillic_e}")));
        // The tenant's own domain and its subdomains are not look-alikes.
        assert!(!look("x@brightwell.example"));
        assert!(!look("x@mail.brightwell.example"));
        // A written-to contact's domain (`rn` folds to `m`); one never written to does not count.
        assert!(look("x@arnerigo.example"));
        assert!(!look("x@amerigo.example"));
        assert!(!look("x@rnodern.example"));
        // Unrelated.
        assert!(!look("x@elsewhere.example"));
    }

    /// Covers: D2
    #[test]
    fn d2_platform_suffix() {
        let f = facts();
        let look = |address: &str, in_tenant: bool| {
            let m = mailbox("", address);
            let mut i = input(&m, &f);
            i.sender_in_tenant = in_tenant;
            lookalike_domain(&i)
        };
        // A suffix that folds to the receiving tenant's suffix, or to its own domain's label.
        assert!(look("bookings.acrne@agents.example", false));
        assert!(look("bookings.brightwell@agents.example", false));
        assert!(look("bookings.brightwe11@agents.example", false));
        // The tenant's own sender, another distinct workspace, and the default tenant (no suffix).
        assert!(!look("bookings.acme@agents.example", true));
        assert!(!look("bookings.globex@agents.example", false));
        assert!(!look("bookings@agents.example", false));
        // The same suffix on another domain is not this rule.
        assert!(!look("bookings.acrne@other.example", false));
    }

    /// Covers: D12
    #[test]
    fn d12_platform_domain_not_known() {
        let f = facts();
        // A `pass` sender of another workspace on the platform domain: not known, shared domain.
        let m = mailbox("Globex", "sales.globex@agents.example");
        let s = signals(&input(&m, &f));
        assert!(!s.known_sender);
        assert!(s.shared_domain_sender);
        assert!(!s.lookalike_domain);
        // The same sender once the identity has written to it is known.
        let written = Facts {
            domains: f.domains.clone(),
            contacts: vec![Contact {
                address: "sales.globex@agents.example",
                name: "Globex",
                outbound_count: 1,
            }],
        };
        let s = signals(&input(&m, &written));
        assert!(s.known_sender && s.shared_domain_sender);
        // A sender of this tenant on the platform domain does not carry shared_domain_sender.
        let own = mailbox("", "ops.acme@agents.example");
        let mut i = input(&own, &f);
        i.sender_in_tenant = true;
        assert!(!signals(&i).shared_domain_sender);
        // A sender on the tenant's own domain with `pass` is known.
        let m = mailbox("Ana", "ana@brightwell.example");
        let s = signals(&input(&m, &f));
        assert!(s.known_sender && !s.shared_domain_sender);
        // A platform-domain local part whose suffix folds to the tenant's suffix: lookalike_domain.
        let m = mailbox("Acme", "billing.acrne@agents.example");
        let s = signals(&input(&m, &f));
        assert!(s.lookalike_domain && s.shared_domain_sender && !s.known_sender);
    }

    /// Covers: Z42
    #[test]
    fn z42_rescued_known_sender() {
        let f = facts();
        let known = |address: &str, verdict, aligned, allow| {
            let m = mailbox("", address);
            let mut i = input(&m, &f);
            i.verdict = verdict;
            i.aligned = aligned;
            i.receive_allow = allow;
            known_sender(&i)
        };
        // The own-domain rule needs `pass`; `arc_rescued` is not enough.
        assert!(known("ana@brightwell.example", Verdict::Pass, true, false));
        assert!(!known(
            "ana@brightwell.example",
            Verdict::ArcRescued,
            false,
            false
        ));
        // The contacts and receive-allow rules apply to a rescued sender as usual.
        assert!(known(
            "ana@amerigo.example",
            Verdict::ArcRescued,
            false,
            false
        ));
        assert!(known(
            "x@elsewhere.example",
            Verdict::ArcRescued,
            false,
            true
        ));
        // Unauthenticated senders are never known; an aligned pass without DMARC is authenticated.
        assert!(!known("ana@amerigo.example", Verdict::None, false, false));
        assert!(!known("x@elsewhere.example", Verdict::Fail, false, true));
        assert!(known("ana@amerigo.example", Verdict::None, true, false));
        assert!(!known("ana@brightwell.example", Verdict::None, true, false));
        assert!(authenticated(Verdict::ArcRescued, false));
        assert!(!authenticated(Verdict::Softfail, false));
    }

    #[test]
    fn reply_to_mismatch_signal() {
        let f = facts();
        let m = mailbox("", "ana@amerigo.example");
        let rt = vec!["pay@evil.example".to_owned()];
        let mut i = input(&m, &f);
        i.reply_to = &rt;
        assert!(signals(&i).reply_to_mismatch);
        let same = vec!["desk@help.amerigo.example".to_owned()];
        i.reply_to = &same;
        assert!(!signals(&i).reply_to_mismatch);
    }
}
