//! The `retention` jobs ([Privacy and erasure § 5]): one per tenant per UTC day (`tenant_id` set) and
//! one deployment-wide job (`tenant_id` NULL) for the control-plane tables. Every bulk D1 delete is
//! batched (§ 6.12); every R2 delete goes to `BACKUP` too when it is bound.
//!
//! [Privacy and erasure § 5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{
    BlobStore as _, Clock as _, ControlDb as _, Ids as _, Platform, QueueProducer as _,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::erasure::{batched_delete, delete_prefix};
use super::{Ctx, Slice, StepErr, StepResult};
use crate::db::{audit, tenants as tdb};
use crate::mailbox::MailboxRequest;
use crate::mailbox::privacy::{EraseMode, PrivacyOp};

const DAY_MS: i64 = 86_400_000;

pub const TENANT_STEPS: &[&str] = &[
    "plan", "raw", "messages", "events", "hygiene", "holds", "audit",
];
pub const GLOBAL_STEPS: &[&str] = &[
    "idempotency",
    "platform_events",
    "jobs",
    "usage",
    "dlq",
    "signing_keys",
    "identity_keys",
    "staging",
    "held_erasures",
    "audit",
];

pub fn steps(params: &Value) -> Vec<&'static str> {
    if params["global"] == true {
        GLOBAL_STEPS.to_vec()
    } else {
        TENANT_STEPS.to_vec()
    }
}

#[derive(Deserialize, Clone)]
struct Ident {
    idx: i64,
    identity_id: String,
    mailbox_do_id: String,
    is_system: i64,
}

fn migrate<S: Sql>(s: &S) -> pylota_identity_platform::PResult<()> {
    s.exec(
        "CREATE TABLE IF NOT EXISTS idents (idx INTEGER PRIMARY KEY, identity_id TEXT NOT NULL UNIQUE, mailbox_do_id TEXT NOT NULL, is_system INTEGER NOT NULL)",
        &[],
    )?;
    Ok(())
}

fn idents<S: Sql>(s: &S) -> pylota_identity_platform::PResult<Vec<Ident>> {
    s.query(
        "SELECT idx, identity_id, mailbox_do_id, is_system FROM idents ORDER BY idx",
        &[],
    )
}

fn cutoff<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, k: &str) -> Option<i64> {
    c.get(k).ok().flatten().and_then(|v| v.parse().ok())
}

pub async fn run<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    step: &str,
    cursor: Option<&str>,
) -> StepResult {
    migrate(c.sql)?;
    if c.params["global"] == true {
        return global(c, step, cursor).await;
    }
    match step {
        "plan" => plan(c).await,
        "raw" | "messages" | "events" | "hygiene" | "holds" => per_identity(c, step, cursor).await,
        "audit" => tenant_audit(c).await,
        _ => Err(StepErr::new("unknown_step")),
    }
}

/// Step 1: the cutoffs from the tenant's effective policy, and the identities.
async fn plan<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    let t: tdb::Row =
        c.p.db()
            .first(tdb::get(c.tenant()))
            .await?
            .ok_or_else(|| StepErr::new("tenant_not_found"))?;
    let pol = crate::policy::effective(c.p, &t);
    let get = |k: &str| pylota_identity_core::policy::get_path(&pol, k).and_then(Value::as_i64);
    let raw_days = get("retention.raw_days").unwrap_or(90);
    // `emergency_prune` (Observability § 5.6, J30): at most 7 days of events while the switch is on.
    let mut events_days = get("retention.events_days").unwrap_or(30);
    if crate::ops::switches::state(c.p, "emergency_prune").await == "on" {
        events_days = events_days.min(7);
    }
    c.set("raw_cutoff", &(c.now - raw_days * DAY_MS).to_string())?;
    c.set("events_cutoff", &(c.now - events_days * DAY_MS).to_string())?;
    match get("retention.message_days") {
        Some(d) => c.set("message_cutoff", &(c.now - d * DAY_MS).to_string())?,
        None => c.del("message_cutoff")?,
    }
    #[derive(Deserialize)]
    struct I {
        id: String,
        mailbox_do_id: String,
        is_system: i64,
    }
    let rows: Vec<I> = c
        .p
        .db()
        .query(Stmt::new(
            "SELECT id, mailbox_do_id, is_system FROM identities WHERE tenant_id = ?1 AND status <> 'deleted' AND mailbox_do_id <> '' ORDER BY id",
            vec![c.tenant().into()],
        ))
        .await?;
    for r in rows {
        c.sql.exec(
            "INSERT OR IGNORE INTO idents (identity_id, mailbox_do_id, is_system) VALUES (?1, ?2, ?3)",
            &[r.id.into(), r.mailbox_do_id.into(), r.is_system.into()],
        )?;
    }
    Ok(Slice::done())
}

