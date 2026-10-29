//! Adding a `cloudflare_zone` domain ([Identities and domains › Adding a domain, Kind `zone`]): the
//! common checks, the onboarding journal, then steps 1–9 against the Cloudflare API. Every step reads
//! first and creates only what is missing, and reuses an object only when the journal lists it, so a
//! failed request can be repeated; an object found that this deployment did not create is refused for
//! a tenant or partner key and adopted (`created: false`) for a platform key (H10, FR-DOM-13).
//!
//! [Identities and domains › Adding a domain, Kind `zone`]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::normalise_domain;
use pylota_identity_core::dns::{self, Answer, RrType, SpfPreflight};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::dns::{Dns as _, RType, Resolver, unquote_txt};
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{ControlDb as _, Ids as _, ObjectClient as _, Platform};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::cloudflare_api::{Cf, CfError};
use super::records::{self, Rec};
use super::ses_api::{Ses, SesError, dkim};
use super::ses_control::SesCaller;
use super::{Grant, check_name, check_zone, claims, deployment_zones, domain_exists, listed_zones};
use crate::auth::Level;
use crate::ctx::Ctx;
use crate::db::{audit, domains as ddb, tenants as tdb};
use crate::errors::ApiError;

/// A tenant's never-verified domains, onboarding rows included (H14).
pub const UNVERIFIED_LIMIT: i64 = 5;
/// Mail domains per Cloudflare zone, routing and sending together (H15).
pub const ZONE_DOMAIN_LIMIT: usize = 30;
/// The subscription create answers that mean the API or the token cannot do it (the S9 fallback).
const MANUAL: [u16; 5] = [401, 403, 404, 405, 501];

/// The request, after defaults.
#[derive(Clone, Debug)]
pub struct Add {
    pub name: String,
    pub receiving: bool,
    pub sending: bool,
    pub replace_mx: bool,
    pub claim: bool,
}

fn upstream(step: &str) -> ApiError {
    ApiError::new(
        ErrorCode::UpstreamError,
        "A provider call failed; retry the request.",
    )
    .details(json!({ "step": step }))
}

fn existing(platform: bool, api_host: &str, tenant_id: &str, name: &str) -> Result<bool, ApiError> {
    if platform {
        Ok(true)
    } else {
        Err(domain_exists(api_host, tenant_id, name))
    }
}

#[derive(Deserialize)]
struct Holder {
    id: String,
    tenant_id: Option<String>,
    ownership_verified_at: Option<i64>,
    state: String,
}

#[derive(Deserialize)]
struct Journal {
    tenant_id: String,
    domain_id: String,
    objects_json: String,
}

#[derive(Deserialize)]
struct N {
    n: i64,
}

/// The journal of this add, its objects as a JSON map.
struct Onboarding<'a, P: Platform> {
    p: &'a P,
    name: String,
    domain_id: String,
    objects: Map<String, Value>,
}

impl<P: Platform> Onboarding<'_, P> {
    /// Appends what a create call returned, before the next call.
    async fn record(&mut self, key: &str, v: Value, zone_id: Option<&str>) -> Result<(), ApiError> {
        self.objects.insert(key.to_owned(), v);
        self.save(zone_id).await
    }

    async fn save(&self, zone_id: Option<&str>) -> Result<(), ApiError> {
        self.p
            .db()
            .execute(Stmt::new(
                "UPDATE domain_onboarding SET objects_json = ?2, zone_id = COALESCE(?3, zone_id), updated_at = ?4 WHERE name = ?1",
                vec![
                    self.name.clone().into(),
                    Value::Object(self.objects.clone()).to_string().into(),
                    zone_id.map(str::to_owned).into(),
                    pylota_identity_platform::Clock::now_ms(self.p.clock()).into(),
                ],
            ))
            .await?;
        Ok(())
    }

    fn has(&self, key: &str) -> bool {
        self.objects.contains_key(key)
    }

    /// Only the provider objects (the token is stored with the row).
    fn provider_objects(&self) -> Value {
        let mut m = self.objects.clone();
        m.remove("ownership_token");
        m.remove("subscription");
        Value::Object(m)
    }
}

