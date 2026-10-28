//! `webhook_endpoints`, `webhook_deliveries` and `webhook_held` (Data model § 1, Webhooks).

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::webhooks::client::Endpoint;
use crate::webhooks::matching::Scope;

/// `SELECT` of the endpoint columns, with its partner's status, then the literal tail.
macro_rules! select_endpoints {
    ($tail:literal $(,)?) => {
        concat!(
            "SELECT e.id, e.tenant_id, e.partner_id, e.url, e.description, e.event_types_json, e.identity_ids_json, e.enabled, e.disabled_reason, e.created_by_key_id, e.created_by_user_id, e.secret_enc, e.prev_secret_enc, e.prev_secret_expires_at, e.headers_sealed, e.header_names_json, e.consecutive_failures, e.created_at, e.updated_at, \
             COALESCE(e.partner_id, t.partner_id) AS owner_partner_id, p.status AS partner_status \
             FROM webhook_endpoints e LEFT JOIN tenants t ON t.id = e.tenant_id LEFT JOIN partners p ON p.id = COALESCE(e.partner_id, t.partner_id)",
            $tail
        )
    };
}

/// `SELECT` of the delivery columns, then the literal tail.
macro_rules! select_deliveries {
    ($tail:literal $(,)?) => {
        concat!(
            "SELECT id, endpoint_id, event_id, event_type, attempt, status, http_status, error, duration_ms, next_attempt_at, created_at FROM webhook_deliveries",
            $tail
        )
    };
}

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub tenant_id: Option<String>,
    pub partner_id: Option<String>,
    pub url: String,
    pub description: Option<String>,
    pub event_types_json: String,
    pub identity_ids_json: Option<String>,
    pub enabled: i64,
    pub disabled_reason: Option<String>,
    pub created_by_key_id: Option<String>,
    pub created_by_user_id: Option<String>,
    pub secret_enc: String,
    pub prev_secret_enc: Option<String>,
    pub prev_secret_expires_at: Option<i64>,
    pub headers_sealed: Option<String>,
    pub header_names_json: String,
    pub consecutive_failures: i64,
    pub created_at: i64,
    pub updated_at: i64,
    /// The endpoint's partner, or its tenant's.
    pub owner_partner_id: Option<String>,
    pub partner_status: Option<String>,
}

impl Row {
    pub fn scope(&self) -> &'static str {
        match (&self.tenant_id, &self.partner_id) {
            (Some(_), _) => "tenant",
            (None, Some(_)) => "partner",
            (None, None) => "platform",
        }
    }

    pub fn event_types(&self) -> Vec<String> {
        serde_json::from_str(&self.event_types_json).unwrap_or_default()
    }

    pub fn matching(&self) -> Scope {
        Scope {
            id: self.id.clone(),
            tenant_id: self.tenant_id.clone(),
            partner_id: self.partner_id.clone(),
            event_types_json: self.event_types_json.clone(),
            identity_ids_json: self.identity_ids_json.clone(),
        }
    }

    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            id: self.id.clone(),
            url: self.url.clone(),
            secret_enc: self.secret_enc.clone(),
            prev_secret_enc: self.prev_secret_enc.clone(),
            prev_secret_expires_at: self.prev_secret_expires_at,
            headers_sealed: self.headers_sealed.clone(),
        }
    }

    /// The `Webhook` object.
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "scope": self.scope(), "partner_id": self.partner_id, "tenant_id": self.tenant_id,
            "url": self.url, "events": self.event_types(),
            "identity_ids": self.identity_ids_json.as_deref().and_then(|j| serde_json::from_str::<Value>(j).ok()).filter(|v| !v.is_null()),
            "description": self.description,
            "header_names": serde_json::from_str::<Value>(&self.header_names_json).unwrap_or_else(|_| json!([])),
            "enabled": self.enabled == 1, "disabled_reason": self.disabled_reason,
            "created_at": rfc3339(self.created_at), "updated_at": rfc3339(self.updated_at),
        })
    }
}

pub fn get(id: &str) -> Stmt {
    Stmt::new(select_endpoints!(" WHERE e.id = ?1"), vec![id.into()])
}

/// The endpoints a list shows, newest first after `after` (`created_at`, `id`): by scope.
pub fn list(
    tenant_id: Option<&str>,
    partner_id: Option<&str>,
    after: Option<(i64, String)>,
    limit: u32,
) -> Stmt {
    let (cursor_t, cursor_id) = match after {
        Some((t, id)) => (Some(t), Some(id)),
        None => (None, None),
    };
    Stmt::new(
        select_endpoints!(
            " WHERE \
             CASE WHEN ?1 IS NOT NULL THEN e.tenant_id = ?1 WHEN ?2 IS NOT NULL THEN e.tenant_id IS NULL AND e.partner_id = ?2 ELSE e.tenant_id IS NULL AND e.partner_id IS NULL END \
             AND (?3 IS NULL OR e.created_at < ?3 OR (e.created_at = ?3 AND e.id < ?4)) \
             ORDER BY e.created_at DESC, e.id DESC LIMIT ?5"
        ),
        vec![
            tenant_id.map(str::to_owned).into(),
            partner_id.map(str::to_owned).into(),
            cursor_t.into(),
            cursor_id.into(),
            (i64::from(limit) + 1).into(),
        ],
    )
}

/// Endpoints of one scope (the limit of 20).
pub fn count(tenant_id: Option<&str>, partner_id: Option<&str>) -> Stmt {
    Stmt::new(
        "SELECT COUNT(*) AS n FROM webhook_endpoints WHERE CASE WHEN ?1 IS NOT NULL THEN tenant_id = ?1 WHEN ?2 IS NOT NULL THEN tenant_id IS NULL AND partner_id = ?2 ELSE tenant_id IS NULL AND partner_id IS NULL END",
        vec![
            tenant_id.map(str::to_owned).into(),
            partner_id.map(str::to_owned).into(),
        ],
    )
}

