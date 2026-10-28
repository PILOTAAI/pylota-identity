//! The mailbox's transactional outbox ([Webhooks › Transactional outbox]): an event is appended in the
//! transaction of the change it reports, and the `outbox` alarm purpose writes its `event_index` row and
//! its `pm-webhooks` message after commit, so delivery is at least once and never lost.
//!
//! [Webhooks › Transactional outbox]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{ControlDb as _, Ids as _, PResult, Platform, QueueProducer as _};
use serde::Deserialize;
use serde_json::Value;

use crate::webhooks::envelope::{self, OwnerKind, WebhookJob};

/// At most this many events per dispatch.
pub const DISPATCH_BATCH: u32 = 100;
/// The retry delay after a failed dispatch: 30 s, doubled per failure, at most 5 minutes.
pub fn backoff_ms(failures: u32) -> i64 {
    (30_000i64 << failures.min(4)).min(300_000)
}

/// The owner every event of this mailbox carries.
pub struct Owner<'a> {
    pub tenant_id: &'a str,
    pub identity_id: &'a str,
}

#[derive(Deserialize)]
struct V {
    v: String,
}

/// Appends one event (steps 1–5 of *Appending*); runs inside the caller's transaction. Returns the
/// sequence, or `None` when the event ID was stored before.
pub fn append<S: Sql>(
    tx: &S,
    owner: &Owner<'_>,
    event_id: &str,
    event_type: &str,
    occurred_at: i64,
    data: &Value,
    now: i64,
) -> PResult<Option<i64>> {
    let seq: Option<V> = tx.first(
        "UPDATE meta SET v = CAST(CAST(v AS INTEGER) + 1 AS TEXT) WHERE k = 'event_seq' RETURNING v",
        &[],
    )?;
    let seq: i64 = seq.and_then(|s| s.v.parse().ok()).unwrap_or(1);
    let payload = envelope::envelope(
        event_id,
        event_type,
        occurred_at,
        Some(owner.tenant_id),
        Some(owner.identity_id),
        Some(seq),
        data,
    );
    let m = tx.exec(
        "INSERT INTO outbox (seq, event_id, type, payload_json, occurred_at, dispatched_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL) ON CONFLICT (event_id) DO NOTHING",
        &[
            seq.into(),
            event_id.into(),
            event_type.into(),
            payload.into(),
            occurred_at.into(),
        ],
    )?;
    if m.changes == 0 {
        tx.exec(
            "UPDATE meta SET v = CAST(CAST(v AS INTEGER) - 1 AS TEXT) WHERE k = 'event_seq'",
            &[],
        )?;
        return Ok(None);
    }
    super::set_wake(tx, "outbox", now)?;
    Ok(Some(seq))
}

/// A new event ID for an event no intent names.
pub fn new_event_id<P: Platform>(p: &P) -> String {
    p.ids().new_id(IdPrefix::Event)
}

#[derive(Deserialize)]
struct Pending {
    seq: i64,
    event_id: String,
    #[serde(rename = "type")]
    event_type: String,
    occurred_at: i64,
}

/// What a dispatch did.
#[derive(Debug, PartialEq, Eq)]
pub enum Dispatched {
    /// Every pending event was dispatched (`n` of them).
    Done(usize),
    /// A full batch was dispatched; more may wait.
    More,
    /// D1 or the queue failed; nothing was marked.
    Failed,
}

/// The `outbox` alarm purpose (steps 1–4 of *Dispatching*). `owner_id` is this object's ID.
pub async fn dispatch<P: Platform, S: Sql>(
    p: &P,
    sql: &S,
    owner: &Owner<'_>,
    owner_id: &str,
    now: i64,
) -> PResult<Dispatched> {
    let rows: Vec<Pending> = sql.query(
        "SELECT seq, event_id, type, occurred_at FROM outbox WHERE dispatched_at IS NULL ORDER BY seq LIMIT ?1",
        &[DISPATCH_BATCH.into()],
    )?;
    if rows.is_empty() {
        return Ok(Dispatched::Done(0));
    }
    let index: Vec<Stmt> = rows
        .iter()
        .map(|r| {
            Stmt::new(
                "INSERT OR IGNORE INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, partner_id, payload_json, occurred_at) VALUES (?1, ?2, ?3, ?4, 'mailbox', ?5, (SELECT partner_id FROM tenants WHERE id = ?2), NULL, ?6)",
                vec![
                    r.event_id.clone().into(),
                    owner.tenant_id.into(),
                    owner.identity_id.into(),
                    r.event_type.clone().into(),
                    owner_id.into(),
                    r.occurred_at.into(),
                ],
            )
        })
        .collect();
    if p.db().batch(index).await.is_err() {
        return Ok(Dispatched::Failed);
    }
    let jobs: Vec<WebhookJob> = rows
        .iter()
        .map(|r| WebhookJob::Fanout {
            v: 1,
            event_id: r.event_id.clone(),
            event_type: r.event_type.clone(),
            tenant_id: Some(owner.tenant_id.to_owned()),
            identity_id: Some(owner.identity_id.to_owned()),
            owner_kind: OwnerKind::Mailbox,
            owner_id: owner_id.to_owned(),
            occurred_at: r.occurred_at,
        })
        .collect();
    if p.queues()
        .send_batch(QueueName::Webhooks, &jobs, 0)
        .await
        .is_err()
    {
        return Ok(Dispatched::Failed);
    }
    let seqs: Vec<i64> = rows.iter().map(|r| r.seq).collect();
    sql.exec(
        "UPDATE outbox SET dispatched_at = ?1 WHERE seq IN (SELECT value FROM json_each(?2))",
        &[
            now.into(),
            serde_json::to_string(&seqs).unwrap_or_default().into(),
        ],
    )?;
    Ok(if rows.len() as u32 == DISPATCH_BATCH {
        Dispatched::More
    } else {
        Dispatched::Done(rows.len())
    })
}