/// MX hosts at `name` on both resolvers; `Err` when a resolver fails.
async fn mx_hosts<P: Platform>(p: &P, name: &str) -> Result<Vec<String>, ApiError> {
    let mut out = Vec::new();
    for r in [Resolver::First, Resolver::Second] {
        let a = p
            .dns()
            .query(r, name, RType::Mx)
            .await
            .map_err(|_| upstream("dns"))?;
        for x in a.records.into_iter().filter(|x| x.rtype == RType::Mx) {
            let h = x.data.split_whitespace().last().unwrap_or("");
            let h = h.trim_end_matches('.').to_ascii_lowercase();
            if !h.is_empty() && !out.contains(&h) {
                out.push(h);
            }
        }
    }
    Ok(out)
}

/// The H2 preflight: the routing include merged into the SPF record already at `name`.
async fn spf_preflight<P: Platform>(p: &P, name: &str, include: &str) -> Result<(), ApiError> {
    let txts = match p.dns().query(Resolver::First, name, RType::Txt).await {
        Ok(a) => a
            .records
            .into_iter()
            .filter(|x| x.rtype == RType::Txt)
            .map(|x| unquote_txt(&x.data))
            .collect::<Vec<_>>(),
        Err(_) => return Err(upstream("dns")),
    };
    let Some(existing) = txts.iter().find(|t| dns::is_spf(t)) else {
        return Ok(());
    };
    let mut answers: dns::Answers = std::collections::HashMap::new();
    let mut todo: Vec<String> = existing
        .split_whitespace()
        .chain(std::iter::once(format!("include:{include}").as_str()).map(|_| ""))
        .filter_map(|t| {
            let t = t.trim_start_matches(['+', '-', '~', '?']);
            t.strip_prefix("include:")
                .or_else(|| t.strip_prefix("redirect="))
                .map(str::to_ascii_lowercase)
        })
        .collect();
    todo.push(include.to_owned());
    let mut fetched = 0;
    while let Some(t) = todo.pop() {
        if fetched >= 24 || answers.contains_key(&(t.clone(), RrType::Txt)) {
            continue;
        }
        fetched += 1;
        let a = match p.dns().query(Resolver::First, &t, RType::Txt).await {
            Ok(a) => {
                let v: Vec<String> = a
                    .records
                    .into_iter()
                    .filter(|x| x.rtype == RType::Txt)
                    .map(|x| unquote_txt(&x.data))
                    .collect();
                for s in v.iter().filter(|s| dns::is_spf(s)) {
                    for tok in s.split_whitespace() {
                        let tok = tok.trim_start_matches(['+', '-', '~', '?']);
                        if let Some(n) = tok
                            .strip_prefix("include:")
                            .or_else(|| tok.strip_prefix("redirect="))
                        {
                            todo.push(n.to_ascii_lowercase());
                        }
                    }
                }
                if v.is_empty() {
                    Answer::Empty
                } else {
                    Answer::Records(v)
                }
            }
            Err(_) => Answer::Error,
        };
        answers.insert((t, RrType::Txt), a);
    }
    match dns::preflight_spf(name, existing, include, &answers) {
        Err(SpfPreflight::LookupLimit { lookups, .. }) => Err(ApiError::new(
            ErrorCode::SpfLookupLimit,
            "Merging the routing SPF include would need more than 10 DNS lookups.",
        )
        .fix("Flatten some includes of the existing SPF record, then repeat the request.")
        .details(json!({ "lookups": lookups }))),
        _ => Ok(()),
    }
}

