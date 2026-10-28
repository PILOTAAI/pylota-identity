//! Queues: producers ([`QueueProducer`]) and the messages a consumer receives ([`Incoming`]).
//! Bodies are JSON: `cf` sends them as plain JS objects parsed from JSON text, because
//! `serde_wasm_bindgen` turns a JSON object into a JS `Map`, which the queue's JSON encoding drops
//! (spike S1).

use std::rc::Rc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{PResult, PlatformError};

/// The work queues (Configuration › Bindings). `Delivery` is produced to only to redrive dead
/// letters; `Phone` exists when `PM_PHONE` is `on`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueueName {
    Inbound,
    Outbound,
    Delivery,
    Webhooks,
    Index,
    Screen,
    Phone,
}

impl QueueName {
    pub const ALL: [QueueName; 7] = [
        QueueName::Inbound,
        QueueName::Outbound,
        QueueName::Delivery,
        QueueName::Webhooks,
        QueueName::Index,
        QueueName::Screen,
        QueueName::Phone,
    ];

    /// The producer binding.
    pub const fn binding(self) -> &'static str {
        match self {
            QueueName::Inbound => "Q_INBOUND",
            QueueName::Outbound => "Q_OUTBOUND",
            QueueName::Delivery => "Q_DELIVERY",
            QueueName::Webhooks => "Q_WEBHOOKS",
            QueueName::Index => "Q_INDEX",
            QueueName::Screen => "Q_SCREEN",
            QueueName::Phone => "Q_PHONE",
        }
    }

    /// The queue's name, as a consumer's batch reports it.
    pub const fn queue_name(self) -> &'static str {
        match self {
            QueueName::Inbound => "pm-inbound",
            QueueName::Outbound => "pm-outbound",
            QueueName::Delivery => "pm-delivery-events",
            QueueName::Webhooks => "pm-webhooks",
            QueueName::Index => "pm-index",
            QueueName::Screen => "pm-screen",
            QueueName::Phone => "pm-phone",
        }
    }

    /// The queue a batch came from, and whether it is that queue's dead-letter queue (`…-dlq`).
    pub fn from_queue_name(name: &str) -> Option<(QueueName, bool)> {
        let (base, dead) = match name.strip_suffix("-dlq") {
            Some(base) => (base, true),
            None => (name, false),
        };
        Self::ALL
            .into_iter()
            .find(|q| q.queue_name() == base)
            .map(|q| (q, dead))
    }
}

pub const MAX_DELAY_S: u32 = 86_400;
pub const MAX_BATCH_MESSAGES: usize = 100;
pub const MAX_BATCH_BYTES: usize = 256 * 1024;

pub trait QueueProducer {
    /// `delay_s` at most [`MAX_DELAY_S`].
    async fn send<T: Serialize>(&self, q: QueueName, body: &T, delay_s: u32) -> PResult<()>;
    /// At most [`MAX_BATCH_MESSAGES`] bodies and [`MAX_BATCH_BYTES`] of JSON.
    async fn send_batch<T: Serialize>(
        &self,
        q: QueueName,
        bodies: &[T],
        delay_s: u32,
    ) -> PResult<()>;
}

/// The JSON texts of a send, after the limits every implementation applies.
pub fn encode<T: Serialize>(q: QueueName, bodies: &[T], delay_s: u32) -> PResult<Vec<String>> {
    let binding = q.binding();
    if delay_s > MAX_DELAY_S {
        return Err(PlatformError::invalid(
            binding,
            format!("delay {delay_s} s; at most {MAX_DELAY_S}"),
        ));
    }
    if bodies.len() > MAX_BATCH_MESSAGES {
        return Err(PlatformError::invalid(
            binding,
            format!("{} messages; at most {MAX_BATCH_MESSAGES}", bodies.len()),
        ));
    }
    let texts = bodies
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| PlatformError::invalid(binding, format!("body: {e}")))?;
    let size: usize = texts.iter().map(String::len).sum();
    if size > MAX_BATCH_BYTES {
        return Err(PlatformError::invalid(
            binding,
            format!("{size} bytes; at most {MAX_BATCH_BYTES} per batch"),
        ));
    }
    Ok(texts)
}

/// How a received message is settled; `cf` wraps the runtime's message, the fake records the calls.
pub trait Settle {
    fn ack(&self);
    fn retry(&self, delay_s: Option<u32>);
}

/// One received message.
pub struct Incoming<T> {
    pub id: String,
    /// When the message was first sent; unchanged across retries (S1).
    pub timestamp_ms: i64,
    /// Delivery attempts so far, starting at 1.
    pub attempts: u32,
    pub body: T,
    handle: Rc<dyn Settle>,
}

impl<T> Incoming<T> {
    pub fn new(
        id: String,
        timestamp_ms: i64,
        attempts: u32,
        body: T,
        handle: Rc<dyn Settle>,
    ) -> Self {
        Self {
            id,
            timestamp_ms,
            attempts,
            body,
            handle,
        }
    }

    pub fn ack(&self) {
        self.handle.ack();
    }

    /// Redelivers the message, after `delay_s` when given.
    pub fn retry(&self, delay_s: Option<u32>) {
        self.handle.retry(delay_s);
    }
}

impl Incoming<serde_json::Value> {
    /// The body as `U`, keeping the handle; on failure the message comes back unchanged.
    pub fn decode<U: DeserializeOwned>(self) -> Result<Incoming<U>, (Self, serde_json::Error)> {
        match U::deserialize(&self.body) {
            Ok(body) => Ok(Incoming {
                id: self.id,
                timestamp_ms: self.timestamp_ms,
                attempts: self.attempts,
                body,
                handle: self.handle,
            }),
            Err(e) => Err((self, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_limits() {
        assert_eq!(
            QueueName::from_queue_name("pm-delivery-events-dlq"),
            Some((QueueName::Delivery, true))
        );
        assert_eq!(
            QueueName::from_queue_name("pm-index"),
            Some((QueueName::Index, false))
        );
        assert_eq!(QueueName::from_queue_name("pm-other"), None);
        assert!(encode(QueueName::Index, &[1], MAX_DELAY_S + 1).is_err());
        assert!(encode(QueueName::Index, &[0u8; 101], 0).is_err());
        assert!(encode(QueueName::Index, &["x".repeat(MAX_BATCH_BYTES)], 0).is_err());
        assert_eq!(
            encode(QueueName::Index, &[serde_json::json!({"a": 1})], 0).unwrap(),
            ["{\"a\":1}"]
        );
    }
}
