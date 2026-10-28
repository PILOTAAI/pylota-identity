//! Domains ([Identities and domains › Domains]): the platform domain is visible to every key with
//! `tenant_id: null`; tenant domains are added with `cloudflare_zone` (the other methods are M23's),
//! verified and watched by their `DomainMonitor`, switched between transports (J5) and removed.
//!
//! [Identities and domains › Domains]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use super::messages::only_fields;
use super::tenant_in_scope;
use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::{audit, domains as ddb};
use crate::domains::monitor::{self, DomainRequest};
use crate::domains::onboard::{self, Add};
use crate::domains::records::{self, Rec};
use crate::domains::ses_api::Ses;
use crate::domains::ses_control::SesCaller;
use crate::errors::ApiError;

fn not_found() -> ApiError {
    ApiError::not_found(ErrorCode::DomainNotFound)
}

/// The domain the key may see (out of scope is the same `404`); `write` refuses identity keys' writes
/// through the route's levels already.
async fn in_scope<P: Platform>(c: &Ctx<P>, write: bool) -> Result<ddb::Row, ApiError> {
    let row: Option<ddb::Row> = c.p.db().first(ddb::get(c.route.param("domain_id"))).await?;
    let row = row.filter(|r| r.state != "removed").ok_or_else(not_found)?;
    if let Some(tid) = &row.tenant_id
        && c.key().level != Level::Platform
    {
        tenant_in_scope(c, tid, write)
            .await
            .map_err(|_| not_found())?;
    } else if row.tenant_id.is_none() && write && c.key().level != Level::Platform {
        return Err(ApiError::scope_denied(None));
    }
    Ok(row)
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    Ok(Reply::json(200, in_scope(c, false).await?.json()))
}

pub async fn list_tenant<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), false).await?;
    let limit = c.limit()?;
    let after = c.after(&[]).await?;
    let rows: Vec<ddb::Row> = c.p.db().query(ddb::of_tenant(&t.id, after, limit)).await?;
    c.page(
        rows,
        limit,
        &[],
        |r| (r.created_at, r.id.clone()),
        ddb::Row::json,
    )
    .await
}

/// `POST /v1/tenants/{tenant_id}/domains`.
pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    let body = c.json()?;
    only_fields(
        &body,
        &[
            "name",
            "method",
            "kind",
            "create_zone",
            "receiving",
            "sending",
            "replace_mx",
            "confirm_dedicated",
            "claim",
            "inbound",
            "smtp",
        ],
    )?;
    let name = body["name"]
        .as_str()
        .ok_or_else(|| ApiError::invalid("name", "A DNS name, such as mail.example.com."))?;
    let method = match (body["method"].as_str(), body["kind"].as_str()) {
        (Some(m), _) => m.to_owned(),
        (None, Some("zone")) if body["create_zone"] == true => "nameservers".into(),
        (None, Some("zone")) => "cloudflare_zone".into(),
        (None, Some("external")) => "send_only".into(),
        _ => {
            return Err(ApiError::invalid(
                "method",
                "One of the connection methods.",
            ));
        }
    };
    match method.as_str() {
        "cloudflare_zone" => {}
        "nameservers" | "delegated_subdomain" | "dns_records" | "send_only" | "smtp_relay" => {
            return Err(ApiError::new(
                ErrorCode::TransportUnavailable,
                "This connection method is not available on this deployment yet.",
            )
            .details(json!({ "reason": "method_not_available", "method": method })));
        }
        _ => {
            return Err(ApiError::invalid(
                "method",
                "One of the connection methods.",
            ));
        }
    }
    let flag = |k: &str, d: bool| body[k].as_bool().unwrap_or(d);
    let req = Add {
        name: name.to_owned(),
        receiving: flag("receiving", true),
        sending: flag("sending", true),
        replace_mx: flag("replace_mx", false),
        claim: flag("claim", false),
    };
    if !req.receiving && !req.sending {
        return Err(ApiError::invalid(
            "receiving",
            "A domain receives, sends, or both.",
        ));
    }
    let row = onboard::add(c, &t, req).await?;
    // Init starts verification; a lost Init is repaired by the monitor's first request.
    let _ = monitor::call(
        c.p.as_ref(),
        &row.monitor_do_id,
        DomainRequest::Init {
            domain_id: row.id.clone(),
        },
    )
    .await;
    let row: ddb::Row = c.p.db().first(ddb::get(&row.id)).await?.unwrap_or(row);
    crate::domains::emit(
        c.p.as_ref(),
        &row,
        "domain.created",
        &crate::domains::domain_data(&row),
        "created",
        c.now,
    )
    .await?;
    Ok(Reply::json(201, row.json()))
}

