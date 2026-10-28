//! `addresses` and `address_tombstones` (Data model § 1).

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub address: String,
    pub local_part: String,
    pub domain_id: String,
    pub tenant_id: String,
    pub identity_id: String,
    pub role: String,
    pub status: String,
    pub routing_rule_id: Option<String>,
    pub retire_at: Option<i64>,
    pub retired_at: Option<i64>,
    pub ses_bounce_rule: Option<String>,
    pub forwarding: Option<String>,
    pub forwarding_checked_at: Option<i64>,
    pub created_at: i64,
}

/// Every address of an identity, oldest first, any status.
pub fn of_identity(identity_id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, address, local_part, domain_id, tenant_id, identity_id, role, status, routing_rule_id, retire_at, retired_at, ses_bounce_rule, forwarding, forwarding_checked_at, created_at FROM addresses WHERE identity_id = ?1 ORDER BY created_at, id",
        vec![identity_id.into()],
    )
}

/// One page of an identity's addresses, newest first.
pub fn page(identity_id: &str, keyset: Option<(i64, String)>, limit: u32) -> Stmt {
    let (kc, ki) = keyset.map_or((None, None), |(c, i)| (Some(c), Some(i)));
    Stmt::new(
        "SELECT id, address, local_part, domain_id, tenant_id, identity_id, role, status, routing_rule_id, retire_at, retired_at, ses_bounce_rule, forwarding, forwarding_checked_at, created_at FROM addresses WHERE identity_id = ?1 \
         AND (?2 IS NULL OR created_at < ?2 OR (created_at = ?2 AND id < ?3)) ORDER BY created_at DESC, id DESC LIMIT ?4",
        vec![identity_id.into(), kc.into(), ki.into(), (limit + 1).into()],
    )
}

/// The addresses of several identities (a page of identities), `ids` a JSON array.
pub fn of_identities(ids: &str) -> Stmt {
    Stmt::new(
        "SELECT id, address, local_part, domain_id, tenant_id, identity_id, role, status, routing_rule_id, retire_at, retired_at, ses_bounce_rule, forwarding, forwarding_checked_at, created_at FROM addresses WHERE identity_id IN (SELECT value FROM json_each(?1)) ORDER BY created_at, id",
        vec![ids.into()],
    )
}

pub fn by_address(address: &str) -> Stmt {
    Stmt::new(
        "SELECT id, address, local_part, domain_id, tenant_id, identity_id, role, status, routing_rule_id, retire_at, retired_at, ses_bounce_rule, forwarding, forwarding_checked_at, created_at FROM addresses WHERE address = ?1",
        vec![address.into()],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn insert(
    id: &str,
    address: &str,
    local_part: &str,
    domain_id: &str,
    tenant_id: &str,
    identity_id: &str,
    role: &str,
    status: &str,
    now: i64,
) -> Stmt {
    Stmt::new(
        "INSERT INTO addresses (id, address, local_part, domain_id, tenant_id, identity_id, role, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        vec![
            id.into(),
            address.into(),
            local_part.into(),
            domain_id.into(),
            tenant_id.into(),
            identity_id.into(),
            role.into(),
            status.into(),
            now.into(),
        ],
    )
}

#[derive(Deserialize)]
pub struct Tombstone {
    pub identity_id: Option<String>,
}

pub fn tombstone(hash: &str) -> Stmt {
    Stmt::new(
        "SELECT identity_id FROM address_tombstones WHERE address_hash = ?1",
        vec![hash.into()],
    )
}

pub fn insert_tombstone(hash: &str, identity_id: &str, reason: &str) -> Stmt {
    Stmt::new(
        "INSERT OR IGNORE INTO address_tombstones (address_hash, identity_id, reason) VALUES (?1, ?2, ?3)",
        vec![hash.into(), identity_id.into(), reason.into()],
    )
}

pub fn delete_of_identity(identity_id: &str) -> Stmt {
    Stmt::new(
        "DELETE FROM addresses WHERE identity_id = ?1",
        vec![identity_id.into()],
    )
}

/// Whether the address is a console user's sign-in address (Identities and domains › Create, step 4).
pub fn is_user_email(address: &str) -> Stmt {
    Stmt::new(
        "SELECT COUNT(*) AS n FROM users WHERE email = ?1",
        vec![address.into()],
    )
}

impl Row {
    /// The Address object.
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "identity_id": self.identity_id, "address": self.address,
            "local_part": self.local_part, "domain_id": self.domain_id, "role": self.role,
            "status": self.status, "retire_at": self.retire_at.map(rfc3339),
            "retired_at": self.retired_at.map(rfc3339), "forwarding": self.forwarding,
            "forwarding_checked_at": self.forwarding_checked_at.map(rfc3339),
            "created_at": rfc3339(self.created_at),
        })
    }
}