/// The system identity always keeps raw MIME 7 days and messages 30 (Privacy § 5.2, step 1).
fn cutoffs<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, i: &Ident) -> (Option<i64>, Option<i64>) {
    if i.is_system == 1 {
        return (Some(c.now - 7 * DAY_MS), Some(c.now - 30 * DAY_MS));
    }
    (cutoff(c, "raw_cutoff"), cutoff(c, "message_cutoff"))
}

async fn delete_keys<P: Platform>(p: &P, keys: &[String]) -> Result<i64, StepErr> {
    if keys.is_empty() {
        return Ok(0);
    }
    p.blobs().delete(keys).await?;
    if let Some(b) = p.backup() {
        b.delete(keys).await?;
    }
    Ok(keys.len() as i64)
}

/// Steps 2–6 over each identity (cursor `{idx}:{rowid}`).
async fn per_identity<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    step: &str,
    cursor: Option<&str>,
) -> StepResult {
    let all = idents(c.sql)?;
    let (idx, rest) = cursor
        .and_then(|c| c.split_once(':'))
        .map_or((0, ""), |(i, r)| (i.parse().unwrap_or(0), r));
    let Some(i) = all.iter().find(|x| x.idx >= idx).cloned() else {
        return Ok(Slice::done());
    };
    let after: i64 = if i.idx == idx {
        rest.parse().unwrap_or(0)
    } else {
        0
    };
    let mb = |req: PrivacyOp| {
        c.mailbox(
            &i.identity_id,
            &i.mailbox_do_id,
            MailboxRequest::Privacy(req),
        )
    };
    let (raw_cut, msg_cut) = cutoffs(c, &i);
    let mut counts = Map::new();
    let mut next: Option<i64> = None;
    match step {
        "raw" => {
            if let Some(cut) = raw_cut {
                let plan = match mb(PrivacyOp::RetentionPlan {
                    raw_cutoff_ms: Some(cut),
                    message_cutoff_ms: None,
                    after_rowid: after,
                    limit: 200,
                })
                .await?
                {
                    Ok(v) => v,
                    Err(e) if e.code == ErrorCode::IdentityNotFound => json!({}),
                    Err(e) => return Err(e.into()),
                };
                let msgs = plan["messages"].as_array().cloned().unwrap_or_default();
                let mut keys: Vec<String> = Vec::new();
                let mut ids: Vec<String> = Vec::new();
                for m in &msgs {
                    let raw: Vec<String> = m["raw_keys"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|k| k.as_str().map(str::to_owned))
                        .collect();
                    let out = m["out_key"].as_str().map(str::to_owned);
                    if raw.is_empty() && out.is_none() {
                        continue;
                    }
                    keys.extend(raw);
                    keys.extend(out);
                    ids.push(m["id"].as_str().unwrap_or("").to_owned());
                }
                // Objects first, then the column: a crash leaves no unreferenced object.
                let n = delete_keys(c.p, &keys).await?;
                if !ids.is_empty() {
                    mb(PrivacyOp::PurgeRaw { message_ids: ids })
                        .await?
                        .map_err(StepErr::from)?;
                }
                pylota_identity_core::jobs::add_counts(
                    &mut counts,
                    &json!({ "r2_objects_deleted": n }),
                );
                next = plan["next_after_rowid"].as_i64();
            }
        }
        "messages" => {
            if let Some(cut) = msg_cut {
                let plan = match mb(PrivacyOp::RetentionPlan {
                    raw_cutoff_ms: None,
                    message_cutoff_ms: Some(cut),
                    after_rowid: after,
                    limit: 100,
                })
                .await?
                {
                    Ok(v) => v,
                    Err(e) if e.code == ErrorCode::IdentityNotFound => json!({}),
                    Err(e) => return Err(e.into()),
                };
                let msgs = plan["messages"].as_array().cloned().unwrap_or_default();
                if !msgs.is_empty() {
                    let vectors: Vec<String> = msgs
                        .iter()
                        .flat_map(|m| m["vector_ids"].as_array().cloned().unwrap_or_default())
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    for b in vectors.chunks(500) {
                        pylota_identity_platform::VectorIndex::delete_by_ids(c.p.vectors(), b)
                            .await?;
                    }
                    let mut r2 = 0;
                    for m in &msgs {
                        for pfx in m["r2_prefixes"].as_array().cloned().unwrap_or_default() {
                            if let Some(pfx) = pfx.as_str() {
                                r2 += delete_prefix(c.p, pfx).await?;
                            }
                        }
                    }
                    let out = mb(PrivacyOp::EraseRows {
                        rowids: msgs.iter().filter_map(|m| m["rowid"].as_i64()).collect(),
                        counterparty: None,
                        mode: EraseMode::Retention,
                    })
                    .await?
                    .map_err(StepErr::from)?;
                    pylota_identity_core::jobs::add_counts(&mut counts, &out["counts"]);
                    pylota_identity_core::jobs::add_counts(
                        &mut counts,
                        &json!({ "r2_objects_deleted": r2, "vectors_deleted": vectors.len() }),
                    );
                }
                next = plan["next_after_rowid"].as_i64();
            }
        }
        "events" => {
            let cut = cutoff(c, "events_cutoff").unwrap_or(c.now - 30 * DAY_MS);
            if let Ok(v) = mb(PrivacyOp::PurgeEvents { cutoff_ms: cut }).await? {
                pylota_identity_core::jobs::add_counts(&mut counts, &v);
            }
        }
        "hygiene" => {
            let _ = mb(PrivacyOp::Hygiene { now_ms: c.now }).await?;
        }
        "holds" => {
            let held = match mb(PrivacyOp::ActiveHolds { now_ms: c.now }).await? {
                Ok(v) => v["held"].as_i64().unwrap_or(0),
                Err(_) => 0,
            };
            if held == 0 {
                continue_held_identity(c.p, c.tenant(), &i.identity_id, c.now).await?;
            }
        }
        _ => {}
    }
    let is_last = !all.iter().any(|x| x.idx > i.idx);
    // The D1 part of the events step and the exports' expiry run once, after the last identity.
    if next.is_none() && is_last {
        match step {
            "events" => {
                let cut = cutoff(c, "events_cutoff").unwrap_or(c.now - 30 * DAY_MS);
                let mut budget = 20;
                let (a, done_a) = batched_delete(
                    c.p,
                    "DELETE FROM webhook_deliveries WHERE rowid IN (SELECT rowid FROM webhook_deliveries WHERE tenant_id = ?1 AND created_at < ?2 LIMIT 1000)",
                    vec![c.tenant().into(), cut.into()],
                    &mut budget,
                )
                .await?;
                let (b, done_b) = batched_delete(
                    c.p,
                    "DELETE FROM event_index WHERE rowid IN (SELECT rowid FROM event_index WHERE tenant_id = ?1 AND occurred_at < ?2 LIMIT 1000)",
                    vec![c.tenant().into(), cut.into()],
                    &mut budget,
                )
                .await?;
                pylota_identity_core::jobs::add_counts(
                    &mut counts,
                    &json!({ "events_deleted": a + b }),
                );
                if !(done_a && done_b) {
                    return Ok(Slice::More {
                        cursor: format!("{}:", i.idx),
                        counts: Value::Object(counts),
                    });
                }
            }
            "hygiene" => expire_exports(c.p, c.tenant(), c.now).await?,
            _ => {}
        }
    }
    let cursor = match next {
        Some(n) => format!("{}:{n}", i.idx),
        None => format!("{}:", i.idx + 1),
    };
    Ok(if next.is_some() || !is_last {
        Slice::More {
            cursor,
            counts: Value::Object(counts),
        }
    } else {
        Slice::Done {
            counts: Value::Object(counts),
        }
    })
}

