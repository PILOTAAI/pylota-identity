//! `identity_keys` and `key_tombstones` ([Agent signing keys § 8]).
//!
//! [Agent signing keys § 8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::{SqlValue, Stmt};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub identity_id: String,
    pub tenant_id: String,
    pub public_jwk: String,
    pub private_enc: Vec<u8>,
    pub status: String,
    pub created_at: i64,
    pub verify_until: Option<i64>,
    pub retired_at: Option<i64>,
}

impl Row {
    /// The IdentityKey object; the sealed seed never leaves the row.
    pub fn json(&self) -> Value {
        json!({
            "kid": self.id, "identity_id": self.identity_id, "status": self.status, "alg": "EdDSA",
            "public_jwk": serde_json::from_str::<Value>(&self.public_jwk).unwrap_or(Value::Null),
            "created_at": rfc3339(self.created_at),
            "verify_until": self.verify_until.map(rfc3339),
            "retired_at": self.retired_at.map(rfc3339),
        })
    }
}

/// One page of an identity's keys, newest first, with an optional status filter.
pub fn page(
    identity_id: &str,
    status: Option<&str>,
    keyset: Option<(i64, String)>,
    limit: u32,
) -> Stmt {
    let (kc, ki) = keyset.map_or((None, None), |(c, i)| (Some(c), Some(i)));
    Stmt::new(
        "SELECT id, identity_id, tenant_id, public_jwk, private_enc, status, created_at, verify_until, retired_at FROM identity_keys WHERE identity_id = ?1 \
         AND (?2 IS NULL OR status = ?2) AND (?3 IS NULL OR created_at < ?3 OR (created_at = ?3 AND id < ?4)) ORDER BY created_at DESC, id DESC LIMIT ?5",
        vec![
            identity_id.into(),
            status.into(),
            kc.into(),
            ki.into(),
            (limit + 1).into(),
        ],
    )
}

pub fn active(identity_id: &str) -> Stmt {
    Stmt::new(
        "SELECT id, identity_id, tenant_id, public_jwk, private_enc, status, created_at, verify_until, retired_at FROM identity_keys WHERE identity_id = ?1 AND status = 'active'",
        vec![identity_id.into()],
    )
}

pub fn get(identity_id: &str, kid: &str) -> Stmt {
    Stmt::new(
        "SELECT id, identity_id, tenant_id, public_jwk, private_enc, status, created_at, verify_until, retired_at FROM identity_keys WHERE identity_id = ?1 AND id = ?2",
        vec![identity_id.into(), kid.into()],
    )
}

/// The keys an identity's JWKS lists: the active one and the retiring ones still inside their overlap.
/// The `verify_until` filter is applied here, so a key leaves the set before the retention job retires it.
pub fn published(identity_id: &str, now: i64) -> Stmt {
    Stmt::new(
        "SELECT id, identity_id, tenant_id, public_jwk, private_enc, status, created_at, verify_until, retired_at FROM identity_keys WHERE identity_id = ?1 \
         AND (status = 'active' OR (status = 'retiring' AND verify_until > ?2)) ORDER BY created_at DESC, id DESC",
        vec![identity_id.into(), now.into()],
    )
}

/// Whether a thumbprint was ever used: a tombstone, or any row (`retired` included).
pub fn kid_used(kid: &str) -> Stmt {
    Stmt::new(
        "SELECT (SELECT COUNT(*) FROM key_tombstones WHERE kid = ?1) + (SELECT COUNT(*) FROM identity_keys WHERE id = ?1) AS n",
        vec![kid.into()],
    )
}

/// A new `active` key. `INSERT OR IGNORE`: the `identity_keys_one_active` index lets one of two racing
/// inserts win (O28), and the loser reads the winner.
pub fn insert_active(
    kid: &str,
    identity_id: &str,
    tenant_id: &str,
    public_jwk: &str,
    private_enc: Vec<u8>,
    now: i64,
) -> Stmt {
    Stmt::new(
        "INSERT OR IGNORE INTO identity_keys (id, identity_id, tenant_id, alg, public_jwk, private_enc, status, created_at) VALUES (?1, ?2, ?3, 'EdDSA', ?4, ?5, 'active', ?6)",
        vec![
            kid.into(),
            identity_id.into(),
            tenant_id.into(),
            public_jwk.into(),
            SqlValue::Blob(private_enc),
            now.into(),
        ],
    )
}

/// The active key becomes `retiring` until `verify_until`.
pub fn retire_active(identity_id: &str, kid: &str, verify_until: i64) -> Stmt {
    Stmt::new(
        "UPDATE identity_keys SET status = 'retiring', verify_until = ?3 WHERE identity_id = ?1 AND id = ?2 AND status = 'active'",
        vec![identity_id.into(), kid.into(), verify_until.into()],
    )
}

/// Any key becomes `retired` at once (O3).
pub fn revoke(identity_id: &str, kid: &str, now: i64) -> Stmt {
    Stmt::new(
        "UPDATE identity_keys SET status = 'retired', retired_at = ?3 WHERE identity_id = ?1 AND id = ?2 AND status <> 'retired'",
        vec![identity_id.into(), kid.into(), now.into()],
    )
}
