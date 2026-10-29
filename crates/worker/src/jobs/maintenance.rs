//! The maintenance jobs `reparse`, `reembed` and `reindex` (J3, Search § 7): each pages through the
//! tenant's mailboxes, narrowed by `identity_ids`, `after` and `before`, and queues per-message work
//! for the consumer that owns it. Messages whose raw MIME is gone are skipped and counted.
//!
//! The re-parse itself (the `pm-inbound` consumer's `Reparse` pointer) and the keyword rebuild arrive
//! with J3's acceptance in M17; until then those pointers are acked without work.

use pylota_identity_api_types::internal::{EmbedReason, IndexJob};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{ControlDb as _, Platform, QueueProducer as _};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Ctx, Slice, StepErr, StepResult};
use crate::mailbox::MailboxRequest;
use crate::mailbox::privacy::PrivacyOp;

pub const STEPS: &[&str] = &["plan", "queue"];

#[derive(Deserialize, Clone)]
struct M {
    idx: i64,
    identity_id: String,
    mailbox_do_id: String,
}

pub async fn run<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    step: &str,
    cursor: Option<&str>,
) -> StepResult {
    c.sql.exec(
        "CREATE TABLE IF NOT EXISTS mids (idx INTEGER PRIMARY KEY, identity_id TEXT NOT NULL UNIQUE, mailbox_do_id TEXT NOT NULL)",
        &[],
    )?;
    match step {
        "plan" => {
            #[derive(Deserialize)]
            struct I {
                id: String,
                mailbox_do_id: String,
            }
            let only: Option<String> = c.params["identity_ids"]
                .as_array()
                .map(|a| serde_json::to_string(a).unwrap_or_default());
            let rows: Vec<I> = c
                .p
                .db()
                .query(Stmt::new(
                    "SELECT id, mailbox_do_id FROM identities WHERE tenant_id = ?1 AND status <> 'deleted' AND mailbox_do_id <> '' AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2))) ORDER BY id",
                    vec![c.tenant().into(), only.into()],
                ))
                .await?;
            for r in rows {
                c.sql.exec(
                    "INSERT OR IGNORE INTO mids (identity_id, mailbox_do_id) VALUES (?1, ?2)",
                    &[r.id.into(), r.mailbox_do_id.into()],
                )?;
            }
            Ok(Slice::done())
        }
        "queue" => queue(c, cursor).await,
        _ => Err(StepErr::new("unknown_step")),
    }
}

async fn queue<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, cursor: Option<&str>) -> StepResult {
    let all: Vec<M> = c.sql.query(
        "SELECT idx, identity_id, mailbox_do_id FROM mids ORDER BY idx",
        &[],
    )?;
    let (idx, rest) = cursor
        .and_then(|c| c.split_once(':'))
        .map_or((0, ""), |(i, r)| (i.parse().unwrap_or(0), r));
    let Some(m) = all.iter().find(|x| x.idx >= idx).cloned() else {
        return Ok(Slice::done());
    };
    let after_rowid: i64 = if m.idx == idx {
        rest.parse().unwrap_or(0)
    } else {
        0
    };
    let page = c
        .mailbox(
            &m.identity_id,
            &m.mailbox_do_id,
            MailboxRequest::Privacy(PrivacyOp::MessagesInRange {
                after_rowid,
                limit: 200,
                after: c.params["after"].as_i64(),
                before: c.params["before"].as_i64(),
            }),
        )
        .await?
        .map_err(StepErr::from)?;
    let mut counts = Map::new();
    let mut queued = 0;
    let mut skipped = 0;
    for msg in page["messages"].as_array().cloned().unwrap_or_default() {
        let id = msg["id"].as_str().unwrap_or("").to_owned();
        let job: Option<Value> = match c.kind.as_str() {
            "reembed" => serde_json::to_value(IndexJob::Embed {
                tenant_id: c.tenant().to_owned(),
                identity_id: m.identity_id.clone(),
                message_id: id,
                reason: EmbedReason::Reembed,
                attempt: 0,
                deferrals: 0,
            })
            .ok(),
            "reindex" => serde_json::to_value(IndexJob::Embed {
                tenant_id: c.tenant().to_owned(),
                identity_id: m.identity_id.clone(),
                message_id: id,
                reason: EmbedReason::Rechunk,
                attempt: 0,
                deferrals: 0,
            })
            .ok(),
            _ if msg["direction"] != "inbound" || msg["raw_r2_key"].is_null() => {
                skipped += 1;
                None
            }
            _ => Some(json!({
                "kind": "reparse", "v": 1, "job_id": c.job_id, "tenant_id": c.tenant(),
                "identity_id": m.identity_id, "message_id": id,
                "parser_version": crate::mailbox::ingest::PARSER_VERSION,
            })),
        };
        if let Some(j) = job {
            let q = if c.kind == "reparse" {
                QueueName::Inbound
            } else {
                QueueName::Index
            };
            c.p.queues().send(q, &j, 0).await?;
            queued += 1;
        }
    }
    pylota_identity_core::jobs::add_counts(
        &mut counts,
        &json!({ "queued": queued, "skipped_raw_expired": skipped }),
    );
    let next = page["next_after_rowid"].as_i64();
    let cursor = match next {
        Some(n) => format!("{}:{n}", m.idx),
        None => format!("{}:", m.idx + 1),
    };
    Ok(if next.is_some() || all.iter().any(|x| x.idx > m.idx) {
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
