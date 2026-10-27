//! The authentication verdict ([Inbound › Authentication verdict], FR-IN-4, D1, D9): the trusted
//! `Authentication-Results`, DKIM and ARC through `mail-auth` over DNS answers the caller fetched, our
//! own DMARC evaluation, the ARC rescue rule ([Parity additions § 6.3]) and the verdict table.
//!
//! `mail-auth` reads the clock itself for signature expiry; everything else here takes its inputs.
//!
//! [Inbound › Authentication verdict]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
//! [Parity additions § 6.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/parity-additions.md

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::task::{Context, Poll, Waker};

use mail_auth::common::cache::NoCache;
use mail_auth::common::parse::TxtRecordParser as _;
use mail_auth::common::verify::DomainKey;
use mail_auth::{
    AuthenticatedMessage, DkimResult, DnsError, Error, MessageAuthenticator, Parameters,
    ResolverCache, Txt,
};
use serde::Serialize;

use crate::address::org_domain;

// ---- RFC 8601 ------------------------------------------------------------------------------------

/// One `method=result` with its properties (`ptype.property`, lower case, and values).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResInfo {
    pub method: String,
    pub result: String,
    pub props: Vec<(String, String)>,
}

impl ResInfo {
    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuthResults {
    pub authserv_id: String,
    pub results: Vec<ResInfo>,
}

impl AuthResults {
    pub fn first(&self, method: &str) -> Option<&ResInfo> {
        self.results.iter().find(|r| r.method == method)
    }
}

/// Removes RFC 5322 comments, keeping quoted strings.
fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for c in s.chars() {
        if escaped {
            if depth == 0 {
                out.push(c);
            }
            escaped = false;
            continue;
        }
        match c {
            '\\' => {
                escaped = true;
                if depth == 0 {
                    out.push(c);
                }
            }
            '"' if depth == 0 => {
                quoted = !quoted;
                out.push(c);
            }
            '(' if !quoted => depth += 1,
            ')' if !quoted && depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ => out.push(c),
        }
    }
    out
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    match v.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => v.to_owned(),
    }
}

/// Parses an `Authentication-Results` (or `ARC-Authentication-Results`, after its `i=` tag) value.
pub fn parse_authentication_results(value: &str) -> Option<AuthResults> {
    let clean = strip_comments(value);
    let mut parts = clean.split(';');
    let mut head = parts.next()?.split_whitespace();
    let mut authserv_id = head.next()?.to_ascii_lowercase();
    if authserv_id.starts_with("i=") {
        // ARC-Authentication-Results: "i=1; authserv-id; …"
        authserv_id = parts
            .next()?
            .split_whitespace()
            .next()?
            .to_ascii_lowercase();
    }
    let mut results = Vec::new();
    for part in parts {
        let mut tokens = part.split_whitespace();
        let Some((method, result)) = tokens.next().and_then(|t| t.split_once('=')) else {
            continue;
        };
        let method = method
            .split('/')
            .next()
            .unwrap_or(method)
            .to_ascii_lowercase();
        if method == "none" {
            continue;
        }
        let mut props = Vec::new();
        for t in tokens {
            if let Some((k, v)) = t.split_once('=') {
                let k = k.to_ascii_lowercase();
                if k.contains('.') {
                    props.push((k, unquote(v)));
                }
            }
        }
        results.push(ResInfo {
            method,
            result: result.to_ascii_lowercase(),
            props,
        });
    }
    Some(AuthResults {
        authserv_id,
        results,
    })
}

/// Step 1 (D9): only the **topmost** header, and only when its authserv-id is `trusted_id`. `headers`
/// are in message order, topmost first.
pub fn trusted_results(headers: &[String], trusted_id: &str) -> Option<AuthResults> {
    if trusted_id.trim().is_empty() {
        return None;
    }
    let top = parse_authentication_results(headers.first()?)?;
    (top.authserv_id == trusted_id.trim().to_ascii_lowercase()).then_some(top)
}

// ---- DNS answers ----------------------------------------------------------------------------------

/// The answer to one TXT query, fetched by `platform::Dns` (step 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxtAnswer {
    /// Every TXT record, its strings joined.
    Records(Vec<Vec<u8>>),
    /// NXDOMAIN or no TXT records.
    NotFound,
    /// Both resolvers failed or timed out.
    TempError,
}

/// Answers keyed by lower-case name without the trailing dot.
pub type DnsAnswers = HashMap<String, TxtAnswer>;

