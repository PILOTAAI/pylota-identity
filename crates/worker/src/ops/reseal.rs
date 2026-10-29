//! The master-key rotation's Worker half (Security § 6.2): every minute, write `master_key.activated`
//! when the active key is new, re-seal up to [`PER_RUN`] values whose envelope names another key across
//! the sealed-column registry (`core::sealed`), and write `master_key.resealed` once nothing is left
//! under the active key. `GET /v1/platform/status` reads [`status`].
//!
//! A value of either type re-seals in place: the envelope is read as text and written back as the
//! column's own type, and only if it still holds the envelope that was read (a concurrent write wins).

use pylota_identity_core::crypto;
use pylota_identity_platform::config::MasterSlot;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{Clock as _, ControlDb as _, Ids as _, Platform, PlatformError};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::audit;

/// Values re-sealed per run (about 720,000 a day).
pub const PER_RUN: usize = 500;

/// The SQL of one registered column: literals, written by [`column!`] from the registry's names.
struct ColumnSql {
    table: &'static str,
    column: &'static str,
    count: &'static str,
    select: &'static str,
    update: &'static str,
}

macro_rules! column {
    ($table:literal, $column:literal, $rid:literal) => {
        ColumnSql {
            table: $table,
            column: $column,
            count: concat!(
                "SELECT COUNT(*) AS n FROM ",
                $table,
                " WHERE ",
                $column,
                " IS NOT NULL AND substr(CAST(",
                $column,
                " AS TEXT), 5, 16) <> ?1"
            ),
            select: concat!(
                "SELECT ",
                $rid,
                " AS rid, CAST(",
                $column,
                " AS TEXT) AS env FROM ",
                $table,
                " WHERE ",
                $column,
                " IS NOT NULL AND substr(CAST(",
                $column,
                " AS TEXT), 5, 16) <> ?1 LIMIT ?2"
            ),
            update: concat!(
                "UPDATE ",
                $table,
                " SET ",
                $column,
                " = CASE WHEN typeof(",
                $column,
                ") = 'blob' THEN CAST(?1 AS BLOB) ELSE ?1 END WHERE (",
                $rid,
                ") = ?2 AND CAST(",
                $column,
                " AS TEXT) = ?3"
            ),
        }
    };
}

/// One entry per `core::sealed::SEALED` entry, in its order (`registry_in_step` checks).
const COLUMNS: &[ColumnSql] = &[
    column!("webhook_endpoints", "secret_enc", "id"),
    column!("webhook_endpoints", "prev_secret_enc", "id"),
    column!("webhook_endpoints", "headers_sealed", "id"),
    column!("identity_keys", "private_enc", "id"),
    column!("signing_keys", "ciphertext", "purpose || ':' || kid"),
    column!("domains", "smtp_sealed", "id"),
    column!("domains", "smtp_pending_sealed", "id"),
    column!("users", "totp_sealed", "id"),
    column!("users", "recovery_codes_sealed", "id"),
    column!("users", "totp_pending_sealed", "id"),
    column!("oauth_states", "pkce_sealed", "state_hash"),
];

fn sql() -> &'static [ColumnSql] {
    COLUMNS
}

/// The active slot's key ID.
pub fn active_kid<P: Platform>(p: &P) -> String {
    crypto::kid(p.secrets().active_master_key())
}

fn slots<P: Platform>(p: &P) -> Vec<&[u8; 32]> {
    let s = p.secrets();
    [s.master_key_a.as_ref(), s.master_key_b.as_ref()]
        .into_iter()
        .flatten()
        .collect()
}

/// Sealed values whose key ID is not the active key's, over every registered column.
pub async fn remaining<P: Platform>(p: &P) -> Result<u64, PlatformError> {
    #[derive(Deserialize)]
    struct N {
        n: i64,
    }
    let kid = active_kid(p);
    let mut total = 0;
    for c in sql() {
        let n: Option<N> = p
            .db()
            .first(Stmt::new(c.count, vec![kid.clone().into()]))
            .await?;
        total += n.map_or(0, |n| n.n.max(0) as u64);
    }
    Ok(total)
}

/// The latest `master_key.{what}` audit row: `(key ID, created_at)`.
async fn latest<P: Platform>(
    p: &P,
    action: &'static str,
) -> Result<Option<(String, i64)>, PlatformError> {
    #[derive(Deserialize)]
    struct R {
        target_id: String,
        created_at: i64,
    }
    let r: Option<R> = p
        .db()
        .first(Stmt::new(
            "SELECT target_id, created_at FROM audit_log WHERE action = ?1 ORDER BY created_at DESC, id DESC LIMIT 1",
            vec![action.into()],
        ))
        .await?;
    Ok(r.map(|r| (r.target_id, r.created_at)))
}

