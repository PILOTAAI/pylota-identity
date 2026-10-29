//! The M10 arms of `pm-index` ([Search § 6.3–6.6]): `Embed` (chunk, embed in batches of 16, upsert in
//! batches of 100, mark, delete what is no longer produced), `DeleteVectors` and `Reconcile`. Workers AI
//! fair use ([F16]): `RL_EMBED` is asked before every embedding request; a refusal, or Workers AI
//! answering that it is rate limited, defers the job (`deferrals + 1`, 60–90 s, `attempt` unchanged);
//! after 120 deferrals the job acks and leaves its rows `pending` for the nightly reconciliation.
//!
//! [Search § 6.3–6.6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
//! [F16]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/edge-cases.md

use std::collections::HashMap;

use pylota_identity_api_types::internal::{EmbedReason, IndexJob};
use pylota_identity_core::search::chunk::{self, MAX_ATTACHMENT_CHUNKS};
use pylota_identity_platform::ai::{AiOptions, is_rate_limited};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::{DoClass, RpcResult};
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::vectorize::VectorRecord;
use pylota_identity_platform::{
    Ai as _, BlobStore as _, Clock as _, ControlDb as _, ObjectClient as _, Platform,
    QueueProducer as _, RateLimiter as _, Rng as _, VectorIndex as _,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::inbound::Done;
use crate::db::{identities as idb, tenants as tdb};
use crate::mailbox::MailboxRequest;
use crate::mailbox::search::{ChunkRow, IndexSource, PutChunksOut, ReconcileOut, SearchOp};
use crate::rpc::envelope_for;
use crate::search::semantic::{tag_of, vectors_of};

/// Texts per embedding request and vectors per upsert.
pub const EMBED_BATCH: usize = 16;
pub const UPSERT_BATCH: usize = 100;
/// The last attempt of an `Embed` job (the tenth try).
pub const LAST_ATTEMPT: u32 = 9;
/// After this many deferrals an `Embed` job gives up and leaves its rows `pending`.
pub const MAX_DEFERRALS: u32 = 120;
/// IDs per `getByIds` call of the reconciliation sample.
const GET_BATCH: usize = 20;

/// `min(30 · 2^attempt, 3600)` seconds; one second in the itest build.
pub fn retry_delay(attempt: u32) -> u32 {
    if cfg!(feature = "itest-hooks") {
        return 1;
    }
    30u32.saturating_mul(1 << attempt.min(7)).min(3_600)
}

/// `60 + jitter(0..30)` seconds; one second in the itest build.
fn defer_delay<P: Platform>(p: &P) -> u32 {
    if cfg!(feature = "itest-hooks") {
        return 1;
    }
    60 + (p.rng().u64() % 31) as u32
}

/// A mailbox of one identity, answering `SearchOp`s.
struct Mailbox<'a, P: Platform> {
    p: &'a P,
    tenant_id: &'a str,
    identity_id: &'a str,
    object_id: String,
    request_id: String,
}

/// How a mailbox call failed: the object answered an error (an erased identity), or the call itself
/// failed (transient).
#[derive(Debug, PartialEq, Eq)]
enum Fail {
    Answer,
    Call,
}

impl<P: Platform> Mailbox<'_, P> {
    async fn call<T: DeserializeOwned>(&self, op: SearchOp) -> Result<T, ()> {
        self.try_call(op).await.map_err(|_| ())
    }

    async fn try_call<T: DeserializeOwned>(&self, op: SearchOp) -> Result<T, Fail> {
        let env = envelope_for(
            self.p,
            Some(self.tenant_id),
            Some(self.identity_id),
            &self.request_id,
            None,
            MailboxRequest::Search(op),
        );
        match self
            .p
            .objects()
            .call::<_, Value>(DoClass::Mailbox, &self.object_id, &env)
            .await
        {
            Ok(RpcResult::Ok(v)) => serde_json::from_value(v).map_err(|_| Fail::Answer),
            Ok(RpcResult::Err(_)) => Err(Fail::Answer),
            Err(_) => Err(Fail::Call),
        }
    }
}

/// The identity's mailbox, or how to settle when there is none.
async fn mailbox<'a, P: Platform>(
    p: &'a P,
    tenant_id: &'a str,
    identity_id: &'a str,
) -> Result<Mailbox<'a, P>, Done> {
    match (
        p.db().first::<idb::Row>(idb::get(identity_id)).await,
        p.db().first::<tdb::Row>(tdb::get(tenant_id)).await,
    ) {
        // The system identity is never embedded (A15).
        (Ok(Some(i)), Ok(Some(t)))
            if i.tenant_id == t.id && !i.mailbox_do_id.is_empty() && i.is_system == 0 =>
        {
            Ok(Mailbox {
                p,
                tenant_id,
                identity_id,
                object_id: i.mailbox_do_id,
                request_id: crate::request_id::new(p),
            })
        }
        (Ok(_), Ok(_)) => Err(Done::Ack),
        _ => Err(Done::Retry),
    }
}

