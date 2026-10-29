//! Erasure requests, legal holds and message deletion ([Privacy and erasure § 6.1, § 8]). A request
//! is validated before anything is written, then its `jobs` and `erasure_requests` rows go in one D1
//! batch and the runner is started; a lost start is repaired by the every-minute cron.
//!
//! [Privacy and erasure § 6.1, § 8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde_json::{Value, json};

use super::messages::{call_mailbox, only_fields};
use super::{identity_in_scope, tenant_in_scope};
use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::identities as idb;
use crate::db::jobs::{self as jdb, Erasure};
use crate::db::{audit, tenants as tdb};
use crate::errors::ApiError;
use crate::jobs::{self, JobRequest};
use crate::mailbox::MailboxRequest;
use crate::mailbox::privacy::{PrivacyOp, Target};

/// What a request erases once validated.
struct Plan {
    tenant: tdb::Row,
    scope: &'static str,
    identity: Option<idb::Row>,
    target_id: Option<String>,
    address: Option<String>,
    counterparty_hash: Option<String>,
    reason: String,
}

fn denied(required: &[&str]) -> ApiError {
    ApiError::new(
        ErrorCode::PermissionDenied,
        "This key lacks a permission the request needs.",
    )
    .details(json!({ "required": required }))
}

/// The latest request of the tenant with the same scope and target.
async fn latest<P: Platform>(c: &Ctx<P>, plan: &Plan) -> Result<Option<Erasure>, ApiError> {
    Ok(c.p
        .db()
        .first(Stmt::new(
            "SELECT id, tenant_id, job_id, scope, identity_id, target_id, status, receipt_json, created_by_key_id, resumes_id, deadline_at, created_at, completed_at FROM erasure_requests \
             WHERE tenant_id = ?1 AND scope = ?2 AND COALESCE(identity_id, '') = ?3 AND COALESCE(target_id, '') = ?4 AND COALESCE(counterparty_hash, '') = ?5 \
             ORDER BY created_at DESC, id DESC LIMIT 1",
            vec![
                plan.tenant.id.clone().into(),
                plan.scope.into(),
                plan.identity.as_ref().map_or_else(String::new, |i| i.id.clone()).into(),
                plan.target_id.clone().unwrap_or_default().into(),
                plan.counterparty_hash.clone().unwrap_or_default().into(),
            ],
        ))
        .await?)
}

/// Whether the target exists in the identity's mailbox (held threads count).
async fn target_exists<P: Platform>(
    c: &Ctx<P>,
    idn: &idb::Row,
    target: Target,
) -> Result<bool, ApiError> {
    let out = call_mailbox(
        c,
        idn,
        MailboxRequest::Privacy(PrivacyOp::ErasurePlan {
            target,
            after_rowid: 0,
            limit: 500,
        }),
    )
    .await?;
    Ok(out["messages"].as_array().is_some_and(|m| !m.is_empty())
        || out["held"].as_array().is_some_and(|h| !h.is_empty()))
}

