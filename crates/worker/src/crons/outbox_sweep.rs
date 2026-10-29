//! The every-minute webhook sweep ([Webhooks › Platform events], [Webhooks › Delivering an attempt]
//! step 2): re-queue a `Fanout` for a platform event of the last hour that was never fanned out, and
//! release the deliveries held for a partner that is active again (J13).
//!
//! [Webhooks › Platform events]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md
//! [Webhooks › Delivering an attempt]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{
    Clock as _, ControlDb as _, Ids as _, Platform, QueueProducer as _,
};
use serde::Deserialize;
use serde_json::Value;

use crate::webhooks::envelope::{OwnerKind, WebhookJob};
use crate::webhooks::replay::WINDOW_MS;

const HOUR_MS: i64 = 3_600_000;

#[derive(Deserialize)]
struct Unfanned {
    id: String,
    tenant_id: Option<String>,
    #[serde(rename = "type")]
    event_type: String,
    owner_kind: String,
    owner_id: String,
    occurred_at: i64,
}

#[derive(Deserialize)]
struct Held {
    endpoint_id: String,
    event_id: String,
    job_json: String,
    occurred_at: i64,
    active: i64,
}

pub async fn run<P: Platform>(p: &P) {
    let now = p.clock().now_ms();
    refan(p, now).await;
    release_held(p, now).await;
}

async fn refan<P: Platform>(p: &P, now: i64) {
    let rows: Vec<Unfanned> = match p
        .db()
        .query(Stmt::new(
            "SELECT id, tenant_id, type, owner_kind, owner_id, occurred_at FROM event_index \
             WHERE owner_kind IN ('platform', 'domain', 'job') AND occurred_at > ?1 AND fanned_out_at IS NULL AND type <> 'webhook.test' LIMIT 100",
            vec![(now - HOUR_MS).into()],
        ))
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };
    let jobs: Vec<WebhookJob> = rows
        .into_iter()
        .map(|r| WebhookJob::Fanout {
            v: 1,
            event_id: r.id,
            event_type: r.event_type,
            tenant_id: r.tenant_id,
            identity_id: None,
            owner_kind: OwnerKind::parse(&r.owner_kind).unwrap_or(OwnerKind::Platform),
            owner_id: r.owner_id,
            occurred_at: r.occurred_at,
        })
        .collect();
    if !jobs.is_empty() {
        let _ = p.queues().send_batch(QueueName::Webhooks, &jobs, 0).await;
    }
}

async fn release_held<P: Platform>(p: &P, now: i64) {
    let rows: Vec<Held> = match p
        .db()
        .query(Stmt::new(
            "SELECT h.endpoint_id, h.event_id, h.job_json, h.occurred_at, p.status = 'active' AS active FROM webhook_held h \
             JOIN partners p ON p.id = h.partner_id \
             WHERE p.status = 'active' OR h.occurred_at < ?1 ORDER BY h.held_at LIMIT 500",
            vec![(now - WINDOW_MS).into()],
        ))
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };
    for chunk in rows.chunks(100) {
        let (expired, live): (Vec<&Held>, Vec<&Held>) =
            chunk.iter().partition(|h| h.occurred_at < now - WINDOW_MS);
        let jobs: Vec<Value> = live
            .iter()
            .filter(|h| h.active == 1)
            .filter_map(|h| serde_json::from_str(&h.job_json).ok())
            .collect();
        if !jobs.is_empty()
            && p.queues()
                .send_batch(QueueName::Webhooks, &jobs, 0)
                .await
                .is_err()
        {
            return;
        }
        let mut stmts = Vec::new();
        for h in &expired {
            let job: Value = serde_json::from_str(&h.job_json).unwrap_or(Value::Null);
            stmts.push(crate::db::webhooks::record(
                &p.ids().new_id(IdPrefix::Delivery),
                &h.endpoint_id,
                job["tenant_id"].as_str(),
                &h.event_id,
                job["event_type"].as_str().unwrap_or(""),
                job["attempt"].as_u64().unwrap_or(1) as u32,
                "dead",
                None,
                Some("event_unavailable"),
                0,
                None,
                now,
            ));
        }
        for h in expired.iter().chain(live.iter().filter(|h| h.active == 1)) {
            stmts.push(Stmt::new(
                "DELETE FROM webhook_held WHERE endpoint_id = ?1 AND event_id = ?2",
                vec![h.endpoint_id.clone().into(), h.event_id.clone().into()],
            ));
        }
        if !stmts.is_empty() {
            let _ = p.db().batch(stmts).await;
        }
    }
}
