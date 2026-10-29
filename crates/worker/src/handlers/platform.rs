//! `POST /v1/platform/keys/{purpose}/rotate` (Security § 6.2): `platform:ops`, audit-logged.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde_json::json;

use crate::ctx::{Ctx, Reply};
use crate::db::audit;
use crate::errors::ApiError;
use crate::keyring::{self, Purpose};

pub async fn rotate_signing_key<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let purpose = match c.route.param("purpose") {
        "web_bot_auth" => {
            return Err(ApiError::new(
                ErrorCode::WebBotAuthDisabled,
                "Signed HTTP requests arrive with M25.",
            )
            .details(json!({ "purpose": "web_bot_auth" })));
        }
        p => Purpose::parse(p).ok_or_else(|| {
            ApiError::invalid(
                "purpose",
                "purpose is thread, link, cursor or web_bot_auth.",
            )
        })?,
    };
    let revoke = match c.query("revoke_previous") {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => return Err(ApiError::invalid("revoke_previous", "true or false.")),
    };
    let r = keyring::rotate(c.p.as_ref(), purpose, revoke).await?;
    c.p.db()
        .execute(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            None,
            Some(&c.key().key_id),
            "signing_key.rotate",
            "signing_key",
            &format!("{}:{}", purpose.as_str(), r.kid),
            &json!({ "revoke_previous": revoke }),
            &c.request_id,
            c.now,
        ))
        .await?;
    Ok(Reply::json(
        200,
        json!({
            "purpose": purpose.as_str(), "kid": r.kid, "created_at": rfc3339(r.created_at),
            "previous": { "kid": r.previous_kid, "verify_until": rfc3339(r.previous_verify_until), "revoked": r.revoked },
        }),
    ))
}

/// `POST /v1/platform/jobs` (J3): a `reparse`, `reembed` or `reindex` job over a tenant's mailboxes,
/// narrowed by `identity_ids`, `after` and `before`.
pub async fn job_create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let body = c.json()?;
    super::messages::only_fields(
        &body,
        &["kind", "tenant_id", "identity_ids", "after", "before"],
    )?;
    let kind = match body["kind"].as_str() {
        Some(k @ ("reparse" | "reembed" | "reindex")) => k.to_owned(),
        _ => return Err(ApiError::invalid("kind", "reparse, reembed or reindex.")),
    };
    let tenant = body["tenant_id"]
        .as_str()
        .ok_or_else(|| ApiError::invalid("tenant_id", "The tenant."))?;
    super::tenant_in_scope(c, tenant, true).await?;
    let time = |k: &str| -> Result<Option<i64>, ApiError> {
        match &body[k] {
            serde_json::Value::Null => Ok(None),
            serde_json::Value::String(s) => pylota_identity_core::days::parse_rfc3339(s)
                .map(Some)
                .ok_or_else(|| ApiError::invalid(k, "An RFC 3339 time.")),
            _ => Err(ApiError::invalid(k, "An RFC 3339 time.")),
        }
    };
    let params = json!({
        "identity_ids": body["identity_ids"], "after": time("after")?, "before": time("before")?,
    });
    let id = crate::jobs::create(c.p.as_ref(), &kind, Some(tenant), &params).await?;
    let j = job_row(c, &id).await?;
    Ok(Reply::json(202, j))
}

async fn job_row<P: Platform>(c: &Ctx<P>, id: &str) -> Result<serde_json::Value, ApiError> {
    #[derive(serde::Deserialize)]
    struct J {
        id: String,
        kind: String,
        tenant_id: Option<String>,
        status: String,
        result_json: Option<String>,
        created_at: i64,
        completed_at: Option<i64>,
    }
    let j: J = c
        .p
        .db()
        .first(pylota_identity_platform::d1::Stmt::new(
            "SELECT id, kind, tenant_id, status, result_json, created_at, completed_at FROM jobs WHERE id = ?1 AND kind NOT IN ('erasure','export')",
            vec![id.into()],
        ))
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::JobNotFound))?;
    Ok(json!({
        "id": j.id, "kind": j.kind, "tenant_id": j.tenant_id, "status": j.status,
        "created_at": rfc3339(j.created_at), "completed_at": j.completed_at.map(rfc3339),
        "result": j.result_json.as_deref().and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok()),
    }))
}

/// `GET /v1/platform/jobs/{job_id}`: any job but erasure and export jobs.
pub async fn job_get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    Ok(Reply::json(200, job_row(c, c.route.param("job_id")).await?))
}