/// `GET /v1/domains/{id}/records`: re-read from the provider APIs where the Worker can, with the last
/// observed state of each record.
pub async fn records<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c, false).await?;
    let mut recs: Vec<Rec> = records::parse(&row.records_json);
    if let (Some(cf), Some(z)) = (
        crate::domains::cloudflare_api::Cf::new(c.p.as_ref()),
        &row.zone_id,
    ) {
        let objects: Value = serde_json::from_str(&row.provider_objects_json).unwrap_or_default();
        let sub = (row.is_apex == 0).then_some(row.name.as_str());
        let mut fresh: Vec<Rec> = Vec::new();
        let mut ok = true;
        if row.receiving == 1 {
            match cf.routing_dns(z, sub).await {
                Ok(r) => fresh.extend(records::from_provider(&row.name, "routing", None, &r)),
                Err(_) => ok = false,
            }
        }
        if let Some(tag) = objects["sending"]["tag"].as_str() {
            match cf.sending_dns(z, tag).await {
                Ok(r) => fresh.extend(records::from_provider(
                    &row.name,
                    "sending",
                    Some(&format!("cf-bounce.{}", row.name)),
                    &r,
                )),
                Err(_) => ok = false,
            }
        }
        if ok && !fresh.is_empty() {
            fresh.extend(
                recs.iter()
                    .filter(|r| matches!(r.source.as_str(), "ses" | "worker"))
                    .cloned(),
            );
            recs = fresh;
            c.p.db()
                .execute(Stmt::new(
                    "UPDATE domains SET records_json = ?2 WHERE id = ?1",
                    vec![
                        row.id.clone().into(),
                        serde_json::to_string(&recs).unwrap_or_default().into(),
                    ],
                ))
                .await?;
        }
    }
    let statuses = last_statuses(c, &row).await;
    let data: Vec<Value> = recs
        .iter()
        .map(|r| {
            let mut v = r.json();
            if let Some(s) = statuses
                .iter()
                .find(|(n, t, _)| n == &r.name && t == &r.rtype)
                .map(|(_, _, s)| s.clone())
            {
                v["status"] = s.into();
            }
            v
        })
        .collect();
    Ok(Reply::json(
        200,
        json!({ "data": data, "checked_at": pylota_identity_core::days::rfc3339(c.now) }),
    ))
}

/// Each record's state from the monitor's last check on the first resolver.
async fn last_statuses<P: Platform>(c: &Ctx<P>, row: &ddb::Row) -> Vec<(String, String, String)> {
    let Ok(h) = monitor::call(
        c.p.as_ref(),
        &row.monitor_do_id,
        DomainRequest::Health {
            domain_id: row.id.clone(),
        },
    )
    .await
    else {
        return Vec::new();
    };
    h["status"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x[0].as_str()?.to_owned(),
                        x[1].as_str()?.to_owned(),
                        x[2].as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `fallback_active` (Fallback behaviour).
pub async fn fallback_active<P: Platform>(c: &Ctx<P>, row: &ddb::Row) -> bool {
    if row.kind == "platform" {
        return false;
    }
    let policy_on = match &row.tenant_id {
        Some(tid) => match c
            .p
            .db()
            .first::<crate::db::tenants::Row>(crate::db::tenants::get(tid))
            .await
        {
            Ok(Some(t)) => {
                pylota_identity_core::policy::get_path(
                    &crate::policy::effective(c.p.as_ref(), &t),
                    "domain_fallback",
                ) != Some(&Value::Bool(false))
            }
            _ => true,
        },
        None => false,
    };
    let manual =
        row.transport == "cloudflare" && row.sending == 1 && row.event_subscription_id.is_none();
    policy_on && (matches!(row.state.as_str(), "failing" | "suspended") || manual)
}

pub async fn health<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c, false).await?;
    let recs = records::parse(&row.records_json);
    let h = monitor::call(
        c.p.as_ref(),
        &row.monitor_do_id,
        DomainRequest::Health {
            domain_id: row.id.clone(),
        },
    )
    .await
    .unwrap_or_else(|_| json!({ "issues": [], "checks": [] }));
    let issues: Vec<Value> = h["issues"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|i| {
            let code = i["code"].as_str().unwrap_or("");
            let rec = i["record"].as_str();
            json!({ "code": code, "record": rec.unwrap_or(&row.name),
                "fix": records::fix(code, &recs, rec).replace("{id}", &row.id) })
        })
        .collect();
    let checks: Vec<Value> = h["checks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|k| {
            json!({ "at": k["at"], "outcome": k["outcome"],
                "resolver": if k["resolver"] == "first" { "cloudflare-doh" } else { "google-doh" } })
        })
        .collect();
    let fallback = fallback_active(c, &row).await;
    Ok(Reply::json(
        200,
        json!({ "state": row.state, "reason": row.state_reason,
            "since": pylota_identity_core::days::rfc3339(row.state_changed_at),
            "issues": issues, "checks": checks, "fallback_active": fallback }),
    ))
}

