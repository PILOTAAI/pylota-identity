//! The `restore_reconcile` job (Observability › Restore from PITR, step 5): R2 is not rewound, so after
//! a mailbox restore it brings back what the mailbox lost from what R2 still holds. Per identity it
//! re-queues the pointer of every inbound `raw.eml` received after `after` whose message the mailbox
//! no longer has, then re-inserts every sent copy (`out/{msg}.eml`) made after `after` as `uncertain`
//! with its idempotency row. Erased messages stay erased; the inbound consumer deduplicates on
//! `raw_sha256`.

use pylota_identity_api_types::ids::time_of;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::r2::BlobBody;
use pylota_identity_platform::{BlobStore as _, ControlDb as _, Platform};
use serde::Deserialize;
use serde_json::json;

use super::{Ctx, Slice, StepErr, StepResult};
use crate::consumers::inbound::{InboundJob, InboundPointer, RawSource};
use crate::mailbox::MailboxRequest;
use crate::mailbox::submit::ReinsertSent;

pub const STEPS: &[&str] = &["plan", "inbound", "outbound"];

/// R2 keys read per slice.
const PAGE: u32 = 100;

#[derive(Deserialize, Clone)]
struct M {
    idx: i64,
    identity_id: String,
    mailbox_do_id: String,
    primary_address: String,
}

pub async fn run<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    step: &str,
    cursor: Option<&str>,
) -> StepResult {
    c.sql.exec(
        "CREATE TABLE IF NOT EXISTS rids (idx INTEGER PRIMARY KEY, identity_id TEXT NOT NULL UNIQUE, mailbox_do_id TEXT NOT NULL, primary_address TEXT NOT NULL)",
        &[],
    )?;
    match step {
        "plan" => {
            #[derive(Deserialize)]
            struct I {
                id: String,
                mailbox_do_id: String,
                primary_address: Option<String>,
            }
            let only = serde_json::to_string(&c.params["identity_ids"]).unwrap_or_default();
            let rows: Vec<I> = c
                .p
                .db()
                .query(Stmt::new(
                    "SELECT i.id, i.mailbox_do_id, (SELECT a.address FROM addresses a WHERE a.identity_id = i.id AND a.role = 'primary') AS primary_address FROM identities i WHERE i.tenant_id = ?1 AND i.status <> 'deleted' AND i.mailbox_do_id <> '' AND i.id IN (SELECT value FROM json_each(?2)) ORDER BY i.id",
                    vec![c.tenant().into(), only.into()],
                ))
                .await?;
            for r in rows {
                c.sql.exec(
                    "INSERT OR IGNORE INTO rids (identity_id, mailbox_do_id, primary_address) VALUES (?1, ?2, ?3)",
                    &[
                        r.id.into(),
                        r.mailbox_do_id.into(),
                        r.primary_address.unwrap_or_default().into(),
                    ],
                )?;
            }
            Ok(Slice::done())
        }
        "inbound" | "outbound" => page(c, step == "inbound", cursor).await,
        _ => Err(StepErr::new("unknown_step")),
    }
}

/// One page of one identity's keys; the cursor is `{idx}:{r2 cursor}`.
async fn page<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    inbound: bool,
    cursor: Option<&str>,
) -> StepResult {
    let after = c.params["after"].as_i64().unwrap_or(i64::MAX);
    let all: Vec<M> = c.sql.query(
        "SELECT idx, identity_id, mailbox_do_id, primary_address FROM rids ORDER BY idx",
        &[],
    )?;
    let (idx, r2_cursor) = cursor
        .and_then(|c| c.split_once(':'))
        .map_or((0, ""), |(i, r)| (i.parse().unwrap_or(0), r));
    let Some(m) = all.iter().find(|x| x.idx >= idx).cloned() else {
        return Ok(Slice::done());
    };
    let r2_cursor = (m.idx == idx && !r2_cursor.is_empty()).then_some(r2_cursor);
    let base = format!("t/{}/i/{}/", c.tenant(), m.identity_id);
    let prefix = format!("{base}{}", if inbound { "m/" } else { "out/" });
    let blobs = c.p.blobs();
    let listed = blobs.list(&prefix, r2_cursor, PAGE).await?;
    let (mut found, mut restored) = (0i64, 0i64);
    for (key, _) in &listed.keys {
        let rest = &key[prefix.len()..];
        let msg = if inbound {
            rest.strip_suffix("/raw.eml").filter(|m| !m.contains('/'))
        } else {
            rest.strip_suffix(".eml").filter(|m| !m.contains('/'))
        };
        let Some(msg) = msg else { continue };
        if !time_of(msg).is_some_and(|t| t as i64 > after) {
            continue;
        }
        found += 1;
        if inbound {
            let stored = c
                .mailbox(
                    &m.identity_id,
                    &m.mailbox_do_id,
                    MailboxRequest::ConfirmStored {
                        message_id: msg.to_owned(),
                    },
                )
                .await??;
            if stored["stored"] == json!(true) || stored["erased"] == json!(true) {
                continue;
            }
            let Some(blob) = blobs.get(key).await? else {
                continue;
            };
            let meta = |k: &str| {
                blob.custom
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
            };
            let job = InboundJob::Message(InboundPointer {
                v: 1,
                source: RawSource::Routing,
                message_id: Some(msg.to_owned()),
                tenant_id: Some(c.tenant().to_owned()),
                identity_id: Some(m.identity_id.clone()),
                r2_key: Some(key.clone()),
                raw_size: blob.size,
                envelope_from: meta("envelope_from").unwrap_or_default(),
                envelope_to: meta("envelope_to").unwrap_or_else(|| m.primary_address.clone()),
                received_at: time_of(msg).unwrap_or_default() as i64,
                request_id: format!("req_{}", &c.job_id[4..]),
                loopback: None,
            });
            if !crate::email::send_with_retries(c.p, &job).await {
                return Err(StepErr::new("queue_unavailable"));
            }
            restored += 1;
        } else {
            let Some(blob) = blobs.get(key).await? else {
                continue;
            };
            let meta = |k: &str| {
                blob.custom
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
            };
            let bytes = match blob.body {
                BlobBody::Bytes(b) => b,
                b @ BlobBody::Js(_) => b.to_vec(),
            };
            let r = ReinsertSent {
                message_id: msg.to_owned(),
                eml_key: key.clone(),
                mime_b64: pylota_identity_core::links::encode_base64(&bytes),
                idem_key_sha256: meta("idem_key_sha256"),
                fingerprint: meta("fingerprint"),
                operation: meta("operation"),
                sent_at: time_of(msg).unwrap_or_default() as i64,
            };
            let out = c
                .mailbox(
                    &m.identity_id,
                    &m.mailbox_do_id,
                    MailboxRequest::ReinsertSent(Box::new(r)),
                )
                .await??;
            if out["inserted"] == json!(true) {
                restored += 1;
            }
        }
    }
    let counts = if inbound {
        json!({ "inbound_found": found, "requeued": restored })
    } else {
        json!({ "outbound_found": found, "reinserted": restored })
    };
    Ok(match listed.cursor {
        Some(next) => Slice::More {
            cursor: format!("{}:{next}", m.idx),
            counts,
        },
        None if all.iter().any(|x| x.idx > m.idx) => Slice::More {
            cursor: format!("{}:", m.idx + 1),
            counts,
        },
        None => Slice::Done { counts },
    })
}
