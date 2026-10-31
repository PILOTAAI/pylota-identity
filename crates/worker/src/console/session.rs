//! Console sessions ([Console › Sessions]): a `__Host-` cookie whose value is never stored, only its
//! keyed hash under the `link` key; one D1 query loads the session, the person and their role in the
//! active workspace; 7 days rolling, 30 days absolute.
//!
//! [Console › Sessions]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#sessions

use pylota_identity_core::console as cc;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Platform, Rng as _};
use serde::Deserialize;

use crate::errors::ApiError;
use crate::keyring::{self, Purpose};

pub const COOKIE: &str = "__Host-pm_session";

/// The signed-in person and their active workspace.
#[derive(Clone, Debug, Deserialize)]
pub struct Session {
    pub id_hash: String,
    pub key_kid: String,
    pub user_id: String,
    pub tenant_id: Option<String>,
    pub csrf_secret: String,
    pub authenticated_at: i64,
    pub last_seen_at: i64,
    pub email: String,
    pub name: Option<String>,
    /// `None` when the person has no membership in the active workspace (removed, W9).
    pub role: Option<String>,
}

impl Session {
    pub fn role(&self) -> Option<cc::Role> {
        self.role.as_deref().and_then(cc::Role::parse)
    }
}

/// The cookie value a request carries, if any.
pub fn cookie_value(cookie_header: Option<&str>) -> Option<String> {
    cookie_header?.split(';').find_map(|c| {
        let (n, v) = c.trim().split_once('=')?;
        (n == COOKIE).then(|| v.to_owned())
    })
}

pub fn set_cookie(value: &str) -> String {
    format!("{COOKIE}={value}; Path=/; Secure; HttpOnly; SameSite=Lax")
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0")
}

/// Loads the session a cookie value names, refreshing `last_seen_at` at most once a minute and
/// re-hashing it under the current `link` key after a rotation.
pub async fn load<P: Platform>(p: &P, value: &str, now: i64) -> Result<Option<Session>, ApiError> {
    let Some(kid) = cc::token_kid(value) else {
        return Ok(None);
    };
    let keys = keyring::keys(p, Purpose::Link).await?;
    let Some(key) = keys.iter().find(|k| k.kid == kid) else {
        return Ok(None);
    };
    let hash = cc::keyed_hash(&key.secret, value);
    let s: Option<Session> = p
        .db()
        .first(Stmt::new(
            "SELECT s.id_hash, s.key_kid, s.user_id, s.tenant_id, s.csrf_secret, s.authenticated_at, s.last_seen_at, u.email, u.name, m.role \
             FROM sessions s JOIN users u ON u.id = s.user_id AND u.status = 'active' \
             LEFT JOIN members m ON m.tenant_id = s.tenant_id AND m.user_id = s.user_id \
             WHERE s.id_hash = ?1 AND s.revoked_at IS NULL AND s.expires_at > ?2",
            vec![hash.clone().into(), now.into()],
        ))
        .await?;
    let Some(mut s) = s else {
        return Ok(None);
    };
    let current = &keys[0];
    if s.key_kid != current.kid || now - s.last_seen_at >= 60_000 {
        let new_hash = if s.key_kid == current.kid {
            s.id_hash.clone()
        } else {
            cc::keyed_hash(&current.secret, value)
        };
        p.db()
            .execute(Stmt::new(
                "UPDATE sessions SET id_hash = ?2, key_kid = ?3, last_seen_at = ?4, expires_at = ?5 WHERE id_hash = ?1",
                vec![
                    s.id_hash.clone().into(),
                    new_hash.clone().into(),
                    current.kid.clone().into(),
                    now.into(),
                    cc::session_expiry(s.authenticated_at, now).into(),
                ],
            ))
            .await?;
        s.id_hash = new_hash;
        s.key_kid = current.kid.clone();
        s.last_seen_at = now;
    }
    Ok(Some(s))
}

/// Creates a session for `user_id` in `tenant_id` and answers its cookie value. The cookie value is
/// hashed under the current `link` key, which also starts it, so the right key is found again.
pub async fn create<P: Platform>(
    p: &P,
    user_id: &str,
    tenant_id: Option<&str>,
    user_agent: Option<&str>,
    now: i64,
) -> Result<String, ApiError> {
    let key = keyring::current(p, Purpose::Link).await?;
    let value = cc::token(&key.kid, &p.rng().bytes::<32>());
    let csrf = cc::token(&key.kid, &p.rng().bytes::<32>());
    p.db()
        .batch(vec![
            Stmt::new(
                "INSERT INTO sessions (id_hash, key_kid, user_id, tenant_id, csrf_secret, authenticated_at, last_seen_at, expires_at, user_agent_hint) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8)",
                vec![
                    cc::keyed_hash(&key.secret, &value).into(),
                    key.kid.clone().into(),
                    user_id.into(),
                    tenant_id.map(str::to_owned).into(),
                    csrf.into(),
                    now.into(),
                    cc::session_expiry(now, now).into(),
                    user_agent.map(cc::browser_family).into(),
                ],
            ),
            Stmt::new(
                "UPDATE users SET last_login_at = ?2, last_tenant_id = COALESCE(?3, last_tenant_id) WHERE id = ?1",
                vec![
                    user_id.into(),
                    now.into(),
                    tenant_id.map(str::to_owned).into(),
                ],
            ),
        ])
        .await?;
    Ok(value)
}

/// The workspace a new session starts in: the last one used, or the person's first.
pub async fn landing_tenant<P: Platform>(p: &P, user_id: &str) -> Result<Option<String>, ApiError> {
    #[derive(Deserialize)]
    struct T {
        tenant_id: String,
    }
    let t: Option<T> = p
        .db()
        .first(Stmt::new(
            "SELECT m.tenant_id FROM members m JOIN users u ON u.id = m.user_id JOIN tenants t ON t.id = m.tenant_id \
             WHERE m.user_id = ?1 AND t.status IN ('active','suspended') ORDER BY (m.tenant_id = u.last_tenant_id) DESC, m.created_at LIMIT 1",
            vec![user_id.into()],
        ))
        .await?;
    Ok(t.map(|t| t.tenant_id))
}

pub fn revoke(id_hash: &str, now: i64) -> Stmt {
    Stmt::new(
        "UPDATE sessions SET revoked_at = ?2 WHERE id_hash = ?1 AND revoked_at IS NULL",
        vec![id_hash.into(), now.into()],
    )
}

pub fn revoke_all(user_id: &str, now: i64) -> Stmt {
    Stmt::new(
        "UPDATE sessions SET revoked_at = ?2 WHERE user_id = ?1 AND revoked_at IS NULL",
        vec![user_id.into(), now.into()],
    )
}

pub fn switch(id_hash: &str, tenant_id: &str) -> Stmt {
    Stmt::new(
        "UPDATE sessions SET tenant_id = ?2 WHERE id_hash = ?1",
        vec![id_hash.into(), tenant_id.into()],
    )
}
