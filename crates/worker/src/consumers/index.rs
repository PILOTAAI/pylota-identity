//! The `pm-index` consumer ([Search § 6]). M7 builds the `AttachmentText` arm
//! ([Inbound › Attachment text extraction]); M10 the `Embed`, `DeleteVectors` and `Reconcile` arms
//! (`consumers::embed`). `Triage` acks without work until M12 and counts
//! `index_job_unimplemented_total`.
//!
//! Retries are counted in the job body: a known transient failure re-enqueues the job with
//! `attempt + 1` and a delay, then acks this delivery.
//!
//! [Search § 6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
//! [Inbound › Attachment text extraction]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_api_types::internal::{EmbedReason, IndexJob};
use pylota_identity_core::attach_text::{self, Method};
use pylota_identity_core::refs::{self, Pack};
use pylota_identity_platform::ai::{MarkdownFormat, MarkdownInput};
use pylota_identity_platform::durable::{DoClass, RpcResult};
use pylota_identity_platform::queues::{Incoming, QueueName};
use pylota_identity_platform::r2::{BlobBody, BlobMeta};
use pylota_identity_platform::{
    Ai as _, BlobStore as _, ControlDb as _, ObjectClient as _, Platform, QueueProducer as _,
};
use serde_json::Value;

use super::inbound::Done;
use crate::db::{identities as idb, tenants as tdb};
use crate::mailbox::MailboxRequest;
use crate::mailbox::attachments::{AttachmentText, Pending, TextItem, index_text};
use crate::mailbox::ingest::RefRow;
use crate::rpc::envelope_for;

/// The last attempt of an `AttachmentText` job (the third try).
const LAST_TEXT_ATTEMPT: u32 = 2;

/// The delay before attempt `next` of an `AttachmentText` job: 60 s, then 300 s. The itest build waits
/// one second, so the suite sees three failures without waiting six minutes.
fn text_delay(next: u32) -> u32 {
    if cfg!(feature = "itest-hooks") {
        return 1;
    }
    if next <= 1 { 60 } else { 300 }
}

pub async fn batch<P: Platform>(p: &P, batch: Vec<Incoming<Value>>) {
    #[cfg(feature = "itest-hooks")]
    crate::hooks::tap_queue(p, "pm-index", &batch).await;
    for m in batch {
        let done = match serde_json::from_value::<IndexJob>(m.body.clone()) {
            Ok(job) => run(p, job).await,
            Err(_) => {
                crate::log::warn("index_bad_job", &[]);
                Done::Ack
            }
        };
        match done {
            Done::Ack => m.ack(),
            Done::Retry => m.retry(None),
            Done::Later(s) => m.retry(Some(s)),
        }
    }
}

async fn run<P: Platform>(p: &P, job: IndexJob) -> Done {
    match job {
        IndexJob::AttachmentText {
            tenant_id,
            identity_id,
            message_id,
            attempt,
        } => attachment_text(p, &tenant_id, &identity_id, &message_id, attempt).await,
        job @ IndexJob::Embed { .. } => super::embed::embed(p, job).await,
        IndexJob::DeleteVectors {
            vector_ids,
            also_next,
            ..
        } => super::embed::delete_vectors(p, &vector_ids, also_next).await,
        IndexJob::Reconcile {
            tenant_id,
            identity_id,
            run_date,
        } => super::embed::reconcile(p, &tenant_id, &identity_id, &run_date).await,
        job @ IndexJob::Triage { .. } => crate::triage::run(p, job).await,
    }
}

/// What one attachment's extraction ended in.
enum Outcome {
    Ready {
        md: String,
        pages: u32,
    },
    Unavailable,
    /// A converter failure: retried while attempts remain.
    Failed,
}

async fn extract<P: Platform>(p: &P, a: &Pending, bytes: Vec<u8>) -> Outcome {
    if bytes.len() > attach_text::MAX_INPUT_BYTES {
        return Outcome::Unavailable;
    }
    let sniffed = a.sniffed_type.as_deref().unwrap_or("");
    let pages = match attach_text::method(sniffed, &a.content_type) {
        Method::Text => vec![attach_text::decode_text(&bytes)],
        Method::Html => vec![attach_text::html_text(&bytes)],
        Method::Message => vec![attach_text::message_text(&bytes)],
        Method::None => return Outcome::Unavailable,
        Method::Converter => {
            let name = a
                .filename
                .clone()
                .unwrap_or_else(|| "attachment".to_owned());
            let mime_type = if sniffed.is_empty() {
                a.content_type.clone()
            } else {
                sniffed.to_owned()
            };
            match p
                .ai()
                .to_markdown(&[MarkdownInput {
                    name,
                    mime_type,
                    bytes,
                }])
                .await
            {
                Ok(out) => match out.into_iter().next() {
                    Some(o) if o.format != MarkdownFormat::Error => {
                        attach_text::converter_pages(&o.data)
                    }
                    _ => return Outcome::Failed,
                },
                Err(_) => return Outcome::Failed,
            }
        }
    };
    match attach_text::render(&pages) {
        Some((md, pages)) => Outcome::Ready { md, pages },
        None => Outcome::Unavailable,
    }
}

