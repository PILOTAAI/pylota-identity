//! `GET /v1/tenants/{tenant_id}/members`, `POST …/invitations`, `DELETE …/invitations/{invitation_id}`
//! and `DELETE …/members/{user_id}` ([Console › Invitations], [Members]): the same services as the
//! console's members page.
//!
//! [Console › Invitations]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#invitations
//! [Members]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#members

use pylota_identity_platform::Platform;
use pylota_identity_platform::ratelimit::RlBucket;
use serde_json::Value;

use super::tenant_in_scope;
use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;
use crate::members::{self, Actor, invitations};

fn actor<P: Platform>(c: &Ctx<P>) -> Actor<'_> {
    Actor {
        id: &c.key().key_id,
        request_id: &c.request_id,
    }
}

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), false).await?;
    Ok(Reply::json(
        200,
        members::list(c.p.as_ref(), &t.id, c.now).await?,
    ))
}

pub async fn invite<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    // A partner key's invitations share `RL_PARTNER` with its tenant creations (J18).
    if c.key().level == Level::Partner
        && let Some(pid) = c.key().partner_id.as_deref()
    {
        crate::ratelimit::check(c.p.as_ref(), RlBucket::Partner, pid).await?;
    }
    let body = c.json()?;
    let email = body.get("email").and_then(Value::as_str).unwrap_or("");
    let role = body.get("role").and_then(Value::as_str).unwrap_or("");
    let a = actor(c);
    let (status, row) =
        invitations::create(c.p.as_ref(), &t, email, role, a.user(), &a, c.now).await?;
    let _ = status;
    Ok(Reply::json(201, row.json()))
}

pub async fn revoke_invitation<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    invitations::revoke(
        c.p.as_ref(),
        &t,
        c.route.param("invitation_id"),
        &actor(c),
        c.now,
    )
    .await?;
    Ok(Reply::no_content())
}

pub async fn remove<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    members::remove(
        c.p.as_ref(),
        &t,
        c.route.param("user_id"),
        false,
        &actor(c),
        c.now,
    )
    .await?;
    Ok(Reply::no_content())
}
