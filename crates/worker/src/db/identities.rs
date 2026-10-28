//! `identities` (Data model § 1).

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

use super::addresses;

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub tenant_id: String,
    pub username: String,
    pub display_name: String,
    pub purpose: Option<String>,
    pub client_id: Option<String>,
    pub client_fingerprint: Option<String>,
    pub owner_name: Option<String>,
    pub owner_email: Option<String>,
    pub signature_text: Option<String>,
    pub signature_html: Option<String>,
    pub status: String,
    pub pause_reason: Option<String>,
    pub send_policy_json: String,
    pub message_hours: Option<i64>,
    pub metadata_json: String,
    pub mailbox_do_id: String,
    pub is_system: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn get(id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, username, display_name, purpose, client_id, client_fingerprint, owner_name, owner_email, signature_text, signature_html, status, pause_reason, send_policy_json, message_hours, metadata_json, mailbox_do_id, is_system, created_at, updated_at FROM identities WHERE id = ?1",
        vec![id.into()],
    )
}

pub fn by_client_id(tenant_id: &str, client_id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, username, display_name, purpose, client_id, client_fingerprint, owner_name, owner_email, signature_text, signature_html, status, pause_reason, send_policy_json, message_hours, metadata_json, mailbox_do_id, is_system, created_at, updated_at FROM identities WHERE tenant_id = ?1 AND client_id = ?2",
        vec![tenant_id.into(), client_id.into()],
    )
}

/// The usernames that count for `username_taken` (every identity of the tenant but deleted ones).
pub fn usernames(tenant_id: &str) -> Stmt {
    Stmt::new(
        "SELECT username FROM identities WHERE tenant_id = ?1 AND status <> 'deleted'",
        vec![tenant_id.into()],
    )
}

#[derive(Deserialize)]
pub struct Username {
    pub username: String,
}

/// A list, newest first. `tenant` narrows to one tenant, `partner` to the tenants of a partner, `only`
/// to one identity (an identity key). The system identity is never listed.
#[allow(clippy::too_many_arguments)]
pub fn list(
    tenant: Option<&str>,
    partner: Option<&str>,
    only: Option<&str>,
    status: Option<&str>,
    purpose: Option<&str>,
    client_id: Option<&str>,
    keyset: Option<(i64, String)>,
    limit: u32,
) -> Stmt {
    let (kc, ki) = keyset.map_or((None, None), |(c, i)| (Some(c), Some(i)));
    Stmt::new(
        "SELECT id, tenant_id, username, display_name, purpose, client_id, client_fingerprint, owner_name, owner_email, signature_text, signature_html, status, pause_reason, send_policy_json, message_hours, metadata_json, mailbox_do_id, is_system, created_at, updated_at FROM identities \
         WHERE is_system = 0 AND (?1 IS NULL OR tenant_id = ?1) AND (?2 IS NULL OR tenant_id IN (SELECT id FROM tenants WHERE partner_id = ?2)) \
         AND (?3 IS NULL OR id = ?3) AND (?4 IS NULL OR status = ?4) AND (?5 IS NULL OR purpose = ?5) AND (?6 IS NULL OR client_id = ?6) \
         AND (?7 IS NULL OR created_at < ?7 OR (created_at = ?7 AND id < ?8)) ORDER BY created_at DESC, id DESC LIMIT ?9",
        vec![
            tenant.map(str::to_owned).into(),
            partner.map(str::to_owned).into(),
            only.map(str::to_owned).into(),
            status.map(str::to_owned).into(),
            purpose.map(str::to_owned).into(),
            client_id.map(str::to_owned).into(),
            kc.into(),
            ki.into(),
            (limit + 1).into(),
        ],
    )
}

/// The system identity's row, when setup has written one that is still live.
pub fn system() -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, username, display_name, purpose, client_id, client_fingerprint, owner_name, owner_email, signature_text, signature_html, status, pause_reason, send_policy_json, message_hours, metadata_json, mailbox_do_id, is_system, created_at, updated_at FROM identities WHERE is_system = 1 AND status NOT IN ('deleting','deleted')",
        vec![],
    )
}

/// Identities whose mailbox setup left to the every-minute cron (`mailbox_do_id = ''`).
pub fn without_mailbox(limit: u32) -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, username, display_name, purpose, client_id, client_fingerprint, owner_name, owner_email, signature_text, signature_html, status, pause_reason, send_policy_json, message_hours, metadata_json, mailbox_do_id, is_system, created_at, updated_at FROM identities WHERE mailbox_do_id = '' AND status NOT IN ('deleting','deleted') LIMIT ?1",
        vec![limit.into()],
    )
}

impl Row {
    pub fn send_policy(&self) -> Value {
        serde_json::from_str(&self.send_policy_json).unwrap_or_else(|_| json!({}))
    }

    pub fn metadata(&self) -> Value {
        serde_json::from_str(&self.metadata_json).unwrap_or_else(|_| json!({}))
    }

    pub fn live(&self) -> bool {
        matches!(self.status.as_str(), "active" | "paused")
    }

    /// The Identity object with its addresses (every status).
    pub fn json(&self, addrs: &[addresses::Row]) -> Value {
        let primary = addrs
            .iter()
            .find(|a| a.role == "primary")
            .map(|a| a.address.clone())
            .unwrap_or_default();
        let owner = match (&self.owner_name, &self.owner_email) {
            (Some(n), Some(e)) => json!({ "name": n, "email": e }),
            _ => Value::Null,
        };
        let mut signature = json!({});
        if let Some(t) = &self.signature_text {
            signature["text"] = t.clone().into();
        }
        if let Some(h) = &self.signature_html {
            signature["html"] = h.clone().into();
        }
        json!({
            "id": self.id, "tenant_id": self.tenant_id, "username": self.username,
            "display_name": self.display_name, "purpose": self.purpose, "status": self.status,
            "pause_reason": if self.status == "paused" { self.pause_reason.clone() } else { None },
            "primary_address": primary,
            "addresses": addrs.iter().map(addresses::Row::json).collect::<Vec<_>>(),
            "owner": owner, "signature": signature, "send_policy": self.send_policy(),
            "retention": { "message_hours": self.message_hours },
            "metadata": self.metadata(), "client_id": self.client_id,
            "created_at": rfc3339(self.created_at), "updated_at": rfc3339(self.updated_at),
        })
    }
}