/// `POST /v1/tenants/{tenant_id}/domains` for `cloudflare_zone`: the domain row on success.
pub async fn add<P: Platform>(c: &Ctx<P>, t: &tdb::Row, req: Add) -> Result<ddb::Row, ApiError> {
    let p = c.p.as_ref();
    let key = c.key();
    let platform = key.level == Level::Platform;
    let api_host = p.config().api_host.clone();
    let now = c.now;
    let name = normalise_domain(&req.name)
        .filter(|n| n.contains('.') && n.len() <= 253)
        .ok_or_else(|| ApiError::invalid("name", "A DNS name, such as mail.example.com."))?;
    // Zone permission by name.
    let policy = crate::policy::effective(p, t);
    let claims = claims(p).await?;
    let pdomain: Option<ddb::Row> = p.db().first(ddb::platform()).await?;
    let pname = pdomain.map(|d| d.name).unwrap_or_default();
    let grant = if platform {
        Grant::Platform
    } else {
        check_name(
            &name,
            &t.id,
            &deployment_zones(p, &pname),
            &claims,
            &listed_zones(&policy),
            req.replace_mx,
        )?
    };
    // Name taken.
    let holder: Option<Holder> = p
        .db()
        .first(Stmt::new(
            "SELECT id, tenant_id, ownership_verified_at, state FROM domains WHERE name = ?1 AND state <> 'removed'",
            vec![name.clone().into()],
        ))
        .await?;
    if let Some(h) = &holder {
        if h.tenant_id.as_deref() == Some(t.id.as_str()) {
            return Err(ApiError::new(
                ErrorCode::DomainExists,
                "This tenant already has this domain.",
            )
            .details(json!({ "domain_id": h.id })));
        }
        if req.claim
            && h.ownership_verified_at.is_none()
            && matches!(h.state.as_str(), "pending" | "verifying")
            && claim_seen(p, &t.id, &name).await
        {
            evict(c, h, &t.id).await?;
            return Err(ApiError::new(
                ErrorCode::DomainClaimPending,
                "The unverified holder is being removed; repeat the request in a minute.",
            )
            .header("retry-after", "60"));
        }
        return Err(domain_exists(&api_host, &t.id, &name));
    }
    // Unverified cap.
    if !platform {
        let n: Option<N> = p
            .db()
            .first(Stmt::new(
                "SELECT (SELECT COUNT(*) FROM domains WHERE tenant_id = ?1 AND state <> 'removed' AND ownership_verified_at IS NULL) + (SELECT COUNT(*) FROM domain_onboarding WHERE tenant_id = ?1 AND name <> ?2) AS n",
                vec![t.id.clone().into(), name.clone().into()],
            ))
            .await?;
        if n.map_or(0, |n| n.n) >= UNVERIFIED_LIMIT {
            return Err(ApiError::new(
                ErrorCode::UnverifiedDomainLimit,
                "This tenant has 5 domains that were never verified.",
            )
            .details(json!({ "limit": UNVERIFIED_LIMIT })));
        }
    }
    let cf = Cf::new(p).ok_or_else(|| {
        ApiError::new(
            ErrorCode::CfTokenRequired,
            "This deployment has no Cloudflare API token for zone domains.",
        )
    })?;
    // The journal.
    let fresh_id = p.ids().new_id(IdPrefix::Domain);
    p.db()
        .execute(Stmt::new(
            "INSERT INTO domain_onboarding (name, domain_id, tenant_id, method, zone_id, objects_json, created_at, updated_at) VALUES (?1, ?2, ?3, 'cloudflare_zone', NULL, '{}', ?4, ?4) ON CONFLICT (name) DO NOTHING",
            vec![name.clone().into(), fresh_id.into(), t.id.clone().into(), now.into()],
        ))
        .await?;
    let j: Journal = p
        .db()
        .first(Stmt::new(
            "SELECT tenant_id, domain_id, objects_json FROM domain_onboarding WHERE name = ?1",
            vec![name.clone().into()],
        ))
        .await?
        .ok_or_else(ApiError::internal)?;
    if j.tenant_id != t.id {
        return Err(domain_exists(&api_host, &t.id, &name));
    }
    let mut o = Onboarding {
        p,
        name: name.clone(),
        domain_id: j.domain_id.clone(),
        objects: serde_json::from_str(&j.objects_json).unwrap_or_default(),
    };
    let out = onboard(c, t, &cf, &mut o, &req, &name, grant, &claims, &policy).await;
    if out.is_err() && o.objects.is_empty() {
        // Refused before anything was created: the journal row is not kept.
        let _ = p
            .db()
            .execute(Stmt::new(
                "DELETE FROM domain_onboarding WHERE name = ?1 AND objects_json = '{}'",
                vec![name.clone().into()],
            ))
            .await;
    }
    out
}

/// Whether `pm-claim={token}` is on both resolvers at `_pylota-identity.{name}`.
async fn claim_seen<P: Platform>(p: &P, tenant_id: &str, name: &str) -> bool {
    let want = format!(
        "pm-claim={}",
        super::claim_token(&p.config().api_host, tenant_id, name)
    );
    for r in [Resolver::First, Resolver::Second] {
        let ok = p
            .dns()
            .query(r, &records::ownership_name(name), RType::Txt)
            .await
            .is_ok_and(|a| {
                a.records
                    .iter()
                    .any(|x| unquote_txt(&x.data).contains(&want))
            });
        if !ok {
            return false;
        }
    }
    true
}

