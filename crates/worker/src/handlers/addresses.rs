//! Addresses (Identities and domains › Addresses). M6 has the list; adding, promoting and retiring an
//! address need a tenant domain and arrive with M13.

use pylota_identity_platform::{ControlDb as _, Platform};

use super::identity_in_scope;
use crate::ctx::{Ctx, Reply};
use crate::db::addresses as adb;
use crate::errors::ApiError;

/// Every address of the identity, in every status.
pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (row, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let limit = c.limit()?;
    let after = c.after(&[]).await?;
    let rows: Vec<adb::Row> = c.p.db().query(adb::page(&row.id, after, limit)).await?;
    c.page(
        rows,
        limit,
        &[],
        |r| (r.created_at, r.id.clone()),
        adb::Row::json,
    )
    .await
}