/// `POST /v1/domains/{id}/verify`: a check now, at most once a minute per domain.
pub async fn verify<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c, true).await?;
    let v = monitor::call(
        c.p.as_ref(),
        &row.monitor_do_id,
        DomainRequest::Verify {
            domain_id: row.id.clone(),
        },
    )
    .await?;
    if let Some(ms) = v["retry_after_ms"].as_i64() {
        let secs = (ms + 999) / 1000;
        return Err(ApiError::new(
            ErrorCode::RateLimited,
            "A verification of this domain ran less than a minute ago.",
        )
        .header("retry-after", secs.to_string())
        .details(json!({ "retry_after": secs })));
    }
    Ok(Reply::json(200, row.json()))
}

/// `POST /v1/domains/{id}/probe`: the alignment probe of `smtp_relay` domains (M23).
pub async fn probe<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c, true).await?;
    Err(ApiError::new(
        ErrorCode::TransportUnavailable,
        "Only smtp_relay domains have an alignment probe.",
    )
    .details(json!({ "reason": "method_not_supported", "method": row.method })))
}

pub async fn reprove<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c, true).await?;
    if row.kind == "platform" {
        if c.key().level != Level::Platform {
            return Err(ApiError::scope_denied(None));
        }
        if crate::domains::cloudflare_api::Cf::new(c.p.as_ref()).is_none() {
            return Err(ApiError::new(
                ErrorCode::CfTokenRequired,
                "Re-proving the platform domain re-reads its zone with the Worker's Cloudflare token.",
            ));
        }
    }
    let v = monitor::call(
        c.p.as_ref(),
        &row.monitor_do_id,
        DomainRequest::Reprove {
            domain_id: row.id.clone(),
        },
    )
    .await?;
    if v["refused"].is_string() {
        return Err(ApiError::new(
            ErrorCode::DomainNotSuspended,
            "Only a suspended domain (or the failing platform domain) is re-proved.",
        )
        .details(json!({ "state": row.state })));
    }
    let row: ddb::Row =
        c.p.db()
            .first(ddb::get(&row.id))
            .await?
            .ok_or_else(not_found)?;
    Ok(Reply::json(200, row.json()))
}

#[derive(Deserialize)]
struct N {
    n: i64,
}

#[derive(Deserialize)]
struct JobStatus {
    status: String,
}

/// `DELETE /v1/domains/{id}`: starts removal (`202`), or restarts a failed one.
pub async fn delete<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c, true).await?;
    if row.kind == "platform" {
        return Err(ApiError::scope_denied(None));
    }
    let live: Option<N> = c
        .p
        .db()
        .first(Stmt::new(
            "SELECT COUNT(*) AS n FROM addresses WHERE domain_id = ?1 AND status IN ('active','retiring')",
            vec![row.id.clone().into()],
        ))
        .await?;
    let job: Option<JobStatus> = c
        .p
        .db()
        .first(Stmt::new(
            "SELECT status FROM jobs WHERE kind = 'domain_remove' AND json_extract(params_json, '$.domain_id') = ?1 ORDER BY created_at DESC LIMIT 1",
            vec![row.id.clone().into()],
        ))
        .await?;
    let v = monitor::call(
        c.p.as_ref(),
        &row.monitor_do_id,
        DomainRequest::Delete {
            domain_id: row.id.clone(),
            live_addresses: live.is_some_and(|n| n.n > 0),
            job_failed: job.is_some_and(|j| j.status == "failed"),
        },
    )
    .await?;
    match v["refused"].as_str() {
        Some("scope_denied") => return Err(ApiError::scope_denied(None)),
        Some("domain_in_use") => {
            return Err(ApiError::new(
                ErrorCode::DomainInUse,
                "Addresses on this domain are active or retiring.",
            )
            .details(json!({ "reason": "addresses" })));
        }
        _ => {}
    }
    c.p.db()
        .execute(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            row.tenant_id.as_deref(),
            Some(&c.key().key_id),
            "domain.delete",
            "domain",
            &row.id,
            &json!({ "name": row.name }),
            &c.request_id,
            c.now,
        ))
        .await?;
    let row: ddb::Row =
        c.p.db()
            .first(ddb::get(&row.id))
            .await?
            .ok_or_else(not_found)?;
    Ok(Reply::json(202, row.json()))
}

