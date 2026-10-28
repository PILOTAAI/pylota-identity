//! A domain's records ([Identities and domains › Records, What each check verifies]): what the
//! provider APIs return, normalised into `records_json`, and one resolver's check of them (FR-DOM-3,
//! FR-DOM-4). Records are never composed from templates: the Worker adds only its own ownership TXT.
//!
//! [Identities and domains › Records, What each check verifies]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use std::collections::{BTreeSet, HashMap};

use pylota_identity_core::address::org_domain;
use pylota_identity_core::auth::{Policy, parse_dmarc};
use pylota_identity_core::dns::{
    self, Answer, RrType, Transport, check_alignment, count_spf_lookups, parse_spf, signing_domains,
};
use pylota_identity_platform::dns::{Dns, DnsError, DnsStatus, RType, Resolver, unquote_txt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::cloudflare_api::Record;

/// One record of `records_json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rec {
    #[serde(rename = "type")]
    pub rtype: String,
    /// Fully qualified, lower case, without the trailing dot.
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u64>,
    /// `mx`, `spf`, `dkim`, `return_path`, `dmarc`, `ownership`, `ns`.
    pub purpose: String,
    pub required: bool,
    /// Which API gave it: `routing`, `sending`, `ses` or `worker` (stored only).
    #[serde(default)]
    pub source: String,
    /// The last observed state (`ok`, `missing`, `mismatch`), when checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl Rec {
    /// The object of the API: with `host`, without `source`.
    pub fn json(&self) -> Value {
        let mut v = json!({
            "type": self.rtype, "name": self.name, "host": dns::host(&self.name),
            "value": self.value, "purpose": self.purpose, "required": self.required,
        });
        if let Some(p) = self.priority {
            v["priority"] = p.into();
        }
        if let Some(s) = &self.status {
            v["status"] = s.clone().into();
        }
        v
    }
}

pub fn parse(json: &str) -> Vec<Rec> {
    serde_json::from_str(json).unwrap_or_default()
}

/// `_pylota-identity.{domain}`.
pub fn ownership_name(domain: &str) -> String {
    format!("_pylota-identity.{domain}")
}

pub fn ownership(domain: &str, token: &str) -> Rec {
    Rec {
        rtype: "TXT".into(),
        name: ownership_name(domain),
        value: format!("pm-verify={token}"),
        priority: None,
        purpose: "ownership".into(),
        required: true,
        source: "worker".into(),
        status: None,
    }
}

fn purpose(domain: &str, return_path: Option<&str>, r: &Record) -> &'static str {
    let name = r.name.trim_end_matches('.');
    if return_path.is_some_and(|rp| name == rp) {
        "return_path"
    } else if name.contains("._domainkey.") {
        "dkim"
    } else if r.rtype == "MX" && name == domain {
        "mx"
    } else if r.rtype == "TXT" && r.content.to_ascii_lowercase().starts_with("v=spf1") {
        "spf"
    } else if name.starts_with("_dmarc.") {
        "dmarc"
    } else {
        "other"
    }
}

/// Provider records as `records_json` entries.
pub fn from_provider(
    domain: &str,
    source: &str,
    return_path: Option<&str>,
    recs: &[Record],
) -> Vec<Rec> {
    recs.iter()
        .map(|r| Rec {
            rtype: r.rtype.clone(),
            name: r.name.trim_end_matches('.').to_ascii_lowercase(),
            value: r.content.clone(),
            priority: r.priority,
            purpose: purpose(domain, return_path, r).into(),
            required: true,
            source: source.into(),
            status: None,
        })
        .collect()
}

/// The three Easy DKIM CNAMEs of an SES identity (`required: false` for a failover identity).
pub fn ses_dkim(domain: &str, tokens: &[String], zone: &str, required: bool) -> Vec<Rec> {
    tokens
        .iter()
        .map(|t| Rec {
            rtype: "CNAME".into(),
            name: format!("{t}._domainkey.{domain}"),
            value: format!("{t}.{zone}"),
            priority: None,
            purpose: "dkim".into(),
            required,
            source: "ses".into(),
            status: None,
        })
        .collect()
}