/// A proved claim evicts the unverified holder (H14): `removing`, its pending addresses deleted, a
/// `domain_remove` job, and an audit row naming both tenants.
async fn evict<P: Platform>(c: &Ctx<P>, h: &Holder, claimant: &str) -> Result<(), ApiError> {
    let p = c.p.as_ref();
    let now = c.now;
    let monitor: Option<MonitorId> = p
        .db()
        .first(Stmt::new(
            "SELECT monitor_do_id FROM domains WHERE id = ?1",
            vec![h.id.clone().into()],
        ))
        .await?;
    // The monitor moves the domain to `removing`, deletes its pending addresses and starts the job.
    let v = super::monitor::call(
        p,
        &monitor.map(|m| m.monitor_do_id).unwrap_or_default(),
        super::monitor::DomainRequest::Evict {
            domain_id: h.id.clone(),
        },
    )
    .await?;
    if v["refused"].is_string() {
        return Ok(());
    }
    p.db()
        .execute(audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            h.tenant_id.as_deref(),
            Some(&c.key().key_id),
            "domain.evict",
            "domain",
            &h.id,
            &json!({ "evicted_tenant_id": h.tenant_id, "claimant_tenant_id": claimant }),
            &c.request_id,
            now,
        ))
        .await?;
    Ok(())
}

#[derive(Deserialize)]
struct MonitorId {
    monitor_do_id: String,
}

fn map_zone_error(e: &CfError, step: &str) -> ApiError {
    let _ = e;
    upstream(step)
}

/// Mail domains the zone holds: its sending domains, plus this deployment's routing domains in it that
/// are not among them.
async fn zone_count<P: Platform>(p: &P, cf: &Cf<'_, P>, zone_id: &str) -> Result<usize, ApiError> {
    let sending = cf
        .sending_domains(zone_id)
        .await
        .map_err(|e| map_zone_error(&e, "sending"))?;
    #[derive(Deserialize)]
    struct Name {
        name: String,
    }
    let routing: Vec<Name> = p
        .db()
        .query(Stmt::new(
            "SELECT name FROM domains WHERE zone_id = ?1 AND inbound = 'routing' AND state <> 'removed'",
            vec![zone_id.into()],
        ))
        .await?;
    Ok(sending.len()
        + routing
            .iter()
            .filter(|r| !sending.iter().any(|(n, _)| n == &r.name))
            .count())
}

fn zone_limit() -> ApiError {
    ApiError::new(
        ErrorCode::ZoneDomainLimit,
        "The Cloudflare zone already holds 30 mail domains.",
    )
    .details(json!({ "limit": ZONE_DOMAIN_LIMIT }))
}