/// `PATCH /v1/domains/{id}`: the J5 transport switch and lifting a sending pause (platform keys).
pub async fn update<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c, true).await?;
    let body = c.json()?;
    only_fields(&body, &["transport", "sending_paused", "smtp"])?;
    let platform = c.key().level == Level::Platform;
    if body.get("smtp").is_some_and(|v| !v.is_null()) {
        return Err(ApiError::new(
            ErrorCode::TransportUnavailable,
            "Only smtp_relay domains have relay settings.",
        )
        .details(json!({ "reason": "method_not_supported" })));
    }
    if let Some(paused) = body.get("sending_paused").filter(|v| !v.is_null()) {
        if !platform {
            return Err(ApiError::scope_denied(Some("sending_paused")));
        }
        if paused == false {
            c.p.db()
                .batch(vec![
                    Stmt::new(
                        "UPDATE domains SET sending_paused_at = NULL, updated_at = ?2 WHERE id = ?1",
                        vec![row.id.clone().into(), c.now.into()],
                    ),
                    audit::insert(
                        &c.p.ids().new_id(IdPrefix::Audit),
                        row.tenant_id.as_deref(),
                        Some(&c.key().key_id),
                        "domain.sending_resumed",
                        "domain",
                        &row.id,
                        &json!({}),
                        &c.request_id,
                        c.now,
                    ),
                ])
                .await?;
        }
    }
    if let Some(t) = body.get("transport").filter(|v| !v.is_null()) {
        if !platform {
            return Err(ApiError::scope_denied(Some("transport")));
        }
        let t = t
            .as_str()
            .filter(|t| matches!(*t, "ses" | "cloudflare"))
            .ok_or_else(|| ApiError::invalid("transport", "ses or cloudflare."))?;
        switch_transport(c, &row, t).await?;
    }
    let row: ddb::Row =
        c.p.db()
            .first(ddb::get(&row.id))
            .await?
            .ok_or_else(not_found)?;
    Ok(Reply::json(200, row.json()))
}

fn unavailable(reason: &str) -> ApiError {
    ApiError::new(
        ErrorCode::TransportUnavailable,
        "This domain cannot switch to that transport now.",
    )
    .details(json!({ "reason": reason }))
}

/// J5: only Cloudflare-method domains switch; `ses` needs a verified identity that can sign now (C21).
async fn switch_transport<P: Platform>(
    c: &Ctx<P>,
    row: &ddb::Row,
    to: &str,
) -> Result<(), ApiError> {
    if row.kind == "platform"
        || !matches!(
            row.method.as_str(),
            "cloudflare_zone" | "nameservers" | "delegated_subdomain"
        )
    {
        return Err(unavailable("method_not_supported"));
    }
    if row.transport == to {
        return Ok(());
    }
    if to == "ses" {
        let has_dkim = records::parse(&row.records_json)
            .iter()
            .any(|r| r.source == "ses" && r.purpose == "dkim");
        let (Some(identity), true) = (&row.ses_identity, has_dkim) else {
            return Err(unavailable("ses_identity_missing"));
        };
        let ses = Ses::new(c.p.as_ref(), SesCaller::Request)
            .ok_or_else(|| unavailable("ses_not_configured"))?;
        let v = match ses.get_identity(identity).await {
            Ok(v) => v,
            Err(crate::domains::ses_api::SesError::Busy { retry_after_ms }) => {
                let secs = (retry_after_ms + 999) / 1000;
                return Err(ApiError::new(
                    ErrorCode::UpstreamRateLimited,
                    "The SES control plane is busy; retry shortly.",
                )
                .header("retry-after", secs.to_string())
                .details(json!({ "retry_after": secs })));
            }
            Err(_) => return Err(unavailable("ses_identity_missing")),
        };
        let dkim_status = v["DkimAttributes"]["Status"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        if v["VerifiedForSendingStatus"] != true || dkim_status != "SUCCESS" {
            return Err(ApiError::new(
                ErrorCode::TransportUnavailable,
                "The SES identity cannot sign yet.",
            )
            .details(
                json!({ "reason": "ses_identity_not_verified", "dkim_status": dkim_status }),
            ));
        }
    }
    c.p.db()
        .batch(vec![
            Stmt::new(
                "UPDATE domains SET transport = ?2, updated_at = ?3 WHERE id = ?1",
                vec![row.id.clone().into(), to.into(), c.now.into()],
            ),
            audit::insert(
                &c.p.ids().new_id(IdPrefix::Audit),
                row.tenant_id.as_deref(),
                Some(&c.key().key_id),
                "domain.transport",
                "domain",
                &row.id,
                &json!({ "from": row.transport, "to": to }),
                &c.request_id,
                c.now,
            ),
        ])
        .await?;
    let _ = monitor::call(
        c.p.as_ref(),
        &row.monitor_do_id,
        DomainRequest::Check {
            domain_id: row.id.clone(),
        },
    )
    .await;
    Ok(())
}
