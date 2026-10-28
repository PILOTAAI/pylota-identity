//! `rpc_intents` (Design conventions § 9): a Durable Object call that must follow a D1 change is
//! written in the same batch, made after commit, and deleted when it succeeds; the every-minute cron
//! re-sends what is left.

use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;

#[allow(clippy::too_many_arguments)]
pub fn insert(
    id: &str,
    class: &str,
    object_id: &str,
    tenant_id: &str,
    identity_id: Option<&str>,
    op: &str,
    body_json: &str,
    now: i64,
) -> Stmt {
    Stmt::new(
        "INSERT OR IGNORE INTO rpc_intents (id, class, object_id, tenant_id, identity_id, op, body_json, occurred_at, attempts, next_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?8, ?8)",
        vec![
            id.into(),
            class.into(),
            object_id.into(),
            tenant_id.into(),
            identity_id.map(str::to_owned).into(),
            op.into(),
            body_json.into(),
            now.into(),
        ],
    )
}

#[derive(Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub class: String,
    pub object_id: String,
    pub tenant_id: String,
    pub identity_id: Option<String>,
    pub op: String,
    pub body_json: String,
    pub attempts: i64,
}

pub fn due(now: i64, limit: u32) -> Stmt {
    Stmt::new(
        "SELECT id, class, object_id, tenant_id, identity_id, op, body_json, attempts FROM rpc_intents WHERE next_at <= ?1 ORDER BY next_at LIMIT ?2",
        vec![now.into(), limit.into()],
    )
}

pub fn done(id: &str) -> Stmt {
    Stmt::new("DELETE FROM rpc_intents WHERE id = ?1", vec![id.into()])
}

/// Backs off 1, 2, 4, … minutes, at most an hour.
pub fn retry(id: &str, attempts: i64, now: i64) -> Stmt {
    let delay = 60_000i64 << attempts.clamp(0, 6);
    Stmt::new(
        "UPDATE rpc_intents SET attempts = attempts + 1, next_at = ?2 WHERE id = ?1",
        vec![id.into(), (now + delay.min(3_600_000)).into()],
    )
}
