//! Authentication and the order of checks ([Security §4.2, §5.2]): the bearer key is verified against
//! its row in constant time, then its status, mode, tenant and partner; the route's permissions and key
//! levels follow, before any target is read.
//!
//! [Security §4.2, §5.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::keys;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{Background as _, Clock as _, ControlDb as _, Platform};
use serde::Deserialize;
use serde_json::json;

use crate::errors::ApiError;
use crate::router::RouteSpec;

pub use pylota_identity_core::keys::{Level, PERMISSIONS, level_can_hold};

/// The permissions a provisional agent sign-up's keys keep (Agent self-sign-up §4.1).
pub const PROVISIONAL: &[&str] = &[
    "identities:read",
    "messages:read",
    "messages:send",
    "messages:write",
    "attachments:read",
    "search:read",
    "events:read",
    "usage:read",
];

/// The resolved scope of the calling key.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyScope {
    pub key_id: String,
    pub name: String,
    pub level: Level,
    pub mode: keys::Mode,
    pub partner_id: Option<String>,
    pub tenant_id: Option<String>,
    pub identity_id: Option<String>,
    /// Listed, plus the implicit `usage:read` of tenant and identity keys.
    pub permissions: Vec<String>,
    pub mcp_tools: Option<serde_json::Value>,
    pub expires_at: Option<i64>,
    pub created_by_user_id: Option<String>,
    pub created_by_role: Option<String>,
    pub created_by_level: String,
    /// The partner whose tenants a tenant or identity key belongs to.
    pub tenant_partner_id: Option<String>,
    pub provisional: bool,
    /// A console session's principal: the signed-in person's `usr_` ID (also in `key_id`). A person
    /// takes the decisions reserved for people (Console › What the console is).
    pub person: Option<String>,
}

impl KeyScope {
    /// Whether the key holds `permission`; `webhooks:manage` includes `webhooks:read`.
    pub fn has(&self, permission: &str) -> bool {
        self.permissions
            .iter()
            .any(|p| p == permission || (permission == "webhooks:read" && p == "webhooks:manage"))
    }

    /// The partner a key acts for: its own (partner key) or its tenant's.
    pub fn effective_partner(&self) -> Option<&str> {
        self.partner_id
            .as_deref()
            .or(self.tenant_partner_id.as_deref())
    }
}

#[derive(Deserialize)]
struct KeyRow {
    id: String,
    hash: String,
    prev_hash: Option<String>,
    prev_expires_at: Option<i64>,
    name: String,
    level: String,
    partner_id: Option<String>,
    tenant_id: Option<String>,
    identity_id: Option<String>,
    mode: String,
    permissions_json: String,
    mcp_tools_json: Option<String>,
    created_by_user_id: Option<String>,
    created_by_role: Option<String>,
    created_by_level: String,
    expires_at: Option<i64>,
    revoked_at: Option<i64>,
    last_used_at: Option<i64>,
    tenant_status: Option<String>,
    tenant_partner_id: Option<String>,
    provisional_until: Option<i64>,
    partner_status: Option<String>,
}

const KEY_QUERY: &str = "SELECT k.id, k.hash, k.prev_hash, k.prev_expires_at, k.name, k.level, k.partner_id, \
    k.tenant_id, k.identity_id, k.mode, k.permissions_json, k.mcp_tools_json, k.created_by_user_id, \
    k.created_by_role, k.created_by_level, k.expires_at, k.revoked_at, k.last_used_at, \
    t.status AS tenant_status, t.partner_id AS tenant_partner_id, t.provisional_until, \
    p.status AS partner_status \
    FROM api_keys k LEFT JOIN tenants t ON t.id = k.tenant_id \
    LEFT JOIN partners p ON p.id = COALESCE(k.partner_id, t.partner_id) \
    WHERE k.lookup = ?1";

/// A fixed value compared against when no row was found, so a miss and a mismatch cost the same.
const DUMMY_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn unauthenticated() -> ApiError {
    ApiError::new(ErrorCode::Unauthenticated, "A valid API key is required.")
}

