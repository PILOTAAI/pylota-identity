//! Queue producers through the binding object, with bodies parsed from JSON text into plain JS objects
//! (S1: `serde_wasm_bindgen` would make a `Map`, which the queue's JSON encoding drops), and the
//! consumer's messages.

use serde::Serialize;
use wasm_bindgen::JsValue;

use super::js;
use crate::queues::{QueueName, QueueProducer, Settle, encode};
use crate::{PResult, PlatformError};

#[derive(Clone)]
pub struct CfQueues {
    pub(crate) env: worker::Env,
}

fn message(text: &str, delay_s: u32) -> Result<JsValue, JsValue> {
    let m = js::object();
    js::set(&m, "body", &js_sys::JSON::parse(text)?)?;
    js::set(&m, "contentType", &JsValue::from_str("json"))?;
    if delay_s > 0 {
        js::set(&m, "delaySeconds", &JsValue::from(delay_s))?;
    }
    Ok(m)
}

impl QueueProducer for CfQueues {
    async fn send<T: Serialize>(&self, q: QueueName, body: &T, delay_s: u32) -> PResult<()> {
        let name = q.binding();
        let texts = encode(q, std::slice::from_ref(body), delay_s)?;
        let queue = js::binding(&self.env, name)?;
        let options = js::object();
        let built = (|| {
            js::set(&options, "contentType", &JsValue::from_str("json"))?;
            if delay_s > 0 {
                js::set(&options, "delaySeconds", &JsValue::from(delay_s))?;
            }
            js_sys::JSON::parse(&texts[0])
        })()
        .map_err(|e| js::error(name, &e))?;
        js::call_async(&queue, "send", &[&built, &options])
            .await
            .map_err(|e| js::error(name, &e))?;
        Ok(())
    }

    async fn send_batch<T: Serialize>(
        &self,
        q: QueueName,
        bodies: &[T],
        delay_s: u32,
    ) -> PResult<()> {
        let name = q.binding();
        let texts = encode(q, bodies, delay_s)?;
        if texts.is_empty() {
            return Ok(());
        }
        let queue = js::binding(&self.env, name)?;
        let messages = texts
            .iter()
            .map(|t| message(t, delay_s))
            .collect::<Result<js_sys::Array, JsValue>>()
            .map_err(|e| js::error(name, &e))?;
        js::call_async(&queue, "sendBatch", &[&messages.into()])
            .await
            .map_err(|e| js::error(name, &e))?;
        Ok(())
    }
}

/// One message of a consumer's batch.
pub(crate) struct CfSettle {
    pub(crate) raw: JsValue,
}

impl Settle for CfSettle {
    fn ack(&self) {
        let _ = js::call(&self.raw, "ack", &[]);
    }

    /// `retry({ delaySeconds })`; `Message::timestamp()` is unchanged by it (S1).
    fn retry(&self, delay_s: Option<u32>) {
        let options = js::object();
        if let Some(d) = delay_s {
            let _ = js::set(&options, "delaySeconds", &JsValue::from(d));
        }
        let _ = js::call(&self.raw, "retry", &[&options]);
    }
}

/// The body, ID, first-send time and attempt count of a raw message.
pub(crate) fn read_message(raw: &JsValue) -> PResult<(String, i64, u32, serde_json::Value)> {
    let fail = |e: JsValue| js::error("queue", &e);
    let id = js::get(raw, "id")
        .map_err(fail)?
        .as_string()
        .unwrap_or_default();
    let timestamp = js::get(raw, "timestamp").map_err(fail)?;
    let timestamp_ms = js::call(&timestamp, "getTime", &[])
        .map_err(fail)?
        .as_f64()
        .unwrap_or(0.0) as i64;
    let attempts = js::get(raw, "attempts")
        .map_err(fail)?
        .as_f64()
        .unwrap_or(1.0) as u32;
    let body: serde_json::Value = js::to_value("queue", &js::get(raw, "body").map_err(fail)?)?;
    if id.is_empty() {
        return Err(PlatformError::invalid("queue", "message without an id"));
    }
    Ok((id, timestamp_ms, attempts, body))
}
