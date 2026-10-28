//! `jobs` and `erasure_requests` (Data model § 1, Privacy § 4): what identity deletion writes in M6.
//! The `JobRunner` that runs them arrives with M14.

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

/// An erasure must finish within 24 hours of its first request (NFR-PRV-1).
pub const ERASURE_DEADLINE_MS: i64 = 24 * 3_600_000;

#[allow(clippy::too_many_arguments)]
pub fn insert_job(
    id: &str,
    tenant_id: &str,
    kind: &str,
    runner_do_id: &str,
    params: &Value,
    key_id: &str,
    now: i64,
) -> Stmt {
    Stmt::new(
        "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_by_key_id, created_at, updated_at) VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?6, ?7, ?7)",
        vec![
            id.into(),
            tenant_id.into(),
            kind.into(),
            runner_do_id.into(),
            params.to_string().into(),
            key_id.into(),
            now.into(),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn insert_erasure(
    id: &str,
    tenant_id: &str,
    job_id: &str,
    scope: &str,
    identity_id: Option<&str>,
    reason: &str,
    key_id: &str,
    deadline_at: i64,
    now: i64,
) -> Stmt {
    Stmt::new(
        "INSERT INTO erasure_requests (id, tenant_id, job_id, scope, identity_id, reason, status, created_by_key_id, deadline_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7, ?8, ?9)",
        vec![
            id.into(),
            tenant_id.into(),
            job_id.into(),
            scope.into(),
            identity_id.map(str::to_owned).into(),
            reason.into(),
            key_id.into(),
            deadline_at.into(),
            now.into(),
        ],
    )
}

/// [`insert_erasure`] for a request that resumes `resumes_id` (Privacy § 6.1).
pub fn resuming(mut s: Stmt, resumes_id: &str) -> Stmt {
    s.sql = "INSERT INTO erasure_requests (id, tenant_id, job_id, scope, identity_id, reason, status, created_by_key_id, deadline_at, created_at, resumes_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7, ?8, ?9, ?10)";
    s.params.push(resumes_id.into());
    s
}

#[derive(Clone, Debug, Deserialize)]
pub struct Erasure {
    pub id: String,
    pub tenant_id: String,
    pub job_id: String,
    pub scope: String,
    pub identity_id: Option<String>,
    pub target_id: Option<String>,
    pub status: String,
    pub receipt_json: Option<String>,
    pub created_by_key_id: Option<String>,
    pub resumes_id: Option<String>,
    pub deadline_at: i64,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

pub fn get_erasure(id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, job_id, scope, identity_id, target_id, status, receipt_json, created_by_key_id, resumes_id, deadline_at, created_at, completed_at FROM erasure_requests WHERE id = ?1",
        vec![id.into()],
    )
}

/// The latest identity-scope request of an identity.
pub fn latest_identity_erasure(tenant_id: &str, identity_id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, job_id, scope, identity_id, target_id, status, receipt_json, created_by_key_id, resumes_id, deadline_at, created_at, completed_at FROM erasure_requests \
         WHERE tenant_id = ?1 AND scope = 'identity' AND identity_id = ?2 ORDER BY created_at DESC, id DESC LIMIT 1",
        vec![tenant_id.into(), identity_id.into()],
    )
}

impl Erasure {
    /// The ErasureRequest object.
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "tenant_id": self.tenant_id, "scope": self.scope, "status": self.status,
            "identity_id": self.identity_id, "target_id": self.target_id, "resumes_id": self.resumes_id,
            "created_at": rfc3339(self.created_at), "deadline_at": rfc3339(self.deadline_at),
            "completed_at": self.completed_at.map(rfc3339),
            "receipt": self.receipt_json.as_deref().and_then(|r| serde_json::from_str::<Value>(r).ok()),
            "created_by_key_id": self.created_by_key_id,
        })
    }
}
