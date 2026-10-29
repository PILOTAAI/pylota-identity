//! Subject-access exports ([Privacy and erasure § 9]): create, read with a fresh one-hour download
//! link, and delete (which revokes every link at once). An export is a read of mail: identity keys
//! cannot create one, and `messages:read`, `attachments:read` and `erasure:manage` are all required.
//!
//! [Privacy and erasure § 9]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::{address, days::rfc3339, links};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{BlobStore as _, ControlDb as _, Ids as _, Platform, Rng as _};
use serde::Deserialize;
use serde_json::{Value, json};

use super::messages::only_fields;
use super::{identity_in_scope, tenant_in_scope};
use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::audit;
use crate::errors::ApiError;
use crate::jobs::{self, JobRequest};
use crate::keyring::{self, Purpose};

#[derive(Deserialize)]
pub struct Export {
    pub id: String,
    pub tenant_id: String,
    pub job_id: String,
    pub scope: String,
    pub identity_id: Option<String>,
    pub status: String,
    pub r2_key: Option<String>,
    pub size: Option<i64>,
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

pub fn get_stmt(id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, job_id, scope, identity_id, status, r2_key, size, expires_at, created_at FROM exports WHERE id = ?1",
        vec![id.into()],
    )
}

/// A link lives an hour, or until the export expires if that is sooner.
const LINK_TTL_MS: i64 = 3_600_000;

async fn link<P: Platform>(c: &Ctx<P>, e: &Export) -> Result<Option<String>, ApiError> {
    let (Some(exp), "completed") = (e.expires_at, e.status.as_str()) else {
        return Ok(None);
    };
    let keys = keyring::keys(c.p.as_ref(), Purpose::Link).await?;
    let Some(key) = keys.iter().find(|k| k.verify_until.is_none()) else {
        return Ok(None);
    };
    let nonce: [u8; 10] = c.p.rng().bytes();
    let token = links::mint_export(
        &key.secret,
        &links::ExportLink {
            kid: key.kid.chars().next().unwrap_or('0'),
            tenant_id: e.tenant_id.clone(),
            export_id: e.id.clone(),
            nonce: nonce
                .iter()
                .map(|b| char::from(pylota_identity_api_types::ids::CROCKFORD[usize::from(b & 31)]))
                .chain(nonce.iter().map(|b| {
                    char::from(pylota_identity_api_types::ids::CROCKFORD[usize::from(b >> 3)])
                }))
                .collect(),
            expires_s: (c.now + LINK_TTL_MS).min(exp) / 1000,
        },
    );
    Ok(Some(format!(
        "https://{}/v1/links/{token}",
        c.p.config().api_host
    )))
}

async fn json_of<P: Platform>(c: &Ctx<P>, e: &Export) -> Result<Value, ApiError> {
    Ok(json!({
        "id": e.id, "tenant_id": e.tenant_id, "scope": e.scope, "identity_id": e.identity_id,
        "status": e.status, "size": e.size, "created_at": rfc3339(e.created_at),
        "expires_at": e.expires_at.map(rfc3339), "download_url": link(c, e).await?,
    }))
}

/// `POST /v1/exports`.
pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let body = c.json()?;
    only_fields(
        &body,
        &["tenant_id", "scope", "counterparty_address", "identity_id"],
    )?;
    let key = c.key().clone();
    let tenant_id = body["tenant_id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| key.tenant_id.clone())
        .ok_or_else(|| ApiError::invalid("tenant_id", "The tenant."))?;
    let t = tenant_in_scope(c, &tenant_id, true).await?;
    let mut params =
        json!({ "include_held": key.permissions.iter().any(|p| p == "quarantine:review") });
    let mut address: Option<String> = None;
    let (scope, identity_id) = match body["scope"].as_str() {
        Some("identity") => {
            let id = body["identity_id"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("identity_id", "The identity."))?;
            let (idn, it) = identity_in_scope(c, id, false).await?;
            if it.id != t.id || idn.is_system == 1 {
                return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
            }
            ("identity", Some(idn.id))
        }
        Some("counterparty") => {
            let raw = body["counterparty_address"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("counterparty_address", "An email address."))?;
            let a = address::parse_envelope(raw)
                .map(|e| e.base())
                .map_err(|_| ApiError::invalid("counterparty_address", "An email address."))?;
            params["counterparty_hash"] = address::address_hash(&c.p.secrets().hash_key, &a).into();
            address = Some(a);
            ("counterparty", None)
        }
        _ => return Err(ApiError::invalid("scope", "counterparty or identity.")),
    };
    let ids = c.p.ids();
    let exp = ids.new_id(IdPrefix::Export);
    let job = jobs::new_job(c.p.as_ref())?;
    params["export_id"] = exp.clone().into();
    params["scope"] = scope.into();
    params["identity_id"] = identity_id.clone().into();
    c.p.db()
        .batch(vec![
            crate::db::jobs::insert_job(&job.id, &t.id, "export", &job.runner_do_id, &params, &key.key_id, c.now),
            Stmt::new(
                "INSERT INTO exports (id, tenant_id, job_id, scope, identity_id, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6)",
                vec![exp.clone().into(), t.id.clone().into(), job.id.clone().into(), scope.into(), identity_id.into(), c.now.into()],
            ),
            audit::insert(
                &ids.new_id(IdPrefix::Audit),
                Some(&t.id),
                Some(&key.key_id),
                "export.create",
                "export",
                &exp,
                &json!({ "scope": scope }),
                &c.request_id,
                c.now,
            ),
        ])
        .await?;
    let mut start = params.clone();
    if let Some(a) = address {
        start["address"] = a.into();
    }
    jobs::start(
        c.p.as_ref(),
        &job.runner_do_id,
        JobRequest::Start {
            job_id: job.id,
            kind: "export".into(),
            tenant_id: Some(t.id.clone()),
            params: start,
            resume_job_id: None,
        },
        Some(&t.id),
    )
    .await;
    let e: Export =
        c.p.db()
            .first(get_stmt(&exp))
            .await?
            .ok_or_else(ApiError::internal)?;
    Ok(Reply::json(202, json_of(c, &e).await?))
}

