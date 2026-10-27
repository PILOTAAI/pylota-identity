//! DNS record parsing and the domain preflights ([Identities and domains › SPF lookup count],
//! [› Alignment], [Domains on any DNS host § 4.1 and § 6]): SPF parsing, merging and RFC 7208 lookup
//! counting (H2), DMARC alignment against the transport's signing domains (H3), DKIM key records, the
//! expected records with their relative `host`, and the doubled-name check (N17).
//!
//! Every answer is an input: the caller fetches the names over DoH and passes them in through
//! [`Resolve`]. Nothing here does I/O.
//!
//! [Identities and domains › SPF lookup count]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md
//! [› Alignment]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md
//! [Domains on any DNS host § 4.1 and § 6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/domain-connections.md

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::address::org_domain;
pub use crate::auth::{DmarcRecord, Policy, parse_dmarc};

// ---- Pre-fetched answers ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RrType {
    A,
    Aaaa,
    Cname,
    Mx,
    Ns,
    Txt,
}

/// The answer to one query, as the caller fetched it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The record data, one string per record (TXT strings joined; MX as `priority host`).
    Records(Vec<String>),
    /// NXDOMAIN, or an answer with no records of the type (both are void lookups, RFC 7208 § 4.6.4).
    Empty,
    /// Timeout, `SERVFAIL` or another failure.
    Error,
}

/// Pre-fetched answers. `None` means the name was not fetched.
pub trait Resolve {
    fn lookup(&self, name: &str, rtype: RrType) -> Option<&Answer>;
}

/// Answers keyed by lower-case name without the trailing dot.
pub type Answers = HashMap<(String, RrType), Answer>;

impl Resolve for Answers {
    fn lookup(&self, name: &str, rtype: RrType) -> Option<&Answer> {
        self.get(&(norm(name), rtype))
    }
}