#[allow(clippy::too_many_arguments)]
async fn onboard<P: Platform>(
    c: &Ctx<P>,
    t: &tdb::Row,
    cf: &Cf<'_, P>,
    o: &mut Onboarding<'_, P>,
    req: &Add,
    name: &str,
    grant: Grant,
    claims: &[super::Claim],
    policy: &Value,
) -> Result<ddb::Row, ApiError> {
    let p = c.p.as_ref();
    let platform = grant == Grant::Platform;
    let api_host = p.config().api_host.clone();
    let now = c.now;
    // 1. Find the zone, then its permission.
    let zone = cf
        .find_zone(name)
        .await
        .map_err(|e| map_zone_error(&e, "zone"))?
        .ok_or_else(|| ApiError::not_found(ErrorCode::DomainNotFound))?;
    if !platform {
        check_zone(
            name,
            &zone.id,
            &zone.name,
            &t.id,
            claims,
            &listed_zones(policy),
        )?;
    }
    o.save(Some(&zone.id)).await?;
    let apex = zone.name == name;
    let sub = (!apex).then_some(name);
    if !o.has("routing")
        && !o.has("sending")
        && zone_count(p, cf, &zone.id).await? >= ZONE_DOMAIN_LIMIT
    {
        return Err(zone_limit());
    }
    // 2. Existing mail at the name (H5).
    let routing_dns = cf
        .routing_dns(&zone.id, sub)
        .await
        .map_err(|e| map_zone_error(&e, "routing"))?;
    let routing_hosts: Vec<String> = routing_dns
        .iter()
        .filter(|r| r.rtype == "MX")
        .map(|r| r.content.trim_end_matches('.').to_ascii_lowercase())
        .collect();
    let include = routing_dns
        .iter()
        .find(|r| r.rtype == "TXT" && r.content.starts_with("v=spf1"))
        .and_then(|r| {
            r.content
                .split_whitespace()
                .find_map(|t| t.strip_prefix("include:"))
                .map(str::to_owned)
        });
    let mut deleted_mx = Vec::new();
    if req.receiving && !o.has("routing") {
        let foreign: Vec<String> = mx_hosts(p, name)
            .await?
            .into_iter()
            .filter(|h| !routing_hosts.contains(h))
            .collect();
        if !foreign.is_empty() {
            if !req.replace_mx {
                return Err(ApiError::new(
                    ErrorCode::ExistingMx,
                    "The name already receives mail elsewhere.",
                )
                .fix("Existing mail to this name would stop. Repeat with \"replace_mx\": true to replace its MX records.")
                .details(json!({ "mx": foreign })));
            }
            if !matches!(grant, Grant::Platform | Grant::Claimed) {
                return Err(super::zone_not_allowed());
            }
            for r in cf
                .records(&zone.id, name, Some("MX"))
                .await
                .map_err(|e| map_zone_error(&e, "replace_mx"))?
            {
                if !routing_hosts.contains(&r.content.trim_end_matches('.').to_ascii_lowercase()) {
                    cf.delete_record(&zone.id, &r.id)
                        .await
                        .map_err(|e| map_zone_error(&e, "replace_mx"))?;
                    deleted_mx.push(json!({ "content": r.content, "priority": r.priority }));
                }
            }
        }
        // 3. SPF preflight (H2).
        if let Some(inc) = &include {
            spf_preflight(p, name, inc).await?;
        }
    }
    // 4. Ownership record.
    let token = match o.objects.get("ownership_token").and_then(Value::as_str) {
        Some(tk) => tk.to_owned(),
        None => {
            let tk = super::ownership_token(p);
            o.record("ownership_token", json!(tk), None).await?;
            tk
        }
    };
    if !o.has("dns_records") {
        let rec = cf
            .create_record(
                &zone.id,
                "TXT",
                &records::ownership_name(name),
                &format!("pm-verify={token}"),
            )
            .await
            .map_err(|e| map_zone_error(&e, "ownership"))?;
        o.record("dns_records", json!([rec.id]), None).await?;
    }
    // 5. Receiving.
    if req.receiving && !o.has("routing") {
        let at_name = cf
            .records(&zone.id, name, Some("MX"))
            .await
            .map_err(|e| map_zone_error(&e, "routing"))?;
        let on = at_name
            .iter()
            .any(|r| routing_hosts.contains(&r.content.trim_end_matches('.').to_ascii_lowercase()));
        let created = if on {
            existing(platform, &api_host, &t.id, name)?;
            false
        } else {
            cf.enable_routing(&zone.id, sub).await.map_err(|e| {
                if e.transient() {
                    upstream("routing")
                } else {
                    zone_limit_or(&e, "routing")
                }
            })?;
            true
        };
        let mut ids: Vec<String> = Vec::new();
        for (n, ty) in [
            (name.to_owned(), "MX"),
            (name.to_owned(), "TXT"),
            (format!("cf2024-1._domainkey.{name}"), "TXT"),
        ] {
            for r in cf
                .records(&zone.id, &n, Some(ty))
                .await
                .map_err(|e| map_zone_error(&e, "routing"))?
            {
                let ours = routing_dns.iter().any(|x| {
                    x.rtype == r.rtype
                        && x.name.trim_end_matches('.') == r.name
                        && x.content.trim_end_matches('.') == r.content.trim_end_matches('.')
                });
                if ours {
                    ids.push(r.id);
                }
            }
        }
        o.record(
            "routing",
            json!({ "name": name, "created": created, "record_ids": ids }),
            None,
        )
        .await?;
    }
    if req.receiving {
        cf.routing_settings(&zone.id, &json!({ "support_subaddress": true }))
            .await
            .map_err(|e| map_zone_error(&e, "routing_settings"))?;
        if apex && !o.has("catch_all") {
            let ca = cf
                .catch_all(&zone.id)
                .await
                .map_err(|e| map_zone_error(&e, "catch_all"))?;
            let foreign = ca["enabled"] == true && ca["actions"][0]["type"] != "worker";
            let created = !foreign || !existing(platform, &api_host, &t.id, name)?;
            cf.put_catch_all(&zone.id, true)
                .await
                .map_err(|e| map_zone_error(&e, "catch_all"))?;
            o.record("catch_all", json!({ "created": created }), None)
                .await?;
        }
    }
    // 6. Sending.
    let mut tag = o
        .objects
        .get("sending")
        .and_then(|s| s["tag"].as_str())
        .map(str::to_owned);
    if req.sending && tag.is_none() {
        let list = cf
            .sending_domains(&zone.id)
            .await
            .map_err(|e| map_zone_error(&e, "sending"))?;
        let (t_, created) = match list.iter().find(|(n, _)| n == name) {
            Some((_, found)) => {
                existing(platform, &api_host, &t.id, name)?;
                (found.clone(), false)
            }
            None => match cf.create_sending(&zone.id, name).await {
                Ok(tg) => (tg, true),
                Err(e) if e.transient() => return Err(upstream("sending")),
                Err(e) => return Err(zone_limit_or_count(p, cf, &zone.id, &e).await),
            },
        };
        o.record("sending", json!({ "tag": t_, "created": created }), None)
            .await?;
        tag = Some(t_);
    }
    if let Some(tg) = &tag {
        cf.patch_sending(
            &zone.id,
            tg,
            &json!({ "drop_suppressed_recipients": false, "preview_enabled": false }),
        )
        .await
        .map_err(|e| map_zone_error(&e, "sending"))?;
    }
    // 6b. The failover SES identity (optional; never refuses the domain).
    let mut ses_records: Vec<Rec> = Vec::new();
    let ses = Ses::new(p, SesCaller::Request).filter(|_| req.sending && !o.has("ses_identity"));
    if let Some(ses) = ses
        && let Some((tokens, zone_host)) = failover_identity(&ses, name, &o.domain_id).await
    {
        let mut ids = Vec::new();
        for tk in &tokens {
            if let Ok(r) = cf
                .create_record(
                    &zone.id,
                    "CNAME",
                    &format!("{tk}._domainkey.{name}"),
                    &format!("{tk}.{zone_host}"),
                )
                .await
            {
                ids.push(r.id);
            }
        }
        o.record(
            "ses_identity",
            json!({ "name": name, "region": ses.region(), "created": true, "dkim_record_ids": ids, "tokens": tokens, "zone": zone_host }),
            None,
        )
        .await?;
    }
    if let Some(s) = o.objects.get("ses_identity") {
        let tokens: Vec<String> = s["tokens"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        ses_records = records::ses_dkim(name, &tokens, s["zone"].as_str().unwrap_or(""), false);
    }
    // 7. The event subscription (S9 fallback: `manual` when the API or token cannot).
    let mut subscription = o
        .objects
        .get("subscription")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if req.sending && subscription.is_none() && !o.objects.contains_key("subscription_manual") {
        let queue = cf
            .queue_id("pm-delivery-events")
            .await
            .map_err(|e| sub_error(&e))?;
        match queue {
            None => {
                o.record("subscription_manual", json!(true), None).await?;
            }
            Some(q) => match cf.subscribe(&zone.id, name, &q).await {
                Ok(id) => {
                    o.record("subscription", json!(id), None).await?;
                    subscription = Some(id);
                }
                Err(e) if MANUAL.contains(&e.status) => {
                    o.record("subscription_manual", json!(true), None).await?;
                }
                Err(e) => return Err(sub_error(&e)),
            },
        }
    }
    // 8. Read the records back.
    let mut recs: Vec<Rec> = Vec::new();
    if req.receiving {
        recs.extend(records::from_provider(name, "routing", None, &routing_dns));
    }
    if let Some(tg) = &tag {
        let sd = cf
            .sending_dns(&zone.id, tg)
            .await
            .map_err(|e| map_zone_error(&e, "records"))?;
        let rp = format!("cf-bounce.{name}");
        recs.extend(records::from_provider(name, "sending", Some(&rp), &sd));
    }
    recs.extend(ses_records);
    recs.push(records::ownership(name, &token));
    // 9. Insert the row and drop the journal row in one batch.
    let monitor = p
        .objects()
        .new_object_id(DoClass::Domain)
        .map_err(|_| ApiError::internal())?;
    let ses_identity = o.objects.get("ses_identity").map(|_| name.to_owned());
    let ses_region = o
        .objects
        .get("ses_identity")
        .and_then(|s| s["region"].as_str().map(str::to_owned));
    let mut objects = o.provider_objects();
    if let Some(m) = objects.as_object_mut() {
        m.remove("subscription_manual");
        if let Some(s) = m.get_mut("ses_identity").and_then(Value::as_object_mut) {
            s.remove("tokens");
            s.remove("zone");
        }
    }
    let audit_id = p.ids().new_id(IdPrefix::Audit);
    p.db()
        .batch(vec![
            Stmt::new(
                "INSERT INTO domains (id, tenant_id, name, kind, method, zone_id, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_reason, state_changed_at, ownership_token, expected_ns_json, provider_objects_json, event_subscription_id, ses_identity, ses_region, records_json, monitor_do_id, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, 'zone', 'cloudflare_zone', ?4, ?5, ?6, ?7, 'cloudflare', 'subaddress', ?8, ?9, 'pending', NULL, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?10, ?10)",
                vec![
                    o.domain_id.clone().into(),
                    t.id.clone().into(),
                    name.into(),
                    zone.id.clone().into(),
                    i64::from(apex).into(),
                    if apex { "catch_all" } else { "literal" }.into(),
                    if req.receiving { "routing" } else { "none" }.into(),
                    i64::from(req.receiving).into(),
                    i64::from(req.sending).into(),
                    now.into(),
                    token.clone().into(),
                    json!(zone.name_servers).to_string().into(),
                    objects.to_string().into(),
                    subscription.clone().into(),
                    ses_identity.into(),
                    ses_region.into(),
                    serde_json::to_string(&recs).unwrap_or_default().into(),
                    monitor.clone().into(),
                ],
            ),
            Stmt::new(
                "DELETE FROM domain_onboarding WHERE name = ?1",
                vec![name.into()],
            ),
            audit::insert(
                &audit_id,
                Some(&t.id),
                Some(&c.key().key_id),
                "domain.create",
                "domain",
                &o.domain_id,
                &json!({ "name": name, "method": "cloudflare_zone", "deleted_mx": deleted_mx }),
                &c.request_id,
                now,
            ),
        ])
        .await?;
    let row: ddb::Row = p
        .db()
        .first(ddb::get(&o.domain_id))
        .await?
        .ok_or_else(ApiError::internal)?;
    Ok(row)
}

/// A refused create that the Worker cannot classify is the zone's mail-domain limit when the zone is
/// full (H15), else an upstream error.
fn zone_limit_or(e: &CfError, step: &str) -> ApiError {
    let _ = e;
    upstream(step)
}

async fn zone_limit_or_count<P: Platform>(
    p: &P,
    cf: &Cf<'_, P>,
    zone_id: &str,
    e: &CfError,
) -> ApiError {
    match zone_count(p, cf, zone_id).await {
        Ok(n) if n >= ZONE_DOMAIN_LIMIT => zone_limit(),
        _ => zone_limit_or(e, "sending"),
    }
}

fn sub_error(e: &CfError) -> ApiError {
    let _ = e;
    upstream("event_subscription")
}

/// Creates (or reuses, when its tags name this deployment and domain) the failover identity: its DKIM
/// tokens and signing zone, or `None` when the step is skipped.
pub async fn failover_identity<P: Platform>(
    ses: &Ses<'_, P>,
    name: &str,
    domain_id: &str,
) -> Option<(Vec<String>, String)> {
    let identity = match ses.create_identity(name, domain_id).await {
        Ok(v) => v,
        Err(SesError::AlreadyExists) => match ses.get_identity(name).await {
            Ok(v) if ses.ours(&v, domain_id) => v,
            _ => return None,
        },
        Err(_) => return None,
    };
    let (tokens, zone) = dkim(&identity);
    (tokens.len() == 3).then_some((tokens, zone))
}
