//! `partners` (Data model § 1): never deleted; `DELETE /v1/partners/{id}` sets `deleted` and scrubs the name.

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub status: String,
    pub default_billing_mode: String,
    pub max_tenants: i64,
    pub ramp_exempt: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn get(id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, name, status, default_billing_mode, max_tenants, ramp_exempt, created_at, updated_at FROM partners WHERE id = ?1",
        vec![id.into()],
    )
}

pub fn list(status: Option<&str>, keyset: Option<(i64, String)>, limit: u32) -> Stmt {
    let (kc, ki) = keyset.map_or((None, None), |(c, i)| (Some(c), Some(i)));
    Stmt::new(
        "SELECT id, name, status, default_billing_mode, max_tenants, ramp_exempt, created_at, updated_at FROM partners \
         WHERE (?1 IS NULL OR status = ?1) AND (?2 IS NULL OR created_at < ?2 OR (created_at = ?2 AND id < ?3)) \
         ORDER BY created_at DESC, id DESC LIMIT ?4",
        vec![
            status.map(str::to_owned).into(),
            kc.into(),
            ki.into(),
            (limit + 1).into(),
        ],
    )
}

pub fn insert(r: &Row) -> Stmt {
    Stmt::new(
        "INSERT INTO partners (id, name, status, default_billing_mode, max_tenants, ramp_exempt, created_at, updated_at) VALUES (?1, ?2, 'active', ?3, ?4, ?5, ?6, ?6)",
        vec![
            r.id.clone().into(),
            r.name.clone().into(),
            r.default_billing_mode.clone().into(),
            r.max_tenants.into(),
            r.ramp_exempt.into(),
            r.created_at.into(),
        ],
    )
}

pub fn update(r: &Row) -> Stmt {
    Stmt::new(
        "UPDATE partners SET name = ?2, status = ?3, default_billing_mode = ?4, max_tenants = ?5, ramp_exempt = ?6, updated_at = ?7 WHERE id = ?1 AND status <> 'deleted'",
        vec![
            r.id.clone().into(),
            r.name.clone().into(),
            r.status.clone().into(),
            r.default_billing_mode.clone().into(),
            r.max_tenants.into(),
            r.ramp_exempt.into(),
            r.updated_at.into(),
        ],
    )
}

/// The soft delete, guarded so that it changes nothing while a tenant of the partner is not erased.
pub fn soft_delete(id: &str, now: i64) -> Vec<Stmt> {
    vec![
        Stmt::new(
            "DELETE FROM webhook_deliveries WHERE endpoint_id IN (SELECT id FROM webhook_endpoints WHERE partner_id = ?1 AND tenant_id IS NULL) AND NOT EXISTS (SELECT 1 FROM tenants WHERE partner_id = ?1 AND status <> 'erased')",
            vec![id.into()],
        ),
        Stmt::new(
            "DELETE FROM webhook_endpoints WHERE partner_id = ?1 AND tenant_id IS NULL AND NOT EXISTS (SELECT 1 FROM tenants WHERE partner_id = ?1 AND status <> 'erased')",
            vec![id.into()],
        ),
        Stmt::new(
            "UPDATE api_keys SET revoked_at = COALESCE(revoked_at, ?2), prev_hash = NULL WHERE partner_id = ?1 AND NOT EXISTS (SELECT 1 FROM tenants WHERE partner_id = ?1 AND status <> 'erased')",
            vec![id.into(), now.into()],
        ),
        Stmt::new(
            "DELETE FROM api_keys WHERE partner_id = ?1 AND NOT EXISTS (SELECT 1 FROM tenants WHERE partner_id = ?1 AND status <> 'erased')",
            vec![id.into()],
        ),
        Stmt::new(
            "UPDATE partners SET status = 'deleted', name = '', deleted_at = ?2, updated_at = ?2 WHERE id = ?1 AND status <> 'deleted' AND NOT EXISTS (SELECT 1 FROM tenants WHERE partner_id = ?1 AND status <> 'erased')",
            vec![id.into(), now.into()],
        ),
    ]
}

impl Row {
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "name": self.name, "status": self.status,
            "default_billing_mode": self.default_billing_mode, "max_tenants": self.max_tenants,
            "ramp_exempt": self.ramp_exempt == 1,
            "created_at": rfc3339(self.created_at), "updated_at": rfc3339(self.updated_at),
        })
    }
}