/// `POST /v1/erasure-requests`.
pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let body = c.json()?;
    only_fields(
        &body,
        &[
            "tenant_id",
            "scope",
            "identity_id",
            "message_id",
            "thread_id",
            "counterparty_address",
            "reason",
        ],
    )?;
    let key = c.key().clone();
    let tenant_id = body["tenant_id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| key.tenant_id.clone())
        .ok_or_else(|| ApiError::invalid("tenant_id", "The tenant whose data is erased."))?;
    let reason = body["reason"].as_str().unwrap_or("").to_owned();
    if reason.chars().count() > 500 {
        return Err(ApiError::invalid("reason", "At most 500 characters."));
    }
    let scope: &'static str = match body["scope"].as_str() {
        Some("message") => "message",
        Some("thread") => "thread",
        Some("counterparty") => "counterparty",
        Some("identity") => "identity",
        Some("tenant") => "tenant",
        _ => {
            return Err(ApiError::invalid(
                "scope",
                "message, thread, counterparty, identity or tenant.",
            ));
        }
    };
    if key.level == Level::Identity && matches!(scope, "counterparty" | "tenant") {
        return Err(ApiError::new(
            ErrorCode::ScopeDenied,
            "An identity key erases only within its own identity.",
        ));
    }
    if scope == "tenant" && !key.permissions.iter().any(|p| p == "tenants:erase") {
        return Err(denied(&["tenants:erase"]));
    }
    let t = tenant_in_scope(c, &tenant_id, false).await?;
    let mut plan = Plan {
        tenant: t,
        scope,
        identity: None,
        target_id: None,
        address: None,
        counterparty_hash: None,
        reason: if reason.is_empty() {
            "requested".into()
        } else {
            reason
        },
    };
    match scope {
        "message" | "thread" | "identity" => {
            let id = body["identity_id"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("identity_id", "The identity."))?;
            let (idn, it) = identity_in_scope(c, id, false).await?;
            if it.id != plan.tenant.id {
                return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
            }
            if scope != "identity" {
                let field = if scope == "message" {
                    "message_id"
                } else {
                    "thread_id"
                };
                let tid = body[field]
                    .as_str()
                    .ok_or_else(|| ApiError::invalid(field, "The target."))?
                    .to_owned();
                plan.target_id = Some(tid);
            }
            plan.identity = Some(idn);
        }
        "counterparty" => {
            let raw = body["counterparty_address"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("counterparty_address", "An email address."))?;
            let a = address::parse_envelope(raw)
                .map(|e| e.base())
                .map_err(|_| ApiError::invalid("counterparty_address", "An email address."))?;
            plan.counterparty_hash = Some(address::address_hash(&c.p.secrets().hash_key, &a));
            plan.address = Some(a);
        }
        _ => {}
    }
    submit(c, plan).await
}

/// Resumption, the tenant-scope table of § 6.1, then the rows and the start.
async fn submit<P: Platform>(c: &Ctx<P>, plan: Plan) -> Result<Reply, ApiError> {
    let last = latest(c, &plan).await?;
    let mut resumes: Option<Erasure> = None;
    if plan.scope == "tenant" {
        if plan.tenant.status == "erased" {
            return Err(ApiError::new(
                ErrorCode::TenantErased,
                "This tenant is already erased.",
            ));
        }
        if plan.tenant.status == "erasing" {
            match last {
                Some(l) if matches!(l.status.as_str(), "queued" | "running") => {
                    return Ok(Reply::json(200, l.json()));
                }
                Some(l) if matches!(l.status.as_str(), "failed" | "completed_with_holds") => {
                    resumes = Some(l);
                }
                _ => {}
            }
        }
    } else {
        match last {
            Some(l) if l.status == "failed" => resumes = Some(l),
            // A resumption already queued or running answers a request that races it.
            Some(l)
                if l.resumes_id.is_some() && matches!(l.status.as_str(), "queued" | "running") =>
            {
                return Ok(Reply::json(200, l.json()));
            }
            _ => {}
        }
    }
    // Without a resumption the target must exist now (a resumed one may already be gone).
    if resumes.is_none()
        && let (Some(idn), Some(tid)) = (&plan.identity, &plan.target_id)
    {
        let target = if plan.scope == "message" {
            Target::Message { id: tid.clone() }
        } else {
            Target::Thread { id: tid.clone() }
        };
        if idn.mailbox_do_id.is_empty() || !target_exists(c, idn, target).await? {
            return Err(ApiError::not_found(if plan.scope == "message" {
                ErrorCode::MessageNotFound
            } else {
                ErrorCode::ThreadNotFound
            }));
        }
    }
    if resumes.is_none()
        && plan.scope == "identity"
        && plan
            .identity
            .as_ref()
            .is_some_and(|i| i.status == "deleted")
    {
        return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
    }
    create_request(c, plan, resumes, json!({}), None).await
}