/// Exports past `expires_at`: the ZIP goes and the export is `expired`.
async fn expire_exports<P: Platform>(p: &P, tenant: &str, now: i64) -> Result<(), StepErr> {
    #[derive(Deserialize)]
    struct E {
        id: String,
        r2_key: Option<String>,
    }
    let rows: Vec<E> = p
        .db()
        .query(Stmt::new(
            "SELECT id, r2_key FROM exports WHERE tenant_id = ?1 AND status = 'completed' AND expires_at < ?2",
            vec![tenant.into(), now.into()],
        ))
        .await?;
    for e in rows {
        if let Some(k) = e.r2_key {
            delete_keys(p, &[k]).await?;
        }
        p.db()
            .execute(Stmt::new(
                "UPDATE exports SET status = 'expired' WHERE id = ?1",
                vec![e.id.into()],
            ))
            .await?;
    }
    Ok(())
}

/// An identity still `deleting` whose latest identity-scope request ended `completed_with_holds`
/// and has no hold left gets its continuation (Privacy § 5.2, step 6).
async fn continue_held_identity<P: Platform>(
    p: &P,
    tenant: &str,
    identity: &str,
    now: i64,
) -> Result<(), StepErr> {
    #[derive(Deserialize)]
    struct L {
        id: String,
        job_id: String,
        status: String,
        identity_status: String,
    }
    let l: Option<L> = p
        .db()
        .first(Stmt::new(
            "SELECT e.id, e.job_id, e.status, i.status AS identity_status FROM erasure_requests e JOIN identities i ON i.id = e.identity_id WHERE e.tenant_id = ?1 AND e.scope = 'identity' AND e.identity_id = ?2 ORDER BY e.created_at DESC, e.id DESC LIMIT 1",
            vec![tenant.into(), identity.into()],
        ))
        .await?;
    if let Some(l) =
        l.filter(|l| l.status == "completed_with_holds" && l.identity_status == "deleting")
    {
        continuation(p, tenant, "identity", Some(identity), &l.id, &l.job_id, now).await?;
    }
    Ok(())
}