/// What a domain's checks need of its row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    pub name: String,
    pub platform: bool,
    pub receiving: bool,
    pub sending: bool,
    pub inbound: String,
    pub transport: String,
    pub mail_from_domain: Option<String>,
    pub ownership_token: Option<String>,
    /// The zone's name, for the NS check.
    pub zone_name: Option<String>,
    pub expected_ns: Vec<String>,
}

/// One issue: its code, level (`fail`, `degraded`, `ownership`) and the record it is about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub code: String,
    pub level: String,
    #[serde(default)]
    pub record: Option<String>,
}

fn issue(code: &str, level: &str, record: Option<&str>) -> Issue {
    Issue {
        code: code.into(),
        level: level.into(),
        record: record.map(str::to_owned),
    }
}

/// One resolver's result: its issues, the names whose queries failed, and each record's status.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolverResult {
    pub issues: Vec<Issue>,
    /// `(name, type)` of the queries that failed; non-empty means the outcome is `error`.
    pub errors: Vec<(String, String)>,
    /// Record name and type → `ok`, `missing` or `mismatch`.
    pub status: Vec<(String, String, String)>,
}

impl ResolverResult {
    /// Step 1 of Outcome per resolver.
    pub fn outcome(&self) -> &'static str {
        if !self.errors.is_empty() {
            "error"
        } else if self.issues.iter().any(|i| i.level == "ownership") {
            "ownership_changed"
        } else if self.issues.iter().any(|i| i.level == "fail") {
            "fail"
        } else if self.issues.iter().any(|i| i.level == "degraded") {
            "degraded"
        } else {
            "pass"
        }
    }
}

fn rtype(t: &str) -> RType {
    match t {
        "MX" => RType::Mx,
        "CNAME" => RType::Cname,
        "NS" => RType::Ns,
        "A" => RType::A,
        "AAAA" => RType::Aaaa,
        _ => RType::Txt,
    }
}

/// One resolver's answers, fetched once per name and type.
struct Lookups<'a, D: Dns> {
    dns: &'a D,
    r: Resolver,
    cache: HashMap<(String, &'static str), Result<Vec<String>, ()>>,
}

impl<D: Dns> Lookups<'_, D> {
    async fn get(&mut self, name: &str, t: RType) -> Result<Vec<String>, ()> {
        let key = (name.to_ascii_lowercase(), t.as_str());
        if let Some(v) = self.cache.get(&key) {
            return v.clone();
        }
        let v = match self.dns.query(self.r, name, t).await {
            Ok(a) if matches!(a.status, DnsStatus::NoError | DnsStatus::NxDomain) => Ok(a
                .records
                .into_iter()
                .filter(|x| x.rtype == t)
                .map(|x| {
                    if t == RType::Txt {
                        unquote_txt(&x.data)
                    } else {
                        x.data
                    }
                })
                .collect()),
            Ok(_) | Err(DnsError::Timeout | DnsError::Http(_) | DnsError::Malformed) => Err(()),
        };
        self.cache.insert(key, v.clone());
        v
    }
}

fn host_of_mx(data: &str) -> String {
    let d = data.trim();
    let h = d.split_once(' ').map_or(d, |(p, h)| {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            h
        } else {
            d
        }
    });
    h.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn spf_include(value: &str) -> Option<String> {
    value
        .split_whitespace()
        .find_map(|t| t.strip_prefix("include:"))
        .map(str::to_ascii_lowercase)
}