/// Writes the job and the request in one batch and starts the runner.
async fn create_request<P: Platform>(
    c: &Ctx<P>,
    plan: Plan,
    resumes: Option<Erasure>,
    extra: Value,
    deadline: Option<i64>,
) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    let now = c.now;
    let ids = c.p.ids();
    let era = ids.new_id(IdPrefix::Erasure);
    let job = jobs::new_job(c.p.as_ref())?;
    let mut params = json!({
        "scope": plan.scope, "erasure_request_id": era,
        "identity_id": plan.identity.as_ref().map(|i| i.id.clone()),
        "target_id": plan.target_id, "counterparty_hash": plan.counterparty_hash,
    });
    if let (Some(o), Some(e)) = (params.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    let deadline = deadline.unwrap_or_else(|| {
        resumes
            .as_ref()
            .map_or(now + jdb::ERASURE_DEADLINE_MS, |l| l.deadline_at)
    });
    let batch = vec![
        jdb::insert_job(
            &job.id,
            &plan.tenant.id,
            "erasure",
            &job.runner_do_id,
            &params,
            &key.key_id,
            now,
        ),
        Stmt::new(
            "INSERT INTO erasure_requests (id, tenant_id, job_id, scope, identity_id, target_id, counterparty_hash, reason, status, created_by_key_id, resumes_id, deadline_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9, ?10, ?11, ?12)",
            vec![
                era.clone().into(),
                plan.tenant.id.clone().into(),
                job.id.clone().into(),
                plan.scope.into(),
                plan.identity.as_ref().map(|i| i.id.clone()).into(),
                plan.target_id.clone().into(),
                plan.counterparty_hash.clone().into(),
                plan.reason.clone().into(),
                key.key_id.clone().into(),
                resumes.as_ref().map(|l| l.id.clone()).into(),
                deadline.into(),
                now.into(),
            ],
        ),
        audit::insert(
            &ids.new_id(IdPrefix::Audit),
            Some(&plan.tenant.id),
            Some(&key.key_id),
            "erasure.create",
            "erasure_request",
            &era,
            &json!({ "scope": plan.scope, "resumes_id": resumes.as_ref().map(|l| l.id.clone()) }),
            &c.request_id,
            now,
        ),
    ];
    if let Err(e) = c.p.db().batch(batch).await {
        // Two resumptions raced: the partial unique index on resumes_id kept the first.
        if let Some(l) = &resumes
            && let Some(first) = c
                .p
                .db()
                .first::<Erasure>(Stmt::new(
                    "SELECT id, tenant_id, job_id, scope, identity_id, target_id, status, receipt_json, created_by_key_id, resumes_id, deadline_at, created_at, completed_at FROM erasure_requests WHERE resumes_id = ?1",
                    vec![l.id.clone().into()],
                ))
                .await?
        {
            return Ok(Reply::json(200, first.json()));
        }
        return Err(e.into());
    }
    let mut start_params = params.clone();
    if let Some(a) = &plan.address {
        start_params["address"] = a.clone().into();
    }
    jobs::start(
        c.p.as_ref(),
        &job.runner_do_id,
        JobRequest::Start {
            job_id: job.id.clone(),
            kind: "erasure".into(),
            tenant_id: Some(plan.tenant.id.clone()),
            params: start_params,
            resume_job_id: resumes.as_ref().map(|l| l.job_id.clone()),
        },
        Some(&plan.tenant.id),
    )
    .await;
    let e: Option<Erasure> = c.p.db().first(jdb::get_erasure(&era)).await?;
    Ok(Reply::json(202, e.ok_or_else(ApiError::internal)?.json()))
}

/// Whether the key sees the request's tenant (reads keep working on an erasing or erased tenant).
async fn visible<P: Platform>(c: &Ctx<P>, e: &Erasure) -> Result<bool, ApiError> {
    if tenant_in_scope(c, &e.tenant_id, false).await.is_err() {
        return Ok(false);
    }
    let key = c.key();
    Ok(key.level != Level::Identity || e.identity_id.is_some() && e.identity_id == key.identity_id)
}

/// `GET /v1/erasure-requests/{erasure_id}`.
pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let e: Option<Erasure> =
        c.p.db()
            .first(jdb::get_erasure(c.route.param("erasure_id")))
            .await?;
    match e {
        Some(e) if visible(c, &e).await? => Ok(Reply::json(200, e.json())),
        _ => Err(ApiError::not_found(ErrorCode::ErasureNotFound)),
    }
}

