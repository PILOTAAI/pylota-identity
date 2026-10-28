//! Route handlers. Each takes the request context and answers a [`Reply`] or an [`ApiError`]; the app
//! has already matched the route, authenticated the key, rate-limited it and checked the route's
//! permissions and levels.

pub mod audit;
pub mod keys;
pub mod meta;
pub mod partners;
pub mod platform;
pub mod policy;
pub mod tenants;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::{ControlDb as _, Platform};

use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::tenants as tdb;
use crate::errors::ApiError;
use crate::router::Handler;

pub async fn dispatch<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    match c.route.spec.handler {
        Handler::Health => meta::health(c),
        Handler::OpenApi => meta::openapi(c),
        Handler::SecurityTxt => meta::security_txt(c),
        Handler::Me => meta::me(c),
        Handler::TenantCreate => tenants::create(c).await,
        Handler::TenantList => tenants::list(c).await,
        Handler::TenantGet => tenants::get(c).await,
        Handler::TenantUpdate => tenants::update(c).await,
        Handler::PolicyGet => policy::get(c).await,
        Handler::PolicyUpdate => policy::update(c).await,
        Handler::PartnerCreate => partners::create(c).await,
        Handler::PartnerList => partners::list(c).await,
        Handler::PartnerGet => partners::get(c).await,
        Handler::PartnerUpdate => partners::update(c).await,
        Handler::PartnerDelete => partners::delete(c).await,
        Handler::KeyCreate => keys::create(c).await,
        Handler::KeyList => keys::list(c).await,
        Handler::KeyGet => keys::get(c).await,
        Handler::KeyRevoke => keys::revoke(c).await,
        Handler::KeyRotate => keys::rotate(c).await,
        Handler::AuditList => audit::list(c).await,
        Handler::SigningKeyRotate => platform::rotate_signing_key(c).await,
    }
}

/// Step 4 of Security § 5.2 for a tenant: one D1 read whether or not the ID exists; out of the key's
/// scope is the same `404 tenant_not_found` as missing. A write by a non-platform key to an `erasing`
/// or `erased` tenant is `404` too (I8).
pub async fn tenant_in_scope<P: Platform>(
    c: &Ctx<P>,
    tenant_id: &str,
    write: bool,
) -> Result<tdb::Row, ApiError> {
    let row: Option<tdb::Row> = c.p.db().first(tdb::get(tenant_id)).await?;
    let key = c.key();
    let visible = row.as_ref().is_some_and(|r| match key.level {
        Level::Platform => true,
        Level::Partner => key.partner_id.is_some() && r.partner_id == key.partner_id,
        Level::Tenant | Level::Identity => key.tenant_id.as_deref() == Some(r.id.as_str()),
    });
    match row {
        Some(r) if visible => {
            if write
                && key.level != Level::Platform
                && matches!(r.status.as_str(), "erasing" | "erased")
            {
                return Err(ApiError::not_found(ErrorCode::TenantNotFound));
            }
            Ok(r)
        }
        _ => Err(ApiError::not_found(ErrorCode::TenantNotFound)),
    }
}

/// `n` from `SELECT COUNT(*) AS n`.
#[derive(serde::Deserialize)]
pub struct Count {
    pub n: i64,
}