/// Re-enqueues `job` after a deferral or a failure; a failed send leaves it to the queue's retry.
async fn requeue<P: Platform>(p: &P, job: &IndexJob, delay: u32) -> Done {
    match p.queues().send(QueueName::Index, job, delay).await {
        Ok(()) => Done::Ack,
        Err(_) => Done::Retry,
    }
}

/// Whether the tenant's embedding share allows one more request.
async fn embed_allowed<P: Platform>(p: &P, tenant_id: &str) -> bool {
    p.limits()
        .allow(RlBucket::Embed, tenant_id)
        .await
        .unwrap_or(false)
}

/// F16: the job again with `deferrals + 1`, or an ack that leaves the rows `pending`.
async fn defer<P: Platform>(p: &P, job: &IndexJob) -> Done {
    let IndexJob::Embed {
        tenant_id,
        deferrals,
        ..
    } = job
    else {
        return Done::Ack;
    };
    crate::metrics::count(p, "ai_deferred_total", Some(tenant_id), 1);
    if *deferrals >= MAX_DEFERRALS {
        crate::log::warn(
            "embed_deferred_out",
            &[("tenant_id", tenant_id.as_str().into())],
        );
        return Done::Ack;
    }
    let mut next = job.clone();
    if let IndexJob::Embed { deferrals, .. } = &mut next {
        *deferrals += 1;
    }
    requeue(p, &next, defer_delay(p)).await
}

/// One chunk to embed: its row and its text.
struct Piece {
    row: ChunkRow,
    text: String,
    attachment: bool,
}

/// The chunks of a message ([Search § 6.2]): the body, then each attachment with ready text.
async fn pieces<P: Platform>(p: &P, message_id: &str, src: &IndexSource) -> Result<Vec<Piece>, ()> {
    let mut out: Vec<Piece> = chunk::body(&src.subject, &src.text)
        .into_iter()
        .enumerate()
        .map(|(n, piece)| Piece {
            row: ChunkRow {
                vector_id: format!("{message_id}:{n}"),
                attachment_id: None,
                ordinal: n as i64,
                page: None,
                char_start: piece.char_start as i64,
                char_end: piece.char_end as i64,
            },
            text: piece.text,
            attachment: false,
        })
        .collect();
    let mut budget = MAX_ATTACHMENT_CHUNKS;
    for (att_id, k, key) in &src.attachments {
        if budget == 0 {
            break;
        }
        let md = match p.blobs().get(key).await {
            Ok(Some(b)) => String::from_utf8_lossy(&b.body.to_vec()).into_owned(),
            Ok(None) => continue,
            Err(_) => return Err(()),
        };
        let pages = chunk::pages(&md, budget);
        budget = budget.saturating_sub(pages.len());
        for (n, (page, piece)) in pages.into_iter().enumerate() {
            out.push(Piece {
                row: ChunkRow {
                    vector_id: format!("{message_id}:a{k}:{n}"),
                    attachment_id: Some(att_id.clone()),
                    ordinal: n as i64,
                    page: Some(i64::from(page)),
                    char_start: piece.char_start as i64,
                    char_end: piece.char_end as i64,
                },
                text: piece.text,
                attachment: true,
            });
        }
    }
    Ok(out)
}

/// The eight metadata fields of a vector ([Data model § 5]); nothing else, and never text.
pub fn metadata(identity_id: &str, src: &IndexSource, attachment: bool) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("identity_id".into(), identity_id.into());
    m.insert("thread_id".into(), src.thread_id.clone().into());
    m.insert("sent_at".into(), src.sent_at.into());
    m.insert(
        "sender_domain".into(),
        src.sender_domain.clone().unwrap_or_default().into(),
    );
    m.insert("direction".into(), src.direction.clone().into());
    m.insert("has_attachment".into(), src.has_attachment.into());
    m.insert(
        "verdict".into(),
        src.verdict.clone().unwrap_or_else(|| "none".into()).into(),
    );
    m.insert(
        "kind".into(),
        if attachment { "attachment" } else { "body" }.into(),
    );
    m
}