/// `GET /v1/erasure-requests` (filters: `tenant_id`, `status`), newest first.
pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    let tenant = c
        .query("tenant_id")
        .map(str::to_owned)
        .or_else(|| key.tenant_id.clone());
    if let Some(t) = &tenant {
        tenant_in_scope(c, t, false).await?;
    } else if key.level != Level::Platform {
        return Err(ApiError::invalid("tenant_id", "The tenant."));
    }
    let limit: i64 = c
        .query("limit")
        .and_then(|l| l.parse().ok())
        .unwrap_or(50)
        .clamp(1, 100);
    let rows: Vec<Erasure> = c
        .p
        .db()
        .query(Stmt::new(
            "SELECT id, tenant_id, job_id, scope, identity_id, target_id, status, receipt_json, created_by_key_id, resumes_id, deadline_at, created_at, completed_at FROM erasure_requests \
             WHERE (?1 IS NULL OR tenant_id = ?1) AND (?2 IS NULL OR status = ?2) AND (?3 IS NULL OR identity_id = ?3) ORDER BY created_at DESC, id DESC LIMIT ?4",
            vec![
                tenant.into(),
                c.query("status").map(str::to_owned).into(),
                (key.level == Level::Identity)
                    .then(|| key.identity_id.clone())
                    .flatten()
                    .into(),
                limit.into(),
            ],
        ))
        .await?;
    Ok(Reply::json(
        200,
        json!({ "data": rows.iter().map(Erasure::json).collect::<Vec<_>>(), "has_more": false, "next_cursor": null }),
    ))
}

/// `POST /v1/platform/erasure-requests/{erasure_id}/reapply` (Privacy § 11): the same scope and
/// target from the stored row alone, the counterparty by its keyed hash.
pub async fn reapply<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    #[derive(serde::Deserialize)]
    struct Src {
        tenant_id: String,
        scope: String,
        identity_id: Option<String>,
        target_id: Option<String>,
        counterparty_hash: Option<String>,
        status: String,
        receipt_json: Option<String>,
    }
    let id = c.route.param("erasure_id").to_owned();
    let src: Src = c
        .p
        .db()
        .first(Stmt::new(
            "SELECT tenant_id, scope, identity_id, target_id, counterparty_hash, status, receipt_json FROM erasure_requests WHERE id = ?1",
            vec![id.clone().into()],
        ))
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::ErasureNotFound))?;
    if !matches!(src.status.as_str(), "completed" | "completed_with_holds") {
        return Err(ApiError::new(
            ErrorCode::ErasureNotCompleted,
            "Only a completed erasure can be re-applied.",
        ));
    }
    let tenant: tdb::Row =
        c.p.db()
            .first(tdb::get(&src.tenant_id))
            .await?
            .ok_or_else(|| ApiError::not_found(ErrorCode::TenantNotFound))?;
    let identity = match &src.identity_id {
        Some(i) => c.p.db().first::<idb::Row>(idb::get(i)).await?,
        None => None,
    };
    let scope: &'static str = match src.scope.as_str() {
        "message" => "message",
        "thread" => "thread",
        "counterparty" => "counterparty",
        "identity" => "identity",
        _ => "tenant",
    };
    let affected: Value = src
        .receipt_json
        .as_deref()
        .and_then(|r| serde_json::from_str::<Value>(r).ok())
        .map_or(Value::Null, |r| r["identities_affected"].clone());
    let plan = Plan {
        tenant,
        scope,
        identity,
        target_id: src.target_id,
        address: None,
        counterparty_hash: src.counterparty_hash,
        reason: format!("reapply_after_restore:{id}"),
    };
    create_request(
        c,
        plan,
        None,
        json!({ "reapply": true, "source_identities": affected }),
        None,
    )
    .await
}

/// The holds' identity check: on an `erasing` tenant the hold routes stay usable for platform and
/// partner keys (Privacy § 8); a `deleting` identity keeps them too.
async fn hold_identity<P: Platform>(c: &Ctx<P>) -> Result<(idb::Row, tdb::Row), ApiError> {
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let key = c.key();
    let frozen = matches!(t.status.as_str(), "erasing" | "erased");
    if (frozen && !matches!(key.level, Level::Platform | Level::Partner))
        || t.status == "erased"
        || idn.status == "deleted"
        || idn.mailbox_do_id.is_empty()
    {
        return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
    }
    Ok((idn, t))
}