/// A `hold_released:{era_id}` request resuming `resumes`, audit-logged (`erasure.create`).
pub async fn continuation<P: Platform>(
    p: &P,
    tenant: &str,
    scope: &str,
    identity: Option<&str>,
    resumes: &str,
    _resumes_job: &str,
    now: i64,
) -> Result<(), StepErr> {
    let ids = p.ids();
    let era = ids.new_id(IdPrefix::Erasure);
    let job = super::new_job(p)?;
    let params = json!({ "scope": scope, "erasure_request_id": era, "identity_id": identity, "target_id": null, "counterparty_hash": null });
    let r = p
        .db()
        .batch(vec![
            Stmt::new(
                "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_at, updated_at) VALUES (?1, ?2, 'erasure', 'queued', ?3, ?4, ?5, ?5)",
                vec![job.id.clone().into(), tenant.into(), job.runner_do_id.clone().into(), params.to_string().into(), now.into()],
            ),
            Stmt::new(
                "INSERT INTO erasure_requests (id, tenant_id, job_id, scope, identity_id, reason, status, resumes_id, deadline_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7, ?8, ?9)",
                vec![
                    era.clone().into(),
                    tenant.into(),
                    job.id.clone().into(),
                    scope.into(),
                    identity.map(str::to_owned).into(),
                    format!("hold_released:{resumes}").into(),
                    resumes.into(),
                    (now + crate::db::jobs::ERASURE_DEADLINE_MS).into(),
                    now.into(),
                ],
            ),
            audit::insert(
                &ids.new_id(IdPrefix::Audit),
                Some(tenant),
                None,
                "erasure.create",
                "erasure_request",
                &era,
                &json!({ "scope": scope, "resumes_id": resumes, "reason": "hold_released" }),
                "req_retention",
                now,
            ),
        ])
        .await;
    // A continuation that already exists (the unique index on resumes_id) is not an error.
    if r.is_err() {
        return Ok(());
    }
    super::start(
        p,
        &job.runner_do_id,
        super::JobRequest::Start {
            job_id: job.id,
            kind: "erasure".into(),
            tenant_id: Some(tenant.to_owned()),
            params,
            // A continuation runs every step again (only a failed request's resumption copies progress).
            resume_job_id: None,
        },
        Some(tenant),
    )
    .await;
    Ok(())
}