/// How the embedding of a batch ended.
enum Embedded {
    Vectors(Vec<Vec<f32>>),
    /// Over the tenant's share, or Workers AI rate limited: defer.
    Defer,
    Failed,
}

async fn embed_batch<P: Platform>(p: &P, tenant_id: &str, model: &str, texts: &[&str]) -> Embedded {
    if !embed_allowed(p, tenant_id).await {
        return Embedded::Defer;
    }
    let input = json!({ "text": texts });
    let opts = AiOptions::content(p.config().ai_gateway.clone());
    match p.ai().run::<Value, Value>(model, &input, &opts).await {
        Ok(out) => match vectors_of(&out, texts.len()) {
            Some(v) => Embedded::Vectors(v),
            None => Embedded::Failed,
        },
        Err(e) if is_rate_limited(&e) => {
            crate::metrics::count(p, "ai_rate_limited_total", Some(tenant_id), 1);
            Embedded::Defer
        }
        Err(_) => Embedded::Failed,
    }
}

pub async fn embed<P: Platform>(p: &P, job: IndexJob) -> Done {
    let IndexJob::Embed {
        tenant_id,
        identity_id,
        message_id,
        attempt,
        ..
    } = &job
    else {
        return Done::Ack;
    };
    let mb = match mailbox(p, tenant_id, identity_id).await {
        Ok(m) => m,
        Err(d) => return d,
    };
    let Ok(src) = mb
        .call::<IndexSource>(SearchOp::IndexSource {
            message_id: message_id.clone(),
        })
        .await
    else {
        return Done::Retry;
    };
    if src.skip {
        return Done::Ack;
    }
    let Ok(pieces) = pieces(p, message_id, &src).await else {
        return Done::Retry;
    };
    let model = p.config().embed_model.clone();
    let tag = tag_of(&model);
    let Ok(put) = mb
        .call::<PutChunksOut>(SearchOp::PutChunks {
            message_id: message_id.clone(),
            chunks: pieces.iter().map(|x| x.row.clone()).collect(),
            model: tag.clone(),
        })
        .await
    else {
        return Done::Retry;
    };
    if put.erased {
        return Done::Ack;
    }
    let todo: Vec<&Piece> = pieces
        .iter()
        .filter(|x| put.embed.contains(&x.row.vector_id))
        .collect();
    let mut embedded: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut deferred = false;
    let mut records: Vec<VectorRecord> = Vec::new();
    for batch in todo.chunks(EMBED_BATCH) {
        let texts: Vec<&str> = batch.iter().map(|x| x.text.as_str()).collect();
        match embed_batch(p, tenant_id, &model, &texts).await {
            Embedded::Vectors(vs) => {
                for (x, v) in batch.iter().zip(vs) {
                    records.push(VectorRecord {
                        id: x.row.vector_id.clone(),
                        namespace: tenant_id.clone(),
                        values: v,
                        metadata: metadata(identity_id, &src, x.attachment),
                    });
                }
            }
            Embedded::Defer => {
                deferred = true;
                break;
            }
            Embedded::Failed => failed.extend(batch.iter().map(|x| x.row.vector_id.clone())),
        }
    }
    for batch in records.chunks(UPSERT_BATCH) {
        let ids = batch.iter().map(|r| r.id.clone());
        match p.vectors().upsert(batch).await {
            Ok(_) => embedded.extend(ids),
            Err(_) => failed.extend(ids),
        }
    }
    if !embedded.is_empty() {
        let still = mb
            .call::<bool>(SearchOp::Mark {
                message_id: message_id.clone(),
                ids: embedded.clone(),
                status: "embedded".into(),
                model: tag.clone(),
            })
            .await;
        match still {
            // Erased while the job ran (I10): delete what was written.
            Ok(false) => {
                let _ = p.vectors().delete_by_ids(&embedded).await;
                return Done::Ack;
            }
            Ok(true) => {}
            Err(()) => return Done::Retry,
        }
    }
    if !failed.is_empty() {
        let _ = mb
            .call::<bool>(SearchOp::Mark {
                message_id: message_id.clone(),
                ids: failed.clone(),
                status: "failed".into(),
                model: tag.clone(),
            })
            .await;
    }
    if !put.delete.is_empty() && p.vectors().delete_by_ids(&put.delete).await.is_ok() {
        let _ = mb
            .call::<Value>(SearchOp::DropDeleting { ids: put.delete })
            .await;
    }
    crate::metrics::count_with(
        p,
        "index_jobs_total",
        Some(tenant_id),
        &["embed", "upserted"],
        embedded.len() as u32,
    );
    if deferred {
        return defer(p, &job).await;
    }
    if !failed.is_empty() && *attempt < LAST_ATTEMPT {
        let mut next = job.clone();
        if let IndexJob::Embed { attempt, .. } = &mut next {
            *attempt += 1;
        }
        return requeue(p, &next, retry_delay(*attempt)).await;
    }
    Done::Ack
}