/// Verifies `Authorization: Bearer <token>` (§4.2 steps 1–9) and schedules the throttled
/// `last_used_at` write (§4.3).
pub async fn authenticate<P: Platform>(
    p: &Rc<P>,
    authorization: Option<&str>,
) -> Result<KeyScope, ApiError> {
    let token = authorization
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .ok_or_else(unauthenticated)?;
    let parts = keys::parse(token).ok_or_else(unauthenticated)?;
    let row: Option<KeyRow> = p
        .db()
        .first(Stmt::new(KEY_QUERY, vec![parts.lookup.into()]))
        .await?;
    let pepper = &p.secrets().key_pepper;
    let now = p.clock().now_ms();
    let Some(row) = row else {
        let _ = keys::verify(pepper, token, DUMMY_HASH);
        return Err(unauthenticated());
    };
    let current = keys::verify(pepper, token, &row.hash);
    let previous = !current
        && row.prev_expires_at.is_some_and(|t| now < t)
        && row
            .prev_hash
            .as_deref()
            .is_some_and(|h| keys::verify(pepper, token, h));
    if !current && !previous {
        return Err(unauthenticated());
    }
    if row.revoked_at.is_some() {
        return Err(ApiError::new(
            ErrorCode::KeyRevoked,
            "This key was revoked.",
        ));
    }
    if row.expires_at.is_some_and(|e| e <= now) {
        return Err(ApiError::new(
            ErrorCode::KeyExpired,
            "This key has expired.",
        ));
    }
    if row.mode != parts.mode.as_str() {
        return Err(unauthenticated());
    }
    if matches!(row.tenant_status.as_deref(), Some("erasing" | "erased")) {
        return Err(ApiError::new(
            ErrorCode::KeyRevoked,
            "This key was revoked.",
        ));
    }
    if row.partner_status.as_deref() == Some("suspended") {
        return Err(ApiError::new(
            ErrorCode::PartnerSuspended,
            "The partner is suspended.",
        ));
    }
    let level = Level::parse(&row.level).ok_or_else(ApiError::internal)?;
    let mut permissions: Vec<String> =
        serde_json::from_str(&row.permissions_json).map_err(|_| ApiError::internal())?;
    if level <= Level::Tenant && !permissions.iter().any(|p| p == "usage:read") {
        permissions.push("usage:read".to_owned());
    }
    let provisional = row.provisional_until.is_some();
    if provisional {
        permissions.retain(|p| PROVISIONAL.contains(&p.as_str()));
    }

    // §4.3: at most one write per key per minute, after the response.
    if row.last_used_at.is_none_or(|t| t < now - 60_000) {
        let db_key = row.id.clone();
        let stmt = Stmt::new(
            "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2 AND (last_used_at IS NULL OR last_used_at < ?1 - 60000)",
            vec![now.into(), db_key.into()],
        );
        let owner = p.clone();
        p.background().wait_until(async move {
            if owner.db().execute(stmt).await.is_err() {
                crate::log::warn("last_used_write_failed", &[]);
            }
        });
    }

    Ok(KeyScope {
        key_id: row.id,
        name: row.name,
        level,
        mode: parts.mode,
        partner_id: row.partner_id,
        tenant_id: row.tenant_id,
        identity_id: row.identity_id,
        permissions,
        mcp_tools: row
            .mcp_tools_json
            .and_then(|t| serde_json::from_str(&t).ok()),
        expires_at: row.expires_at,
        created_by_user_id: row.created_by_user_id,
        created_by_role: row.created_by_role,
        created_by_level: row.created_by_level,
        tenant_partner_id: row.tenant_partner_id,
        provisional,
        person: None,
    })
}

/// Steps 2 and 3 of §5.2: the route's permissions (and its foreign ones when `foreign` is true), then
/// its key levels. Reads only the key and the route.
pub fn check_route(scope: &KeyScope, spec: &RouteSpec, foreign: bool) -> Result<(), ApiError> {
    let mut missing: Vec<&str> = spec
        .permissions
        .iter()
        .copied()
        .filter(|p| !scope.has(p))
        .collect();
    if foreign {
        missing.extend(
            spec.foreign_permissions
                .iter()
                .copied()
                .filter(|p| !scope.has(p)),
        );
    }
    if !missing.is_empty() {
        let mut e = ApiError::permission_denied(&missing);
        if scope.provisional {
            e = e.details(json!({ "required": missing, "reason": "signup_unverified" }));
        }
        return Err(e);
    }
    if !spec.levels.is_empty() && !spec.levels.contains(&scope.level) {
        return Err(ApiError::scope_denied(None));
    }
    Ok(())
}

/// Whether API keys may take the decisions reserved for people on this tenant (Workspace policy §3):
/// `PM_QUARANTINE_KEY_RELEASE` on, `PM_CONSOLE` off, or the tenant's `quarantine.key_release` and then
/// only for a platform key or the tenant's own partner key.
pub fn key_may_decide<P: Platform>(
    p: &P,
    scope: &KeyScope,
    tenant_key_release: bool,
    tenant_partner: Option<&str>,
) -> bool {
    if scope.person.is_some() {
        return true;
    }
    let cfg = p.config();
    let global = match &cfg.console {
        None => true,
        Some(c) => c.quarantine_key_release,
    };
    if global {
        return true;
    }
    tenant_key_release
        && (scope.level == Level::Platform
            || (scope.level == Level::Partner
                && scope.partner_id.is_some()
                && scope.partner_id.as_deref() == tenant_partner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_hold_permissions_as_the_table_says() {
        assert!(level_can_hold(Level::Platform, "platform:ops"));
        assert!(!level_can_hold(Level::Partner, "partners:manage"));
        assert!(level_can_hold(Level::Partner, "tenants:manage"));
        assert!(!level_can_hold(Level::Tenant, "tenants:manage"));
        assert!(!level_can_hold(Level::Identity, "audit:read"));
        assert!(!level_can_hold(Level::Identity, "tenants:erase"));
        assert!(level_can_hold(Level::Identity, "identities:sign"));
        assert!(!level_can_hold(Level::Partner, "identities:sign"));
        assert!(!level_can_hold(Level::Tenant, "made:up"));
        assert!(Level::Identity < Level::Tenant && Level::Partner < Level::Platform);
    }
}
