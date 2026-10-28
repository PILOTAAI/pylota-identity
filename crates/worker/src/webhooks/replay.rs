//! Replay ([Webhooks › Replay]): select an endpoint's events from `event_index` by IDs or by window,
//! keep those it would receive, and queue a `Deliver` for each with the retry schedule restarted.
//!
//! [Webhooks › Replay]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{ControlDb as _, PResult, Platform, QueueProducer as _};
use serde::Deserialize;

use super::envelope::{OwnerKind, WebhookJob};
use super::matching::{self, Facts, Scope};

/// The replay window: 30 days from `occurred_at`.
pub const WINDOW_MS: i64 = 30 * 86_400_000;
/// Events per request.
pub const MAX_EVENTS: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Ids(Vec<String>),
    Window {
        since: i64,
        until: Option<i64>,
        status: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct Row {
    id: String,
    tenant_id: Option<String>,
    identity_id: Option<String>,
    #[serde(rename = "type")]
    event_type: String,
    owner_kind: String,
    owner_id: String,
    partner_id: Option<String>,
    first_attempt: i64,
    is_system: i64,
}

/// Selects, filters and queues; `Err(n)` when more than [`MAX_EVENTS`] match the selection.
pub async fn replay<P: Platform>(
    p: &P,
    e: &Scope,
    sel: &Selection,
    window_ms: i64,
    now: i64,
) -> PResult<Result<usize, usize>> {
    let (ids, since, until, status) = match sel {
        Selection::Ids(ids) => (
            Some(serde_json::to_string(ids).unwrap_or_default()),
            None,
            None,
            None,
        ),
        Selection::Window {
            since,
            until,
            status,
        } => (None, Some(*since), *until, status.clone()),
    };
    let stmt = Stmt::new(
        "SELECT x.id, x.tenant_id, x.identity_id, x.type, x.owner_kind, x.owner_id, x.partner_id, \
           1 + COALESCE((SELECT MAX(d.attempt) FROM webhook_deliveries d WHERE d.endpoint_id = ?1 AND d.event_id = x.id), 0) AS first_attempt, \
           COALESCE((SELECT i.is_system FROM identities i WHERE i.id = x.identity_id), 0) AS is_system \
         FROM event_index x \
         WHERE x.type <> 'webhook.test' AND x.occurred_at >= ?2 \
           AND (?3 IS NULL OR x.tenant_id = ?3) AND (?3 IS NOT NULL OR ?4 IS NULL OR x.partner_id = ?4) \
           AND (?5 IS NULL OR x.id IN (SELECT value FROM json_each(?5))) \
           AND (?6 IS NULL OR x.occurred_at >= ?6) AND (?7 IS NULL OR x.occurred_at < ?7) \
           AND (?8 IS NULL \
                OR (?8 = 'dead' AND EXISTS (SELECT 1 FROM webhook_deliveries d WHERE d.endpoint_id = ?1 AND d.event_id = x.id AND d.status = 'dead') \
                    AND NOT EXISTS (SELECT 1 FROM webhook_deliveries d WHERE d.endpoint_id = ?1 AND d.event_id = x.id AND d.status = 'succeeded')) \
                OR (?8 = 'failed' AND (SELECT d.status FROM webhook_deliveries d WHERE d.endpoint_id = ?1 AND d.event_id = x.id ORDER BY d.attempt DESC LIMIT 1) = 'failed') \
                OR (?8 = 'succeeded' AND EXISTS (SELECT 1 FROM webhook_deliveries d WHERE d.endpoint_id = ?1 AND d.event_id = x.id AND d.status = 'succeeded'))) \
         ORDER BY x.occurred_at LIMIT ?9",
        vec![
            e.id.clone().into(),
            (now - window_ms).into(),
            e.tenant_id.clone().into(),
            e.partner_id.clone().into(),
            ids.into(),
            since.into(),
            until.into(),
            status.into(),
            ((MAX_EVENTS + 1) as i64).into(),
        ],
    );
    let rows: Vec<Row> = p.db().query(stmt).await?;
    if rows.len() > MAX_EVENTS {
        return Ok(Err(rows.len()));
    }
    let jobs: Vec<WebhookJob> = rows
        .iter()
        .filter(|r| {
            matching::matches(
                e,
                &Facts {
                    event_type: &r.event_type,
                    tenant_id: r.tenant_id.as_deref(),
                    partner_id: r.partner_id.as_deref(),
                    identity_id: r.identity_id.as_deref(),
                    system_identity: r.is_system == 1,
                    about_endpoint: None,
                },
            )
        })
        .filter_map(|r| {
            let owner_kind = OwnerKind::parse(&r.owner_kind)?;
            Some(WebhookJob::Deliver {
                v: 1,
                event_id: r.id.clone(),
                event_type: r.event_type.clone(),
                endpoint_id: e.id.clone(),
                tenant_id: r.tenant_id.clone(),
                identity_id: r.identity_id.clone(),
                owner_kind,
                owner_id: r.owner_id.clone(),
                attempt: r.first_attempt as u32,
                first_attempt: r.first_attempt as u32,
                replay: true,
            })
        })
        .collect();
    for chunk in jobs.chunks(100) {
        p.queues().send_batch(QueueName::Webhooks, chunk, 0).await?;
    }
    Ok(Ok(jobs.len()))
}