/// Step 7: one audit row per step that deleted anything (FR-PRV-2).
async fn tenant_audit<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    let mut batch = Vec::new();
    for step in ["raw", "messages", "events", "hygiene"] {
        let counts = c.step_counts(step)?;
        let any = counts
            .as_object()
            .is_some_and(|o| o.values().any(|v| v.as_i64().unwrap_or(0) > 0));
        if !any {
            continue;
        }
        let cut = match step {
            "raw" => cutoff(c, "raw_cutoff"),
            "messages" => cutoff(c, "message_cutoff"),
            "events" => cutoff(c, "events_cutoff"),
            _ => None,
        };
        batch.push(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            Some(c.tenant()),
            None,
            "retention.purge",
            "tenant",
            c.tenant(),
            &json!({
                "step": step, "cutoff_ms": cut, "identities": idents(c.sql)?.len(),
                "r2_objects_deleted": counts["r2_objects_deleted"].as_i64().unwrap_or(0),
                "messages_deleted": counts["messages_deleted"].as_i64().unwrap_or(0),
                "vectors_deleted": counts["vectors_deleted"].as_i64().unwrap_or(0),
                "events_deleted": counts["events_deleted"].as_i64().unwrap_or(0),
            }),
            &format!("req_{}", &c.job_id[4..]),
            c.now,
        ));
    }
    if !batch.is_empty() {
        c.p.db().batch(batch).await?;
    }
    Ok(Slice::done())
}

// ---- The global job (Privacy § 5.3) ------------------------------------------------------------

async fn global<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    step: &str,
    cursor: Option<&str>,
) -> StepResult {
    let mut budget = 20;
    let now = c.now;
    let del = |sql: &'static str, args: Vec<pylota_identity_platform::d1::SqlValue>| (sql, args);
    let work: Vec<(&'static str, Vec<pylota_identity_platform::d1::SqlValue>)> = match step {
        "idempotency" => vec![
            del(
                "DELETE FROM idempotency_records WHERE rowid IN (SELECT rowid FROM idempotency_records WHERE expires_at < ?1 LIMIT 1000)",
                vec![now.into()],
            ),
            del(
                "DELETE FROM link_uses WHERE rowid IN (SELECT rowid FROM link_uses WHERE expires_at < ?1 LIMIT 1000)",
                vec![now.into()],
            ),
        ],
        "platform_events" => vec![
            del(
                "DELETE FROM webhook_deliveries WHERE rowid IN (SELECT rowid FROM webhook_deliveries WHERE tenant_id IS NULL AND created_at < ?1 LIMIT 1000)",
                vec![(now - 30 * DAY_MS).into()],
            ),
            del(
                "DELETE FROM event_index WHERE rowid IN (SELECT rowid FROM event_index WHERE tenant_id IS NULL AND occurred_at < ?1 LIMIT 1000)",
                vec![(now - 30 * DAY_MS).into()],
            ),
        ],
        "jobs" => vec![del(
            "DELETE FROM jobs WHERE rowid IN (SELECT rowid FROM jobs WHERE kind NOT IN ('erasure','export') AND status IN ('completed','failed','canceled') AND completed_at < ?1 LIMIT 1000)",
            vec![(now - 90 * DAY_MS).into()],
        )],
        "usage" => vec![del(
            "DELETE FROM usage_daily WHERE rowid IN (SELECT rowid FROM usage_daily WHERE day < ?1 LIMIT 1000)",
            vec![pylota_identity_core::days::utc_day(now - 92 * DAY_MS).into()],
        )],
        "dlq" => vec![del(
            "DELETE FROM dlq_items WHERE rowid IN (SELECT rowid FROM dlq_items WHERE first_seen_at < ?1 LIMIT 1000)",
            vec![(now - 14 * DAY_MS).into()],
        )],
        "signing_keys" => vec![del(
            "DELETE FROM signing_keys WHERE rowid IN (SELECT rowid FROM signing_keys WHERE verify_until IS NOT NULL AND verify_until < ?1 LIMIT 1000)",
            vec![now.into()],
        )],
        // Retiring identity keys past their overlap become `retired`; the rows stay until identity
        // deletion so a thumbprint is never reused (Agent signing keys § 2).
        "identity_keys" => vec![del(
            "UPDATE identity_keys SET status = 'retired', retired_at = ?1 WHERE rowid IN (SELECT rowid FROM identity_keys WHERE status = 'retiring' AND verify_until <= ?1 LIMIT 1000)",
            vec![now.into()],
        )],
        "staging" => return staging(c, cursor).await,
        "held_erasures" => return held_erasures(c).await,
        "audit" => return global_audit(c).await,
        _ => return Err(StepErr::new("unknown_step")),
    };
    let start: usize = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
    let mut total = 0;
    for (i, (sql, args)) in work.into_iter().enumerate().skip(start) {
        let (n, done) = batched_delete(c.p, sql, args, &mut budget).await?;
        total += n;
        if !done {
            return Ok(Slice::More {
                cursor: i.to_string(),
                counts: json!({ "rows_deleted": total }),
            });
        }
    }
    Ok(Slice::Done {
        counts: json!({ "rows_deleted": total }),
    })
}

