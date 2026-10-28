//! `rpc_intents` (Design conventions § 9): a Durable Object call that must follow a D1 change is
//! written in the same batch, made after commit, and deleted when it succeeds; the every-minute cron
//! re-sends what is left.

use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;

const INSERT: &str = "INSERT OR IGNORE INTO rpc_intents (id, class, object_id, tenant_id, identity_id, op, body_json, occurred_at, attempts, next_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?8 + 60000, ?8)";

/// `insert` for a row of a tenant created in the same batch: written only once the tenant row exists
/// (`handlers::tenants::create`).
pub fn when_tenant(mut s: Stmt) -> Stmt {
    debug_assert_eq!(s.sql, INSERT);
    s.sql = "INSERT OR IGNORE INTO rpc_intents (id, class, object_id, tenant_id, identity_id, op, body_json, occurred_at, attempts, next_at, created_at) SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?8 + 60000, ?8 WHERE EXISTS (SELECT 1 FROM tenants WHERE id = ?4)";
    s
}

/// `insert` for the `init` of a mailbox the cron just minted: written only if this run's ID won.
pub fn when_minted(mut s: Stmt) -> Stmt {
    debug_assert_eq!(s.sql, INSERT);
    s.sql = "INSERT OR IGNORE INTO rpc_intents (id, class, object_id, tenant_id, identity_id, op, body_json, occurred_at, attempts, next_at, created_at) SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?8 + 60000, ?8 WHERE EXISTS (SELECT 1 FROM identities WHERE id = ?5 AND mailbox_do_id = ?3)";
    s
}

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
        INSERT,
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

/// Rows read per cron run (Design conventions § 9).
pub const BATCH: u32 = 100;

#[derive(Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub class: String,
    pub object_id: String,
    pub tenant_id: String,
    pub identity_id: Option<String>,
    pub op: String,
    pub body_json: String,
    pub occurred_at: i64,
    pub attempts: i64,
}

pub fn due(now: i64, limit: u32) -> Stmt {
    Stmt::new(
        "SELECT id, class, object_id, tenant_id, identity_id, op, body_json, occurred_at, attempts FROM rpc_intents WHERE next_at <= ?1 ORDER BY occurred_at, created_at, id LIMIT ?2",
        vec![now.into(), limit.into()],
    )
}

/// Every pending intent of one object, in the order their changes committed.
pub fn of_object(object_id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, class, object_id, tenant_id, identity_id, op, body_json, occurred_at, attempts FROM rpc_intents WHERE object_id = ?1 ORDER BY occurred_at, created_at, id LIMIT 100",
        vec![object_id.into()],
    )
}

#[derive(Deserialize)]
pub struct Object {
    pub object_id: String,
}

/// The objects a request wrote intents for (`{request_id}:…`).
pub fn objects_of_request(request_id: &str) -> Stmt {
    Stmt::new(
        "SELECT DISTINCT object_id FROM rpc_intents WHERE substr(id, 1, length(?1) + 1) = ?1 || ':'",
        vec![request_id.into()],
    )
}

pub fn done(id: &str) -> Stmt {
    Stmt::new("DELETE FROM rpc_intents WHERE id = ?1", vec![id.into()])
}

/// Backs off `min(60 s × 2^attempts, 15 min)`.
pub fn retry(id: &str, attempts: i64, now: i64) -> Stmt {
    let delay = (60_000i64 << attempts.clamp(0, 4)).min(900_000);
    Stmt::new(
        "UPDATE rpc_intents SET attempts = attempts + 1, next_at = ?2 WHERE id = ?1",
        vec![id.into(), (now + delay).into()],
    )
}
