//! The mailbox alarm purposes of M9 ([Data model § 2]): `dispatch` re-queues a queued message whose
//! queue pointer was lost, `claim` resolves claims older than five minutes to `uncertain`, and
//! `maintenance` runs the daily sweep of orphan objects ([Inbound › Orphan objects], I9). Thread locks
//! expire lazily and reconciliation is event-driven, so neither has a purpose.
//!
//! [Data model § 2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/data-model.md
//! [Inbound › Orphan objects]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_api_types::ids::crockford_value;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::{BlobStore as _, PResult, Platform};
use serde::Deserialize;

use super::{deliveries, outbox};

/// A queued message unclaimed for this long after its pointer is queued again.
pub const DISPATCH_AFTER_MS: i64 = 300_000;
/// A compose marker this old with no message row names an orphan.
const COMPOSE_STALE_MS: i64 = 600_000;
/// Keys younger than this are never swept (longer than any queue keeps a pointer).
const ORPHAN_AGE_MS: i64 = 15 * 86_400_000;
const SWEEP_KEYS: u32 = 1_000;
const SWEEP_PAGES: usize = 5;
pub const MAINTENANCE_MS: i64 = 86_400_000;

#[derive(Deserialize)]
struct Kv {
    k: String,
    v: String,
}

#[derive(Deserialize)]
struct One {
    #[allow(dead_code)]
    x: i64,
}

/// The time of a ULID-bearing ID (`msg_…`), from its first ten characters.
pub fn id_time(id: &str) -> Option<i64> {
    let ulid = id.split_once('_')?.1;
    let mut t: i64 = 0;
    for c in ulid.bytes().take(10) {
        t = t * 32 + i64::from(crockford_value(c.to_ascii_uppercase())?);
    }
    Some(t)
}

/// Queued messages whose `dispatch:` time is more than five minutes old and that have no claim.
pub async fn dispatch<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    now: i64,
) -> PResult<()> {
    let due: Vec<Kv> = s.query(
        "SELECT k, v FROM meta WHERE k LIKE 'dispatch:%' AND CAST(v AS INTEGER) + ?1 <= ?2",
        &[DISPATCH_AFTER_MS.into(), now.into()],
    )?;
    for d in due {
        let msg = &d.k["dispatch:".len()..];
        let queued = deliveries::by_id(s, msg)?.is_some_and(|r| r.status == "queued");
        if !queued || deliveries::claim_of(s, msg)?.is_some() {
            if !queued {
                s.exec("DELETE FROM meta WHERE k = ?1", &[d.k.clone().into()])?;
            }
            continue;
        }
        crate::consumers::outbound::enqueue(
            p,
            owner.tenant_id,
            owner.identity_id,
            msg,
            &crate::request_id::new(p),
            0,
        )
        .await;
        s.exec(
            "UPDATE meta SET v = ?2 WHERE k = ?1",
            &[d.k.clone().into(), now.to_string().into()],
        )?;
    }
    rearm(s, "dispatch:%", "dispatch", DISPATCH_AFTER_MS)
}

/// The stale-claim rule for every claim older than five minutes.
pub fn claims<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    now: i64,
) -> PResult<()> {
    let claims: Vec<Kv> = s.query("SELECT k, v FROM meta WHERE k LIKE 'claim:%'", &[])?;
    for c in claims {
        let msg = c.k["claim:".len()..].to_owned();
        let at: i64 =
            c.v.split_once('|')
                .and_then(|(_, a)| a.parse().ok())
                .unwrap_or(0);
        if now - at < deliveries::CLAIM_MS {
            continue;
        }
        s.transaction_sync(|tx| -> PResult<()> {
            match deliveries::by_id(tx, &msg)? {
                Some(row) if row.status == "queued" => {
                    deliveries::stale_claim(p, tx, owner, &row, at, now)
                }
                _ => {
                    tx.exec("DELETE FROM meta WHERE k = ?1", &[c.k.clone().into()])?;
                    Ok(())
                }
            }
        })?;
    }
    let left: Vec<Kv> = s.query("SELECT k, v FROM meta WHERE k LIKE 'claim:%'", &[])?;
    let next = left
        .iter()
        .filter_map(|c| c.v.split_once('|').and_then(|(_, a)| a.parse::<i64>().ok()))
        .min();
    match next {
        Some(at) => super::set_wake(s, "claim", at + deliveries::CLAIM_MS),
        None => {
            s.exec("DELETE FROM meta WHERE k = 'alarm:claim'", &[])?;
            Ok(())
        }
    }
}

