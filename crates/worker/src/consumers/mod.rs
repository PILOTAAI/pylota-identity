//! Queue consumers ([Design conventions § 7]): each handles its messages one at a time and acks or
//! retries each itself.
//!
//! [Design conventions § 7]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/index.md

pub mod delivery;
pub mod dlq;
pub mod embed;
pub mod inbound;
pub mod index;
pub mod outbound;
pub mod webhooks;

use pylota_identity_platform::Platform;
use pylota_identity_platform::queues::{Incoming, QueueName};
use serde_json::Value;

/// One batch of a queue (its dead-letter queue when `dead_letter`).
pub async fn run<P: Platform>(
    p: &P,
    queue: QueueName,
    dead_letter: bool,
    batch: Vec<Incoming<Value>>,
) {
    match (queue, dead_letter) {
        (QueueName::Inbound, false) => inbound::batch(p, batch).await,
        (QueueName::Index, false) => index::batch(p, batch).await,
        (QueueName::Webhooks, false) => webhooks::batch(p, batch).await,
        (QueueName::Outbound, false) => outbound::batch(p, batch).await,
        (QueueName::Delivery, false) => delivery::batch(p, batch).await,
        (q, true) => dlq::batch(p, q, batch).await,
        // The other queues are consumed from their milestones; until then a message waits.
        _ => {
            for m in &batch {
                m.retry(None);
            }
        }
    }
}