fn norm(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

// ---- SPF records --------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mechanism {
    All,
    /// The target domain-spec.
    Include(String),
    /// The domain-spec, `None` for the current domain.
    A(Option<String>),
    Mx(Option<String>),
    Ptr(Option<String>),
    Ip4,
    Ip6,
    Exists(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermKind {
    Directive(Mechanism),
    Redirect(String),
    Exp(String),
    /// Unknown modifiers are ignored (RFC 7208 § 6).
    OtherModifier,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    /// The term as written, so a merged record keeps the customer's spelling.
    pub raw: String,
    pub kind: TermKind,
}

/// A parsed `v=spf1` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpfRecord {
    pub terms: Vec<Term>,
}

impl fmt::Display for SpfRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("v=spf1")?;
        for t in &self.terms {
            write!(f, " {}", t.raw)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpfError {
    /// The text is not a `v=spf1` record.
    NotSpf,
    /// A term that does not parse (permerror).
    Syntax(String),
    /// More than 10 DNS-querying terms (permerror).
    TooManyLookups,
    /// More than 2 void lookups (permerror).
    TooManyVoidLookups,
    /// An `include` or `redirect` target without an SPF record (permerror).
    NoRecord(String),
    /// More than one SPF record at a target (permerror).
    MultipleRecords(String),
    /// A target that includes itself (permerror).
    Loop(String),
    /// Nesting deeper than 10 (permerror).
    TooDeep,
    /// A target's TXT query failed (temperror).
    TempError(String),
}

impl SpfError {
    pub fn is_permerror(&self) -> bool {
        !matches!(self, SpfError::TempError(_))
    }
}

/// Whether a TXT string is an SPF record (`v=spf1` then a space or the end, RFC 7208 § 4.5).
pub fn is_spf(txt: &str) -> bool {
    let t = txt.trim_start();
    t.len() >= 6
        && t.as_bytes()[..6].eq_ignore_ascii_case(b"v=spf1")
        && t[6..].chars().next().is_none_or(|c| c == ' ')
}

fn split_spec(rest: &str) -> Option<String> {
    // `:domain-spec` with an optional `/cidr`; a bare `/cidr` means the current domain.
    let spec = rest.strip_prefix(':')?;
    let spec = spec.split('/').next().unwrap_or(spec);
    (!spec.is_empty()).then(|| spec.trim_end_matches('.').to_ascii_lowercase())
}

fn parse_term(raw: &str) -> Result<TermKind, SpfError> {
    let bad = || SpfError::Syntax(raw.to_owned());
    // A modifier is `name=value` whose name comes before any `:` or `/`.
    if let Some((name, value)) = raw.split_once('=')
        && !name.contains([':', '/'])
    {
        if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return Err(bad());
        }
        let value = value.trim_end_matches('.').to_ascii_lowercase();
        return Ok(match name.to_ascii_lowercase().as_str() {
            "redirect" if !value.is_empty() => TermKind::Redirect(value),
            "exp" if !value.is_empty() => TermKind::Exp(value),
            "redirect" | "exp" => return Err(bad()),
            _ => TermKind::OtherModifier,
        });
    }
    let body = raw.strip_prefix(['+', '-', '~', '?']).unwrap_or(raw);
    let end = body.find([':', '/']).unwrap_or(body.len());
    let (name, rest) = body.split_at(end);
    let spec = split_spec(rest);
    let m = match name.to_ascii_lowercase().as_str() {
        "all" if rest.is_empty() => Mechanism::All,
        "include" => Mechanism::Include(spec.ok_or_else(bad)?),
        "exists" => Mechanism::Exists(spec.ok_or_else(bad)?),
        "a" => Mechanism::A(spec),
        "mx" => Mechanism::Mx(spec),
        "ptr" => Mechanism::Ptr(spec),
        "ip4" if rest.len() > 1 => Mechanism::Ip4,
        "ip6" if rest.len() > 1 => Mechanism::Ip6,
        _ => return Err(bad()),
    };
    Ok(TermKind::Directive(m))
}

/// Parses one TXT string as an SPF record. At most one `redirect` and one `exp` (RFC 7208 § 6).
pub fn parse_spf(txt: &str) -> Result<SpfRecord, SpfError> {
    if !is_spf(txt) {
        return Err(SpfError::NotSpf);
    }
    let mut terms = Vec::new();
    let (mut redirects, mut exps) = (0, 0);
    for raw in txt.trim_start()[6..].split_ascii_whitespace() {
        let kind = parse_term(raw)?;
        match kind {
            TermKind::Redirect(_) => redirects += 1,
            TermKind::Exp(_) => exps += 1,
            _ => {}
        }
        if redirects > 1 || exps > 1 {
            return Err(SpfError::Syntax(raw.to_owned()));
        }
        terms.push(Term {
            raw: raw.to_owned(),
            kind,
        });
    }
    Ok(SpfRecord { terms })
}

/// `record` with `include:{include}` added before its `all` (or its first modifier, or at the end),
/// unless it already includes that target.
pub fn merge_include(record: &SpfRecord, include: &str) -> SpfRecord {
    let include = norm(include);
    let has = record
        .terms
        .iter()
        .any(|t| matches!(&t.kind, TermKind::Directive(Mechanism::Include(d)) if *d == include));
    let mut out = record.clone();
    if !has {
        let at = record
            .terms
            .iter()
            .position(|t| !matches!(t.kind, TermKind::Directive(ref m) if *m != Mechanism::All))
            .unwrap_or(record.terms.len());
        out.terms.insert(
            at,
            Term {
                raw: format!("include:{include}"),
                kind: TermKind::Directive(Mechanism::Include(include)),
            },
        );
    }
    out
}

// ---- Lookup counting -----------------------------------------------------------------------------------

/// The result of [`count_spf_lookups`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpfCount {
    /// DNS-querying terms: `include`, `a`, `mx`, `ptr`, `exists` and `redirect`, recursively.
    pub lookups: u32,
    /// Lookups whose answer was [`Answer::Empty`].
    pub void_lookups: u32,
    /// The first permerror or temperror met; the limits are checked after the walk.
    pub error: Option<SpfError>,
    /// Names the walk needed that the answers do not hold; fetch them and count again.
    pub missing: Vec<(String, RrType)>,
}

impl SpfCount {
    /// Within RFC 7208's limits, without errors, and with every answer present.
    pub fn ok(&self) -> bool {
        self.error.is_none() && self.missing.is_empty()
    }
}

/// The SPF limits (RFC 7208 § 4.6.4).
pub const MAX_LOOKUPS: u32 = 10;
pub const MAX_VOID_LOOKUPS: u32 = 2;
const MAX_DEPTH: usize = 10;
/// The walk stops counting here, so a hostile tree of repeated includes stays cheap.
const WALK_CAP: u32 = 100;

/// Counts the lookups of `record`, published at `domain`, through every `include` and `redirect`
/// target in `resolve` (RFC 7208 § 4.6.4; depth ≤ 10). Every term is counted, not only those an
/// evaluation would reach, so the count is the worst case a receiver can see. A domain-spec with a
/// macro (`%{…}`) counts one lookup and is not followed.
pub fn count_spf_lookups(record: &SpfRecord, domain: &str, resolve: &impl Resolve) -> SpfCount {
    let mut c = SpfCount::default();
    let mut chain = vec![norm(domain)];
    walk(record, &norm(domain), resolve, &mut chain, &mut c);
    if c.error.is_none() {
        if c.lookups > MAX_LOOKUPS {
            c.error = Some(SpfError::TooManyLookups);
        } else if c.void_lookups > MAX_VOID_LOOKUPS {
            c.error = Some(SpfError::TooManyVoidLookups);
        }
    }
    c
}

