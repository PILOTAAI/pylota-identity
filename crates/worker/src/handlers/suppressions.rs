//! Suppressions (FR-DLV-2): list, add (`manual` by default) and remove. Only a keyed hash of an address
//! is stored, so an entry is found by hashing the address the caller names; removing a `complaint`
//! needs `confirm_complaint_removal` and is audit-logged.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::address_hash;
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use super::tenant_in_scope;
use crate::consumers::delivery::hint;
use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;

#[derive(Deserialize)]
struct Row {
    address_hash: String,
    address_hint: String,
    reason: String,
    created_at: i64,
    expires_at: Option<i64>,
}

impl Row {
    fn json(&self) -> Value {
        json!({ "address_hint": self.address_hint, "reason": self.reason,
                "created_at": rfc3339(self.created_at), "expires_at": self.expires_at.map(rfc3339) })
    }
}

const REASONS: &[&str] = &[
    "hard_bounce",
    "complaint",
    "unsubscribe",
    "manual",
    "provider",
];

fn address(a: &str) -> Result<String, ApiError> {
    pylota_identity_core::compose::recipient(a)
        .map_err(|_| ApiError::invalid("address", "Not a valid address."))
}

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), false).await?;
    let limit = c.limit()?;
    let filters = ["address", "reason"];
    let after = c.after(&filters).await?;
    let (ac, ai) = after.map_or((None, None), |(t, k)| (Some(t), Some(k)));
    let hash = match c.query("address") {
        Some(a) => Some(address_hash(&c.p.secrets().hash_key, &address(a)?)),
        None => None,
    };
    let reason = c.query("reason").map(str::to_owned);
    if reason.as_deref().is_some_and(|r| !REASONS.contains(&r)) {
        return Err(ApiError::invalid("reason", "Unknown reason."));
    }
    let rows: Vec<Row> = c
        .p
        .db()
        .query(Stmt::new(
            "SELECT address_hash, address_hint, reason, created_at, expires_at FROM suppressions WHERE tenant_id = ?1 AND (?2 IS NULL OR address_hash = ?2) AND (?3 IS NULL OR reason = ?3) \
             AND (?4 IS NULL OR created_at < ?4 OR (created_at = ?4 AND address_hash < ?5)) ORDER BY created_at DESC, address_hash DESC LIMIT ?6",
            vec![t.id.into(), hash.into(), reason.into(), ac.into(), ai.into(), (limit + 1).into()],
        ))
        .await?;
    c.page(
        rows,
        limit,
        &filters,
        |r| (r.created_at, r.address_hash.clone()),
        Row::json,
    )
    .await
}

pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    let b = c.json()?;
    super::messages::only_fields(&b, &["address", "reason", "note"])?;
    let a = address(b["address"].as_str().unwrap_or(""))?;
    let reason = b["reason"].as_str().unwrap_or("manual");
    if !REASONS.contains(&reason) {
        return Err(ApiError::invalid("reason", "Unknown reason."));
    }
    let hash = address_hash(&c.p.secrets().hash_key, &a);
    let db = c.p.db();
    db.execute(Stmt::new(
        "INSERT INTO suppressions (tenant_id, address_hash, address_hint, reason, source_message_id, note, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, NULL) \
         ON CONFLICT (tenant_id, address_hash) DO UPDATE SET reason = excluded.reason, note = excluded.note, expires_at = NULL",
        vec![t.id.clone().into(), hash.clone().into(), hint(&a).into(), reason.into(), b["note"].as_str().map(str::to_owned).into(), c.now.into()],
    ))
    .await?;
    let row: Row = db
        .first(Stmt::new(
            "SELECT address_hash, address_hint, reason, created_at, expires_at FROM suppressions WHERE tenant_id = ?1 AND address_hash = ?2",
            vec![t.id.into(), hash.into()],
        ))
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "Retry."))?;
    Ok(Reply::json(201, row.json()))
}

pub async fn delete<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    let a = address(&super::lists::percent_decode(c.route.param("address")))?;
    let b = c.json()?;
    super::messages::only_fields(&b, &["confirm_complaint_removal"])?;
    let hash = address_hash(&c.p.secrets().hash_key, &a);
    let db = c.p.db();
    let row: Row = db
        .first(Stmt::new(
            "SELECT address_hash, address_hint, reason, created_at, expires_at FROM suppressions WHERE tenant_id = ?1 AND address_hash = ?2",
            vec![t.id.clone().into(), hash.clone().into()],
        ))
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::SuppressionNotFound))?;
    if row.reason == "complaint" && b["confirm_complaint_removal"] != true {
        return Err(ApiError::invalid(
            "confirm_complaint_removal",
            "Removing a complaint needs confirm_complaint_removal: true.",
        ));
    }
    db.execute(Stmt::new(
        "DELETE FROM suppressions WHERE tenant_id = ?1 AND address_hash = ?2",
        vec![t.id.clone().into(), hash.into()],
    ))
    .await?;
    db.execute(crate::db::audit::insert(
        &c.p.ids().new_id(IdPrefix::Audit),
        Some(&t.id),
        Some(&c.key().key_id),
        "suppression.delete",
        "suppression",
        &row.address_hint,
        &json!({ "reason": row.reason }),
        &c.request_id,
        c.now,
    ))
    .await?;
    Ok(Reply::no_content())
}
