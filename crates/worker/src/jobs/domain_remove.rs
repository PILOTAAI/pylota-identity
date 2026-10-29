//! The `domain_remove` job ([Identities and domains › Domain removal]): undoes onboarding from the
//! domain's recorded provider objects only, never from an object found by name (FR-DOM-13). Each step
//! is idempotent and reads absence back rather than inferring it from an error code (C10); a zone that
//! is gone from the account marks the remaining zone-bound steps done (H11). Tenant erasure runs the
//! same steps inline through [`remove`].
//!
//! [Identities and domains › Domain removal]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::{Clock as _, ControlDb as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Ctx, Slice, StepErr, StepResult};
use crate::db::domains as ddb;
use crate::domains::cloudflare_api::{Cf, CfError};
use crate::domains::monitor::{self, DomainRequest};
use crate::domains::ses_api::{Ses, SesError};
use crate::domains::ses_control::SesCaller;

pub const STEPS: &[&str] = &[
    "delete_rules",
    "disable_catch_all",
    "disable_routing",
    "disable_sending",
    "delete_subscription",
    "delete_ses_identity",
    "prune_retired_rules",
    "delete_ownership_record",
    "delete_zone",
    "finish",
];

/// Steps that act on the zone, skipped once it is gone.
const ZONE_BOUND: &[&str] = &[
    "delete_rules",
    "disable_catch_all",
    "disable_routing",
    "disable_sending",
    "delete_ownership_record",
    "delete_zone",
];

/// What a step decided.
enum Outcome {
    Done,
    ZoneGone,
}

fn err(code: &str) -> StepErr {
    StepErr::new(code)
}

/// Whether the zone this row names is gone from the account: no zone of its name, or one with
/// another ID.
async fn zone_gone(cf: &Cf<'_, impl Platform>, row: &ddb::Row) -> Result<bool, StepErr> {
    let Some(zone_id) = row.zone_id.as_deref() else {
        return Ok(true);
    };
    let z = cf
        .find_zone(&row.name)
        .await
        .map_err(|_| err("cloudflare_unavailable"))?;
    Ok(z.is_none_or(|z| z.id != zone_id))
}

/// A Cloudflare failure: transient ones retry; another `4xx` checks whether the zone is gone.
async fn on_cf(cf: &Cf<'_, impl Platform>, row: &ddb::Row, e: CfError) -> Result<Outcome, StepErr> {
    if e.transient() {
        return Err(err("cloudflare_unavailable"));
    }
    if zone_gone(cf, row).await? {
        return Ok(Outcome::ZoneGone);
    }
    Err(err("cloudflare_refused"))
}

#[derive(Deserialize)]
struct Rule {
    zone_id: String,
    routing_rule_id: String,
}