fn fail(c: &mut SpfCount, e: SpfError) {
    if c.error.is_none() {
        c.error = Some(e);
    }
}

fn missing(c: &mut SpfCount, name: &str, rtype: RrType) {
    let k = (name.to_owned(), rtype);
    if !c.missing.contains(&k) {
        c.missing.push(k);
    }
}

fn walk(
    record: &SpfRecord,
    domain: &str,
    resolve: &impl Resolve,
    chain: &mut Vec<String>,
    c: &mut SpfCount,
) {
    let has_all = record
        .terms
        .iter()
        .any(|t| t.kind == TermKind::Directive(Mechanism::All));
    for t in &record.terms {
        if c.lookups >= WALK_CAP {
            return;
        }
        match &t.kind {
            TermKind::Directive(Mechanism::Include(target)) => {
                c.lookups += 1;
                follow(target, resolve, chain, c);
            }
            // `redirect` is ignored when the record has `all` (RFC 7208 § 6.1).
            TermKind::Redirect(target) if !has_all => {
                c.lookups += 1;
                follow(target, resolve, chain, c);
            }
            TermKind::Directive(Mechanism::A(d)) => {
                c.lookups += 1;
                probe(d.as_deref().unwrap_or(domain), RrType::A, resolve, c);
            }
            TermKind::Directive(Mechanism::Mx(d)) => {
                c.lookups += 1;
                probe(d.as_deref().unwrap_or(domain), RrType::Mx, resolve, c);
            }
            TermKind::Directive(Mechanism::Exists(d)) => {
                c.lookups += 1;
                probe(d, RrType::A, resolve, c);
            }
            // The PTR answer depends on the client IP, so it is never void here.
            TermKind::Directive(Mechanism::Ptr(_)) => c.lookups += 1,
            _ => {}
        }
    }
}

/// A void-lookup probe of a querying mechanism's name.
fn probe(name: &str, rtype: RrType, resolve: &impl Resolve, c: &mut SpfCount) {
    if name.contains('%') {
        return;
    }
    match resolve.lookup(name, rtype) {
        Some(Answer::Empty) => c.void_lookups += 1,
        Some(_) => {}
        None => missing(c, name, rtype),
    }
}

fn follow(target: &str, resolve: &impl Resolve, chain: &mut Vec<String>, c: &mut SpfCount) {
    if target.contains('%') {
        return;
    }
    if chain.iter().any(|d| d == target) {
        return fail(c, SpfError::Loop(target.to_owned()));
    }
    if chain.len() > MAX_DEPTH {
        return fail(c, SpfError::TooDeep);
    }
    let records = match resolve.lookup(target, RrType::Txt) {
        None => return missing(c, target, RrType::Txt),
        Some(Answer::Error) => return fail(c, SpfError::TempError(target.to_owned())),
        Some(Answer::Empty) => {
            c.void_lookups += 1;
            return fail(c, SpfError::NoRecord(target.to_owned()));
        }
        Some(Answer::Records(r)) => r,
    };
    let mut spf = records.iter().filter(|r| is_spf(r));
    let (Some(one), None) = (spf.next(), spf.next()) else {
        let e = if records.iter().any(|r| is_spf(r)) {
            SpfError::MultipleRecords(target.to_owned())
        } else {
            SpfError::NoRecord(target.to_owned())
        };
        return fail(c, e);
    };
    match parse_spf(one) {
        Ok(rec) => {
            chain.push(target.to_owned());
            walk(&rec, target, resolve, chain, c);
            chain.pop();
        }
        Err(e) => fail(c, e),
    }
}

// ---- SPF preflight (H2) ----------------------------------------------------------------------------------

/// Why [`preflight_spf`] did not return a record to publish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpfPreflight {
    /// `400 spf_lookup_limit`: more than 10 lookups or more than 2 void lookups after the merge.
    LookupLimit {
        lookups: u32,
        void_lookups: u32,
        /// The record's top-level `include` targets, for the fix (the ones to flatten).
        flatten: Vec<String>,
    },
    /// The merged record has another permerror (the existing record is broken).
    Invalid(SpfError),
    /// Answers are missing or a query failed: fetch `missing` (or retry) and run again.
    Undecided { missing: Vec<(String, RrType)> },
}

impl SpfPreflight {
    pub const LOOKUP_LIMIT: &'static str = "spf_lookup_limit";
}

