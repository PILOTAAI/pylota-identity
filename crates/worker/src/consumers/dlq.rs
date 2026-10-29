//! The dead-letter consumers (Observability § 8.1): every message of a `*-dlq` queue is recorded in
//! `dlq_items` once (`INSERT OR IGNORE` on `(queue, message_id)`), logged without its body and acked.
//! A failed insert leaves the message for the dead-letter queue's own retry. The alert evaluator turns
//! open items into the `dlq_items` observation and the `dlq:{queue}` alert; the platform API lists and
//! redrives them.

use pylota_identity_core::ses::sha256_hex;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::queues::{Incoming, QueueName};
use pylota_identity_platform::{Clock as _, ControlDb as _, Ids as _, Platform};
use serde_json::Value;

pub async fn batch<P: Platform>(p: &P, queue: QueueName, batch: Vec<Incoming<Value>>) {
    let now = p.clock().now_ms();
    for m in batch {
        let body = m.body.to_string();
        let tenant = m.body["tenant_id"].as_str().map(str::to_owned);
        let kind = m.body["kind"].as_str().map(str::to_owned);
        let r = p
            .db()
            .execute(Stmt::new(
                "INSERT OR IGNORE INTO dlq_items (id, queue, message_id, body_json, body_sha256, tenant_id, kind, first_seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                vec![
                    p.ids().new_id(IdPrefix::DeadLetter).into(),
                    queue.queue_name().into(),
                    m.id.clone().into(),
                    body.clone().into(),
                    sha256_hex(body.as_bytes()).into(),
                    tenant.clone().into(),
                    kind.clone().into(),
                    now.into(),
                ],
            ))
            .await;
        match r {
            Ok(_) => {
                crate::log::error(
                    "dlq_item",
                    &[
                        ("queue", queue.queue_name().into()),
                        ("message_id", m.id.clone().into()),
                        ("tenant_id", tenant.unwrap_or_default().into()),
                        ("detail", kind.unwrap_or_default().into()),
                    ],
                );
                m.ack();
            }
            Err(_) => m.retry(None),
        }
    }
}