async fn attachment_text<P: Platform>(
    p: &P,
    tenant_id: &str,
    identity_id: &str,
    message_id: &str,
    attempt: u32,
) -> Done {
    let (idn, t) = match (
        p.db().first::<idb::Row>(idb::get(identity_id)).await,
        p.db().first::<tdb::Row>(tdb::get(tenant_id)).await,
    ) {
        (Ok(Some(i)), Ok(Some(t))) if i.tenant_id == t.id && !i.mailbox_do_id.is_empty() => (i, t),
        (Ok(_), Ok(_)) => return Done::Ack,
        _ => return Done::Retry,
    };
    let request_id = crate::request_id::new(p);
    let call = |req: MailboxRequest| {
        let env = envelope_for(
            p,
            Some(tenant_id),
            Some(identity_id),
            &request_id,
            None,
            req,
        );
        let mailbox = idn.mailbox_do_id.clone();
        async move {
            p.objects()
                .call::<_, Value>(DoClass::Mailbox, &mailbox, &env)
                .await
        }
    };
    let pending: Vec<Pending> = match call(MailboxRequest::PendingAttachmentText {
        message_id: message_id.to_owned(),
    })
    .await
    {
        Ok(RpcResult::Ok(v)) if v["erased"] == false => {
            serde_json::from_value(v["items"].clone()).unwrap_or_default()
        }
        Ok(_) => return Done::Ack,
        Err(_) => return Done::Retry,
    };
    if pending.is_empty() {
        return Done::Ack;
    }

    let policy = crate::policy::effective(p, &t);
    let packs: Vec<Pack> = pylota_identity_core::policy::get_path(&policy, "search.refs_packs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().and_then(Pack::from_name))
                .collect()
        })
        .unwrap_or_else(|| vec![Pack::Core]);

    let mut items = Vec::new();
    let mut written = Vec::new();
    let mut failed = false;
    for a in &pending {
        let bytes = match p.blobs().get(&a.r2_key).await {
            Ok(Some(b)) => b.body.to_vec(),
            // Not put yet (post-commit still running): try again later.
            Ok(None) => {
                failed = true;
                continue;
            }
            Err(_) => return Done::Retry,
        };
        match extract(p, a, bytes).await {
            Outcome::Ready { md, pages } => {
                let key = format!(
                    "t/{tenant_id}/i/{identity_id}/m/{message_id}/a/{}.md",
                    a.attachment_id
                );
                let meta = BlobMeta {
                    content_type: Some("text/markdown; charset=utf-8".into()),
                    custom: vec![
                        ("tenant".into(), tenant_id.to_owned()),
                        ("identity".into(), identity_id.to_owned()),
                        ("message".into(), message_id.to_owned()),
                    ],
                };
                if p.blobs()
                    .put(&key, BlobBody::Bytes(md.clone().into_bytes()), &meta)
                    .await
                    .is_err()
                {
                    return Done::Retry;
                }
                written.push(key.clone());
                let refs: Vec<RefRow> = attach_text::split_rendered(&md)
                    .iter()
                    .enumerate()
                    .flat_map(|(i, page)| {
                        refs::extract_attachment(
                            page,
                            &a.attachment_id,
                            i as u32 + 1,
                            &packs,
                            &[],
                            &t.timezone,
                        )
                    })
                    .map(|r| RefRow {
                        kind: r.kind.as_str().into_owned(),
                        value: r.value,
                        source: r.source.to_string(),
                    })
                    .collect();
                items.push(TextItem {
                    attachment_id: a.attachment_id.clone(),
                    status: "ready".into(),
                    pages: Some(pages),
                    text_r2_key: Some(key),
                    refs,
                    text: index_text(&md),
                });
            }
            Outcome::Unavailable => items.push(unavailable(a)),
            Outcome::Failed if attempt >= LAST_TEXT_ATTEMPT => {
                crate::metrics::count_with(
                    p,
                    "index_jobs_total",
                    Some(tenant_id),
                    &["attachment_text", "unavailable"],
                    1,
                );
                items.push(unavailable(a));
            }
            Outcome::Failed => failed = true,
        }
    }

    // Record what is final now; the failed attachments stay pending for the next attempt.
    if !items.is_empty() {
        let ready = items.iter().any(|i| i.status == "ready");
        let req = MailboxRequest::AttachmentTextReady(AttachmentText {
            message_id: message_id.to_owned(),
            items,
        });
        match call(req).await {
            Ok(RpcResult::Ok(v)) if v["erased"] == true => {
                let _ = p.blobs().delete(&written).await;
                return Done::Ack;
            }
            Ok(RpcResult::Ok(_)) => {}
            _ => return Done::Retry,
        }
        if ready {
            let job = IndexJob::embed(
                tenant_id,
                identity_id,
                message_id,
                EmbedReason::AttachmentText,
            );
            if p.queues().send(QueueName::Index, &job, 0).await.is_err() {
                return Done::Retry;
            }
        }
    }
    if failed {
        let next = IndexJob::AttachmentText {
            tenant_id: tenant_id.to_owned(),
            identity_id: identity_id.to_owned(),
            message_id: message_id.to_owned(),
            attempt: attempt + 1,
        };
        if attempt >= LAST_TEXT_ATTEMPT {
            return Done::Ack;
        }
        if p.queues()
            .send(QueueName::Index, &next, text_delay(attempt + 1))
            .await
            .is_err()
        {
            return Done::Retry;
        }
    }
    Done::Ack
}

fn unavailable(a: &Pending) -> TextItem {
    TextItem {
        attachment_id: a.attachment_id.clone(),
        status: "unavailable".into(),
        pages: None,
        text_r2_key: None,
        refs: Vec::new(),
        text: String::new(),
    }
}