/// The SPF preflight of a domain add ([H2]): the `existing` record at `domain` merged with
/// `include:{include}` (the Email Routing include at the apex, `amazonses.com` at `pm-bounce.{domain}`),
/// counted through `resolve`. `Ok` is the record to publish.
///
/// [H2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/edge-cases.md
pub fn preflight_spf(
    domain: &str,
    existing: &str,
    include: &str,
    resolve: &impl Resolve,
) -> Result<SpfRecord, SpfPreflight> {
    let rec = parse_spf(existing).map_err(SpfPreflight::Invalid)?;
    let merged = merge_include(&rec, include);
    let c = count_spf_lookups(&merged, domain, resolve);
    match c.error {
        Some(SpfError::TooManyLookups | SpfError::TooManyVoidLookups) => {
            Err(SpfPreflight::LookupLimit {
                lookups: c.lookups,
                void_lookups: c.void_lookups,
                flatten: merged
                    .terms
                    .iter()
                    .filter_map(|t| match &t.kind {
                        TermKind::Directive(Mechanism::Include(d)) if *d != norm(include) => {
                            Some(d.clone())
                        }
                        _ => None,
                    })
                    .collect(),
            })
        }
        Some(SpfError::TempError(_)) => Err(SpfPreflight::Undecided { missing: c.missing }),
        Some(e) => Err(SpfPreflight::Invalid(e)),
        None if !c.missing.is_empty() => Err(SpfPreflight::Undecided { missing: c.missing }),
        None => Ok(merged),
    }
}

// ---- DMARC alignment (H3) --------------------------------------------------------------------------------

/// The transports whose signing domains are known from DNS alone (`smtp` is proved by the probe).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Cloudflare,
    Ses,
}

/// The domains a transport authenticates with when sending as `domain`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningDomains {
    /// DKIM `d=`.
    pub dkim: String,
    /// The return path (envelope MAIL FROM) domain SPF is checked on.
    pub return_path: String,
}

/// Cloudflare signs with `d=` the sending domain and uses `cf-bounce.{domain}`; SES Easy DKIM signs with
/// `d=` the domain and uses the custom MAIL FROM, or its own `amazonses.com` domain without one.
pub fn signing_domains(
    transport: Transport,
    domain: &str,
    mail_from: Option<&str>,
) -> SigningDomains {
    let domain = norm(domain);
    let return_path = match transport {
        Transport::Cloudflare => format!("cf-bounce.{domain}"),
        Transport::Ses => mail_from.map_or_else(|| "amazonses.com".to_owned(), norm),
    };
    SigningDomains {
        dkim: domain,
        return_path,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alignment {
    pub dkim: bool,
    pub spf: bool,
    /// Health issue `dmarc_alignment_impossible` (fail): neither aligns while the policy enforces.
    pub impossible: bool,
}

impl Alignment {
    pub const IMPOSSIBLE: &'static str = "dmarc_alignment_impossible";
}

fn aligns(a: &str, domain: &str, strict: bool) -> bool {
    if strict {
        a == domain
    } else {
        org_domain(a) == org_domain(domain)
    }
}

/// Whether mail from `domain` signed as `signing` can align under `dmarc`, found at `record_domain`
/// (`_dmarc.{domain}` or the organisational domain, whose `sp` then applies to a subdomain).
pub fn check_alignment(
    domain: &str,
    dmarc: &DmarcRecord,
    record_domain: &str,
    signing: &SigningDomains,
) -> Alignment {
    let domain = norm(domain);
    let dkim = aligns(&norm(&signing.dkim), &domain, dmarc.strict_dkim);
    let spf = aligns(&norm(&signing.return_path), &domain, dmarc.strict_spf);
    let policy = match dmarc.sp {
        Some(sp) if norm(record_domain) != domain => sp,
        _ => dmarc.p,
    };
    Alignment {
        dkim,
        spf,
        impossible: !dkim && !spf && policy != Policy::None,
    }
}

// ---- DKIM key records ------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DkimKey {
    /// `k=`, default `rsa`.
    pub k: String,
    /// `p=`, whitespace removed; empty for a revoked key.
    pub p: String,
}

/// Parses a DKIM key TXT record (RFC 6376 § 3.6.1): `v=DKIM1` first when present, `p=` required.
pub fn parse_dkim_key(txt: &str) -> Option<DkimKey> {
    let mut k = "rsa".to_owned();
    let mut p = None;
    for (i, tag) in txt.split(';').enumerate() {
        let Some((name, value)) = tag.split_once('=') else {
            if tag.trim().is_empty() {
                continue;
            }
            return None;
        };
        let value: String = value.chars().filter(|c| !c.is_whitespace()).collect();
        match name.trim() {
            "v" if i != 0 || value != "DKIM1" => return None,
            "k" => k = value.to_ascii_lowercase(),
            "p" => p = Some(value),
            _ => {}
        }
    }
    Some(DkimKey { k, p: p? })
}

// ---- Expected records and doubled names (N17) ------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Mx,
    Spf,
    Dkim,
    ReturnPath,
    Dmarc,
    Ownership,
    Ns,
}

