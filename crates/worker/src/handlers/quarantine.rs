//! Quarantine ([Inbound › Read path and release]): the list, and release with the person-or-key rule of
//! FR-CON-6 (J14, J16). The release decision reads the tenant's policy from D1 for this request.
//!
//! [Inbound › Read path and release]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::policy;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde_json::{Value, json};

use super::identity_in_scope;
use super::messages::{boundary, mailbox, only_fields, page};
use crate::auth::key_may_decide;
use crate::ctx::{Ctx, Reply};
use crate::db::addresses as adb;
use crate::db::{audit, tenants as tdb};
use crate::errors::ApiError;
use crate::mailbox::read::{Actor, KeyRelease, ReadRequest};

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let limit = c.limit()?;
    let cursor = boundary(c, &[]).await?;
    let out = mailbox(c, &idn, ReadRequest::ListQuarantine { limit, cursor }).await?;
    page(c, out, limit, &[]).await
}

pub async fn release<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let body = c.json()?;
    only_fields(&body, &["reason"])?;
    let reason = match body["reason"].as_str() {
        Some(r) if !r.trim().is_empty() && r.chars().count() <= 500 => r.trim().to_owned(),
        _ => {
            return Err(ApiError::invalid(
                "reason",
                "A reason of 1 to 500 characters.",
            ));
        }
    };
    // The policy as stored now, never a cached copy.
    let t: tdb::Row =
        c.p.db()
            .first(tdb::get(&idn.tenant_id))
            .await?
            .ok_or_else(|| ApiError::not_found(ErrorCode::IdentityNotFound))?;
    let eff = crate::policy::effective(c.p.as_ref(), &t);
    let tenant_key_release =
        policy::get_path(&eff, "quarantine.key_release") == Some(&Value::Bool(true));
    let key = c.key().clone();
    if !key_may_decide(
        c.p.as_ref(),
        &key,
        tenant_key_release,
        t.partner_id.as_deref(),
    ) {
        return Err(ApiError::new(
            ErrorCode::PermissionDenied,
            "A person releases mail on this deployment.",
        )
        .details(json!({ "required": ["quarantine:review"], "reason": "person_required" })));
    }
    let addresses: Vec<adb::Row> = c.p.db().query(adb::of_identity(&idn.id)).await?;
    let identity_addresses = addresses
        .into_iter()
        .filter(|a| matches!(a.status.as_str(), "active" | "retiring"))
        .map(|a| a.address)
        .collect();
    let message_id = c.route.param("message_id").to_owned();
    let m = mailbox(
        c,
        &idn,
        ReadRequest::Release {
            message_id: message_id.clone(),
            actor: Actor::Key {
                key_id: key.key_id.clone(),
            },
            reason: reason.clone(),
            key_release: KeyRelease::Allowed,
            triage_enabled: policy::get_path(&eff, "triage.enabled") != Some(&Value::Bool(false)),
            webhook_text_bytes: policy::get_path(&eff, "webhook_text_bytes")
                .and_then(Value::as_u64)
                .unwrap_or(16_384) as usize,
            identity_addresses,
        },
    )
    .await?;
    let audit_id = c.p.ids().new_id(IdPrefix::Audit);
    c.p.db()
        .execute(audit::insert(
            &audit_id,
            Some(&idn.tenant_id),
            Some(&key.key_id),
            "quarantine.release",
            "message",
            &message_id,
            &json!({ "identity_id": idn.id, "reason": reason }),
            &c.request_id,
            c.now,
        ))
        .await?;
    Ok(Reply::json(200, m))
}