fn rearm<S: Sql>(s: &S, like: &str, purpose: &str, after: i64) -> PResult<()> {
    #[derive(Deserialize)]
    struct T {
        t: Option<i64>,
    }
    let t: Option<T> = s.first(
        "SELECT MIN(CAST(v AS INTEGER)) AS t FROM meta WHERE k LIKE ?1",
        &[like.into()],
    )?;
    s.exec(
        "DELETE FROM meta WHERE k = ?1",
        &[format!("alarm:{purpose}").into()],
    )?;
    if let Some(at) = t.and_then(|t| t.t) {
        super::set_wake(s, purpose, at + after)?;
    }
    Ok(())
}

/// Whether a row names `key`.
fn named<S: Sql>(s: &S, key: &str) -> PResult<bool> {
    let hit: Option<One> = s.first(
        "SELECT 1 AS x WHERE EXISTS (SELECT 1 FROM messages WHERE raw_r2_key = ?1) OR EXISTS (SELECT 1 FROM attachments WHERE r2_key = ?1 OR text_r2_key = ?1) OR EXISTS (SELECT 1 FROM uploads WHERE r2_key = ?1)",
        &[key.into()],
    )?;
    Ok(hit.is_some())
}

/// The message ID a key belongs to (`…/m/{msg}/…` or `…/out/{msg}…`).
fn key_message(key: &str) -> Option<&str> {
    key.split('/')
        .find(|seg| seg.starts_with("msg_"))
        .map(|seg| seg.trim_end_matches(".eml"))
}

/// The daily maintenance: stale compose markers, then the rolling sweep. Answers the keys deleted.
pub async fn maintenance<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    now: i64,
) -> PResult<u32> {
    let blobs = p.blobs();
    let prefix = format!("t/{}/i/{}/", owner.tenant_id, owner.identity_id);
    let mut deleted = 0u32;
    let markers: Vec<Kv> = s.query("SELECT k, v FROM meta WHERE k LIKE 'compose:%'", &[])?;
    for m in markers {
        let msg = &m.k["compose:".len()..];
        let stale = m.v.parse::<i64>().is_ok_and(|t| now - t > COMPOSE_STALE_MS);
        if !stale || deliveries::by_id(s, msg)?.is_some() {
            continue;
        }
        let out = format!("{prefix}out/{msg}");
        let mut keys = vec![format!("{out}.eml")];
        if let Ok(page) = blobs.list(&format!("{out}/"), None, SWEEP_KEYS).await {
            keys.extend(page.keys.into_iter().map(|(k, _)| k));
        }
        if blobs.delete(&keys).await.is_ok() {
            deleted += keys.len() as u32;
            s.exec("DELETE FROM meta WHERE k = ?1", &[m.k.clone().into()])?;
        }
    }
    let mut cursor: Option<String> = s
        .first::<Kv>("SELECT k, v FROM meta WHERE k = 'orphan_sweep_cursor'", &[])?
        .map(|r| r.v);
    for _ in 0..SWEEP_PAGES {
        let Ok(page) = blobs.list(&prefix, cursor.as_deref(), SWEEP_KEYS).await else {
            break;
        };
        let mut orphans = Vec::new();
        for (k, _) in &page.keys {
            let old = key_message(k)
                .and_then(id_time)
                .is_some_and(|t| now - t > ORPHAN_AGE_MS);
            if old && !named(s, k)? {
                orphans.push(k.clone());
            }
        }
        if !orphans.is_empty() && blobs.delete(&orphans).await.is_ok() {
            deleted += orphans.len() as u32;
        }
        cursor = page.cursor;
        if cursor.is_none() {
            break;
        }
    }
    match &cursor {
        Some(c) => {
            s.exec(
                "INSERT OR REPLACE INTO meta (k, v) VALUES ('orphan_sweep_cursor', ?1)",
                &[c.clone().into()],
            )?;
        }
        None => {
            s.exec("DELETE FROM meta WHERE k = 'orphan_sweep_cursor'", &[])?;
        }
    }
    if deleted > 0 {
        crate::metrics::count(
            p,
            "r2_orphans_deleted_total",
            Some(owner.tenant_id),
            deleted,
        );
    }
    s.exec(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('alarm:maintenance', ?1)",
        &[(now + MAINTENANCE_MS).to_string().into()],
    )?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_time_and_key_message() {
        let id = pylota_identity_api_types::ids::format(
            pylota_identity_api_types::ids::IdPrefix::Message,
            1_790_000_000_123,
            42,
        );
        assert_eq!(id_time(&id), Some(1_790_000_000_123));
        assert_eq!(
            key_message(&format!("t/a/i/b/out/{id}.eml")),
            Some(id.as_str())
        );
        assert_eq!(
            key_message(&format!("t/a/i/b/m/{id}/raw.eml")),
            Some(id.as_str())
        );
    }
}