/// One record of `records_json`, as the provider APIs gave it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedRecord {
    #[serde(rename = "type")]
    pub rtype: RrType,
    /// Fully qualified, without the trailing dot.
    pub name: String,
    pub value: String,
    pub priority: Option<u16>,
    pub purpose: Purpose,
    pub required: bool,
}

/// `name` relative to its registrable domain (Public Suffix List), `@` at the registrable domain
/// itself; DNS hosts differ in which form they want ([N17]).
///
/// [N17]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/edge-cases.md
pub fn host(name: &str) -> String {
    let name = norm(name);
    let org = org_domain(&name);
    match name.strip_suffix(org).and_then(|h| h.strip_suffix('.')) {
        Some(h) if !h.is_empty() => h.to_owned(),
        _ => "@".to_owned(),
    }
}

/// Where the record lands when a DNS host appends the zone to a fully qualified name:
/// `{name}.{registrable domain}`.
pub fn doubled_name(name: &str) -> String {
    let name = norm(name);
    format!("{name}.{}", org_domain(&name))
}

fn unquote(s: &str) -> String {
    // TXT data as DoH JSON shows it: quoted strings, possibly several.
    let s = s.trim();
    if s.starts_with('"') {
        s.split('"').skip(1).step_by(2).collect()
    } else {
        s.to_owned()
    }
}

/// Whether observed record data equals the expected value: TXT exactly after unquoting; host names
/// without case or the trailing dot, an MX answer's leading priority ignored.
pub fn value_matches(rtype: RrType, expected: &str, observed: &str) -> bool {
    match rtype {
        RrType::Txt => unquote(observed) == unquote(expected),
        RrType::Mx => {
            let o = observed.trim();
            let host = o.split_once(' ').map_or(o, |(p, h)| {
                if p.bytes().all(|b| b.is_ascii_digit()) {
                    h
                } else {
                    o
                }
            });
            norm(host) == norm(expected)
        }
        _ => norm(observed) == norm(expected),
    }
}

/// Health issue `record_doubled_name` (degraded).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DoubledName {
    /// The expected record's name.
    pub name: String,
    /// Where the record was found.
    pub found_at: String,
    /// The value to enter at the DNS host instead.
    pub host: String,
}

impl DoubledName {
    pub const CODE: &'static str = "record_doubled_name";
}

/// The queries [`doubled_names`] needs.
pub fn doubled_queries(records: &[ExpectedRecord]) -> Vec<(String, RrType)> {
    let mut q: Vec<(String, RrType)> = Vec::new();
    for r in records {
        let k = (doubled_name(&r.name), r.rtype);
        if !q.contains(&k) {
            q.push(k);
        }
    }
    q
}

