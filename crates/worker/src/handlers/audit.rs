//! `GET /v1/audit-events` (REST API). A partner key reads its own tenants' rows only; rows about a
//! partner itself (no `tenant_id`) are for platform keys.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::parse_rfc3339;
use pylota_identity_platform::{ControlDb as _, Platform};

use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::audit::{self, Within};
use crate::errors::ApiError;

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key();
    let limit = c.limit()?;
    let filters = [
        "tenant_id",
        "actor_key_id",
        "action",
        "target_id",
        "after",
        "before",
    ];
    let time = |name: &str| -> Result<Option<i64>, ApiError> {
        c.query(name)
            .map(|s| {
                parse_rfc3339(s).ok_or_else(|| ApiError::invalid(name, "Not an RFC 3339 time."))
            })
            .transpose()
    };
    let (after_t, before_t) = (time("after")?, time("before")?);
    let tenant_filter = c.query("tenant_id");
    if let Some(t) = tenant_filter {
        // Step 5 of Security § 5.2: a tenant outside the key's scope is 404, never an empty page.
        super::tenant_in_scope(c, t, false)
            .await
            .map_err(|_| ApiError::not_found(ErrorCode::TenantNotFound))?;
    }
    let within = match key.level {
        Level::Platform => Within::All,
        Level::Partner => Within::Partner(key.partner_id.as_deref().unwrap_or("")),
        _ => Within::Tenant(key.tenant_id.as_deref().unwrap_or("")),
    };
    let keyset = c.after(&filters).await?;
    let rows: Vec<audit::Row> =
        c.p.db()
            .query(audit::list(
                within,
                tenant_filter,
                c.query("actor_key_id"),
                c.query("action"),
                c.query("target_id"),
                after_t,
                before_t,
                keyset,
                limit,
            ))
            .await?;
    c.page(
        rows,
        limit,
        &filters,
        |r| (r.created_at, r.id.clone()),
        audit::Row::json,
    )
    .await
}