pub async fn delete_vectors<P: Platform>(p: &P, vector_ids: &[String], also_next: bool) -> Done {
    for batch in vector_ids.chunks(500) {
        if p.vectors().delete_by_ids(batch).await.is_err() {
            return Done::Retry;
        }
        if also_next
            && let Some(next) = p.vectors_next()
            && next.delete_by_ids(batch).await.is_err()
        {
            return Done::Retry;
        }
    }
    Done::Ack
}

/// One identity's nightly reconciliation ([Search § 6.6] steps 2–4).
pub async fn reconcile<P: Platform>(
    p: &P,
    tenant_id: &str,
    identity_id: &str,
    run_date: &str,
) -> Done {
    let mb = match mailbox(p, tenant_id, identity_id).await {
        Ok(m) => m,
        Err(d) => return d,
    };
    // A mailbox that answers an error (erased meanwhile) reports nothing to reconcile, so the night's
    // evaluation does not wait for it.
    let r = match mb.try_call::<ReconcileOut>(SearchOp::Reconcile).await {
        Ok(r) => r,
        Err(Fail::Answer) => ReconcileOut::default(),
        Err(Fail::Call) => return Done::Retry,
    };
    let mut messages: Vec<String> = r.retry.iter().chain(&r.missing).cloned().collect();
    // The sample: embedded IDs Vectorize no longer has go back to `pending`.
    let mut by_msg: HashMap<String, String> = HashMap::new();
    let ids: Vec<String> = r
        .sample
        .iter()
        .map(|(v, m)| {
            by_msg.insert(v.clone(), m.clone());
            v.clone()
        })
        .collect();
    let mut missing_ids = Vec::new();
    for batch in ids.chunks(GET_BATCH) {
        match p.vectors().get_by_ids(batch).await {
            Ok(found) => missing_ids.extend(batch.iter().filter(|id| !found.contains(id)).cloned()),
            Err(_) => return Done::Retry,
        }
    }
    if !missing_ids.is_empty() {
        if mb
            .call::<Value>(SearchOp::MarkPending {
                ids: missing_ids.clone(),
            })
            .await
            .is_err()
        {
            return Done::Retry;
        }
        messages.extend(missing_ids.iter().filter_map(|v| by_msg.get(v).cloned()));
    }
    messages.sort();
    messages.dedup();
    let jobs: Vec<IndexJob> = messages
        .iter()
        .map(|m| IndexJob::embed(tenant_id, identity_id, m, EmbedReason::Reconcile))
        .collect();
    for batch in jobs.chunks(100) {
        if p.queues()
            .send_batch(QueueName::Index, batch, 0)
            .await
            .is_err()
        {
            return Done::Retry;
        }
    }
    crate::log::info(
        "index_reconcile",
        &[
            ("identity_id", identity_id.into()),
            ("embedded_rows", r.embedded_rows.into()),
            ("pending_rows", r.pending_rows.into()),
            ("failed_rows", r.failed_rows.into()),
            ("requeued", (jobs.len() as i64).into()),
        ],
    );
    // A retried job overwrites its own row.
    let row = Stmt::new(
        "INSERT OR REPLACE INTO index_reconcile (run_date, identity_id, embedded_rows, pending_rows, failed_rows, reported_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        vec![
            run_date.into(),
            identity_id.into(),
            r.embedded_rows.into(),
            r.pending_rows.into(),
            r.failed_rows.into(),
            p.clock().now_ms().into(),
        ],
    );
    match p.db().execute(row).await {
        Ok(_) => Done::Ack,
        Err(_) => Done::Retry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_and_metadata() {
        if !cfg!(feature = "itest-hooks") {
            assert_eq!(retry_delay(0), 30);
            assert_eq!(retry_delay(3), 240);
            assert_eq!(retry_delay(8), 3_600);
        }
        let src = IndexSource {
            thread_id: "thr_1".into(),
            sent_at: 1_757_837_520,
            direction: "outbound".into(),
            ..IndexSource::default()
        };
        let m = metadata("idn_1", &src, false);
        let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "direction",
                "has_attachment",
                "identity_id",
                "kind",
                "sender_domain",
                "sent_at",
                "thread_id",
                "verdict"
            ]
        );
        assert_eq!(m["verdict"], "none");
        assert_eq!(m["kind"], "body");
    }
}