/// The doubled-name check of `external` domains ([Domains on any DNS host § 6]): a record at
/// `{name}.{registrable domain}` whose data matches the expected value.
///
/// [Domains on any DNS host § 6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/domain-connections.md
pub fn doubled_names(records: &[ExpectedRecord], resolve: &impl Resolve) -> Vec<DoubledName> {
    records
        .iter()
        .filter_map(|r| {
            let at = doubled_name(&r.name);
            let Some(Answer::Records(found)) = resolve.lookup(&at, r.rtype) else {
                return None;
            };
            found
                .iter()
                .any(|o| value_matches(r.rtype, &r.value, o))
                .then(|| DoubledName {
                    name: norm(&r.name),
                    found_at: at,
                    host: host(&r.name),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(list: &[(&str, RrType, Answer)]) -> Answers {
        list.iter()
            .map(|(n, t, a)| ((norm(n), *t), a.clone()))
            .collect()
    }

    fn txt(s: &str) -> Answer {
        Answer::Records(vec![s.to_owned()])
    }

    #[test]
    fn spf_parse_and_merge() {
        let r =
            parse_spf("v=spf1 a mx:mail.x.example/24 ip4:192.0.2.0/24 include:_spf.x.example ~all")
                .unwrap();
        assert_eq!(r.terms.len(), 5);
        assert_eq!(
            r.terms[1].kind,
            TermKind::Directive(Mechanism::Mx(Some("mail.x.example".into())))
        );
        assert_eq!(parse_spf("v=spf10 a"), Err(SpfError::NotSpf));
        assert_eq!(parse_spf("txt"), Err(SpfError::NotSpf));
        assert!(matches!(
            parse_spf("v=spf1 bogus:x"),
            Err(SpfError::Syntax(_))
        ));
        assert!(matches!(
            parse_spf("v=spf1 redirect=a.example redirect=b.example"),
            Err(SpfError::Syntax(_))
        ));
        assert!(matches!(
            parse_spf("v=spf1 include"),
            Err(SpfError::Syntax(_))
        ));
        // The include goes before `all`; an existing include is not repeated.
        let m = merge_include(&r, "amazonses.com");
        assert_eq!(
            m.to_string(),
            "v=spf1 a mx:mail.x.example/24 ip4:192.0.2.0/24 include:_spf.x.example \
             include:amazonses.com ~all"
        );
        assert_eq!(merge_include(&m, "AMAZONSES.COM."), m);
        // Before the first modifier when there is no `all`.
        let r = parse_spf("v=spf1 ip4:192.0.2.1 redirect=_spf.x.example").unwrap();
        assert_eq!(
            merge_include(&r, "_spf.mx.cloudflare.net").to_string(),
            "v=spf1 ip4:192.0.2.1 include:_spf.mx.cloudflare.net redirect=_spf.x.example"
        );
    }

    /// Covers: H2
    #[test]
    fn h2_spf_lookup_count() {
        let base = [
            (
                "amazonses.com",
                RrType::Txt,
                txt("v=spf1 ip4:199.255.192.0/22 -all"),
            ),
            // Three lookups each: an include, an `a` and an `mx` inside it.
            (
                "a.example",
                RrType::Txt,
                txt("v=spf1 a mx include:deep.example -all"),
            ),
            (
                "deep.example",
                RrType::Txt,
                txt("v=spf1 ip4:192.0.2.0/24 -all"),
            ),
            (
                "a.example",
                RrType::A,
                Answer::Records(vec!["192.0.2.1".into()]),
            ),
            (
                "a.example",
                RrType::Mx,
                Answer::Records(vec!["10 mx.a.example".into()]),
            ),
            // `redirect` is followed and counted.
            ("b.example", RrType::Txt, txt("v=spf1 redirect=c.example")),
            ("c.example", RrType::Txt, txt("v=spf1 mx -all")),
            (
                "c.example",
                RrType::Mx,
                Answer::Records(vec!["10 mx.c.example".into()]),
            ),
            (
                "brightwell.example",
                RrType::A,
                Answer::Records(vec!["192.0.2.9".into()]),
            ),
        ];
        let ans = answers(&base);
        // 4 (a.example) + 3 (b.example, c.example, its mx) + 1 (a) = 8.
        let existing = "v=spf1 include:a.example include:b.example a ~all";
        let r = parse_spf(existing).unwrap();
        let c = count_spf_lookups(&r, "brightwell.example", &ans);
        assert_eq!((c.lookups, c.void_lookups, c.ok()), (8, 0, true));
        // Merged with amazonses.com: 9 lookups, published.
        let merged = preflight_spf("brightwell.example", existing, "amazonses.com", &ans).unwrap();
        assert!(merged.to_string().contains("include:amazonses.com ~all"));
        assert_eq!(
            count_spf_lookups(&merged, "brightwell.example", &ans).lookups,
            9
        );

        // Two more `ptr` reach exactly 10 with the merge: still allowed.
        let ten = "v=spf1 include:a.example include:b.example a ptr ~all";
        assert!(preflight_spf("brightwell.example", ten, "amazonses.com", &ans).is_ok());
        // 11: refused with spf_lookup_limit, the count and the includes to flatten.
        let eleven = "v=spf1 include:a.example include:b.example a ptr ptr ~all";
        assert_eq!(
            preflight_spf("brightwell.example", eleven, "amazonses.com", &ans),
            Err(SpfPreflight::LookupLimit {
                lookups: 11,
                void_lookups: 0,
                flatten: vec!["a.example".into(), "b.example".into()],
            })
        );
        assert_eq!(SpfPreflight::LOOKUP_LIMIT, "spf_lookup_limit");

        // Void lookups: three `a:` names with no records.
        let mut v = base.to_vec();
        for n in ["v1.example", "v2.example", "v3.example"] {
            v.push((n, RrType::A, Answer::Empty));
        }
        let ans_v = answers(&v);
        let two = "v=spf1 a:v1.example a:v2.example ~all";
        assert!(preflight_spf("brightwell.example", two, "amazonses.com", &ans_v).is_ok());
        let three = "v=spf1 a:v1.example a:v2.example a:v3.example ~all";
        assert!(matches!(
            preflight_spf("brightwell.example", three, "amazonses.com", &ans_v),
            Err(SpfPreflight::LookupLimit {
                void_lookups: 3,
                ..
            })
        ));

        // `redirect` is ignored when the record has `all`.
        let r = parse_spf("v=spf1 redirect=b.example -all").unwrap();
        assert_eq!(count_spf_lookups(&r, "x.example", &ans).lookups, 0);

        // Missing answers are listed, and the preflight cannot decide.
        assert_eq!(
            preflight_spf(
                "brightwell.example",
                "v=spf1 include:new.example ~all",
                "amazonses.com",
                &ans
            ),
            Err(SpfPreflight::Undecided {
                missing: vec![("new.example".into(), RrType::Txt)]
            })
        );
        // A failed query is undecided too, never a refusal.
        let mut e = base.to_vec();
        e.push(("err.example", RrType::Txt, Answer::Error));
        assert!(matches!(
            preflight_spf(
                "brightwell.example",
                "v=spf1 include:err.example ~all",
                "amazonses.com",
                &answers(&e)
            ),
            Err(SpfPreflight::Undecided { .. })
        ));

        // Permerrors: a loop, an include without SPF, two SPF records, depth over 10.
        let bad = answers(&[
            (
                "loop.example",
                RrType::Txt,
                txt("v=spf1 include:loop2.example -all"),
            ),
            (
                "loop2.example",
                RrType::Txt,
                txt("v=spf1 include:loop.example -all"),
            ),
            (
                "nospf.example",
                RrType::Txt,
                txt("google-site-verification=x"),
            ),
            (
                "two.example",
                RrType::Txt,
                Answer::Records(vec!["v=spf1 -all".into(), "v=spf1 ~all".into()]),
            ),
            ("gone.example", RrType::Txt, Answer::Empty),
        ]);
        let count = |s: &str| count_spf_lookups(&parse_spf(s).unwrap(), "x.example", &bad);
        assert_eq!(
            count("v=spf1 include:loop.example").error,
            Some(SpfError::Loop("loop.example".into()))
        );
        assert_eq!(
            count("v=spf1 include:nospf.example").error,
            Some(SpfError::NoRecord("nospf.example".into()))
        );
        assert_eq!(
            count("v=spf1 include:two.example").error,
            Some(SpfError::MultipleRecords("two.example".into()))
        );
        let gone = count("v=spf1 include:gone.example");
        assert_eq!(
            (gone.void_lookups, gone.error.unwrap().is_permerror()),
            (1, true)
        );
        let mut chain = Vec::new();
        for i in 0..12 {
            chain.push((
                format!("d{i}.example"),
                RrType::Txt,
                txt(&format!("v=spf1 include:d{}.example", i + 1)),
            ));
        }
        let deep: Answers = chain.into_iter().map(|(n, t, a)| ((n, t), a)).collect();
        let c = count_spf_lookups(
            &parse_spf("v=spf1 include:d0.example").unwrap(),
            "x.example",
            &deep,
        );
        assert_eq!(c.error, Some(SpfError::TooDeep));
    }

    /// Covers: H3
    #[test]
    fn h3_strict_alignment() {
        let dmarc = |s: &str| parse_dmarc(s).unwrap();
        let cf = signing_domains(Transport::Cloudflare, "agents.brightwell.example", None);
        assert_eq!(cf.return_path, "cf-bounce.agents.brightwell.example");
        let ses = signing_domains(
            Transport::Ses,
            "agents.brightwell.example",
            Some("pm-bounce.agents.brightwell.example"),
        );
        let ses_default = signing_domains(Transport::Ses, "agents.brightwell.example", None);
        let d = "agents.brightwell.example";
        // The record found at `_dmarc.{domain}`.
        let at = d;

        // Relaxed: everything aligns.
        let relaxed = dmarc("v=DMARC1; p=reject");
        let a = check_alignment(d, &relaxed, at, &cf);
        assert_eq!((a.dkim, a.spf, a.impossible), (true, true, false));

        // aspf=s: Cloudflare's cf-bounce return path never aligns, its DKIM does.
        let s_spf = dmarc("v=DMARC1; p=reject; aspf=s");
        let a = check_alignment(d, &s_spf, at, &cf);
        assert_eq!((a.dkim, a.spf, a.impossible), (true, false, false));
        // SES with a custom MAIL FROM: relaxed SPF aligns, strict does not; Easy DKIM aligns either way.
        let a = check_alignment(d, &relaxed, at, &ses);
        assert_eq!((a.dkim, a.spf), (true, true));
        let a = check_alignment(d, &s_spf, at, &ses);
        assert_eq!((a.dkim, a.spf, a.impossible), (true, false, false));
        // SES without a MAIL FROM: SPF never aligns.
        assert!(!check_alignment(d, &relaxed, at, &ses_default).spf);

        // adkim=s and aspf=s: d= equals the domain for both transports.
        let strict = dmarc("v=DMARC1; p=quarantine; adkim=s; aspf=s");
        for s in [&cf, &ses] {
            let a = check_alignment(d, &strict, at, s);
            assert_eq!((a.dkim, a.spf, a.impossible), (true, false, false));
        }

        // A signer whose d= is another domain under strict tags cannot align: impossible while the
        // policy enforces, not under p=none.
        let foreign = SigningDomains {
            dkim: "brightwell.example".into(),
            return_path: "bounce.brightwell.example".into(),
        };
        assert!(
            !check_alignment(d, &dmarc("v=DMARC1; p=reject; adkim=r"), at, &foreign).impossible
        );
        let a = check_alignment(d, &strict, at, &foreign);
        assert_eq!((a.dkim, a.spf, a.impossible), (false, false, true));
        assert!(
            !check_alignment(d, &dmarc("v=DMARC1; p=none; adkim=s; aspf=s"), at, &foreign)
                .impossible
        );
        // The organisational record's sp= applies to the subdomain.
        let org = dmarc("v=DMARC1; p=reject; sp=none; adkim=s; aspf=s");
        assert!(!check_alignment(d, &org, "brightwell.example", &foreign).impossible);
        assert!(check_alignment(d, &org, d, &foreign).impossible);
        assert_eq!(Alignment::IMPOSSIBLE, "dmarc_alignment_impossible");
    }

    #[test]
    fn dkim_key_parse() {
        let k = parse_dkim_key("v=DKIM1; k=rsa; p=MIIB IjAN; t=s").unwrap();
        assert_eq!((k.k.as_str(), k.p.as_str()), ("rsa", "MIIBIjAN"));
        assert_eq!(parse_dkim_key("p=").unwrap().p, "");
        assert_eq!(parse_dkim_key("k=ed25519").map(|k| k.p), None);
        assert_eq!(parse_dkim_key("k=rsa; v=DKIM1; p=x"), None);
        assert_eq!(parse_dkim_key("garbage"), None);
    }

    #[test]
    fn doubled_name_detected() {
        let rec = |rtype, name: &str, value: &str, purpose| ExpectedRecord {
            rtype,
            name: name.into(),
            value: value.into(),
            priority: None,
            purpose,
            required: true,
        };
        let records = [
            rec(
                RrType::Txt,
                "_pylota-identity.agents.brightwell.example",
                "pm-verify=abc",
                Purpose::Ownership,
            ),
            rec(
                RrType::Mx,
                "agents.brightwell.example",
                "inbound-smtp.eu-west-2.amazonaws.com",
                Purpose::Mx,
            ),
        ];
        assert_eq!(host("agents.brightwell.example"), "agents");
        assert_eq!(host("brightwell.example."), "@");
        assert_eq!(
            host("_pylota-identity.agents.brightwell.example"),
            "_pylota-identity.agents"
        );
        assert_eq!(
            doubled_name("agents.brightwell.example"),
            "agents.brightwell.example.brightwell.example"
        );
        let q = doubled_queries(&records);
        assert_eq!(
            q[1],
            (
                "agents.brightwell.example.brightwell.example".into(),
                RrType::Mx
            )
        );

        // The MX landed at the doubled name with the expected value: record_doubled_name.
        let ans = answers(&[
            (
                "agents.brightwell.example.brightwell.example",
                RrType::Mx,
                Answer::Records(vec!["10 inbound-smtp.eu-west-2.amazonaws.com.".into()]),
            ),
            (
                "_pylota-identity.agents.brightwell.example.brightwell.example",
                RrType::Txt,
                Answer::Empty,
            ),
        ]);
        let found = doubled_names(&records, &ans);
        assert_eq!(
            found,
            vec![DoubledName {
                name: "agents.brightwell.example".into(),
                found_at: "agents.brightwell.example.brightwell.example".into(),
                host: "agents".into(),
            }]
        );
        assert_eq!(DoubledName::CODE, "record_doubled_name");

        // A record at the doubled name with another value is not this issue.
        let other = answers(&[(
            "agents.brightwell.example.brightwell.example",
            RrType::Mx,
            Answer::Records(vec!["10 mx.elsewhere.example".into()]),
        )]);
        assert!(doubled_names(&records, &other).is_empty());
        // TXT values match through DoH quoting.
        let quoted = answers(&[(
            "_pylota-identity.agents.brightwell.example.brightwell.example",
            RrType::Txt,
            Answer::Records(vec!["\"pm-verify=abc\"".into()]),
        )]);
        assert_eq!(doubled_names(&records, &quoted).len(), 1);
    }
}