async fn load<P: Platform>(c: &Ctx<P>) -> Result<Export, ApiError> {
    let not_found = || ApiError::not_found(ErrorCode::ExportNotFound);
    let e: Export =
        c.p.db()
            .first(get_stmt(c.route.param("export_id")))
            .await?
            .ok_or_else(not_found)?;
    tenant_in_scope(c, &e.tenant_id, false)
        .await
        .map_err(|_| not_found())?;
    let key = c.key();
    if key.level == Level::Identity && (e.identity_id.is_none() || e.identity_id != key.identity_id)
    {
        return Err(not_found());
    }
    Ok(e)
}

/// `GET /v1/exports/{export_id}`: every read mints a new link.
pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let e = load(c).await?;
    Ok(Reply::json(200, json_of(c, &e).await?))
}

/// `DELETE /v1/exports/{export_id}`: a completed export's ZIP goes (`expired`), a queued or running
/// one stops (`canceled`); every earlier link then fails.
pub async fn delete<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let e = load(c).await?;
    match e.status.as_str() {
        "completed" => {
            if let Some(k) = &e.r2_key {
                c.p.blobs().delete(std::slice::from_ref(k)).await?;
                if let Some(b) = c.p.backup() {
                    b.delete(std::slice::from_ref(k)).await?;
                }
            }
            c.p.db()
                .execute(Stmt::new(
                    "UPDATE exports SET status = 'expired' WHERE id = ?1 AND status = 'completed'",
                    vec![e.id.clone().into()],
                ))
                .await?;
        }
        "queued" | "running" => {
            #[derive(Deserialize)]
            struct J {
                runner_do_id: String,
            }
            if let Some(j) =
                c.p.db()
                    .first::<J>(Stmt::new(
                        "SELECT runner_do_id FROM jobs WHERE id = ?1",
                        vec![e.job_id.clone().into()],
                    ))
                    .await?
            {
                jobs::cancel(c.p.as_ref(), &j.runner_do_id, Some(&e.tenant_id)).await;
            }
            c.p.db()
                .execute(Stmt::new(
                    "UPDATE exports SET status = 'canceled' WHERE id = ?1 AND status IN ('queued','running')",
                    vec![e.id.clone().into()],
                ))
                .await?;
        }
        _ => {}
    }
    c.p.db()
        .execute(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            Some(&e.tenant_id),
            Some(&c.key().key_id),
            "export.delete",
            "export",
            &e.id,
            &json!({}),
            &c.request_id,
            c.now,
        ))
        .await?;
    Ok(Reply::no_content())
}

/// `GET /v1/links/{token}` for an export: MAC, expiry, the export still completed, then the link's
/// one use (I14). Every refusal is `404 export_not_found`.
pub async fn download<P: Platform>(c: &Ctx<P>, l: links::ExportLink) -> Result<Reply, ApiError> {
    let not_found = || ApiError::not_found(ErrorCode::ExportNotFound);
    let e: Export =
        c.p.db()
            .first(get_stmt(&l.export_id))
            .await?
            .ok_or_else(not_found)?;
    if e.tenant_id != l.tenant_id
        || e.status != "completed"
        || e.expires_at.is_none_or(|x| x <= c.now)
    {
        return Err(not_found());
    }
    let used =
        c.p.db()
            .batch(vec![Stmt::new(
                "INSERT INTO link_uses (nonce, expires_at) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                vec![l.nonce.clone().into(), (l.expires_s * 1000).into()],
            )])
            .await?;
    if used.first().is_none_or(|m| m.changes == 0) {
        return Err(not_found());
    }
    let blob =
        c.p.blobs()
            .get(e.r2_key.as_deref().unwrap_or(""))
            .await?
            .ok_or_else(not_found)?;
    Ok(Reply {
        status: 200,
        body: None,
        raw: Some(("application/zip", blob.body.to_vec())),
        headers: vec![(
            "content-disposition".into(),
            format!("attachment; filename=\"{}.zip\"", e.id),
        )],
        stream: None,
    })
}