/// Checks `records` on one resolver. `ns_due`: the weekly NS row runs in this cycle.
pub async fn check<D: Dns>(
    dns: &D,
    r: Resolver,
    f: &Facts,
    records: &[Rec],
    ns_due: bool,
) -> ResolverResult {
    let mut l = Lookups {
        dns,
        r,
        cache: HashMap::new(),
    };
    let mut out = ResolverResult::default();
    let d = f.name.as_str();
    let status = |name: &str, t: &str, s: &str, out: &mut ResolverResult| {
        out.status
            .push((name.to_owned(), t.to_owned(), s.to_owned()));
    };
    let routing = f.receiving && f.inbound == "routing";
    let cf_sending = f.sending && f.transport == "cloudflare";
    let ses_counts = f.transport == "ses" || f.inbound == "ses";
    // MX at the domain.
    let mx_expected: BTreeSet<String> = records
        .iter()
        .filter(|x| x.purpose == "mx" && x.rtype == "MX" && x.name == d)
        .map(|x| x.value.trim_end_matches('.').to_ascii_lowercase())
        .collect();
    if routing && !mx_expected.is_empty() {
        match l.get(d, RType::Mx).await {
            Err(()) => out.errors.push((d.into(), "MX".into())),
            Ok(found) => {
                let found: BTreeSet<String> = found.iter().map(|m| host_of_mx(m)).collect();
                if !mx_expected.is_subset(&found) {
                    out.issues.push(issue("mx_missing", "fail", Some(d)));
                    status(d, "MX", "missing", &mut out);
                } else {
                    status(d, "MX", "ok", &mut out);
                }
                if found
                    .iter()
                    .any(|h| !mx_expected.contains(h) && !h.ends_with(".cloudflare.net"))
                {
                    out.issues.push(issue("mx_unexpected", "degraded", Some(d)));
                }
            }
        }
    }
    // SPF at the domain.
    let spf_expected = records
        .iter()
        .find(|x| x.purpose == "spf" && x.name == d && x.source == "routing");
    if routing && let Some(exp) = spf_expected {
        match l.get(d, RType::Txt).await {
            Err(()) => out.errors.push((d.into(), "TXT".into())),
            Ok(txts) => {
                let spfs: Vec<&String> = txts.iter().filter(|t| dns::is_spf(t)).collect();
                let want = spf_include(&exp.value);
                match spfs.as_slice() {
                    [] => out.issues.push(issue("spf_missing", "degraded", Some(d))),
                    [one] => {
                        let has = want.as_ref().is_none_or(|w| {
                            one.to_ascii_lowercase().contains(&format!("include:{w}"))
                        });
                        if !has {
                            out.issues.push(issue("spf_missing", "degraded", Some(d)));
                        } else if let Ok(rec) = parse_spf(one) {
                            // Lookups: include and redirect targets, fetched once each.
                            let mut answers: dns::Answers = HashMap::new();
                            let mut todo: Vec<String> = rec_targets(one);
                            let mut seen = 0;
                            while let Some(t) = todo.pop() {
                                if seen >= 12 || answers.contains_key(&(t.clone(), RrType::Txt)) {
                                    continue;
                                }
                                seen += 1;
                                let a = match l.get(&t, RType::Txt).await {
                                    Ok(v) if v.is_empty() => Answer::Empty,
                                    Ok(v) => {
                                        for s in v.iter().filter(|s| dns::is_spf(s)) {
                                            todo.extend(rec_targets(s));
                                        }
                                        Answer::Records(v)
                                    }
                                    Err(()) => Answer::Error,
                                };
                                answers.insert((t, RrType::Txt), a);
                            }
                            let count = count_spf_lookups(&rec, d, &answers);
                            if matches!(
                                count.error,
                                Some(
                                    dns::SpfError::TooManyLookups
                                        | dns::SpfError::TooManyVoidLookups
                                )
                            ) {
                                out.issues
                                    .push(issue("spf_too_many_lookups", "degraded", Some(d)));
                            }
                        }
                    }
                    _ => out.issues.push(issue("spf_multiple", "degraded", Some(d))),
                }
            }
        }
    }
    // Every other record of the provider APIs: equality of each name's set.
    for x in records.iter().filter(|x| {
        x.name != d
            && matches!(x.purpose.as_str(), "dkim" | "return_path")
            && (x.source != "ses" || ses_counts || f.transport == "cloudflare")
    }) {
        let counts = match x.source.as_str() {
            "routing" => routing,
            "sending" => cf_sending,
            "ses" => true,
            _ => false,
        };
        if !counts {
            continue;
        }
        let found = match l.get(&x.name, rtype(&x.rtype)).await {
            Ok(v) => v,
            Err(()) => {
                out.errors.push((x.name.clone(), x.rtype.clone()));
                continue;
            }
        };
        let ok = found.iter().any(|o| {
            if x.rtype == "MX" {
                host_of_mx(o) == x.value.trim_end_matches('.').to_ascii_lowercase()
            } else {
                dns::value_matches(
                    if x.rtype == "CNAME" {
                        RrType::Cname
                    } else {
                        RrType::Txt
                    },
                    &x.value,
                    o,
                )
            }
        });
        let st = if ok {
            "ok"
        } else if found.is_empty() {
            "missing"
        } else {
            "mismatch"
        };
        status(&x.name, &x.rtype, st, &mut out);
        if ok {
            continue;
        }
        // The failover identity's CNAMEs inform only while the domain sends through Cloudflare.
        let informational = x.source == "ses" && !ses_counts;
        if informational {
            continue;
        }
        let (code, level) = match (x.source.as_str(), x.purpose.as_str()) {
            ("routing", _) => ("routing_dkim_missing", "degraded"),
            (_, "return_path") => ("return_path_missing", "fail"),
            (_, _) if st == "mismatch" && x.rtype == "TXT" => ("dkim_mismatch", "fail"),
            _ => ("dkim_missing", "fail"),
        };
        if !out.issues.iter().any(|i| i.code == code) {
            out.issues.push(issue(code, level, Some(&x.name)));
        }
    }
    // DMARC.
    if f.sending {
        let own = format!("_dmarc.{d}");
        let org = org_domain(d).to_owned();
        let (found, at) = match l.get(&own, RType::Txt).await {
            Err(()) => {
                out.errors.push((own.clone(), "TXT".into()));
                (Vec::new(), own.clone())
            }
            Ok(v) if v.iter().any(|t| t.starts_with("v=DMARC1")) || org == d => (v, own.clone()),
            Ok(_) => {
                let at = format!("_dmarc.{org}");
                match l.get(&at, RType::Txt).await {
                    Ok(v) => (v, at),
                    Err(()) => {
                        out.errors.push((at.clone(), "TXT".into()));
                        (Vec::new(), at)
                    }
                }
            }
        };
        let dmarcs: Vec<&String> = found.iter().filter(|t| t.starts_with("v=DMARC1")).collect();
        let queried = !out.errors.iter().any(|(n, _)| n.starts_with("_dmarc."));
        match dmarcs.as_slice() {
            _ if !queried => {}
            [] => out
                .issues
                .push(issue("dmarc_missing", "degraded", Some(&own))),
            [one] => match parse_dmarc(one) {
                None => out
                    .issues
                    .push(issue("dmarc_missing", "degraded", Some(&own))),
                Some(rec) => {
                    let t = if f.transport == "ses" {
                        Transport::Ses
                    } else {
                        Transport::Cloudflare
                    };
                    let sign = signing_domains(t, d, f.mail_from_domain.as_deref());
                    if f.transport != "smtp"
                        && check_alignment(d, &rec, at.trim_start_matches("_dmarc."), &sign)
                            .impossible
                    {
                        out.issues
                            .push(issue("dmarc_alignment_impossible", "fail", Some(&own)));
                    } else if rec.p == Policy::None {
                        out.issues
                            .push(issue("dmarc_policy_none", "degraded", Some(&own)));
                    }
                }
            },
            _ => out
                .issues
                .push(issue("dmarc_multiple", "degraded", Some(&own))),
        }
    }
    // Ownership TXT.
    if !f.platform
        && let Some(token) = &f.ownership_token
    {
        let name = ownership_name(d);
        match l.get(&name, RType::Txt).await {
            Err(()) => out.errors.push((name.clone(), "TXT".into())),
            Ok(v) => {
                let ok = v.iter().any(|t| t.contains(&format!("pm-verify={token}")));
                status(&name, "TXT", if ok { "ok" } else { "missing" }, &mut out);
                if !ok {
                    out.issues
                        .push(issue("ownership_record_missing", "ownership", Some(&name)));
                }
            }
        }
    }
    // NS (weekly, and on every check while a weekly result is not ok).
    if ns_due
        && !f.expected_ns.is_empty()
        && let Some(z) = &f.zone_name
    {
        match l.get(z, RType::Ns).await {
            Err(()) => out.errors.push((z.clone(), "NS".into())),
            Ok(v) => {
                let found: BTreeSet<String> = v
                    .iter()
                    .map(|n| n.trim().trim_end_matches('.').to_ascii_lowercase())
                    .collect();
                let want: BTreeSet<String> = f.expected_ns.iter().cloned().collect();
                if found != want {
                    out.issues
                        .push(issue("nameservers_changed", "ownership", Some(z)));
                }
            }
        }
    }
    out
}

