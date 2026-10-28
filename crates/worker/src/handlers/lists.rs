//! Allow and block lists ([Parity additions § 3], FR-OUT-16, FR-OUT-17):
//! `/v1/tenants/{tenant_id}/lists/{direction}/{kind}[/{entry}]` and the identity-scope routes under
//! `/v1/identities/{identity_id}/lists/…`. Entries are stored lower case with an A-label domain; `PUT`
//! again changes only `note`.
//!
//! [Parity additions § 3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/parity-additions.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::rfc3339;
use pylota_identity_core::lists::normalise_entry;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{identity_in_scope, tenant_in_scope};
use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;

#[derive(Deserialize)]
struct Row {
    identity_id: String,
    direction: String,
    kind: String,
    entry: String,
    note: Option<String>,
    created_at: i64,
}

impl Row {
    fn json(&self) -> Value {
        json!({
            "direction": self.direction, "kind": self.kind, "entry": self.entry,
            "scope": if self.identity_id.is_empty() { "tenant" } else { "identity" },
            "identity_id": (!self.identity_id.is_empty()).then_some(&self.identity_id),
            "note": self.note, "created_at": rfc3339(self.created_at),
        })
    }
}

/// `%XX` escapes of a path segment.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The tenant and identity scope of a request, and the checked direction and kind.
async fn scope<P: Platform>(
    c: &Ctx<P>,
    write: bool,
) -> Result<(String, String, String, String), ApiError> {
    let (tenant, identity) = if c.route.spec.pattern.starts_with("/v1/identities/") {
        let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), write).await?;
        if idn.is_system == 1 {
            return Err(ApiError::invalid_reason(
                "system_identity",
                "The system identity has no lists.",
            ));
        }
        (idn.tenant_id, idn.id)
    } else {
        let t = tenant_in_scope(c, c.route.param("tenant_id"), write).await?;
        (t.id, String::new())
    };
    let direction = c.route.param("direction");
    let kind = c.route.param("kind");
    if !matches!(direction, "receive" | "send") {
        return Err(ApiError::invalid("direction", "receive or send."));
    }
    if !matches!(kind, "allow" | "block") {
        return Err(ApiError::invalid("kind", "allow or block."));
    }
    Ok((tenant, identity, direction.to_owned(), kind.to_owned()))
}

fn entry<P: Platform>(c: &Ctx<P>) -> Result<String, ApiError> {
    normalise_entry(&percent_decode(c.route.param("entry")))
        .ok_or_else(|| ApiError::invalid("entry", "An address or @domain."))
}

const SELECT_ONE: &str = "SELECT identity_id, direction, kind, entry, note, created_at FROM sender_lists WHERE tenant_id = ?1 AND identity_id = ?2 AND direction = ?3 AND kind = ?4 AND entry = ?5";

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (tenant, identity, direction, kind) = scope(c, false).await?;
    let limit = c.limit()?;
    let after = c.after(&[]).await?;
    let (ac, ai) = after.map_or((None, None), |(t, e)| (Some(t), Some(e)));
    let rows: Vec<Row> = c
        .p
        .db()
        .query(Stmt::new(
            "SELECT identity_id, direction, kind, entry, note, created_at FROM sender_lists WHERE tenant_id = ?1 AND identity_id = ?2 AND direction = ?3 AND kind = ?4 \
             AND (?5 IS NULL OR created_at < ?5 OR (created_at = ?5 AND entry < ?6)) ORDER BY created_at DESC, entry DESC LIMIT ?7",
            vec![tenant.into(), identity.into(), direction.into(), kind.into(), ac.into(), ai.into(), (limit + 1).into()],
        ))
        .await?;
    c.page(
        rows,
        limit,
        &[],
        |r| (r.created_at, r.entry.clone()),
        Row::json,
    )
    .await
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (tenant, identity, direction, kind) = scope(c, false).await?;
    let e = entry(c)?;
    let row: Option<Row> =
        c.p.db()
            .first(Stmt::new(
                SELECT_ONE,
                vec![
                    tenant.into(),
                    identity.into(),
                    direction.into(),
                    kind.into(),
                    e.into(),
                ],
            ))
            .await?;
    let row = row.ok_or_else(|| ApiError::not_found(ErrorCode::ListEntryNotFound))?;
    Ok(Reply::json(200, row.json()))
}

pub async fn put<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (tenant, identity, direction, kind) = scope(c, true).await?;
    let e = entry(c)?;
    let b = c.json()?;
    super::messages::only_fields(&b, &["note"])?;
    let note = match &b["note"] {
        Value::Null => None,
        Value::String(n) if n.chars().count() <= 500 => Some(n.clone()),
        _ => return Err(ApiError::invalid("note", "At most 500 characters.")),
    };
    let db = c.p.db();
    let existed: Option<Row> = db
        .first(Stmt::new(
            SELECT_ONE,
            vec![
                tenant.clone().into(),
                identity.clone().into(),
                direction.clone().into(),
                kind.clone().into(),
                e.clone().into(),
            ],
        ))
        .await?;
    db.execute(Stmt::new(
        "INSERT INTO sender_lists (tenant_id, identity_id, direction, kind, entry, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT (tenant_id, identity_id, direction, kind, entry) DO UPDATE SET note = excluded.note",
        vec![tenant.clone().into(), identity.clone().into(), direction.clone().into(), kind.clone().into(), e.clone().into(), note.into(), c.now.into()],
    ))
    .await?;
    let row: Row = db
        .first(Stmt::new(
            SELECT_ONE,
            vec![
                tenant.into(),
                identity.into(),
                direction.into(),
                kind.into(),
                e.into(),
            ],
        ))
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "Retry."))?;
    Ok(Reply::json(
        if existed.is_some() { 200 } else { 201 },
        row.json(),
    ))
}

pub async fn delete<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (tenant, identity, direction, kind) = scope(c, true).await?;
    let e = entry(c)?;
    let m = c
        .p
        .db()
        .execute(Stmt::new(
            "DELETE FROM sender_lists WHERE tenant_id = ?1 AND identity_id = ?2 AND direction = ?3 AND kind = ?4 AND entry = ?5",
            vec![tenant.into(), identity.into(), direction.into(), kind.into(), e.into()],
        ))
        .await?;
    if m.changes == 0 {
        return Err(ApiError::not_found(ErrorCode::ListEntryNotFound));
    }
    Ok(Reply::no_content())
}
