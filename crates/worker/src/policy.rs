//! The one policy write service ([Workspace policy § 4]): `POST /v1/tenants`, `PATCH
//! /v1/tenants/{tenant_id}` and `PATCH /v1/tenants/{tenant_id}/policy` all write through [`write`].
//! It validates, checks every field against its class and ceilings (`core::policy::check_write`), and
//! commits one D1 batch guarded by `policy_version`, retried at most three times.
//!
//! [Workspace policy § 4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/workspace-policy.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::policy::{self, Ceilings, Refusal, Writer};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde_json::{Value, json};

use crate::auth::{KeyScope, Level, key_may_decide};
use crate::ctx::Ctx;
use crate::db::{audit, tenants as tdb};
use crate::errors::ApiError;

/// The writer a key is for a tenant: its level, or the tenant's own partner.
pub fn writer_of(key: &KeyScope, tenant: &tdb::Row) -> Writer {
    match key.level {
        Level::Platform => Writer::Platform,
        Level::Partner if tenant.partner_id.is_some() && tenant.partner_id == key.partner_id => {
            Writer::Partner
        }
        _ => Writer::WorkspaceKey,
    }
}

pub fn deployment_defaults<P: Platform>(p: &P) -> Value {
    policy::deployment_defaults(&p.config().default_policy)
}

/// The tenant's effective policy.
pub fn effective<P: Platform>(p: &P, t: &tdb::Row) -> Value {
    policy::effective(&deployment_defaults(p), &t.stored_policy())
}

/// The error for a refused field (Configuration › Who may change a field).
pub fn refusal(r: Refusal, writer: Writer) -> ApiError {
    let workspace = matches!(writer, Writer::WorkspaceKey | Writer::WorkspacePerson);
    match r {
        Refusal::NotWritable { field } => {
            let mut d = json!({ "field": field });
            if workspace {
                d["reason"] = "not_writable".into();
            }
            ApiError::new(
                ErrorCode::ScopeDenied,
                "This key may not write this policy field.",
            )
            .details(d)
        }
        Refusal::AboveCeiling {
            field,
            ceiling,
            source,
        } => {
            let mut d = json!({ "field": field });
            if workspace {
                d["reason"] = "above_ceiling".into();
                d["ceiling"] = ceiling;
                d["ceiling_source"] = source.as_str().into();
            }
            ApiError::new(
                ErrorCode::ScopeDenied,
                "The value is above this field's ceiling.",
            )
            .details(d)
        }
        Refusal::PersonRequired { field } => ApiError::new(
            ErrorCode::PermissionDenied,
            "Only a person may loosen this field.",
        )
        .details(json!({ "field": field, "reason": "person_required" })),
    }
}

/// Validates a patch's shape: a non-empty object whose merge gives a valid `TenantPolicy`.
pub fn validate(patch: &Value, effective: &Value) -> Result<(), ApiError> {
    let Some(obj) = patch.as_object() else {
        return Err(ApiError::invalid("policy", "policy must be an object."));
    };
    if obj.is_empty() {
        return Err(ApiError::invalid("policy", "The policy patch is empty."));
    }
    let mut after = effective.clone();
    policy::merge(&mut after, patch);
    serde_json::from_value::<pylota_identity_api_types::TenantPolicy>(after)
        .map_err(|e| ApiError::invalid("policy", e.to_string()))?;
    Ok(())
}

/// Checks a patch for the calling key on a tenant, without writing (the first refused field decides).
pub fn check<P: Platform>(c: &Ctx<P>, t: &tdb::Row, patch: &Value) -> Result<Writer, ApiError> {
    let writer = writer_of(c.key(), t);
    let eff = effective(c.p.as_ref(), t);
    validate(patch, &eff)?;
    let dep = deployment_defaults(c.p.as_ref());
    let (pc, ptc) = (t.platform_ceilings(), t.partner_ceilings());
    let decide = key_may_decide(
        c.p.as_ref(),
        c.key(),
        policy::get_path(&eff, "quarantine.key_release") == Some(&Value::Bool(true)),
        t.partner_id.as_deref(),
    );
    policy::check_write(
        writer,
        patch,
        &eff,
        &Ceilings {
            deployment: &dep,
            platform: &pc,
            partner: &ptc,
        },
        decide,
    )
    .map_err(|r| refusal(r, writer))?;
    Ok(writer)
}