/// `staging`: outage copies older than an hour are re-queued once a day as `Staged` pointers; never
/// deleted here (the 15-day lifecycle rule does that).
async fn staging<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, cursor: Option<&str>) -> StepResult {
    let page = c.p.blobs().list("inbound-staging/", cursor, 500).await?;
    let mut requeued = 0;
    let mut lost = 0;
    for (key, _) in &page.keys {
        // Only the outage copies: never ses/ or role/.
        let rest = &key["inbound-staging/".len()..];
        if !rest.starts_with(|ch: char| ch.is_ascii_digit()) {
            continue;
        }
        let Some(blob) = c.p.blobs().get(key).await? else {
            continue;
        };
        let meta = |k: &str| {
            blob.custom
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
        };
        let received_at: i64 = meta("received_at")
            .and_then(|v| v.parse().ok())
            .unwrap_or(c.now);
        if c.now - received_at < 3_600_000 {
            continue;
        }
        if c.now - received_at > 14 * DAY_MS {
            lost += 1;
            continue;
        }
        let ptr = crate::consumers::inbound::InboundJob::Staged(
            crate::consumers::inbound::StagedPointer {
                v: 1,
                staging_key: key.clone(),
                raw_size: blob.size,
                envelope_from: meta("envelope_from").unwrap_or_default(),
                envelope_to: meta("envelope_to").unwrap_or_default(),
                received_at,
                request_id: format!("req_{}", &c.job_id[4..]),
            },
        );
        if c.p.queues().send(QueueName::Inbound, &ptr, 0).await.is_ok() {
            requeued += 1;
        }
    }
    if lost > 0 {
        crate::metrics::count(c.p, "inbound_lost_total", None, lost);
    }
    let counts = json!({ "staging_requeued": requeued });
    Ok(match page.cursor {
        Some(next) => Slice::More {
            cursor: next,
            counts,
        },
        None => Slice::Done { counts },
    })
}