fn key(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

/// The TXT names step 2 prefetches: each DKIM signature's key (at most 10), each ARC instance's keys
/// (at most 5), and `_dmarc` of the `From` domain and of its organisational domain.
pub fn prefetch_names(raw: &[u8], from_domain: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    let mut push = |n: String| {
        if !names.contains(&n) {
            names.push(n);
        }
    };
    if let Some(m) = AuthenticatedMessage::parse(raw) {
        for h in m.dkim_headers.iter().take(10) {
            push(key(&format!("{}._domainkey.{}", h.header.s, h.header.d)));
        }
        for h in m.ams_headers.iter().take(5) {
            push(key(&format!("{}._domainkey.{}", h.header.s, h.header.d)));
        }
        for h in m.as_headers.iter().take(5) {
            push(key(&format!("{}._domainkey.{}", h.header.s, h.header.d)));
        }
    }
    if let Some(d) = from_domain {
        push(key(&format!("_dmarc.{d}")));
        push(key(&format!("_dmarc.{}", org_domain(d))));
    }
    names
}

/// The `mail-auth` TXT cache, filled from [`DnsAnswers`]; misses are counted.
struct Prefilled {
    map: HashMap<Box<str>, Txt>,
    misses: RefCell<usize>,
}

impl Prefilled {
    fn new(answers: &DnsAnswers) -> Self {
        let mut map = HashMap::new();
        for (name, answer) in answers {
            let value: Txt = match answer {
                TxtAnswer::Records(records) => {
                    let mut parsed = Err(Error::Dns(DnsError::InvalidRecordType));
                    for r in records {
                        parsed = DomainKey::parse(r);
                        if parsed.is_ok() {
                            break;
                        }
                    }
                    parsed.into()
                }
                TxtAnswer::NotFound => Txt::Error(Error::Dns(DnsError::RecordNotFound(3))),
                TxtAnswer::TempError => {
                    Txt::Error(Error::Dns(DnsError::Resolver("prefetch failed".into())))
                }
            };
            map.insert(format!("{}.", key(name)).into_boxed_str(), value);
        }
        Prefilled {
            map,
            misses: RefCell::new(0),
        }
    }
}

impl ResolverCache<Box<str>, Txt> for Prefilled {
    fn get<Q>(&self, name: &Q) -> Option<Txt>
    where
        Box<str>: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        let hit = self.map.get(name).cloned();
        // A miss returns a temporary error rather than reaching mail-auth's own DoH client.
        Some(hit.unwrap_or_else(|| {
            *self.misses.borrow_mut() += 1;
            Txt::Error(Error::Dns(DnsError::Resolver("not prefetched".into())))
        }))
    }
    fn remove<Q>(&self, _: &Q) -> Option<Txt>
    where
        Box<str>: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        None
    }
    // The type mail-auth names its expiry with (std's `Instant` natively, `web-time`'s on wasm32).
    fn insert(&self, _: Box<str>, _: Txt, _: web_time::Instant) {}
}

/// Polls a future that cannot wait: every DNS answer is in the cache, so `mail-auth` completes on the
/// first poll. `None` if it did not.
fn ready<F: Future>(f: F) -> Option<F::Output> {
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => Some(v),
        Poll::Pending => None,
    }
}

// ---- DKIM and ARC ----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SigResult {
    Pass,
    Neutral,
    Fail,
    Permerror,
    Temperror,
    None,
}

