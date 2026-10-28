//! Domains, read only in M6: the platform domain is visible to every key with `tenant_id: null`. Adding,
//! verifying and removing tenant domains arrive with M13.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::{ControlDb as _, Platform};

use super::tenant_in_scope;
use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::domains as ddb;
use crate::errors::ApiError;

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row: Option<ddb::Row> = c.p.db().first(ddb::get(c.route.param("domain_id"))).await?;
    let not_found = || ApiError::not_found(ErrorCode::DomainNotFound);
    let row = row.filter(|r| r.state != "removed").ok_or_else(not_found)?;
    if let Some(tid) = &row.tenant_id {
        let key = c.key();
        if key.level != Level::Platform {
            // Out of scope is the same 404 as missing.
            tenant_in_scope(c, tid, false)
                .await
                .map_err(|_| not_found())?;
        }
    }
    Ok(Reply::json(200, row.json()))
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