/// One step for one domain.
async fn step<P: Platform>(
    p: &P,
    row: &ddb::Row,
    name: &str,
    params: &Value,
    now: i64,
) -> Result<Outcome, StepErr> {
    let o: Value = serde_json::from_str(&row.provider_objects_json).unwrap_or_else(|_| json!({}));
    let zone = row.zone_id.clone().unwrap_or_default();
    let cf = Cf::new(p);
    match name {
        "delete_rules" => {
            let Some(cf) = cf else {
                return Ok(Outcome::Done);
            };
            let mut rules: Vec<(String, String)> = p
                .db()
                .query::<Rule>(Stmt::new(
                    "SELECT COALESCE(d.zone_id, '') AS zone_id, a.routing_rule_id FROM addresses a JOIN domains d ON d.id = a.domain_id WHERE a.domain_id = ?1 AND a.routing_rule_id IS NOT NULL",
                    vec![row.id.clone().into()],
                ))
                .await?
                .into_iter()
                .map(|r| (r.zone_id, r.routing_rule_id))
                .collect();
            for r in params["pending_rules"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                if let (Some(z), Some(id)) = (r[0].as_str(), r[1].as_str()) {
                    rules.push((z.to_owned(), id.to_owned()));
                }
            }
            for (z, id) in rules {
                let z = if z.is_empty() { zone.clone() } else { z };
                if let Err(e) = cf.delete_rule(&z, &id).await {
                    match cf.rule_exists(&z, &id).await {
                        Ok(false) => {}
                        Ok(true) => return Err(err("routing_rule_delete_failed")),
                        Err(_) => {
                            if let Outcome::ZoneGone = on_cf(&cf, row, e).await? {
                                return Ok(Outcome::ZoneGone);
                            }
                        }
                    }
                }
            }
            Ok(Outcome::Done)
        }
        "disable_catch_all" => {
            let (Some(cf), true) = (cf, o["catch_all"]["created"] == true) else {
                return Ok(Outcome::Done);
            };
            match cf.put_catch_all(&zone, false).await {
                Ok(()) => Ok(Outcome::Done),
                Err(e) => on_cf(&cf, row, e).await,
            }
        }
        "disable_routing" => {
            let (Some(cf), true) = (cf, o["routing"]["created"] == true) else {
                return Ok(Outcome::Done);
            };
            if o["zone"]["created"] == true {
                return Ok(Outcome::Done);
            }
            // The zone-wide disable only for the zone's last routing domain, at its apex (H11).
            #[derive(Deserialize)]
            struct N {
                n: i64,
            }
            let others = p
                .db()
                .first::<N>(Stmt::new(
                    "SELECT COUNT(*) AS n FROM domains WHERE zone_id = ?1 AND id <> ?2 AND state NOT IN ('removed') AND inbound = 'routing'",
                    vec![zone.clone().into(), row.id.clone().into()],
                ))
                .await?
                .map_or(0, |n| n.n);
            let r = if row.is_apex == 1 && others == 0 {
                cf.disable_zone_routing(&zone).await
            } else {
                let unlocked = cf.unlock_routing(&zone, &row.name).await;
                if let Err(e) = unlocked {
                    return on_cf(&cf, row, e).await;
                }
                let mut out = Ok(());
                for id in o["routing"]["record_ids"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                {
                    if let Some(id) = id.as_str()
                        && let Err(e) = cf.delete_record(&zone, id).await
                    {
                        match cf.record(&zone, id).await {
                            Ok(None) => {}
                            _ => out = Err(e),
                        }
                    }
                }
                out
            };
            match r {
                Ok(()) => Ok(Outcome::Done),
                Err(e) => on_cf(&cf, row, e).await,
            }
        }
        "disable_sending" => {
            let Some(cf) = cf else {
                return Ok(Outcome::Done);
            };
            let Some(tag) = o["sending"]["tag"]
                .as_str()
                .filter(|_| o["sending"]["created"] == true)
            else {
                return Ok(Outcome::Done);
            };
            delete_sending_suppressions(p, &cf, &row.name).await?;
            match cf.delete_sending(&zone, tag).await {
                Ok(()) => Ok(Outcome::Done),
                Err(e) => {
                    let still = cf
                        .sending_domains(&zone)
                        .await
                        .map(|l| l.iter().any(|(_, t)| t == tag));
                    match still {
                        Ok(false) => Ok(Outcome::Done),
                        _ => on_cf(&cf, row, e).await,
                    }
                }
            }
        }
        "delete_subscription" => {
            let (Some(cf), Some(id)) = (cf, row.event_subscription_id.as_deref()) else {
                return Ok(Outcome::Done);
            };
            match cf.unsubscribe(id).await {
                Ok(()) => Ok(Outcome::Done),
                Err(e) if !e.transient() => Ok(Outcome::Done),
                Err(_) => Err(err("cloudflare_unavailable")),
            }
        }
        "delete_ses_identity" => {
            if o["ses_identity"]["created"] != true {
                return Ok(Outcome::Done);
            }
            if let Some(ses) = Ses::new(p, SesCaller::Removal) {
                match ses.delete_identity(&row.name).await {
                    Ok(()) | Err(SesError::NotFound) => {}
                    Err(_) => return Err(err("ses_unavailable")),
                }
            }
            if let Some(cf) = cf {
                for id in o["ses_identity"]["dkim_record_ids"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                {
                    if let Some(id) = id.as_str()
                        && cf.delete_record(&zone, id).await.is_err()
                        && !matches!(cf.record(&zone, id).await, Ok(None))
                        && !zone_gone(&cf, row).await?
                    {
                        return Err(err("dns_record_delete_failed"));
                    }
                }
            }
            Ok(Outcome::Done)
        }
        "prune_retired_rules" => {
            super::erasure::prune_retired_rules(p, &row.id).await?;
            Ok(Outcome::Done)
        }
        "delete_ownership_record" => {
            let Some(cf) = cf else {
                return Ok(Outcome::Done);
            };
            for id in o["dns_records"].as_array().cloned().unwrap_or_default() {
                if let Some(id) = id.as_str()
                    && let Err(e) = cf.delete_record(&zone, id).await
                    && !matches!(cf.record(&zone, id).await, Ok(None))
                {
                    return on_cf(&cf, row, e).await;
                }
            }
            Ok(Outcome::Done)
        }
        "delete_zone" => {
            // A zone found through cloudflare_zone belongs to the account owner and is never deleted.
            if o["zone"]["created"] == true {
                p.db()
                    .execute(Stmt::new(
                        "DELETE FROM zone_claims WHERE zone_id = ?1 AND tenant_id = ?2 AND domain_id = ?3",
                        vec![zone.clone().into(), row.tenant_id.clone().into(), row.id.clone().into()],
                    ))
                    .await?;
            }
            p.db()
                .execute(Stmt::new(
                    "DELETE FROM zone_claims WHERE domain_id = ?1",
                    vec![row.id.clone().into()],
                ))
                .await?;
            Ok(Outcome::Done)
        }
        "finish" => {
            let reason = params["reason"].as_str().unwrap_or("requested").to_owned();
            if !row.monitor_do_id.is_empty() {
                let _ = monitor::call(
                    p,
                    &row.monitor_do_id,
                    DomainRequest::JobCompleted {
                        domain_id: row.id.clone(),
                        reason,
                    },
                )
                .await;
            }
            p.db()
                .execute(Stmt::new(
                    "UPDATE domains SET state = 'removed', state_changed_at = CASE WHEN state = 'removed' THEN state_changed_at ELSE ?2 END, updated_at = ?2 WHERE id = ?1",
                    vec![row.id.clone().into(), now.into()],
                ))
                .await?;
            if !row.monitor_do_id.is_empty() {
                monitor::call(
                    p,
                    &row.monitor_do_id,
                    DomainRequest::Retire {
                        domain_id: row.id.clone(),
                    },
                )
                .await
                .map_err(|_| err("monitor_unavailable"))?;
            }
            Ok(Outcome::Done)
        }
        _ => Err(err("unknown_step")),
    }
}

/// The provider's suppressions scoped to the sending domain hold clear addresses: every one that is
/// not `read_only` goes (I12).
async fn delete_sending_suppressions<P: Platform>(
    p: &P,
    cf: &Cf<'_, P>,
    domain: &str,
) -> Result<(), StepErr> {
    let Some(account) = p.config().cf_account_id.clone() else {
        return Ok(());
    };
    let list = cf
        .call(
            "GET",
            &format!("/accounts/{account}/email/sending/suppressions?scope_type=sending_domain&scope_value={domain}"),
            None,
        )
        .await
        .map_err(|_| err("cloudflare_unavailable"))?;
    for item in list.as_array().cloned().unwrap_or_default() {
        let d = item["domain"]
            .as_str()
            .or(item["sending_domain"].as_str())
            .unwrap_or("");
        if item["read_only"] == true
            || item["scope"] == "account"
            || !d.eq_ignore_ascii_case(domain)
        {
            continue;
        }
        if let Some(id) = item["id"].as_str() {
            match cf
                .call(
                    "DELETE",
                    &format!("/accounts/{account}/email/sending/suppressions/{id}"),
                    None,
                )
                .await
            {
                Ok(_) => {}
                Err(e) if e.status == 404 => {}
                Err(_) => return Err(err("cloudflare_unavailable")),
            }
        }
    }
    Ok(())
}

async fn row_of<P: Platform>(p: &P, id: &str) -> Result<ddb::Row, StepErr> {
    p.db()
        .first::<ddb::Row>(ddb::get(id))
        .await?
        .ok_or_else(|| err("domain_not_found"))
}

pub async fn run<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    name: &str,
    _cursor: Option<&str>,
) -> StepResult {
    let row = row_of(c.p, c.params["domain_id"].as_str().unwrap_or("")).await?;
    if c.get("zone_gone")?.is_some() && ZONE_BOUND.contains(&name) {
        return Ok(Slice::Done {
            counts: json!({ "skipped_zone_gone": 1 }),
        });
    }
    match step(c.p, &row, name, &c.params, c.now).await? {
        Outcome::Done => Ok(Slice::done()),
        Outcome::ZoneGone => {
            c.set("zone_gone", "1")?;
            Ok(Slice::Done {
                counts: json!({ "skipped_zone_gone": 1 }),
            })
        }
    }
}

/// Every step for one domain, inline (tenant erasure's `remove_domains`).
pub async fn remove<P: Platform>(p: &P, row: &ddb::Row, now: i64) -> Result<(), StepErr> {
    let params = json!({ "domain_id": row.id, "reason": "tenant_erased" });
    let mut gone = false;
    for name in STEPS {
        if gone && ZONE_BOUND.contains(name) {
            continue;
        }
        let fresh = row_of(p, &row.id).await?;
        if let Outcome::ZoneGone = step(p, &fresh, name, &params, now).await? {
            gone = true;
        }
    }
    Ok(())
}

/// The daily restart of failed removals (C3): the `*/15` run whose UTC hour is 04 and minute below
/// 15 starts a new job for every `removing` domain whose last job is `failed`.
pub async fn restart_failed<P: Platform>(p: &P) {
    #[derive(Deserialize)]
    struct D {
        id: String,
        tenant_id: Option<String>,
        state_reason: Option<String>,
    }
    let Ok(rows) = p
        .db()
        .query::<D>(Stmt::new(
            "SELECT d.id, d.tenant_id, d.state_reason FROM domains d WHERE d.state = 'removing' AND (SELECT j.status FROM jobs j WHERE j.kind = 'domain_remove' AND json_extract(j.params_json, '$.domain_id') = d.id ORDER BY j.created_at DESC LIMIT 1) = 'failed' LIMIT 50",
            vec![],
        ))
        .await
    else {
        return;
    };
    let now = p.clock().now_ms();
    for d in rows {
        let reason = d.state_reason.unwrap_or_else(|| "requested".into());
        let Ok(stmt) =
            crate::domains::remove_job(p, &d.id, d.tenant_id.as_deref(), &reason, &[], now)
        else {
            continue;
        };
        if p.db().execute(stmt).await.is_ok() {
            start_for(p, &d.id).await;
        }
    }
}

/// The domain's last failed removal job other than `except`: a new job resumes from its first step
/// not done (H12).
pub async fn resume_of<P: Platform>(p: &P, domain_id: &str, except: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct I {
        id: String,
    }
    p.db()
        .first::<I>(Stmt::new(
            "SELECT id FROM jobs WHERE kind = 'domain_remove' AND status = 'failed' AND id <> ?2 AND json_extract(params_json, '$.domain_id') = ?1 ORDER BY created_at DESC LIMIT 1",
            vec![domain_id.into(), except.into()],
        ))
        .await
        .ok()
        .flatten()
        .map(|i| i.id)
}

/// Starts the domain's queued removal jobs now (the rows M13's monitor and eviction write); the
/// every-minute cron repairs a start that was lost.
pub async fn start_for<P: Platform>(p: &P, domain_id: &str) {
    #[derive(Deserialize)]
    struct J {
        id: String,
        tenant_id: Option<String>,
        runner_do_id: String,
        params_json: String,
    }
    let Ok(rows) = p
        .db()
        .query::<J>(Stmt::new(
            "SELECT id, tenant_id, runner_do_id, params_json FROM jobs WHERE kind = 'domain_remove' AND status = 'queued' AND json_extract(params_json, '$.domain_id') = ?1",
            vec![domain_id.into()],
        ))
        .await
    else {
        return;
    };
    for j in rows {
        let resume = resume_of(p, domain_id, &j.id).await;
        super::start(
            p,
            &j.runner_do_id,
            super::JobRequest::Start {
                job_id: j.id,
                kind: "domain_remove".into(),
                tenant_id: j.tenant_id.clone(),
                params: serde_json::from_str(&j.params_json).unwrap_or_else(|_| json!({})),
                resume_job_id: resume,
            },
            j.tenant_id.as_deref(),
        )
        .await;
    }
}
