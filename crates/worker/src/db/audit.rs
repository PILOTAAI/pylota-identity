//! `audit_log`: administrative actions (REST API › `GET /v1/audit-events`).

use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

const INSERT: &str = "INSERT INTO audit_log (id, tenant_id, actor_key_id, actor_user_id, action, target_type, target_id, details_json, request_id, created_at) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9)";

/// `insert` for a row of a tenant created in the same batch: written only once the tenant row exists
/// (`handlers::tenants::create`).
pub fn when_tenant(mut s: Stmt) -> Stmt {
    debug_assert_eq!(s.sql, INSERT);
    s.sql = "INSERT INTO audit_log (id, tenant_id, actor_key_id, actor_user_id, action, target_type, target_id, details_json, request_id, created_at) SELECT ?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9 WHERE EXISTS (SELECT 1 FROM tenants WHERE id = ?2)";
    s
}

/// The insert of one audit row, for a handler's D1 batch.
#[allow(clippy::too_many_arguments)]
pub fn insert(
    id: &str,
    tenant_id: Option<&str>,
    actor_key_id: Option<&str>,
    action: &str,
    target_type: &str,
    target_id: &str,
    details: &Value,
    request_id: &str,
    now: i64,
) -> Stmt {
    Stmt::new(
        INSERT,
        vec![
            id.into(),
            tenant_id.map(str::to_owned).into(),
            actor_key_id.map(str::to_owned).into(),
            action.into(),
            target_type.into(),
            target_id.into(),
            details.to_string().into(),
            request_id.into(),
            now.into(),
        ],
    )
}

#[derive(Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub tenant_id: Option<String>,
    pub actor_key_id: Option<String>,
    pub actor_user_id: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub details_json: Option<String>,
    pub request_id: Option<String>,
    pub created_at: i64,
}

impl Row {
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "tenant_id": self.tenant_id, "actor_key_id": self.actor_key_id,
            "actor_user_id": self.actor_user_id, "action": self.action, "target_type": self.target_type,
            "target_id": self.target_id,
            "details": self.details_json.as_deref().and_then(|d| serde_json::from_str::<Value>(d).ok()).unwrap_or(json!({})),
            "request_id": self.request_id,
            "created_at": pylota_identity_core::days::rfc3339(self.created_at),
        })
    }
}

/// Filters of the list; `scope` restricts rows to a tenant set: `All` (platform), the tenants of a
/// partner, or one tenant.
pub enum Within<'a> {
    All,
    Partner(&'a str),
    Tenant(&'a str),
}

#[allow(clippy::too_many_arguments)]
pub fn list(
    within: Within<'_>,
    tenant_id: Option<&str>,
    actor_key_id: Option<&str>,
    action: Option<&str>,
    target_id: Option<&str>,
    after: Option<i64>,
    before: Option<i64>,
    keyset: Option<(i64, String)>,
    limit: u32,
) -> Stmt {
    let (wkind, wval) = match within {
        Within::All => ("all", String::new()),
        Within::Partner(p) => ("partner", p.to_owned()),
        Within::Tenant(t) => ("tenant", t.to_owned()),
    };
    let (kc, ki) = keyset.map_or((None, None), |(c, i)| (Some(c), Some(i)));
    Stmt::new(
        "SELECT a.id, a.tenant_id, a.actor_key_id, a.actor_user_id, a.action, a.target_type, a.target_id, a.details_json, a.request_id, a.created_at \
         FROM audit_log a LEFT JOIN tenants t ON t.id = a.tenant_id \
         WHERE (?1 = 'all' OR (?1 = 'partner' AND t.partner_id = ?2) OR (?1 = 'tenant' AND a.tenant_id = ?2)) \
         AND (?3 IS NULL OR a.tenant_id = ?3) AND (?4 IS NULL OR a.actor_key_id = ?4) AND (?5 IS NULL OR a.action = ?5) \
         AND (?6 IS NULL OR a.target_id = ?6) AND (?7 IS NULL OR a.created_at > ?7) AND (?8 IS NULL OR a.created_at < ?8) \
         AND (?9 IS NULL OR a.created_at < ?9 OR (a.created_at = ?9 AND a.id < ?10)) \
         ORDER BY a.created_at DESC, a.id DESC LIMIT ?11",
        vec![
            wkind.into(),
            wval.into(),
            tenant_id.map(str::to_owned).into(),
            actor_key_id.map(str::to_owned).into(),
            action.map(str::to_owned).into(),
            target_id.map(str::to_owned).into(),
            after.into(),
            before.into(),
            kc.into(),
            ki.into(),
            (limit + 1).into(),
        ],
    )
}
