//! `GET` and `PATCH /v1/tenants/{tenant_id}/policy` (Workspace policy § 5).

use pylota_identity_platform::Platform;

use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;
use crate::policy;

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = super::tenant_in_scope(c, c.route.param("tenant_id"), false).await?;
    Ok(Reply::json(200, policy::view(c, &t)))
}

pub async fn update<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = super::tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    let patch = c.json()?;
    let after = policy::write(c, &t.id, &patch).await?;
    Ok(Reply::json(200, policy::view(c, &after)))
}