/// The `include` and `redirect` targets of an SPF record.
fn rec_targets(spf: &str) -> Vec<String> {
    spf.split_whitespace()
        .filter_map(|t| {
            let t = t.trim_start_matches(['+', '-', '~', '?']);
            t.strip_prefix("include:")
                .or_else(|| t.strip_prefix("redirect="))
                .map(|x| x.to_ascii_lowercase())
        })
        .collect()
}

/// The fix text of an issue, quoting the record's name and value.
pub fn fix(code: &str, records: &[Rec], record: Option<&str>) -> String {
    let r = record.and_then(|n| records.iter().find(|x| x.name == n));
    let quote = r.map_or(String::new(), |x| {
        format!(" Publish {} {} \"{}\".", x.rtype, x.name, x.value)
    });
    match code {
        "mx_missing" => format!("Set the MX records at the domain to the hosts listed.{quote}"),
        "mx_unexpected" => "Remove the MX records that are not listed for the domain.".into(),
        "spf_missing" => format!("Publish one SPF record with the listed include.{quote}"),
        "spf_multiple" => "Merge the SPF records into one.".into(),
        "spf_too_many_lookups" => "Flatten includes so the SPF record needs at most 10 lookups.".into(),
        "routing_dkim_missing" | "dkim_missing" | "dkim_mismatch" | "return_path_missing" => {
            format!("Restore the record exactly as listed.{quote}")
        }
        "dmarc_missing" => "Publish a DMARC record with p=quarantine or p=reject.".into(),
        "dmarc_policy_none" => "Raise the DMARC policy to quarantine or reject.".into(),
        "dmarc_multiple" => "Keep one DMARC record.".into(),
        "dmarc_alignment_impossible" => {
            "Relax adkim (or aspf) so the sending domain can align.".into()
        }
        "ownership_record_missing" => format!("Publish the ownership record.{quote}"),
        "nameservers_changed" => {
            "The domain's name servers changed: re-prove ownership with POST /v1/domains/{id}/reprove.".into()
        }
        "registration_changed" => {
            "The domain's registration changed: re-prove ownership with POST /v1/domains/{id}/reprove.".into()
        }
        "dns_unresolvable" => "The domain's names do not resolve on either resolver; check the name servers.".into(),
        "dnssec_bogus" => {
            "Remove the stale DS record at the registrar or re-sign the zone.".into()
        }
        "routing_rule_failed" => "A routing rule could not be created; it is retried.".into(),
        "onboarding_failed" => "A provider call failed during onboarding; it is retried.".into(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use pylota_identity_platform::fakes::{FakeDns, block_on};

    use super::*;

    fn rec(t: &str, name: &str, value: &str, purpose: &str, source: &str) -> Rec {
        Rec {
            rtype: t.into(),
            name: name.into(),
            value: value.into(),
            priority: None,
            purpose: purpose.into(),
            required: true,
            source: source.into(),
            status: None,
        }
    }

    fn facts() -> Facts {
        Facts {
            name: "mail.acme.example".into(),
            receiving: true,
            sending: true,
            inbound: "routing".into(),
            transport: "cloudflare".into(),
            ownership_token: Some("TOKEN".into()),
            ..Facts::default()
        }
    }

    fn records() -> Vec<Rec> {
        vec![
            rec(
                "MX",
                "mail.acme.example",
                "route1.mx.cloudflare.net",
                "mx",
                "routing",
            ),
            rec(
                "TXT",
                "mail.acme.example",
                "v=spf1 include:_spf.mx.cloudflare.net ~all",
                "spf",
                "routing",
            ),
            rec(
                "TXT",
                "cf-send._domainkey.mail.acme.example",
                "v=DKIM1; k=rsa; p=KEY",
                "dkim",
                "sending",
            ),
            rec(
                "CNAME",
                "t1._domainkey.mail.acme.example",
                "t1.dkim.amazonses.com",
                "dkim",
                "ses",
            ),
        ]
    }

    fn healthy_dns() -> FakeDns {
        let d = FakeDns::new();
        d.set(
            None,
            "mail.acme.example",
            RType::Mx,
            &["13 route1.mx.cloudflare.net."],
        );
        d.set(
            None,
            "mail.acme.example",
            RType::Txt,
            &["v=spf1 include:_spf.mx.cloudflare.net ~all"],
        );
        d.set(
            None,
            "_spf.mx.cloudflare.net",
            RType::Txt,
            &["v=spf1 ip4:192.0.2.0/24 ~all"],
        );
        d.set(
            None,
            "cf-send._domainkey.mail.acme.example",
            RType::Txt,
            &["v=DKIM1; k=rsa; p=KEY"],
        );
        d.set(
            None,
            "_dmarc.mail.acme.example",
            RType::Txt,
            &["v=DMARC1; p=quarantine"],
        );
        d.set(
            None,
            "_pylota-identity.mail.acme.example",
            RType::Txt,
            &["pm-verify=TOKEN"],
        );
        d
    }

    #[test]
    fn outcomes() {
        let d = healthy_dns();
        let r = block_on(check(&d, Resolver::First, &facts(), &records(), false));
        // The failover CNAME is missing, but informs only while the domain sends through Cloudflare.
        assert_eq!(r.outcome(), "pass", "{r:?}");
        assert!(
            r.status
                .iter()
                .any(|(n, _, s)| n.starts_with("t1.") && s == "missing")
        );
        d.remove("cf-send._domainkey.mail.acme.example", RType::Txt);
        let r = block_on(check(&d, Resolver::First, &facts(), &records(), false));
        assert_eq!(
            (r.outcome(), r.issues[0].code.as_str()),
            ("fail", "dkim_missing")
        );
        let d = healthy_dns();
        d.remove("_pylota-identity.mail.acme.example", RType::Txt);
        let r = block_on(check(&d, Resolver::First, &facts(), &records(), false));
        assert_eq!(r.outcome(), "ownership_changed");
        let d = healthy_dns();
        d.set(
            None,
            "_dmarc.mail.acme.example",
            RType::Txt,
            &["v=DMARC1; p=none"],
        );
        let r = block_on(check(&d, Resolver::First, &facts(), &records(), false));
        assert_eq!(r.outcome(), "degraded");
        let d = healthy_dns();
        d.answer(
            Some(Resolver::Second),
            "mail.acme.example",
            RType::Mx,
            Err(DnsError::Timeout),
        );
        let r = block_on(check(&d, Resolver::Second, &facts(), &records(), false));
        assert_eq!(r.outcome(), "error");
        // After a PATCH to ses the CNAMEs count.
        let mut f = facts();
        f.transport = "ses".into();
        let r = block_on(check(
            &healthy_dns(),
            Resolver::First,
            &f,
            &records(),
            false,
        ));
        assert!(r.issues.iter().any(|i| i.code == "dkim_missing"), "{r:?}");
    }

    #[test]
    fn provider_records_classified() {
        let recs = vec![
            Record {
                id: "1".into(),
                rtype: "MX".into(),
                name: "a.example".into(),
                content: "route1.mx.cloudflare.net".into(),
                priority: Some(13),
            },
            Record {
                id: "2".into(),
                rtype: "TXT".into(),
                name: "a.example".into(),
                content: "v=spf1 include:_spf.mx.cloudflare.net ~all".into(),
                priority: None,
            },
            Record {
                id: "3".into(),
                rtype: "MX".into(),
                name: "cf-bounce.a.example".into(),
                content: "bounce.mx.cloudflare.net".into(),
                priority: Some(10),
            },
        ];
        let out = from_provider("a.example", "sending", Some("cf-bounce.a.example"), &recs);
        let p: Vec<&str> = out.iter().map(|r| r.purpose.as_str()).collect();
        assert_eq!(p, ["mx", "spf", "return_path"]);
        assert_eq!(out[2].json()["host"], "cf-bounce");
    }
}
