//! `tenants` (Data model § 1).

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub partner_id: Option<String>,
    pub mode: String,
    pub status: String,
    pub suspended_at: Option<i64>,
    pub suspended_by: Option<String>,
    pub address_suffix: String,
    pub timezone: String,
    pub sending_paused_at: Option<i64>,
    pub policy_json: String,
    pub policy_ceilings_json: String,
    pub partner_ceilings_json: String,
    pub policy_version: i64,
    pub quota_do_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn get(id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, slug, name, partner_id, mode, status, suspended_at, suspended_by, address_suffix, timezone, sending_paused_at, policy_json, policy_ceilings_json, partner_ceilings_json, policy_version, quota_do_id, created_at, updated_at FROM tenants WHERE id = ?1",
        vec![id.into()],
    )
}

/// The tenant list, newest first, within a partner's tenants when `partner` is set.
#[allow(clippy::too_many_arguments)]
pub fn list(
    partner_scope: Option<&str>,
    status: Option<&str>,
    mode: Option<&str>,
    partner_id: Option<&str>,
    keyset: Option<(i64, String)>,
    limit: u32,
) -> Stmt {
    let (kc, ki) = keyset.map_or((None, None), |(c, i)| (Some(c), Some(i)));
    Stmt::new(
        "SELECT id, slug, name, partner_id, mode, status, suspended_at, suspended_by, address_suffix, timezone, sending_paused_at, policy_json, policy_ceilings_json, partner_ceilings_json, policy_version, quota_do_id, created_at, updated_at FROM tenants \
         WHERE (?1 IS NULL OR partner_id = ?1) AND (?2 IS NULL OR status = ?2) AND (?3 IS NULL OR mode = ?3) AND (?4 IS NULL OR partner_id = ?4) \
         AND (?5 IS NULL OR created_at < ?5 OR (created_at = ?5 AND id < ?6)) ORDER BY created_at DESC, id DESC LIMIT ?7",
        vec![
            partner_scope.map(str::to_owned).into(),
            status.map(str::to_owned).into(),
            mode.map(str::to_owned).into(),
            partner_id.map(str::to_owned).into(),
            kc.into(),
            ki.into(),
            (limit + 1).into(),
        ],
    )
}

/// Tenants of a partner that are not erased (`max_tenants`, `partner_has_tenants`).
pub fn count_live_of_partner(partner_id: &str) -> Stmt {
    Stmt::new(
        "SELECT COUNT(*) AS n FROM tenants WHERE partner_id = ?1 AND status <> 'erased'",
        vec![partner_id.into()],
    )
}

/// D1 binds a number from the Worker as a REAL, which `json_object` would write as `1793184352843.0`:
/// every time in a JSON body is cast to INTEGER (`it::intents::j21_identity_event_retried`).
///
/// A suspension's pause of every `active` identity (Identities and domains › Updates, pause and tenant
/// suspension): one `identity.paused` intent per identity, written before the update that pauses them.
pub fn pause_identities(tenant_id: &str, request_id: &str, now: i64) -> Vec<Stmt> {
    vec![
        Stmt::new(
            "INSERT OR IGNORE INTO rpc_intents (id, class, object_id, tenant_id, identity_id, op, body_json, occurred_at, attempts, next_at, created_at) \
             SELECT ?2 || ':' || id || ':identity.paused', 'mailbox', mailbox_do_id, tenant_id, id, 'emit_event', \
             json_object('op', 'emit_event', 'event_id', '', 'event_type', 'identity.paused', 'occurred_at', CAST(?3 AS INTEGER), 'data', json_object('identity_id', id, 'reason', 'tenant_suspended', 'metrics', NULL)), \
             ?3, 0, ?3 + 60000, ?3 FROM identities WHERE tenant_id = ?1 AND status = 'active' AND mailbox_do_id <> ''",
            vec![tenant_id.into(), request_id.into(), now.into()],
        ),
        Stmt::new(
            "UPDATE identities SET status = 'paused', pause_reason = 'tenant_suspended', updated_at = ?2 WHERE tenant_id = ?1 AND status = 'active'",
            vec![tenant_id.into(), now.into()],
        ),
    ]
}

/// The resume of a suspension: only the identities the suspension paused.
pub fn resume_identities(tenant_id: &str, request_id: &str, now: i64) -> Vec<Stmt> {
    vec![
        Stmt::new(
            "INSERT OR IGNORE INTO rpc_intents (id, class, object_id, tenant_id, identity_id, op, body_json, occurred_at, attempts, next_at, created_at) \
             SELECT ?2 || ':' || id || ':identity.resumed', 'mailbox', mailbox_do_id, tenant_id, id, 'emit_event', \
             json_object('op', 'emit_event', 'event_id', '', 'event_type', 'identity.resumed', 'occurred_at', CAST(?3 AS INTEGER), 'data', json_object('identity_id', id)), \
             ?3, 0, ?3 + 60000, ?3 FROM identities WHERE tenant_id = ?1 AND status = 'paused' AND pause_reason = 'tenant_suspended' AND mailbox_do_id <> ''",
            vec![tenant_id.into(), request_id.into(), now.into()],
        ),
        Stmt::new(
            "UPDATE identities SET status = 'active', pause_reason = NULL, updated_at = ?2 WHERE tenant_id = ?1 AND status = 'paused' AND pause_reason = 'tenant_suspended'",
            vec![tenant_id.into(), now.into()],
        ),
    ]
}

impl Row {
    pub fn stored_policy(&self) -> Value {
        serde_json::from_str(&self.policy_json).unwrap_or_else(|_| json!({}))
    }

    pub fn platform_ceilings(&self) -> Value {
        serde_json::from_str(&self.policy_ceilings_json).unwrap_or_else(|_| json!({}))
    }

    /// The partner's ceilings; a tenant of no partner has none, whatever the column holds (J26).
    pub fn partner_ceilings(&self) -> Value {
        if self.partner_id.is_none() {
            return json!({});
        }
        serde_json::from_str(&self.partner_ceilings_json).unwrap_or_else(|_| json!({}))
    }

    /// The Tenant object, with the effective policy.
    pub fn json(&self, effective_policy: &Value) -> Value {
        json!({
            "id": self.id, "slug": self.slug, "name": self.name, "mode": self.mode, "status": self.status,
            "suspended_by": if self.status == "suspended" { self.suspended_by.clone() } else { None },
            "partner_id": self.partner_id, "address_suffix": self.address_suffix, "timezone": self.timezone,
            "policy": effective_policy,
            "sending_paused_at": self.sending_paused_at.map(rfc3339),
            "created_at": rfc3339(self.created_at), "updated_at": rfc3339(self.updated_at),
        })
    }
}
