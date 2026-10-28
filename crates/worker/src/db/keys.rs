//! `api_keys` (Data model § 1, Security § 4).

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub lookup: String,
    pub name: String,
    pub level: String,
    pub partner_id: Option<String>,
    pub tenant_id: Option<String>,
    pub identity_id: Option<String>,
    pub mode: String,
    pub permissions_json: String,
    pub mcp_tools_json: Option<String>,
    pub created_by_key_id: Option<String>,
    pub created_by_user_id: Option<String>,
    pub created_by_role: Option<String>,
    pub created_by_level: String,
    pub expires_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    /// The tenant's partner, joined (tenant and identity keys).
    pub tenant_partner_id: Option<String>,
}

pub fn get(id: &str) -> Stmt {
    Stmt::new(
        "SELECT k.id, k.lookup, k.name, k.level, k.partner_id, k.tenant_id, k.identity_id, k.mode, k.permissions_json, k.mcp_tools_json, k.created_by_key_id, k.created_by_user_id, k.created_by_role, k.created_by_level, k.expires_at, k.revoked_at, k.last_used_at, k.created_at, t.partner_id AS tenant_partner_id \
         FROM api_keys k LEFT JOIN tenants t ON t.id = k.tenant_id WHERE k.id = ?1",
        vec![id.into()],
    )
}

/// The keys a caller reaches (Security § 4.6, Managing existing keys), newest first: `reach` is
/// `platform`, `partner` (the tenant and identity keys of its tenants), `tenant` or `identity`.
pub fn list(
    reach: &str,
    value: &str,
    tenant_filter: Option<&str>,
    keyset: Option<(i64, String)>,
    limit: u32,
) -> Stmt {
    let (kc, ki) = keyset.map_or((None, None), |(c, i)| (Some(c), Some(i)));
    Stmt::new(
        "SELECT k.id, k.lookup, k.name, k.level, k.partner_id, k.tenant_id, k.identity_id, k.mode, k.permissions_json, k.mcp_tools_json, k.created_by_key_id, k.created_by_user_id, k.created_by_role, k.created_by_level, k.expires_at, k.revoked_at, k.last_used_at, k.created_at, t.partner_id AS tenant_partner_id \
         FROM api_keys k LEFT JOIN tenants t ON t.id = k.tenant_id \
         WHERE (?1 = 'platform' OR (?1 = 'partner' AND k.level IN ('tenant','identity') AND t.partner_id = ?2) \
                OR (?1 = 'tenant' AND k.tenant_id = ?2) OR (?1 = 'identity' AND k.level = 'identity' AND k.identity_id = ?2)) \
         AND (?3 IS NULL OR k.tenant_id = ?3) \
         AND (?4 IS NULL OR k.created_at < ?4 OR (k.created_at = ?4 AND k.id < ?5)) ORDER BY k.created_at DESC, k.id DESC LIMIT ?6",
        vec![
            reach.into(),
            value.into(),
            tenant_filter.map(str::to_owned).into(),
            kc.into(),
            ki.into(),
            (limit + 1).into(),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn insert(r: &Row, hash: &str) -> Stmt {
    Stmt::new(
        "INSERT INTO api_keys (id, lookup, hash, name, level, partner_id, tenant_id, identity_id, mode, permissions_json, mcp_tools_json, created_by_key_id, created_by_user_id, created_by_role, created_by_level, expires_at, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        vec![
            r.id.clone().into(),
            r.lookup.clone().into(),
            hash.into(),
            r.name.clone().into(),
            r.level.clone().into(),
            r.partner_id.clone().into(),
            r.tenant_id.clone().into(),
            r.identity_id.clone().into(),
            r.mode.clone().into(),
            r.permissions_json.clone().into(),
            r.mcp_tools_json.clone().into(),
            r.created_by_key_id.clone().into(),
            r.created_by_user_id.clone().into(),
            r.created_by_role.clone().into(),
            r.created_by_level.clone().into(),
            r.expires_at.into(),
            r.created_at.into(),
        ],
    )
}

pub fn revoke(id: &str, now: i64) -> Stmt {
    Stmt::new(
        "UPDATE api_keys SET revoked_at = COALESCE(revoked_at, ?2), prev_hash = NULL, prev_expires_at = NULL WHERE id = ?1",
        vec![id.into(), now.into()],
    )
}

/// § 4.5 step 2–3 in one statement.
pub fn rotate(
    id: &str,
    hash: &str,
    prev_expires_at: Option<i64>,
    user: Option<&str>,
    role: Option<&str>,
) -> Stmt {
    Stmt::new(
        "UPDATE api_keys SET prev_hash = CASE WHEN ?3 IS NULL THEN NULL ELSE hash END, prev_expires_at = ?3, hash = ?2, created_by_user_id = ?4, created_by_role = ?5 WHERE id = ?1",
        vec![
            id.into(),
            hash.into(),
            prev_expires_at.into(),
            user.map(str::to_owned).into(),
            role.map(str::to_owned).into(),
        ],
    )
}

/// Active (neither revoked nor expired) tenant and identity keys of a tenant.
pub fn count_active_of_tenant(tenant_id: &str, now: i64) -> Stmt {
    Stmt::new(
        "SELECT COUNT(*) AS n FROM api_keys WHERE tenant_id = ?1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?2)",
        vec![tenant_id.into(), now.into()],
    )
}

/// Active partner keys of a partner.
pub fn count_active_of_partner(partner_id: &str, now: i64) -> Stmt {
    Stmt::new(
        "SELECT COUNT(*) AS n FROM api_keys WHERE partner_id = ?1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?2)",
        vec![partner_id.into(), now.into()],
    )
}

impl Row {
    pub fn permissions(&self) -> Vec<String> {
        serde_json::from_str(&self.permissions_json).unwrap_or_default()
    }

    /// The API key object.
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "name": self.name, "level": self.level, "mode": self.mode,
            "partner_id": self.partner_id, "tenant_id": self.tenant_id, "identity_id": self.identity_id,
            "permissions": self.permissions(),
            "mcp_tools": self.mcp_tools_json.as_deref().and_then(|t| serde_json::from_str::<Value>(t).ok()),
            "created_by_user_id": self.created_by_user_id,
            "expires_at": self.expires_at.map(rfc3339), "revoked_at": self.revoked_at.map(rfc3339),
            "last_used_at": self.last_used_at.map(rfc3339), "created_at": rfc3339(self.created_at),
        })
    }
}