/// A delivery row as the API shows it.
#[derive(Clone, Debug, Deserialize)]
pub struct Delivery {
    pub id: String,
    pub endpoint_id: String,
    pub event_id: String,
    pub event_type: String,
    pub attempt: i64,
    pub status: String,
    pub http_status: Option<i64>,
    pub error: Option<String>,
    pub duration_ms: Option<i64>,
    pub next_attempt_at: Option<i64>,
    pub created_at: i64,
}

impl Delivery {
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "webhook_id": self.endpoint_id, "event_id": self.event_id, "event_type": self.event_type,
            "attempt": self.attempt, "status": self.status, "http_status": self.http_status, "error": self.error,
            "duration_ms": self.duration_ms, "next_attempt_at": self.next_attempt_at.map(rfc3339),
            "created_at": rfc3339(self.created_at),
        })
    }
}

pub fn delivery(id: &str) -> Stmt {
    Stmt::new(select_deliveries!(" WHERE id = ?1"), vec![id.into()])
}

/// An endpoint's delivery log, newest first, with its filters and keyset.
#[allow(clippy::too_many_arguments)]
pub fn deliveries(
    endpoint_id: &str,
    status: Option<String>,
    event_type: Option<String>,
    after: Option<i64>,
    cursor: Option<(i64, String)>,
    limit: u32,
) -> Stmt {
    let (ct, cid) = match cursor {
        Some((t, id)) => (Some(t), Some(id)),
        None => (None, None),
    };
    Stmt::new(
        select_deliveries!(
            " WHERE endpoint_id = ?1 AND (?2 IS NULL OR status = ?2) AND (?3 IS NULL OR event_type = ?3) AND (?4 IS NULL OR created_at >= ?4) AND (?5 IS NULL OR created_at < ?5 OR (created_at = ?5 AND id < ?6)) ORDER BY created_at DESC, id DESC LIMIT ?7"
        ),
        vec![
            endpoint_id.into(),
            status.into(),
            event_type.into(),
            after.into(),
            ct.into(),
            cid.into(),
            (i64::from(limit) + 1).into(),
        ],
    )
}

/// One `PATCH`: each column changes only when its flag is set.
pub struct Update {
    pub url: Option<String>,
    pub event_types_json: Option<String>,
    pub identity_ids_json: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub enabled: Option<bool>,
    pub disabled_reason: Option<Option<String>>,
    /// Re-enabled by this caller: failures reset, creator recorded.
    pub reenabled_by: Option<(String, Option<String>)>,
    pub clear_headers: bool,
}

pub fn update(id: &str, u: Update, now: i64) -> Stmt {
    let flag = |b: bool| i64::from(b);
    let (re, re_key, re_user) = match u.reenabled_by {
        Some((k, user)) => (true, Some(k), user),
        None => (false, None, None),
    };
    Stmt::new(
        "UPDATE webhook_endpoints SET \
           url = COALESCE(?3, url), \
           event_types_json = COALESCE(?4, event_types_json), \
           identity_ids_json = CASE WHEN ?5 = 1 THEN ?6 ELSE identity_ids_json END, \
           description = CASE WHEN ?7 = 1 THEN ?8 ELSE description END, \
           enabled = COALESCE(?9, enabled), \
           disabled_reason = CASE WHEN ?10 = 1 THEN ?11 ELSE disabled_reason END, \
           consecutive_failures = CASE WHEN ?12 = 1 THEN 0 ELSE consecutive_failures END, \
           created_by_key_id = CASE WHEN ?12 = 1 THEN ?13 ELSE created_by_key_id END, \
           created_by_user_id = CASE WHEN ?12 = 1 THEN ?14 ELSE created_by_user_id END, \
           headers_sealed = CASE WHEN ?15 = 1 THEN NULL ELSE headers_sealed END, \
           header_names_json = CASE WHEN ?15 = 1 THEN '[]' ELSE header_names_json END, \
           updated_at = ?2 \
         WHERE id = ?1",
        vec![
            id.into(),
            now.into(),
            u.url.into(),
            u.event_types_json.into(),
            flag(u.identity_ids_json.is_some()).into(),
            u.identity_ids_json.flatten().into(),
            flag(u.description.is_some()).into(),
            u.description.flatten().into(),
            u.enabled.map(i64::from).into(),
            flag(u.disabled_reason.is_some()).into(),
            u.disabled_reason.flatten().into(),
            flag(re).into(),
            re_key.into(),
            re_user.into(),
            flag(u.clear_headers).into(),
        ],
    )
}

/// Records one attempt; ignored when the attempt is already recorded.
#[allow(clippy::too_many_arguments)]
pub fn record(
    id: &str,
    endpoint_id: &str,
    tenant_id: Option<&str>,
    event_id: &str,
    event_type: &str,
    attempt: u32,
    status: &str,
    http_status: Option<u16>,
    error: Option<&str>,
    duration_ms: i64,
    next_attempt_at: Option<i64>,
    now: i64,
) -> Stmt {
    Stmt::new(
        "INSERT OR IGNORE INTO webhook_deliveries (id, endpoint_id, tenant_id, event_id, event_type, attempt, status, http_status, error, duration_ms, next_attempt_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        vec![
            id.into(),
            endpoint_id.into(),
            tenant_id.map(str::to_owned).into(),
            event_id.into(),
            event_type.into(),
            i64::from(attempt).into(),
            status.into(),
            http_status.map(i64::from).into(),
            error.map(str::to_owned).into(),
            duration_ms.into(),
            next_attempt_at.into(),
            now.into(),
        ],
    )
}