async fn thread_summary<P: Platform>(
    c: &Ctx<P>,
    idn: &idb::Row,
    thread_id: &str,
) -> Result<Value, ApiError> {
    let out = call_mailbox(
        c,
        idn,
        MailboxRequest::Read(crate::mailbox::read::ReadRequest::GetThread {
            thread_id: thread_id.to_owned(),
            messages_limit: 1,
            cursor: None,
            include: Default::default(),
            review: true,
        }),
    )
    .await?;
    Ok(out["thread"].clone())
}

/// `POST /v1/identities/{identity_id}/threads/{thread_id}/hold`.
pub async fn hold_set<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, t) = hold_identity(c).await?;
    let body = c.json()?;
    only_fields(&body, &["reason", "until"])?;
    let reason = body["reason"]
        .as_str()
        .filter(|r| !r.is_empty() && r.chars().count() <= 500)
        .ok_or_else(|| ApiError::invalid("reason", "1 to 500 characters."))?
        .to_owned();
    let until = match &body["until"] {
        Value::Null => None,
        Value::String(s) => Some(
            pylota_identity_core::days::parse_rfc3339(s)
                .ok_or_else(|| ApiError::invalid("until", "An RFC 3339 time."))?,
        ),
        _ => return Err(ApiError::invalid("until", "An RFC 3339 time.")),
    };
    let thread_id = c.route.param("thread_id").to_owned();
    let key = c.key().clone();
    let out = call_mailbox(
        c,
        &idn,
        MailboxRequest::Privacy(PrivacyOp::SetHold {
            thread_id: thread_id.clone(),
            reason,
            until,
            set_by: key.key_id.clone(),
        }),
    )
    .await?;
    if out["found"] != true {
        return Err(ApiError::not_found(ErrorCode::ThreadNotFound));
    }
    c.p.db()
        .execute(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            Some(&t.id),
            Some(&key.key_id),
            "hold.set",
            "thread",
            &thread_id,
            &json!({ "identity_id": idn.id, "until": until }),
            &c.request_id,
            c.now,
        ))
        .await?;
    Ok(Reply::json(200, thread_summary(c, &idn, &thread_id).await?))
}

/// `DELETE /v1/identities/{identity_id}/threads/{thread_id}/hold`.
pub async fn hold_remove<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, t) = hold_identity(c).await?;
    let thread_id = c.route.param("thread_id").to_owned();
    let summary = thread_summary(c, &idn, &thread_id).await?;
    call_mailbox(
        c,
        &idn,
        MailboxRequest::Privacy(PrivacyOp::ClearHold {
            thread_id: thread_id.clone(),
        }),
    )
    .await?;
    let key = c.key().clone();
    c.p.db()
        .execute(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            Some(&t.id),
            Some(&key.key_id),
            "hold.removed",
            "thread",
            &thread_id,
            &json!({ "identity_id": idn.id }),
            &c.request_id,
            c.now,
        ))
        .await?;
    let mut s = summary;
    s["hold"] = Value::Null;
    Ok(Reply::json(200, s))
}

/// `DELETE /v1/identities/{identity_id}/messages/{message_id}`: a message-scope request, refused
/// with `423 legal_hold` when the message's thread is held.
pub async fn delete_message<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let id = c.route.param("message_id").to_owned();
    let out = call_mailbox(
        c,
        &idn,
        MailboxRequest::Privacy(PrivacyOp::ErasurePlan {
            target: Target::Message { id: id.clone() },
            after_rowid: 0,
            limit: 1,
        }),
    )
    .await?;
    if out["held"].as_array().is_some_and(|h| !h.is_empty()) {
        return Err(ApiError::new(
            ErrorCode::LegalHold,
            "The message's thread is under a legal hold.",
        ));
    }
    if out["messages"].as_array().is_none_or(|m| m.is_empty()) {
        return Err(ApiError::not_found(ErrorCode::MessageNotFound));
    }
    let plan = Plan {
        tenant: t,
        scope: "message",
        identity: Some(idn),
        target_id: Some(id),
        address: None,
        counterparty_hash: None,
        reason: "message_deleted".into(),
    };
    create_request(c, plan, None, json!({}), None).await
}
