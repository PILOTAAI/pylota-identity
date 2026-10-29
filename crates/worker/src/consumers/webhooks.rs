//! The `pm-webhooks` consumer ([Webhooks]): a `Fanout` becomes one `Deliver` per matching endpoint; a
//! `Deliver` is one signed attempt, recorded, with the next attempt re-enqueued on the retry schedule
//! and the endpoint disabled on `410` or on 100 failures over 24 hours.
//!
//! The event-log append (step 0, M30) and the Notifier hand-off (M26) arrive with their milestones.
//!
//! [Webhooks]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use std::cell::RefCell;
use std::collections::HashMap;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::{DoClass, RpcResult};
use pylota_identity_platform::queues::{Incoming, QueueName};
use pylota_identity_platform::{
    Clock as _, ControlDb as _, Ids as _, ObjectClient as _, Platform, QueueProducer as _, Rng as _,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::webhooks::{self as wdb, Row};
use crate::mailbox::MailboxRequest;
use crate::rpc::envelope_for;
use crate::webhooks::client;
use crate::webhooks::envelope::{OwnerKind, WebhookJob, envelope};
use crate::webhooks::matching::{self, Facts, Scope};

/// Attempts per delivery.
pub const ATTEMPTS: u32 = 13;
/// The delay after attempt `k` fails (k = 1…12), in seconds (Retry schedule, J4).
pub const SCHEDULE_S: [u32; 12] = [
    30, 120, 600, 1800, 3600, 7200, 14400, 28800, 43200, 43200, 43200, 68400,
];
/// Failures in a row that disable an endpoint, when they span at least a day.
const DISABLE_AFTER: i64 = 100;
const DAY_MS: i64 = 86_400_000;
/// Retry of a message after a D1 or RPC error.
const RETRY_S: u32 = if cfg!(feature = "itest-hooks") { 1 } else { 30 };
const CACHE_MS: i64 = 30_000;

/// The jittered delay before the next attempt (±10%, whole seconds, at most a day).
pub fn delay_s<P: Platform>(p: &P, k: u32) -> u32 {
    let base = SCHEDULE_S[(k.clamp(1, 12) - 1) as usize];
    let r: [u8; 2] = p.rng().bytes();
    let unit = f64::from(u16::from_le_bytes(r)) / f64::from(u16::MAX); // 0..1
    let jittered = f64::from(base) * (0.9 + 0.2 * unit);
    (jittered.round() as u32).min(86_400)
}

/// The queue delay actually used: the schedule, or one second in the itest build, which records the
/// scheduled time in `next_attempt_at` so the suite checks the schedule without waiting it out.
fn queue_delay(scheduled_s: u32) -> u32 {
    if cfg!(feature = "itest-hooks") {
        1
    } else {
        scheduled_s
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Candidate {
    id: String,
    tenant_id: Option<String>,
    partner_id: Option<String>,
    event_types_json: String,
    identity_ids_json: Option<String>,
}

/// (tenant, partner) → (enabled endpoints that can see their events, expiry).
type EndpointCache = HashMap<(Option<String>, Option<String>), (Vec<Candidate>, i64)>;

thread_local! {
    static ENDPOINTS: RefCell<EndpointCache> = RefCell::new(HashMap::new());
    /// The system identity's ID, and when it was read.
    static SYSTEM: RefCell<Option<(Option<String>, i64)>> = const { RefCell::new(None) };
}

async fn candidates<P: Platform>(
    p: &P,
    tenant: Option<&str>,
    partner: Option<&str>,
    now: i64,
) -> Result<Vec<Candidate>, ()> {
    let key = (tenant.map(str::to_owned), partner.map(str::to_owned));
    if let Some(c) = ENDPOINTS.with(|e| {
        e.borrow()
            .get(&key)
            .filter(|(_, exp)| *exp > now)
            .map(|(c, _)| c.clone())
    }) {
        return Ok(c);
    }
    let rows: Vec<Candidate> = p
        .db()
        .query(Stmt::new(
            "SELECT id, tenant_id, partner_id, event_types_json, identity_ids_json FROM webhook_endpoints \
             WHERE enabled = 1 AND (tenant_id = ?1 OR (tenant_id IS NULL AND partner_id IS NULL) OR (tenant_id IS NULL AND partner_id = ?2))",
            vec![tenant.map(str::to_owned).into(), partner.map(str::to_owned).into()],
        ))
        .await
        .map_err(|_| ())?;
    ENDPOINTS.with(|e| {
        let mut e = e.borrow_mut();
        if e.len() > 1000 {
            e.clear();
        }
        e.insert(key, (rows.clone(), now + CACHE_MS));
    });
    Ok(rows)
}

async fn system_identity<P: Platform>(p: &P, now: i64) -> Result<Option<String>, ()> {
    if let Some(id) = SYSTEM.with(|s| {
        s.borrow()
            .as_ref()
            .filter(|(_, at)| now - at < CACHE_MS)
            .map(|(id, _)| id.clone())
    }) {
        return Ok(id);
    }
    #[derive(Deserialize)]
    struct I {
        id: String,
    }
    let row: Option<I> = p
        .db()
        .first(Stmt::new(
            "SELECT id FROM identities WHERE is_system = 1 LIMIT 1",
            vec![],
        ))
        .await
        .map_err(|_| ())?;
    let id = row.map(|r| r.id);
    SYSTEM.with(|s| *s.borrow_mut() = Some((id.clone(), now)));
    Ok(id)
}

#[derive(Deserialize)]
struct IndexRow {
    partner_id: Option<String>,
    payload_json: Option<String>,
    occurred_at: i64,
}

async fn index_row<P: Platform>(p: &P, event_id: &str) -> Result<Option<IndexRow>, ()> {
    p.db()
        .first(Stmt::new(
            "SELECT partner_id, payload_json, occurred_at FROM event_index WHERE id = ?1",
            vec![event_id.into()],
        ))
        .await
        .map_err(|_| ())
}

/// How one message ended.
enum Done {
    Ack,
    Retry,
}

pub async fn batch<P: Platform>(p: &P, batch: Vec<Incoming<Value>>) {
    #[cfg(feature = "itest-hooks")]
    crate::hooks::tap_queue(p, "pm-webhooks", &batch).await;
    // Payloads of mailbox events, fetched once per owner for the whole batch.
    let mut wanted: HashMap<(String, Option<String>, Option<String>), Vec<String>> = HashMap::new();
    let jobs: Vec<Option<WebhookJob>> = batch
        .iter()
        .map(|m| serde_json::from_value(m.body.clone()).ok())
        .collect();
    for j in jobs.iter().flatten() {
        if let WebhookJob::Deliver {
            owner_kind: OwnerKind::Mailbox,
            owner_id,
            tenant_id,
            identity_id,
            event_id,
            ..
        } = j
        {
            wanted
                .entry((owner_id.clone(), tenant_id.clone(), identity_id.clone()))
                .or_default()
                .push(event_id.clone());
        }
    }
    let mut payloads: HashMap<String, Payload> = HashMap::new();
    let mut failed_owners: Vec<String> = Vec::new();
    for ((owner, tenant, identity), ids) in wanted {
        match get_events(p, &owner, tenant.as_deref(), identity.as_deref(), &ids).await {
            Fetch::Found(found) => {
                for id in ids {
                    payloads.insert(
                        id.clone(),
                        found
                            .get(&id)
                            .cloned()
                            .map_or(Payload::Gone, Payload::Found),
                    );
                }
            }
            Fetch::Gone => {
                for id in ids {
                    payloads.insert(id, Payload::Gone);
                }
            }
            Fetch::Failed => failed_owners.push(owner),
        }
    }
    for (m, job) in batch.iter().zip(jobs) {
        let done = match job {
            Some(WebhookJob::Fanout {
                event_id,
                event_type,
                tenant_id,
                identity_id,
                owner_kind,
                owner_id,
                ..
            }) => {
                fanout(
                    p,
                    &event_id,
                    &event_type,
                    tenant_id,
                    identity_id,
                    owner_kind,
                    owner_id,
                )
                .await
            }
            Some(WebhookJob::Deliver {
                owner_kind: OwnerKind::Mailbox,
                ref owner_id,
                ..
            }) if failed_owners.contains(owner_id) => Done::Retry,
            Some(job @ WebhookJob::Deliver { .. }) => deliver(p, job, &payloads).await,
            None => {
                crate::log::warn("webhook_bad_job", &[]);
                Done::Ack
            }
        };
        match done {
            Done::Ack => m.ack(),
            Done::Retry => m.retry(Some(RETRY_S)),
        }
    }
}

#[derive(Clone)]
enum Payload {
    Found((String, i64)),
    Gone,
}

/// What a mailbox answered to `GetEvents`.
enum Fetch {
    /// Event ID → (envelope, occurred_at); IDs no longer in the outbox are missing.
    Found(HashMap<String, (String, i64)>),
    /// The identity is gone (its mailbox erased): no payload can come back.
    Gone,
    Failed,
}

/// `GetEvents` on a mailbox.
async fn get_events<P: Platform>(
    p: &P,
    owner: &str,
    tenant: Option<&str>,
    identity: Option<&str>,
    ids: &[String],
) -> Fetch {
    let request_id = crate::request_id::new(p);
    let env = envelope_for(
        p,
        tenant,
        identity,
        &request_id,
        None,
        MailboxRequest::GetEvents {
            event_ids: ids.to_vec(),
        },
    );
    match p
        .objects()
        .call::<_, Value>(DoClass::Mailbox, owner, &env)
        .await
    {
        Ok(RpcResult::Ok(v)) => {
            #[derive(Deserialize)]
            struct E {
                event_id: String,
                payload_json: String,
                occurred_at: i64,
            }
            let events: Vec<E> = serde_json::from_value(v["events"].clone()).unwrap_or_default();
            Fetch::Found(
                events
                    .into_iter()
                    .map(|e| (e.event_id, (e.payload_json, e.occurred_at)))
                    .collect(),
            )
        }
        Ok(RpcResult::Err(e)) if e.error.code == ErrorCode::IdentityNotFound => Fetch::Gone,
        _ => Fetch::Failed,
    }
}

#[allow(clippy::too_many_arguments)]
async fn fanout<P: Platform>(
    p: &P,
    event_id: &str,
    event_type: &str,
    tenant_id: Option<String>,
    identity_id: Option<String>,
    owner_kind: OwnerKind,
    owner_id: String,
) -> Done {
    let now = p.clock().now_ms();
    if event_type == "webhook.test" {
        return Done::Ack;
    }
    let Ok(index) = index_row(p, event_id).await else {
        return Done::Retry;
    };
    let partner = index.as_ref().and_then(|i| i.partner_id.clone());
    let about = if event_type == "webhook.disabled" {
        index
            .as_ref()
            .and_then(|i| i.payload_json.as_deref())
            .and_then(|j| serde_json::from_str::<Value>(j).ok())
            .and_then(|v| v["data"]["webhook_id"].as_str().map(str::to_owned))
    } else {
        None
    };
    let (Ok(cands), Ok(system)) = (
        candidates(p, tenant_id.as_deref(), partner.as_deref(), now).await,
        system_identity(p, now).await,
    ) else {
        return Done::Retry;
    };
    let facts = Facts {
        event_type,
        tenant_id: tenant_id.as_deref(),
        partner_id: partner.as_deref(),
        identity_id: identity_id.as_deref(),
        system_identity: identity_id.is_some() && identity_id == system,
        about_endpoint: about.as_deref(),
    };
    let jobs: Vec<WebhookJob> = cands
        .into_iter()
        .filter(|c| {
            matching::matches(
                &Scope {
                    id: c.id.clone(),
                    tenant_id: c.tenant_id.clone(),
                    partner_id: c.partner_id.clone(),
                    event_types_json: c.event_types_json.clone(),
                    identity_ids_json: c.identity_ids_json.clone(),
                },
                &facts,
            )
        })
        .map(|c| WebhookJob::Deliver {
            v: 1,
            event_id: event_id.to_owned(),
            event_type: event_type.to_owned(),
            endpoint_id: c.id,
            tenant_id: tenant_id.clone(),
            identity_id: identity_id.clone(),
            owner_kind,
            owner_id: owner_id.clone(),
            attempt: 1,
            first_attempt: 1,
            replay: false,
        })
        .collect();
    for chunk in jobs.chunks(100) {
        if p.queues()
            .send_batch(QueueName::Webhooks, chunk, 0)
            .await
            .is_err()
        {
            return Done::Retry;
        }
    }
    if matches!(
        owner_kind,
        OwnerKind::Platform | OwnerKind::Domain | OwnerKind::Job
    ) && p
        .db()
        .execute(Stmt::new(
            "UPDATE event_index SET fanned_out_at = ?2 WHERE id = ?1 AND fanned_out_at IS NULL",
            vec![event_id.into(), now.into()],
        ))
        .await
        .is_err()
    {
        return Done::Retry;
    }
    Done::Ack
}

#[derive(Deserialize)]
struct Status {
    status: String,
}

async fn deliver<P: Platform>(p: &P, job: WebhookJob, payloads: &HashMap<String, Payload>) -> Done {
    let WebhookJob::Deliver {
        event_id,
        event_type,
        endpoint_id,
        tenant_id,
        owner_kind,
        attempt,
        first_attempt,
        replay,
        ..
    } = job.clone()
    else {
        return Done::Ack;
    };
    let now = p.clock().now_ms();
    let db = p.db();
    // 1. Already done?
    let this: Result<Option<Status>, _> = db
        .first(Stmt::new(
            "SELECT status FROM webhook_deliveries WHERE endpoint_id = ?1 AND event_id = ?2 AND attempt = ?3",
            vec![endpoint_id.clone().into(), event_id.clone().into(), i64::from(attempt).into()],
        ))
        .await;
    let Ok(this) = this else {
        return Done::Retry;
    };
    match this.map(|s| s.status) {
        Some(s) if s == "succeeded" || s == "dead" => return Done::Ack,
        Some(_) => return schedule(p, &job, attempt, first_attempt).await,
        None => {}
    }
    if !replay {
        let ok: Result<Option<Status>, _> = db
            .first(Stmt::new(
                "SELECT status FROM webhook_deliveries WHERE endpoint_id = ?1 AND event_id = ?2 AND status = 'succeeded' LIMIT 1",
                vec![endpoint_id.clone().into(), event_id.clone().into()],
            ))
            .await;
        match ok {
            Ok(Some(_)) => return Done::Ack,
            Ok(None) => {}
            Err(_) => return Done::Retry,
        }
    }
    // 2. The endpoint, and its partner's status.
    let row: Row = match db.first(wdb::get(&endpoint_id)).await {
        Ok(Some(r)) => r,
        Ok(None) => return Done::Ack,
        Err(_) => return Done::Retry,
    };
    let record =
        |status: &str, http: Option<u16>, error: Option<&str>, ms: i64, next: Option<i64>| {
            wdb::record(
                &p.ids().new_id(IdPrefix::Delivery),
                &endpoint_id,
                tenant_id.as_deref(),
                &event_id,
                &event_type,
                attempt,
                status,
                http,
                error,
                ms,
                next,
                now,
            )
        };
    if row.enabled == 0 {
        return match db
            .execute(record("dead", None, Some("endpoint_disabled"), 0, None))
            .await
        {
            Ok(_) => Done::Ack,
            Err(_) => Done::Retry,
        };
    }
    if row.partner_status.as_deref() == Some("suspended")
        && let Some(partner) = &row.owner_partner_id
    {
        // J13: parked until the partner is active again; no attempt is used.
        let occurred = match payloads.get(&event_id) {
            Some(Payload::Found((_, at))) => *at,
            _ => now,
        };
        return match db
            .execute(Stmt::new(
                "INSERT OR IGNORE INTO webhook_held (endpoint_id, event_id, partner_id, job_json, occurred_at, held_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                vec![
                    endpoint_id.clone().into(),
                    event_id.clone().into(),
                    partner.clone().into(),
                    serde_json::to_string(&job).unwrap_or_default().into(),
                    occurred.into(),
                    now.into(),
                ],
            ))
            .await
        {
            Ok(_) => Done::Ack,
            Err(_) => Done::Retry,
        };
    }
    // 3. The payload.
    let payload = match owner_kind {
        // Domain and job events keep their payload in the index too (a removed domain's monitor and a
        // finished job keep nothing).
        OwnerKind::Platform | OwnerKind::Domain | OwnerKind::Job => {
            match index_row(p, &event_id).await {
                Ok(Some(IndexRow {
                    payload_json: Some(b),
                    occurred_at,
                    ..
                })) => Some((b, occurred_at)),
                Ok(_) => None,
                Err(_) => return Done::Retry,
            }
        }
        OwnerKind::Mailbox => match payloads.get(&event_id) {
            Some(Payload::Found(f)) => Some(f.clone()),
            _ => None,
        },
    };
    let Some((body, occurred_at)) = payload else {
        crate::metrics::count(p, "webhook_dead_total", tenant_id.as_deref(), 1);
        return match db
            .execute(record("dead", None, Some("event_unavailable"), 0, None))
            .await
        {
            Ok(_) => Done::Ack,
            Err(_) => Done::Retry,
        };
    };
    // 4. Sign and send.
    let outcome = client::attempt(p, &row.endpoint(), &event_id, body.as_bytes()).await;
    crate::log::info(
        "webhook_attempt",
        &[
            ("endpoint", endpoint_id.clone().into()),
            ("event", event_id.clone().into()),
            ("attempt", i64::from(attempt).into()),
            ("status", i64::from(outcome.http_status.unwrap_or(0)).into()),
            ("error", outcome.error.unwrap_or("").into()),
            ("duration_ms", outcome.duration_ms.into()),
        ],
    );
    // `result` is in the `webhook_attempt` line; labelled counters come with M17's catalogue.
    crate::metrics::count(p, "webhook_attempts_total", tenant_id.as_deref(), 1);
    // 5. Record.
    let k = attempt - first_attempt + 1;
    if outcome.succeeded {
        let r = db
            .batch(vec![
                record(
                    "succeeded",
                    outcome.http_status,
                    None,
                    outcome.duration_ms,
                    None,
                ),
                Stmt::new(
                    "UPDATE webhook_endpoints SET consecutive_failures = 0 WHERE id = ?1",
                    vec![endpoint_id.clone().into()],
                ),
            ])
            .await;
        if r.is_err() {
            return Done::Retry;
        }
        if k == 1 && !replay {
            crate::log::info(
                "metric",
                &[
                    ("name", "webhook_delivery_latency_ms".into()),
                    ("value", (now - occurred_at).max(0).into()),
                    ("event_class", event_class(&event_type).into()),
                ],
            );
        }
        return Done::Ack;
    }
    let last = k >= ATTEMPTS;
    let scheduled = (!last).then(|| delay_s(p, k));
    let status = if last { "dead" } else { "failed" };
    #[derive(Deserialize)]
    struct F {
        consecutive_failures: i64,
    }
    let r = db
        .batch(vec![
            record(
                status,
                outcome.http_status,
                outcome.error,
                outcome.duration_ms,
                scheduled.map(|s| now + i64::from(s) * 1000),
            ),
            Stmt::new(
                "UPDATE webhook_endpoints SET consecutive_failures = consecutive_failures + 1, updated_at = ?2 WHERE id = ?1",
                vec![endpoint_id.clone().into(), now.into()],
            ),
        ])
        .await;
    if r.is_err() {
        return Done::Retry;
    }
    if last {
        crate::metrics::count(p, "webhook_dead_total", tenant_id.as_deref(), 1);
    }
    // 6. Auto-disable.
    let failures: Option<F> = db
        .first(Stmt::new(
            "SELECT consecutive_failures FROM webhook_endpoints WHERE id = ?1",
            vec![endpoint_id.clone().into()],
        ))
        .await
        .ok()
        .flatten();
    let gone = outcome.error == Some("status_410");
    let mut disable = gone;
    if !gone && failures.is_some_and(|f| f.consecutive_failures >= DISABLE_AFTER) {
        #[derive(Deserialize)]
        struct M {
            first: Option<i64>,
        }
        let first: Option<M> = db
            .first(Stmt::new(
                "SELECT MIN(created_at) AS first FROM webhook_deliveries WHERE endpoint_id = ?1 \
                 AND created_at > COALESCE((SELECT MAX(created_at) FROM webhook_deliveries WHERE endpoint_id = ?1 AND status = 'succeeded'), 0)",
                vec![endpoint_id.clone().into()],
            ))
            .await
            .ok()
            .flatten();
        disable = first
            .and_then(|m| m.first)
            .is_some_and(|f| now - f >= DAY_MS);
    }
    if disable {
        disable_endpoint(
            p,
            &row,
            &event_id,
            attempt,
            if gone { "gone" } else { "failing" },
            now,
        )
        .await;
        return Done::Ack;
    }
    // 7. The next attempt.
    match scheduled {
        Some(s) => {
            let next = with_attempt(&job, attempt + 1);
            if p.queues()
                .send(QueueName::Webhooks, &next, queue_delay(s))
                .await
                .is_err()
            {
                return Done::Retry;
            }
            Done::Ack
        }
        None => Done::Ack,
    }
}

fn event_class(event_type: &str) -> &'static str {
    if matches!(event_type, "message.received" | "message.quarantined") {
        "inbound"
    } else {
        "other"
    }
}

fn with_attempt(job: &WebhookJob, attempt: u32) -> WebhookJob {
    let mut j = job.clone();
    if let WebhookJob::Deliver { attempt: a, .. } = &mut j {
        *a = attempt;
    }
    j
}

/// A `failed` attempt seen again: make sure its successor is queued.
async fn schedule<P: Platform>(p: &P, job: &WebhookJob, attempt: u32, first_attempt: u32) -> Done {
    let k = attempt - first_attempt + 1;
    if k >= ATTEMPTS {
        return Done::Ack;
    }
    let s = delay_s(p, k);
    match p
        .queues()
        .send(
            QueueName::Webhooks,
            &with_attempt(job, attempt + 1),
            queue_delay(s),
        )
        .await
    {
        Ok(()) => Done::Ack,
        Err(_) => Done::Retry,
    }
}

/// Disables an endpoint and writes `webhook.disabled` in the same batch, then queues its `Fanout`.
async fn disable_endpoint<P: Platform>(
    p: &P,
    row: &Row,
    trigger_event: &str,
    attempt: u32,
    reason: &str,
    now: i64,
) {
    let event_id = pylota_identity_core::ids::derived(
        IdPrefix::Event,
        now,
        &p.secrets().hash_key,
        &format!("webhook.disabled:{}:{trigger_event}:{attempt}", row.id),
    );
    let data = json!({ "webhook_id": row.id, "reason": reason });
    let body = envelope(
        &event_id,
        "webhook.disabled",
        now,
        row.tenant_id.as_deref(),
        None,
        None,
        &data,
    );
    let r = p
        .db()
        .batch(vec![
            Stmt::new(
                "UPDATE webhook_endpoints SET enabled = 0, disabled_reason = 'failing', updated_at = ?2 WHERE id = ?1 AND enabled = 1",
                vec![row.id.clone().into(), now.into()],
            ),
            Stmt::new(
                "INSERT OR IGNORE INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, partner_id, payload_json, occurred_at) VALUES (?1, ?2, NULL, 'webhook.disabled', 'platform', 'platform', ?3, ?4, ?5)",
                vec![
                    event_id.clone().into(),
                    row.tenant_id.clone().into(),
                    row.owner_partner_id.clone().into(),
                    body.into(),
                    now.into(),
                ],
            ),
        ])
        .await;
    if r.is_err() {
        return;
    }
    crate::metrics::count(p, "webhook_disabled_total", row.tenant_id.as_deref(), 1);
    ENDPOINTS.with(|e| e.borrow_mut().clear());
    let fan = WebhookJob::Fanout {
        v: 1,
        event_id,
        event_type: "webhook.disabled".into(),
        tenant_id: row.tenant_id.clone(),
        identity_id: None,
        owner_kind: OwnerKind::Platform,
        owner_id: "platform".into(),
        occurred_at: now,
    };
    // A lost send is repaired by the every-minute sweep of unfanned platform events.
    let _ = p.queues().send(QueueName::Webhooks, &fan, 0).await;
}