async fn record<P: Platform>(p: &P, action: &str, kid: &str, details: Value, request_id: &str) {
    let _ = p
        .db()
        .execute(audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            None,
            None,
            action,
            "master_key",
            kid,
            &details,
            request_id,
            p.clock().now_ms(),
        ))
        .await;
}

/// One sweep run.
pub async fn run<P: Platform>(p: &P, request_id: &str) {
    let kid = active_kid(p);
    match latest(p, "master_key.activated").await {
        Ok(Some((k, _))) if k == kid => {}
        Ok(_) => {
            let slot = match p.secrets().master_key_active {
                MasterSlot::A => "a",
                MasterSlot::B => "b",
            };
            record(
                p,
                "master_key.activated",
                &kid,
                json!({ "slot": slot }),
                request_id,
            )
            .await;
        }
        Err(_) => return,
    }
    let (resealed, unopenable) = sweep(p, &kid).await;
    if resealed > 0 || unopenable > 0 {
        crate::log::info(
            "secrets_reseal_progress",
            &[
                ("count", (resealed as i64).into()),
                ("detail", format!("unopenable:{unopenable}").into()),
            ],
        );
    }
    // `resealed_at`: the first run that finds nothing left under the active key.
    let Ok(left) = remaining(p).await else {
        return;
    };
    if left == 0 {
        match latest(p, "master_key.resealed").await {
            Ok(Some((k, _))) if k == kid => {}
            Ok(_) => record(p, "master_key.resealed", &kid, json!({}), request_id).await,
            Err(_) => {}
        }
    }
}

/// Re-seals up to [`PER_RUN`] stale values; answers `(re-sealed, unopenable)`.
async fn sweep<P: Platform>(p: &P, kid: &str) -> (usize, usize) {
    #[derive(Deserialize)]
    struct R {
        rid: String,
        env: String,
    }
    let keys = slots(p);
    let active = p.secrets().active_master_key();
    let (mut resealed, mut unopenable) = (0, 0);
    for c in sql() {
        let budget = PER_RUN - resealed - unopenable;
        if budget == 0 {
            break;
        }
        let rows: Vec<R> = match p
            .db()
            .query(Stmt::new(
                c.select,
                vec![kid.into(), (budget as i64).into()],
            ))
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };
        for r in rows {
            let aad = crypto::aad(c.table, c.column, &r.rid);
            let Ok(plain) = crypto::open(&keys, &aad, &r.env) else {
                unopenable += 1;
                continue;
            };
            let nonce: [u8; 12] = pylota_identity_platform::Rng::bytes(p.rng());
            let fresh = crypto::seal(active, nonce, &aad, &plain);
            if p.db()
                .execute(Stmt::new(
                    c.update,
                    vec![fresh.into(), r.rid.into(), r.env.into()],
                ))
                .await
                .is_ok()
            {
                resealed += 1;
            }
        }
    }
    (resealed, unopenable)
}

/// The `master_key` block of `GET /v1/platform/status`.
pub async fn status<P: Platform>(p: &P) -> Result<Value, PlatformError> {
    let s = p.secrets();
    let kid = active_kid(p);
    let slot = |name: &str, key: Option<&[u8; 32]>| json!({ "slot": name, "present": key.is_some(), "kid": key.map(crypto::kid) });
    let activated = latest(p, "master_key.activated").await?;
    let resealed = latest(p, "master_key.resealed").await?;
    let at = |r: Option<(String, i64)>| {
        r.filter(|(k, _)| *k == kid)
            .map(|(_, t)| pylota_identity_core::days::rfc3339(t))
    };
    Ok(json!({
        "active_slot": match s.master_key_active { MasterSlot::A => "a", MasterSlot::B => "b" },
        "slots": [slot("a", s.master_key_a.as_ref()), slot("b", s.master_key_b.as_ref())],
        "remaining": remaining(p).await?,
        "activated_at": at(activated),
        "resealed_at": at(resealed),
    }))
}

#[cfg(test)]
mod tests {
    use pylota_identity_core::sealed::SEALED;

    use super::*;

    /// The literal SQL covers exactly the registry, in its order, with its row-ID expressions.
    #[test]
    fn registry_in_step() {
        assert_eq!(COLUMNS.len(), SEALED.len());
        for (c, s) in COLUMNS.iter().zip(SEALED) {
            assert_eq!((c.table, c.column), (s.table, s.column));
            assert!(c.select.contains(&format!("{} AS rid,", s.row_id)));
            assert!(c.update.contains(&format!("WHERE ({}) = ?2", s.row_id)));
        }
    }
}