/// Writes a patch (§ 4 steps 2–6) and returns the tenant as stored after it.
pub async fn write<P: Platform>(
    c: &Ctx<P>,
    tenant_id: &str,
    patch: &Value,
) -> Result<tdb::Row, ApiError> {
    for _ in 0..3 {
        let t: tdb::Row =
            c.p.db()
                .first(tdb::get(tenant_id))
                .await?
                .ok_or_else(|| ApiError::not_found(ErrorCode::TenantNotFound))?;
        let writer = check(c, &t, patch)?;
        let dep = deployment_defaults(c.p.as_ref());
        let (policy_json, pc, ptc) = policy::apply_write(
            writer,
            &t.stored_policy(),
            patch,
            &t.platform_ceilings(),
            &t.partner_ceilings(),
            &dep,
        );
        let fields: Vec<String> = policy::patch_leaves(patch, &dep)
            .unwrap_or_default()
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        let aud = c.p.ids().new_id(IdPrefix::Audit);
        let evt = c.p.ids().new_id(IdPrefix::Event);
        let v = t.policy_version;
        let details = json!({ "fields": fields, "policy_version": v + 1 });
        let payload = json!({ "tenant_id": tenant_id, "policy_version": v + 1, "fields": fields });
        let mut audit_stmt = audit::insert(
            &aud,
            Some(tenant_id),
            Some(&c.key().key_id),
            "tenant.policy_update",
            "tenant",
            tenant_id,
            &details,
            &c.request_id,
            c.now,
        );
        audit_stmt.sql = "INSERT INTO audit_log (id, tenant_id, actor_key_id, actor_user_id, action, target_type, target_id, details_json, request_id, created_at) SELECT ?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9 WHERE EXISTS (SELECT 1 FROM tenants WHERE id = ?2 AND policy_version = ?10)";
        audit_stmt.params.push(v.into());
        let batch = vec![
            audit_stmt,
            Stmt::new(
                "INSERT INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, partner_id, payload_json, occurred_at) SELECT ?1, ?2, NULL, 'tenant.policy_updated', 'platform', 'platform', partner_id, ?3, ?4 FROM tenants WHERE id = ?2 AND policy_version = ?5",
                vec![
                    evt.into(),
                    tenant_id.into(),
                    payload.to_string().into(),
                    c.now.into(),
                    v.into(),
                ],
            ),
            Stmt::new(
                "UPDATE tenants SET policy_json = ?2, policy_ceilings_json = ?3, partner_ceilings_json = ?4, policy_version = policy_version + 1, updated_at = ?5 WHERE id = ?1 AND policy_version = ?6",
                vec![
                    tenant_id.into(),
                    policy_json.to_string().into(),
                    pc.to_string().into(),
                    ptc.to_string().into(),
                    c.now.into(),
                    v.into(),
                ],
            ),
        ];
        let metas = c.p.db().batch(batch).await?;
        if metas.last().is_some_and(|m| m.changes == 1) {
            return c
                .p
                .db()
                .first(tdb::get(tenant_id))
                .await?
                .ok_or_else(ApiError::internal);
        }
    }
    Err(
        ApiError::new(ErrorCode::Unavailable, "Concurrent policy writes; retry.")
            .details(json!({ "reason": "policy_contention" })),
    )
}

/// The policy view (Workspace policy § 5): the effective policy and what the caller may do per field.
pub fn view<P: Platform>(c: &Ctx<P>, t: &tdb::Row) -> Value {
    let writer = writer_of(c.key(), t);
    let eff = effective(c.p.as_ref(), t);
    let dep = deployment_defaults(c.p.as_ref());
    let (pc, ptc) = (t.platform_ceilings(), t.partner_ceilings());
    let ceilings = Ceilings {
        deployment: &dep,
        platform: &pc,
        partner: &ptc,
    };
    let decide = key_may_decide(
        c.p.as_ref(),
        c.key(),
        policy::get_path(&eff, "quarantine.key_release") == Some(&Value::Bool(true)),
        t.partner_id.as_deref(),
    );
    let fields: Vec<Value> = policy::FIELDS
        .iter()
        .filter(|(f, _)| !f.ends_with(".*"))
        .map(|(field, class)| {
            use pylota_identity_core::policy::FieldClass as C;
            let writable = match (writer, class) {
                (Writer::Platform, _) => true,
                (_, C::PlatformOnly) => false,
                (Writer::Partner, C::PlatformOrPartner) => true,
                (_, C::PlatformOrPartner) => false,
                _ => true,
            };
            let (ceiling, source) = if *class == C::LowerOnly && writer != Writer::Platform {
                let (v, s) = ceilings.ceiling(writer, field);
                (v, Value::from(s.as_str()))
            } else {
                (Value::Null, Value::Null)
            };
            json!({
                "field": field, "class": class.as_str(), "writable": writable, "ceiling": ceiling,
                "ceiling_source": source,
                "person_required_to_loosen": *class == C::Guard && writer == Writer::WorkspaceKey && !decide,
            })
        })
        .collect();
    json!({ "tenant_id": t.id, "policy_version": t.policy_version, "policy": eff, "fields": fields })
}