/// `held_erasures`: an `erasing` tenant whose latest tenant-scope request ended
/// `completed_with_holds` and whose identities hold nothing any more gets its continuation.
async fn held_erasures<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    #[derive(Deserialize)]
    struct T {
        id: String,
        job_id: String,
        tenant_id: String,
    }
    let rows: Vec<T> = c
        .p
        .db()
        .query(Stmt::new(
            "SELECT e.id, e.job_id, e.tenant_id FROM erasure_requests e JOIN tenants t ON t.id = e.tenant_id WHERE t.status = 'erasing' AND e.scope = 'tenant' AND e.status = 'completed_with_holds' \
             AND e.created_at = (SELECT MAX(created_at) FROM erasure_requests x WHERE x.tenant_id = e.tenant_id AND x.scope = 'tenant') LIMIT 50",
            vec![],
        ))
        .await?;
    let mut started = 0;
    for t in rows {
        #[derive(Deserialize)]
        struct I {
            id: String,
            mailbox_do_id: String,
        }
        let ids: Vec<I> = c
            .p
            .db()
            .query(Stmt::new(
                "SELECT id, mailbox_do_id FROM identities WHERE tenant_id = ?1 AND status = 'deleting' AND mailbox_do_id <> ''",
                vec![t.tenant_id.clone().into()],
            ))
            .await?;
        let mut held = 0;
        for i in ids {
            let env = crate::rpc::envelope_for(
                c.p,
                Some(&t.tenant_id),
                Some(&i.id),
                "req_retention",
                None,
                MailboxRequest::Privacy(PrivacyOp::ActiveHolds { now_ms: c.now }),
            );
            match pylota_identity_platform::ObjectClient::call::<_, Value>(
                c.p.objects(),
                pylota_identity_platform::durable::DoClass::Mailbox,
                &i.mailbox_do_id,
                &env,
            )
            .await
            {
                Ok(crate::rpc::RpcResult::Ok(v)) => held += v["held"].as_i64().unwrap_or(0),
                Ok(_) => {}
                Err(_) => return Err(StepErr::new("mailbox_unreachable")),
            }
        }
        if held == 0 {
            continuation(c.p, &t.tenant_id, "tenant", None, &t.id, &t.job_id, c.now).await?;
            started += 1;
        }
    }
    Ok(Slice::Done {
        counts: json!({ "continuations": started }),
    })
}

async fn global_audit<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    let mut batch = Vec::new();
    for step in GLOBAL_STEPS.iter().filter(|s| **s != "audit") {
        let counts = c.step_counts(step)?;
        batch.push(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            None,
            None,
            "retention.purge",
            "deployment",
            "global",
            &json!({ "step": step, "counts": counts }),
            &format!("req_{}", &c.job_id[4..]),
            c.now,
        ));
    }
    c.p.db().batch(batch).await?;
    Ok(Slice::done())
}

/// The `*/15` cron (Privacy § 5.1): up to 50 active tenants whose latest retention job was created
/// before today get one; the global job and the backup job once per UTC day; the daily restart of
/// failed domain removals in the run whose UTC hour is 04 and minute below 15.
pub async fn schedule<P: Platform>(p: &P) {
    let now = p.clock().now_ms();
    let today = now.div_euclid(DAY_MS) * DAY_MS;
    #[derive(Deserialize)]
    struct T {
        id: String,
    }
    if let Ok(rows) = p
        .db()
        .query::<T>(Stmt::new(
            "SELECT t.id FROM tenants t WHERE t.status IN ('active','suspended') AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.tenant_id = t.id AND j.kind = 'retention' AND j.created_at >= ?1) ORDER BY t.id LIMIT 50",
            vec![today.into()],
        ))
        .await
    {
        for t in rows {
            let _ = super::create(p, "retention", Some(&t.id), &json!({})).await;
        }
    }
    #[derive(Deserialize)]
    struct N {
        n: i64,
    }
    let ran = |kind: &'static str| async move {
        p.db()
            .first::<N>(Stmt::new(
                "SELECT COUNT(*) AS n FROM jobs WHERE tenant_id IS NULL AND kind = ?1 AND created_at >= ?2",
                vec![kind.into(), today.into()],
            ))
            .await
            .ok()
            .flatten()
            .is_none_or(|n| n.n > 0)
    };
    if !ran("retention").await {
        let _ = super::create(p, "retention", None, &json!({ "global": true })).await;
    }
    if p.backup().is_some() && !ran("backup").await {
        let _ = super::create(p, "backup", None, &json!({})).await;
    }
    let minute_of_day = (now - today) / 60_000;
    if (240..255).contains(&minute_of_day) {
        super::domain_remove::restart_failed(p).await;
    }
}
