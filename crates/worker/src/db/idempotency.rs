//! `idempotency_records` for non-mail requests (REST API › Idempotency): scoped per calling key and per
//! tenant (or per partner, or the deployment), kept 30 days. One-time secrets are stripped before a
//! response is stored.

use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;

pub const KEEP_MS: i64 = 30 * 86_400_000;

#[derive(Debug, Deserialize)]
pub struct Row {
    pub fingerprint: String,
    pub status: String,
    pub response_status: Option<i64>,
    pub response_body: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn begin(
    scope: &str,
    key_id: &str,
    tenant_id: Option<&str>,
    idem_key: &str,
    method: &str,
    path: &str,
    fingerprint: &str,
    now: i64,
) -> Stmt {
    Stmt::new(
        "INSERT OR IGNORE INTO idempotency_records (scope, key_id, tenant_id, identity_id, idem_key, method, path, fingerprint, status, created_at, expires_at) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, 'in_progress', ?8, ?9)",
        vec![
            scope.into(),
            key_id.into(),
            tenant_id.map(str::to_owned).into(),
            idem_key.into(),
            method.into(),
            path.into(),
            fingerprint.into(),
            now.into(),
            (now + KEEP_MS).into(),
        ],
    )
}

pub fn get(scope: &str, key_id: &str, idem_key: &str, now: i64) -> Stmt {
    Stmt::new(
        "SELECT fingerprint, status, response_status, response_body FROM idempotency_records WHERE scope = ?1 AND key_id = ?2 AND idem_key = ?3 AND expires_at > ?4",
        vec![scope.into(), key_id.into(), idem_key.into(), now.into()],
    )
}

/// Removes an expired record so the key can be used again.
pub fn expire(scope: &str, key_id: &str, idem_key: &str, now: i64) -> Stmt {
    Stmt::new(
        "DELETE FROM idempotency_records WHERE scope = ?1 AND key_id = ?2 AND idem_key = ?3 AND expires_at <= ?4",
        vec![scope.into(), key_id.into(), idem_key.into(), now.into()],
    )
}

pub fn complete(scope: &str, key_id: &str, idem_key: &str, status: u16, body: &str) -> Stmt {
    Stmt::new(
        "UPDATE idempotency_records SET status = 'completed', response_status = ?4, response_body = ?5 WHERE scope = ?1 AND key_id = ?2 AND idem_key = ?3",
        vec![
            scope.into(),
            key_id.into(),
            idem_key.into(),
            u32::from(status).into(),
            body.into(),
        ],
    )
}

pub fn abandon(scope: &str, key_id: &str, idem_key: &str) -> Stmt {
    Stmt::new(
        "DELETE FROM idempotency_records WHERE scope = ?1 AND key_id = ?2 AND idem_key = ?3 AND status = 'in_progress'",
        vec![scope.into(), key_id.into(), idem_key.into()],
    )
}