impl From<&DkimResult> for SigResult {
    fn from(r: &DkimResult) -> Self {
        match r {
            DkimResult::Pass => SigResult::Pass,
            DkimResult::Neutral(_) => SigResult::Neutral,
            DkimResult::Fail(_) => SigResult::Fail,
            DkimResult::PermError(_) => SigResult::Permerror,
            DkimResult::TempError(_) => SigResult::Temperror,
            DkimResult::None => SigResult::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DkimEntry {
    pub d: String,
    pub s: String,
    pub result: SigResult,
    pub aligned: bool,
}

/// The newest ARC set, for `auth_json.arc` and the rescue rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ArcNewest {
    pub seal_d: String,
    pub ams_d: String,
    /// The newest AMS's `h=`, lower case.
    pub ams_h: Vec<String>,
    /// `t=` of the seal and the AMS, 0 when absent.
    pub seal_t: u64,
    pub ams_t: u64,
    pub aar: Option<AuthResults>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ArcEntry {
    /// `pass`, `fail` or `none`.
    pub result: &'static str,
    pub instances: usize,
    pub newest: Option<ArcNewest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signatures {
    pub dkim: Vec<DkimEntry>,
    pub arc: ArcEntry,
    /// TXT lookups that were not prefetched (`auth_dns_cache_miss_total`; a bug when non-zero).
    pub cache_misses: usize,
}

/// Steps 3 and 4: DKIM (at most 10 entries) and ARC over the raw message.
pub fn verify_signatures(raw: &[u8], answers: &DnsAnswers) -> Signatures {
    let none = ArcEntry {
        result: "none",
        instances: 0,
        newest: None,
    };
    let Some(message) = AuthenticatedMessage::parse(raw) else {
        return Signatures {
            dkim: Vec::new(),
            arc: none,
            cache_misses: 0,
        };
    };
    let cache = Prefilled::new(answers);
    let auth = MessageAuthenticator::new_doh_cloudflare();
    type Params<'a> = Parameters<
        'a,
        &'a AuthenticatedMessage<'a>,
        Prefilled,
        NoCache<Box<str>, mail_auth::RecordSet<mail_auth::MX>>,
        NoCache<Box<str>, mail_auth::RecordSet<std::net::Ipv4Addr>>,
        NoCache<Box<str>, mail_auth::RecordSet<std::net::Ipv6Addr>>,
        NoCache<std::net::IpAddr, mail_auth::RecordSet<Box<str>>>,
    >;
    let params = || -> Params<'_> { Parameters::new(&message).with_txt_cache(&cache) };
    let dkim_out = ready(auth.verify_dkim(params())).unwrap_or_default();
    let dkim = dkim_out
        .iter()
        .take(10)
        .map(|o| DkimEntry {
            d: o.signature()
                .map(|s| s.d.to_ascii_lowercase())
                .unwrap_or_default(),
            s: o.signature().map(|s| s.s.clone()).unwrap_or_default(),
            result: o.result().into(),
            aligned: false,
        })
        .collect();
    let arc = match ready(auth.verify_arc(params())) {
        Some(out) => {
            let instances = message.as_headers.len();
            let result = match out.result() {
                DkimResult::Pass => "pass",
                DkimResult::None if instances == 0 => "none",
                _ => "fail",
            };
            let newest_i = message.as_headers.iter().map(|h| h.header.i).max();
            let newest = newest_i.and_then(|i| {
                let seal = message.as_headers.iter().find(|h| h.header.i == i)?;
                let ams = message.ams_headers.iter().find(|h| h.header.i == i)?;
                let aar = message
                    .aar_headers
                    .iter()
                    .find(|h| h.header.i == i)
                    .and_then(|h| parse_authentication_results(&String::from_utf8_lossy(h.value)));
                Some(ArcNewest {
                    seal_d: seal.header.d.to_ascii_lowercase(),
                    ams_d: ams.header.d.to_ascii_lowercase(),
                    ams_h: ams
                        .header
                        .h
                        .iter()
                        .map(|h| h.to_ascii_lowercase())
                        .collect(),
                    seal_t: seal.header.t,
                    ams_t: ams.header.t,
                    aar,
                })
            });
            ArcEntry {
                result,
                instances,
                newest,
            }
        }
        None => none,
    };
    let cache_misses = *cache.misses.borrow();
    Signatures {
        dkim,
        arc,
        cache_misses,
    }
}

// ---- DMARC ------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    None,
    Quarantine,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DmarcRecord {
    pub p: Policy,
    pub sp: Option<Policy>,
    /// `adkim=s`
    pub strict_dkim: bool,
    /// `aspf=s`
    pub strict_spf: bool,
}

fn policy(v: &str) -> Option<Policy> {
    match v.trim().to_ascii_lowercase().as_str() {
        "none" => Some(Policy::None),
        "quarantine" => Some(Policy::Quarantine),
        "reject" => Some(Policy::Reject),
        _ => None,
    }
}

/// Parses one TXT string as a DMARC record: `v=DMARC1` first, then the tags. `None` when it is not one.
pub fn parse_dmarc(txt: &str) -> Option<DmarcRecord> {
    let mut tags = txt.split(';').map(str::trim).filter(|t| !t.is_empty());
    let (k, v) = tags.next()?.split_once('=')?;
    if !k.trim().eq_ignore_ascii_case("v") || !v.trim().eq_ignore_ascii_case("DMARC1") {
        return None;
    }
    let mut rec = DmarcRecord {
        p: Policy::None,
        sp: None,
        strict_dkim: false,
        strict_spf: false,
    };
    let mut has_p = false;
    for t in tags {
        let Some((k, v)) = t.split_once('=') else {
            continue;
        };
        match k.trim().to_ascii_lowercase().as_str() {
            "p" => {
                if let Some(p) = policy(v) {
                    rec.p = p;
                    has_p = true;
                }
            }
            "sp" => rec.sp = policy(v),
            "adkim" => rec.strict_dkim = v.trim().eq_ignore_ascii_case("s"),
            "aspf" => rec.strict_spf = v.trim().eq_ignore_ascii_case("s"),
            _ => {}
        }
    }
    // A record without a valid p= is treated as p=none (RFC 7489 § 6.6.3 applies an sp or none).
    if !has_p {
        rec.p = rec.sp.unwrap_or(Policy::None);
    }
    Some(rec)
}

enum Lookup {
    Found(DmarcRecord),
    Absent,
    Failed,
}

fn lookup(answers: &DnsAnswers, domain: &str) -> Lookup {
    match answers.get(&key(&format!("_dmarc.{domain}"))) {
        Some(TxtAnswer::Records(records)) => {
            let found: Vec<DmarcRecord> = records
                .iter()
                .filter_map(|r| parse_dmarc(&String::from_utf8_lossy(r)))
                .collect();
            // More than one record, or none: no policy.
            match found.len() {
                1 => Lookup::Found(found.into_iter().next().expect("one")),
                _ => Lookup::Absent,
            }
        }
        Some(TxtAnswer::NotFound) => Lookup::Absent,
        Some(TxtAnswer::TempError) | None => Lookup::Failed,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DmarcResult {
    Pass,
    Fail,
    Temperror,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignedBy {
    Dkim,
    Spf,
}

/// The SPF input of step 5.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spf {
    /// `pass`, `fail`, `softfail`, `neutral`, `none`, …
    pub result: String,
    /// The checked MAIL FROM domain.
    pub domain: Option<String>,
    /// `false` without a trusted header (Email Routing): `auth_json.spf.source = null`.
    pub checked: bool,
}

impl Spf {
    /// SPF from the trusted header (`spf=` and `smtp.mailfrom`), else unchecked `none`.
    pub fn from_trusted(trusted: Option<&AuthResults>, envelope_from: &str) -> Spf {
        let Some(r) = trusted.and_then(|t| t.first("spf")) else {
            return Spf {
                result: "none".into(),
                domain: None,
                checked: false,
            };
        };
        let mailfrom = r.prop("smtp.mailfrom").unwrap_or(envelope_from);
        let domain = mailfrom
            .rsplit_once('@')
            .map_or(mailfrom, |(_, d)| d)
            .trim()
            .to_ascii_lowercase();
        Spf {
            result: r.result.clone(),
            domain: (!domain.is_empty()).then_some(domain),
            checked: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Dmarc {
    pub result: DmarcResult,
    /// The policy in force (`sp` for a subdomain under an organisational record).
    pub policy: Option<Policy>,
    pub record_domain: Option<String>,
    /// Set whatever the result, so alignment is recorded without a record (D1).
    pub aligned_by: Option<AlignedBy>,
    pub spf_aligned: bool,
}

fn aligned(a: &str, from: &str, strict: bool) -> bool {
    let a = a.trim_end_matches('.').to_ascii_lowercase();
    if strict {
        a == from
    } else {
        org_domain(&a) == org_domain(from)
    }
}

/// Step 6, our DMARC. `dkim` entries get their `aligned` flag set.
pub fn dmarc(
    from_domain: &str,
    dkim: &mut [DkimEntry],
    spf: &Spf,
    answers: &DnsAnswers,
    trusted: Option<&AuthResults>,
) -> Dmarc {
    let from = from_domain.trim_end_matches('.').to_ascii_lowercase();
    let org = org_domain(&from).to_owned();
    let (record, record_domain, failed) = match lookup(answers, &from) {
        Lookup::Found(r) => (Some(r), Some(from.clone()), false),
        Lookup::Failed => (None, None, true),
        Lookup::Absent if org != from => match lookup(answers, &org) {
            Lookup::Found(r) => (Some(r), Some(org.clone()), false),
            Lookup::Failed => (None, None, true),
            Lookup::Absent => (None, None, false),
        },
        Lookup::Absent => (None, None, false),
    };
    let strict_dkim = record.as_ref().is_some_and(|r| r.strict_dkim);
    let strict_spf = record.as_ref().is_some_and(|r| r.strict_spf);
    for e in dkim.iter_mut() {
        e.aligned = e.result == SigResult::Pass && aligned(&e.d, &from, strict_dkim);
    }
    let dkim_ok = dkim.iter().any(|e| e.aligned);
    let spf_aligned = spf.result == "pass"
        && spf
            .domain
            .as_deref()
            .is_some_and(|d| aligned(d, &from, strict_spf));
    let aligned_by = if dkim_ok {
        Some(AlignedBy::Dkim)
    } else if spf_aligned {
        Some(AlignedBy::Spf)
    } else {
        None
    };
    let mut result = match (&record, failed) {
        (Some(_), _) if aligned_by.is_some() => DmarcResult::Pass,
        (Some(_), _) => DmarcResult::Fail,
        (None, true) if aligned_by.is_some() => DmarcResult::Pass,
        (None, true) => DmarcResult::Temperror,
        (None, false) => DmarcResult::None,
    };
    if result == DmarcResult::Temperror
        && let Some(t) = trusted.and_then(|t| t.first("dmarc"))
    {
        match t.result.as_str() {
            "pass" => result = DmarcResult::Pass,
            "fail" => result = DmarcResult::Fail,
            _ => {}
        }
    }
    let policy = record.as_ref().map(|r| match (&record_domain, r.sp) {
        (Some(d), Some(sp)) if *d == org && from != org => sp,
        _ => r.p,
    });
    Dmarc {
        result,
        policy,
        record_domain,
        aligned_by,
        spf_aligned,
    }
}

// ---- ARC rescue and the verdict -------------------------------------------------------------------

pub struct RescueInput<'a> {
    /// The effective `inbound.trusted_arc_sealers`, lower case.
    pub trusted_sealers: &'a [String],
    pub arc: &'a ArcEntry,
    pub from_domain: &'a str,
    /// `received_at` in Unix seconds.
    pub received_at: i64,
    /// The `Date` header in Unix seconds.
    pub date: Option<i64>,
    /// Condition 7, computed by the caller from the identity's addresses (step 12).
    pub recipient_bound: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "result", content = "reason", rename_all = "snake_case")]
pub enum Rescue {
    Rescued,
    Refused(&'static str),
}

/// [Parity additions § 6.3]: every condition must hold; the first that fails is the reason.
pub fn arc_rescue(i: &RescueInput<'_>) -> Rescue {
    if i.trusted_sealers.is_empty() {
        return Rescue::Refused("off");
    }
    let Some(n) = i.arc.newest.as_ref().filter(|_| i.arc.result == "pass") else {
        return Rescue::Refused("chain");
    };
    if n.seal_d != n.ams_d || !i.trusted_sealers.contains(&n.seal_d) {
        return Rescue::Refused("untrusted_sealer");
    }
    if !n.ams_h.iter().any(|h| h == "from") {
        return Rescue::Refused("from_unsigned");
    }
    let from = i.from_domain.to_ascii_lowercase();
    let dmarc_pass = n
        .aar
        .as_ref()
        .and_then(|a| a.first("dmarc"))
        .is_some_and(|d| {
            d.result == "pass"
                && d.prop("header.from")
                    .is_some_and(|h| h.eq_ignore_ascii_case(&from))
        });
    if !dmarc_pass {
        return Rescue::Refused("no_dmarc_pass");
    }
    let window = |t: i64| t >= i.received_at - 72 * 3600 && t <= i.received_at + 15 * 60;
    let fresh = match (n.seal_t, n.ams_t) {
        (s, _) if s > 0 => window(s as i64),
        (_, a) if a > 0 => window(a as i64),
        _ => n.ams_h.iter().any(|h| h == "date") && i.date.is_some_and(window),
    };
    if !fresh {
        return Rescue::Refused("stale");
    }
    if !i.recipient_bound {
        return Rescue::Refused("no_binding");
    }
    Rescue::Rescued
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    ArcRescued,
    Unverified,
    Fail,
    Unaligned,
    Softfail,
    None,
}

impl Verdict {
    /// Only `fail` and `unverified` can quarantine on their own (D1).
    pub fn may_quarantine(self) -> bool {
        matches!(self, Verdict::Fail | Verdict::Unverified)
    }
}

pub struct VerdictInput<'a> {
    pub loopback: bool,
    pub dmarc: &'a Dmarc,
    pub spf_checked: bool,
    pub multiple_from: bool,
    /// The rescue outcome, when the caller ran [`arc_rescue`].
    pub rescue: Option<&'a Rescue>,
}

/// Whether [`arc_rescue`] should run: DMARC `fail` or the `unverified` condition, and one `From`.
pub fn rescue_applies(d: &Dmarc, multiple_from: bool) -> bool {
    !multiple_from && d.result == DmarcResult::Fail
}

/// Step 7, then the `multiple_from` cap at `unaligned`.
pub fn verdict(i: &VerdictInput<'_>) -> Verdict {
    let d = i.dmarc;
    let enforcing = matches!(d.policy, Some(Policy::Quarantine | Policy::Reject));
    let v = if i.loopback || d.result == DmarcResult::Pass {
        Verdict::Pass
    } else if d.result == DmarcResult::Fail
        && !i.multiple_from
        && i.rescue == Some(&Rescue::Rescued)
    {
        Verdict::ArcRescued
    } else if d.result == DmarcResult::Fail
        && enforcing
        && d.aligned_by != Some(AlignedBy::Dkim)
        && !i.spf_checked
    {
        Verdict::Unverified
    } else if d.result == DmarcResult::Fail && enforcing {
        Verdict::Fail
    } else if d.result == DmarcResult::Fail {
        Verdict::Unaligned
    } else if d.result == DmarcResult::Temperror {
        Verdict::Softfail
    } else {
        Verdict::None
    };
    if i.multiple_from && matches!(v, Verdict::Pass | Verdict::ArcRescued) && !i.loopback {
        Verdict::Unaligned
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: &str = "mx.cloudflare.net; spf=pass smtp.mailfrom=bounce@mail.brightwell.example; \
                       dkim=pass header.d=brightwell.example; dmarc=pass header.from=brightwell.example";

    #[test]
    fn rfc8601_parse() {
        let r = parse_authentication_results(
            "mx.cloudflare.net (cf); spf=pass (sender ok) smtp.mailfrom=bounce@x.example;\r\n dkim=fail \
             header.d=x.example header.s=s1; arc=none; dmarc/1=pass header.from=x.example",
        )
        .unwrap();
        assert_eq!(r.authserv_id, "mx.cloudflare.net");
        assert_eq!(
            r.first("spf").unwrap().prop("smtp.mailfrom"),
            Some("bounce@x.example")
        );
        assert_eq!(r.first("dkim").unwrap().prop("header.s"), Some("s1"));
        assert_eq!(r.first("dmarc").unwrap().result, "pass");
        assert_eq!(
            parse_authentication_results("x.example; none")
                .unwrap()
                .results,
            vec![]
        );
        let aar =
            parse_authentication_results("i=1; mx.google.com; dmarc=pass header.from=x.example")
                .unwrap();
        assert_eq!(aar.authserv_id, "mx.google.com");
    }

    #[test]
    fn d9_forged_ar_ignored() {
        let forged =
            "mx.cloudflare.net; spf=pass smtp.mailfrom=ceo@victim.example; dmarc=pass".to_owned();
        let other = "evil.example; spf=pass smtp.mailfrom=ceo@victim.example".to_owned();
        // A trusted-looking header below the topmost one is ignored.
        assert_eq!(
            trusted_results(&[other.clone(), forged.clone()], "mx.cloudflare.net"),
            None
        );
        // The topmost header with the trusted id is used, whatever is below it.
        let t = trusted_results(&[TOP.to_owned(), forged.clone()], "mx.cloudflare.net").unwrap();
        assert_eq!(
            t.first("spf").unwrap().prop("smtp.mailfrom"),
            Some("bounce@mail.brightwell.example")
        );
        // No trusted id configured: nothing is trusted, and SPF is unchecked.
        assert_eq!(trusted_results(&[TOP.to_owned()], ""), None);
        let spf = Spf::from_trusted(None, "x@y.example");
        assert!(!spf.checked && spf.result == "none");
        let spf = Spf::from_trusted(Some(&t), "ignored@else.example");
        assert_eq!(
            (spf.result.as_str(), spf.domain.as_deref()),
            ("pass", Some("mail.brightwell.example"))
        );
        // Our own DKIM still runs: a forged dkim=pass in a header does not make an aligned signature.
        let mut dkim = vec![];
        let d = dmarc(
            "victim.example",
            &mut dkim,
            &Spf::from_trusted(None, ""),
            &answers(&[("_dmarc.victim.example", "v=DMARC1; p=reject")]),
            None,
        );
        assert_eq!(d.result, DmarcResult::Fail);
    }

    fn answers(pairs: &[(&str, &str)]) -> DnsAnswers {
        pairs
            .iter()
            .map(|(k, v)| {
                let a = match *v {
                    "NX" => TxtAnswer::NotFound,
                    "TEMP" => TxtAnswer::TempError,
                    v => TxtAnswer::Records(vec![v.as_bytes().to_vec()]),
                };
                ((*k).to_owned(), a)
            })
            .collect()
    }

    fn sig(d: &str, result: SigResult) -> DkimEntry {
        DkimEntry {
            d: d.into(),
            s: "s1".into(),
            result,
            aligned: false,
        }
    }

    fn spf(result: &str, domain: &str) -> Spf {
        Spf {
            result: result.into(),
            domain: Some(domain.into()),
            checked: true,
        }
    }

    fn v(d: &Dmarc, spf_checked: bool) -> Verdict {
        verdict(&VerdictInput {
            loopback: false,
            dmarc: d,
            spf_checked,
            multiple_from: false,
            rescue: None,
        })
    }

    #[test]
    fn d1_no_dmarc_record() {
        let a = answers(&[("_dmarc.mail.x.example", "NX"), ("_dmarc.x.example", "NX")]);
        let mut dkim = vec![sig("x.example", SigResult::Pass)];
        let d = dmarc(
            "mail.x.example",
            &mut dkim,
            &spf("fail", "x.example"),
            &a,
            None,
        );
        assert_eq!(
            (d.result, d.aligned_by, d.policy),
            (DmarcResult::None, Some(AlignedBy::Dkim), None)
        );
        assert!(dkim[0].aligned);
        assert_eq!(v(&d, true), Verdict::None);
        assert!(!Verdict::None.may_quarantine());
        // SPF alignment recorded too.
        let d = dmarc(
            "x.example",
            &mut [],
            &spf("pass", "bounce.x.example"),
            &a,
            None,
        );
        assert_eq!((d.aligned_by, d.spf_aligned), (Some(AlignedBy::Spf), true));
    }

    #[test]
    fn d1_p_none_unaligned() {
        let a = answers(&[(
            "_dmarc.x.example",
            "v=DMARC1; p=none; rua=mailto:r@x.example",
        )]);
        let mut dkim = vec![sig("other.example", SigResult::Pass)];
        let d = dmarc(
            "x.example",
            &mut dkim,
            &spf("pass", "other.example"),
            &a,
            None,
        );
        assert_eq!(
            (d.result, d.policy),
            (DmarcResult::Fail, Some(Policy::None))
        );
        assert_eq!(v(&d, true), Verdict::Unaligned);
        assert!(!Verdict::Unaligned.may_quarantine());
    }

    #[test]
    fn d1_verdict_table() {
        let reject = answers(&[("_dmarc.x.example", "v=DMARC1; p=reject")]);
        let pass = dmarc(
            "x.example",
            &mut [sig("x.example", SigResult::Pass)],
            &spf("none", "x.example"),
            &reject,
            None,
        );
        assert_eq!(v(&pass, false), Verdict::Pass);
        let fail = dmarc(
            "x.example",
            &mut [sig("x.example", SigResult::Fail)],
            &spf("fail", "x.example"),
            &reject,
            None,
        );
        assert_eq!(v(&fail, true), Verdict::Fail);
        assert_eq!(v(&fail, false), Verdict::Unverified);
        // Subdomain under an organisational record uses sp.
        let org = answers(&[
            ("_dmarc.a.x.example", "NX"),
            ("_dmarc.x.example", "v=DMARC1; p=reject; sp=quarantine"),
        ]);
        let d = dmarc("a.x.example", &mut [], &spf("none", ""), &org, None);
        assert_eq!(
            (d.policy, d.record_domain.as_deref()),
            (Some(Policy::Quarantine), Some("x.example"))
        );
        // A failed lookup with nothing aligned is temperror → softfail; the trusted header can decide.
        let temp = answers(&[("_dmarc.x.example", "TEMP")]);
        let d = dmarc("x.example", &mut [], &spf("none", ""), &temp, None);
        assert_eq!(
            (d.result, v(&d, true)),
            (DmarcResult::Temperror, Verdict::Softfail)
        );
        let t = parse_authentication_results("mx.cloudflare.net; dmarc=fail header.from=x.example")
            .unwrap();
        assert_eq!(
            dmarc("x.example", &mut [], &spf("none", ""), &temp, Some(&t)).result,
            DmarcResult::Fail
        );
        // Two records: no policy.
        let two = DnsAnswers::from([(
            "_dmarc.x.example".to_owned(),
            TxtAnswer::Records(vec![
                b"v=DMARC1; p=reject".to_vec(),
                b"v=DMARC1; p=none".to_vec(),
            ]),
        )]);
        assert_eq!(
            dmarc("x.example", &mut [], &spf("none", ""), &two, None).result,
            DmarcResult::None
        );
        // Multiple From caps at unaligned; loopback always passes.
        let capped = verdict(&VerdictInput {
            loopback: false,
            dmarc: &pass,
            spf_checked: true,
            multiple_from: true,
            rescue: None,
        });
        assert_eq!(capped, Verdict::Unaligned);
        let lb = verdict(&VerdictInput {
            loopback: true,
            dmarc: &fail,
            spf_checked: true,
            multiple_from: false,
            rescue: None,
        });
        assert_eq!(lb, Verdict::Pass);
        assert!(parse_dmarc("p=reject; v=DMARC1").is_none());
        assert!(parse_dmarc("v=spf1 -all").is_none());
    }

    #[test]
    fn d1_strict_alignment() {
        let a = answers(&[("_dmarc.x.example", "v=DMARC1; p=reject; adkim=s; aspf=s")]);
        let mut dkim = vec![sig("mail.x.example", SigResult::Pass)];
        let d = dmarc(
            "x.example",
            &mut dkim,
            &spf("pass", "bounce.x.example"),
            &a,
            None,
        );
        assert_eq!(d.result, DmarcResult::Fail);
        assert!(!dkim[0].aligned && !d.spf_aligned);
    }

    #[test]
    fn arc_rescue_rule() {
        let arc = ArcEntry {
            result: "pass",
            instances: 1,
            newest: Some(ArcNewest {
                seal_d: "google.com".into(),
                ams_d: "google.com".into(),
                ams_h: vec!["from".into(), "to".into(), "date".into()],
                seal_t: 1_793_138_000,
                ams_t: 0,
                aar: parse_authentication_results(
                    "i=1; mx.google.com; dmarc=pass header.from=x.example",
                ),
            }),
        };
        let sealers = vec!["google.com".to_owned()];
        fn input<'a>(
            sealers: &'a [String],
            arc: &'a ArcEntry,
            bound: bool,
            received: i64,
        ) -> RescueInput<'a> {
            RescueInput {
                trusted_sealers: sealers,
                arc,
                from_domain: "x.example",
                received_at: received,
                date: None,
                recipient_bound: bound,
            }
        }
        assert_eq!(
            arc_rescue(&input(&sealers, &arc, true, 1_793_138_400)),
            Rescue::Rescued
        );
        assert_eq!(
            arc_rescue(&input(&[], &arc, true, 1_793_138_400)),
            Rescue::Refused("off")
        );
        assert_eq!(
            arc_rescue(&input(
                &["sub.google.com".into()],
                &arc,
                true,
                1_793_138_400
            )),
            Rescue::Refused("untrusted_sealer")
        );
        assert_eq!(
            arc_rescue(&input(&sealers, &arc, true, 1_793_138_400 + 73 * 3600)),
            Rescue::Refused("stale")
        );
        assert_eq!(
            arc_rescue(&input(&sealers, &arc, false, 1_793_138_400)),
            Rescue::Refused("no_binding")
        );
        let mut failed = arc.clone();
        failed.result = "fail";
        assert_eq!(
            arc_rescue(&input(&sealers, &failed, true, 1_793_138_400)),
            Rescue::Refused("chain")
        );
    }

    #[test]
    fn dkim_through_mail_auth() {
        // A message whose DKIM key lookup fails is temperror, one that is not found is permerror; nothing
        // reaches the network either way.
        let raw = b"DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed; d=x.example; s=s1; h=from:subject; \
                    bh=47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=; b=AAAA\r\nFrom: a@x.example\r\nSubject: t\r\n\r\n";
        assert_eq!(
            prefetch_names(raw, Some("mail.x.example")),
            vec![
                "s1._domainkey.x.example",
                "_dmarc.mail.x.example",
                "_dmarc.x.example"
            ]
        );
        let s = verify_signatures(raw, &answers(&[("s1._domainkey.x.example", "TEMP")]));
        assert_eq!(
            (s.dkim.len(), s.dkim[0].result, s.cache_misses),
            (1, SigResult::Temperror, 0)
        );
        let s = verify_signatures(raw, &answers(&[("s1._domainkey.x.example", "NX")]));
        assert_eq!(s.dkim[0].result, SigResult::Permerror);
        let s = verify_signatures(raw, &DnsAnswers::new());
        assert_eq!(
            (s.dkim[0].result, s.cache_misses),
            (SigResult::Temperror, 1)
        );
        assert_eq!(s.arc.result, "none");
    }
}
