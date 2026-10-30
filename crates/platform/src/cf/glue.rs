//! The functions behind [`export_worker!`](crate::export_worker): each converts the runtime's JS
//! values, calls the application trait and converts the result back, as the code `worker-macros`
//! 0.8.7 generates for `#[event(...)]` and `#[durable_object]` does, including its answer to a
//! conversion error (a plain `500`). Spike S1 wrote and ran the first version against the runtime.
//!
//! Differences from `worker-macros` 0.8.7, each deliberate:
//!
//! - A failed `email` or `queue` handler is a rejected promise, not a panic. The runtime treats both as
//!   a thrown exception, and a rejection leaves the wasm instance usable.
//! - Configuration is read once per isolate before any handler runs. While it is invalid, `fetch`
//!   answers `503 unavailable` with `details.config_invalid` naming the variable (on `/health` as on
//!   every other path), mail is refused as a temporary failure, queue messages are retried, cron ticks
//!   and alarms do nothing, and Durable Object requests get the same `503` (Rust workspace § 6.1).
//! - The raw message and request bodies stay in the JS heap ([`JsBuffer`]) until the application
//!   reads them.

pub use js_sys;
pub use wasm_bindgen;
pub use wasm_bindgen::JsValue;
pub use wasm_bindgen_futures;

use wasm_bindgen::JsCast as _;

pub use super::CfPlatform;
use super::basic::dup_state;
use super::js;
use super::queues::{CfSettle, read_message};
use super::{CfBackground, CfHibernation, CfIds, CfSql, CfStorage, CfWebSocket};
pub use crate::app::{
    EmailIn, EmailOutcome, HttpRequestIn, HttpResponseOut, ScheduledIn, UpgradeResponse, WsMessage,
};
use crate::buffer::JsBuffer;
use crate::config::ConfigInvalid;
use crate::ids::{IdPrefix, Ids as _};
use crate::queues::{Incoming, QueueName};

/// The argument types of the exports, named so the macro expansion needs no `worker` path.
pub type RawRequest = worker::worker_sys::web_sys::Request;
pub type RawEnv = worker::Env;
pub type RawContext = worker::worker_sys::Context;
pub type RawEmail = worker::ForwardableEmailMessage;
pub type RawBatch = worker::worker_sys::MessageBatch;
pub type RawScheduledEvent = worker::worker_sys::ScheduledEvent;
pub type RawScheduleContext = worker::worker_sys::ScheduleContext;
pub type RawObjectState = worker::worker_sys::DurableObjectState;
pub type RawWebSocket = worker::worker_sys::web_sys::WebSocket;

/// A Durable Object's state, handed to [`ObjectApp::new`].
pub struct CfObjectState {
    raw: RawObjectState,
    state: worker::State,
}

impl CfObjectState {
    /// The object's own ID (hex).
    pub fn id(&self) -> String {
        self.state.id().to_string()
    }

    /// `ctx.storage.sql`.
    pub fn sql(&self) -> CfSql {
        let storage = self.state.storage();
        CfSql {
            sql: storage.sql(),
            storage,
        }
    }

    /// The alarm and `deleteAll`.
    pub fn storage(&self) -> CfStorage {
        CfStorage {
            storage: self.state.storage(),
            raw_state: self.raw.clone().into(),
        }
    }

    /// Hibernating WebSockets (`TenantEvents`).
    pub fn hibernation(&self) -> CfHibernation {
        CfHibernation::new(&self.raw)
    }
}

/// What the Worker implements. Futures are not `Send`: the Worker is single-threaded
/// ([Rust workspace §5], rule 1). The defaults are those of a Worker that has not built the handler
/// yet: mail is refused as a temporary failure and queue messages are retried.
///
/// [Rust workspace §5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#[allow(async_fn_in_trait)]
pub trait WorkerApp: Sized + 'static {
    /// Called once per invocation.
    fn new(platform: CfPlatform) -> Self;
    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut;
    async fn email(&self, _msg: EmailIn) -> EmailOutcome {
        EmailOutcome::TempFail
    }
    /// One batch of `queue` (its dead-letter queue when `dead_letter`); every message is acked or
    /// retried. A message nobody settles is acked when the batch succeeds (the runtime's default).
    async fn queue(
        &self,
        _queue: QueueName,
        _dead_letter: bool,
        batch: Vec<Incoming<serde_json::Value>>,
    ) {
        for message in &batch {
            message.retry(None);
        }
    }
    async fn scheduled(&self, _ev: ScheduledIn) {}
}

/// What each Durable Object class implements. Only `TenantEvents` is exported with the hibernation
/// handlers; the defaults close the socket with `1011`.
#[allow(async_fn_in_trait)]
pub trait ObjectApp: Sized + 'static {
    fn new(state: CfObjectState, platform: CfPlatform) -> Self;
    /// Decodes the `RpcEnvelope`, checks the owner, dispatches and encodes the `RpcResult`.
    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut;
    /// Runs the due purposes (Design conventions, rule 5).
    async fn alarm(&self) {}
    async fn websocket_message(&self, ws: CfWebSocket, _msg: WsMessage) {
        let _ = ws.close(1011, "unexpected message");
    }
    async fn websocket_close(&self, ws: CfWebSocket, _code: u16, _reason: String, _clean: bool) {
        let _ = ws.close(1011, "closed");
    }
    async fn websocket_error(&self, ws: CfWebSocket, _err: String) {
        let _ = ws.close(1011, "error");
    }
}

/// The JS object behind an exported class: the application's object, or the reason it could not be
/// built.
#[doc(hidden)]
pub enum Object<O> {
    Ready(O),
    Invalid(ConfigInvalid),
}

#[doc(hidden)]
pub async fn fetch<A: WorkerApp>(
    req: RawRequest,
    env: RawEnv,
    ctx: RawContext,
) -> Result<JsValue, JsValue> {
    let ctx = worker::Context::new(ctx);
    let response = match CfPlatform::new(env, CfBackground::Worker(ctx)) {
        #[allow(unused_mut)]
        Ok(mut platform) => {
            #[cfg(feature = "itest-hooks")]
            {
                super::itest::sync(crate::Platform::config(&platform)).await;
                platform.apply_itest_overrides();
            }
            let app = A::new(platform);
            handle_http(req, "fetch", |input| app.fetch(input)).await
        }
        Err(e) => config_invalid(e),
    };
    Ok(response.into())
}

#[doc(hidden)]
pub async fn email<A: WorkerApp>(
    message: RawEmail,
    env: RawEnv,
    ctx: RawContext,
) -> Result<JsValue, JsValue> {
    let ctx = worker::Context::new(ctx);
    #[allow(unused_mut)]
    let Ok(mut platform) = CfPlatform::new(env, CfBackground::Worker(ctx)) else {
        return Err(JsValue::from_str("temporary failure"));
    };
    #[cfg(feature = "itest-hooks")]
    {
        super::itest::sync(crate::Platform::config(&platform)).await;
        platform.apply_itest_overrides();
    }
    let app = A::new(platform);
    let input = match email_in(&message).await {
        Ok(input) => input,
        Err(_) => {
            worker::console_error!("event=glue_error handler=email");
            return Err(JsValue::from_str("temporary failure"));
        }
    };
    match app.email(input).await {
        EmailOutcome::Accept => Ok(JsValue::UNDEFINED),
        EmailOutcome::Reject { smtp } => {
            message.set_reject(&smtp);
            Ok(JsValue::UNDEFINED)
        }
        EmailOutcome::TempFail => Err(JsValue::from_str("temporary failure")),
    }
}

#[doc(hidden)]
pub async fn queue<A: WorkerApp>(
    batch: RawBatch,
    env: RawEnv,
    ctx: RawContext,
) -> Result<JsValue, JsValue> {
    let ctx = worker::Context::new(ctx);
    let raw: Vec<JsValue> = batch.messages()?.iter().collect();
    let retry_all = |raw: &[JsValue]| {
        for m in raw {
            let _ = js::call(m, "retry", &[]);
        }
    };
    #[allow(unused_mut)]
    let Ok(mut platform) = CfPlatform::new(env, CfBackground::Worker(ctx)) else {
        retry_all(&raw);
        return Ok(JsValue::UNDEFINED);
    };
    #[cfg(feature = "itest-hooks")]
    {
        super::itest::sync(crate::Platform::config(&platform)).await;
        platform.apply_itest_overrides();
    }
    let name = batch.queue()?.as_string().unwrap_or_default();
    let Some((queue, dead_letter)) = QueueName::from_queue_name(&name) else {
        worker::console_error!("event=glue_error handler=queue reason=unknown_queue");
        retry_all(&raw);
        return Ok(JsValue::UNDEFINED);
    };
    let mut messages = Vec::with_capacity(raw.len());
    for m in raw {
        match read_message(&m) {
            Ok((id, timestamp_ms, attempts, body)) => messages.push(Incoming::new(
                id,
                timestamp_ms,
                attempts,
                body,
                std::rc::Rc::new(CfSettle { raw: m }),
            )),
            Err(_) => {
                worker::console_error!("event=glue_error handler=queue reason=unreadable_message");
                let _ = js::call(&m, "retry", &[]);
            }
        }
    }
    A::new(platform).queue(queue, dead_letter, messages).await;
    Ok(JsValue::UNDEFINED)
}

#[doc(hidden)]
pub async fn scheduled<A: WorkerApp>(
    event: RawScheduledEvent,
    env: RawEnv,
    ctx: RawScheduleContext,
) -> Result<JsValue, JsValue> {
    let background: worker::worker_sys::Context = ctx.unchecked_into();
    #[allow(unused_mut)]
    let Ok(mut platform) =
        CfPlatform::new(env, CfBackground::Worker(worker::Context::new(background)))
    else {
        return Ok(JsValue::UNDEFINED);
    };
    #[cfg(feature = "itest-hooks")]
    {
        super::itest::sync(crate::Platform::config(&platform)).await;
        platform.apply_itest_overrides();
    }
    let event = worker::ScheduledEvent::from(event);
    A::new(platform)
        .scheduled(ScheduledIn {
            cron: event.cron(),
            scheduled_time_ms: event.schedule() as i64,
        })
        .await;
    Ok(JsValue::UNDEFINED)
}

#[doc(hidden)]
pub fn object_new<O: ObjectApp>(state: RawObjectState, env: RawEnv) -> Object<O> {
    match CfPlatform::new(env, CfBackground::Object(dup_state(&state))) {
        Ok(platform) => Object::Ready(O::new(
            CfObjectState {
                state: worker::State::from(dup_state(&state)),
                raw: state,
            },
            platform,
        )),
        Err(e) => Object::Invalid(e),
    }
}

#[doc(hidden)]
pub async fn object_fetch<O: ObjectApp>(
    object: &Object<O>,
    req: RawRequest,
) -> Result<JsValue, JsValue> {
    let response = match object {
        Object::Ready(o) => handle_http(req, "object_fetch", |input| o.fetch(input)).await,
        Object::Invalid(e) => config_invalid(*e),
    };
    Ok(response.into())
}

#[doc(hidden)]
pub async fn object_alarm<O: ObjectApp>(object: &Object<O>) -> Result<JsValue, JsValue> {
    if let Object::Ready(o) = object {
        o.alarm().await;
    }
    Ok(JsValue::UNDEFINED)
}

#[doc(hidden)]
pub async fn object_websocket_message<O: ObjectApp>(
    object: &Object<O>,
    ws: RawWebSocket,
    message: JsValue,
) -> Result<JsValue, JsValue> {
    let ws = CfWebSocket(ws.into());
    let message = match message.as_string() {
        Some(text) => WsMessage::Text(text),
        None => WsMessage::Binary(js_sys::Uint8Array::new(&message).to_vec()),
    };
    match object {
        Object::Ready(o) => o.websocket_message(ws, message).await,
        Object::Invalid(_) => {
            let _ = ws.close(1011, "unavailable");
        }
    }
    Ok(JsValue::NULL)
}

#[doc(hidden)]
pub async fn object_websocket_close<O: ObjectApp>(
    object: &Object<O>,
    ws: RawWebSocket,
    code: u16,
    reason: String,
    clean: bool,
) -> Result<JsValue, JsValue> {
    if let Object::Ready(o) = object {
        o.websocket_close(CfWebSocket(ws.into()), code, reason, clean)
            .await;
    }
    Ok(JsValue::NULL)
}

#[doc(hidden)]
pub async fn object_websocket_error<O: ObjectApp>(
    object: &Object<O>,
    ws: RawWebSocket,
    error: JsValue,
) -> Result<JsValue, JsValue> {
    if let Object::Ready(o) = object {
        o.websocket_error(CfWebSocket(ws.into()), js::message(&error))
            .await;
    }
    Ok(JsValue::NULL)
}

async fn handle_http<F, Fut>(
    req: RawRequest,
    handler: &str,
    run: F,
) -> worker::worker_sys::web_sys::Response
where
    F: FnOnce(HttpRequestIn) -> Fut,
    Fut: core::future::Future<Output = HttpResponseOut>,
{
    let input = match request_in(req).await {
        Ok(input) => input,
        Err(_) => return internal_error(handler),
    };
    match response_out(run(input).await) {
        Ok(response) => response,
        Err(_) => internal_error(handler),
    }
}

async fn request_in(req: RawRequest) -> Result<HttpRequestIn, JsValue> {
    let method = req.method();
    let headers = worker::Headers(req.headers());
    let body = match method.as_str() {
        "GET" | "HEAD" => None,
        _ => {
            let buffer = js::call_async(&req, "arrayBuffer", &[]).await?;
            let array = js_sys::Uint8Array::new(&buffer);
            (array.length() > 0).then_some(JsBuffer { array })
        }
    };
    Ok(HttpRequestIn {
        method,
        url: req.url(),
        cf_connecting_ip: headers.get("cf-connecting-ip").ok().flatten(),
        headers: headers.entries().collect(),
        body,
    })
}

async fn email_in(message: &RawEmail) -> Result<EmailIn, JsValue> {
    let from = js::get(message, "from")?.as_string().unwrap_or_default();
    let to = js::get(message, "to")?.as_string().unwrap_or_default();
    let headers = worker::Headers(message.headers()).entries().collect();
    // The raw stream read whole into an ArrayBuffer that stays in the JS heap (§5 rule 5).
    let response = js::construct("Response", &[&message.raw().into()])?;
    let buffer = js::call_async(&response, "arrayBuffer", &[]).await?;
    let array = js_sys::Uint8Array::new(&buffer);
    Ok(EmailIn {
        from,
        to,
        headers,
        raw_size: message.raw_size() as u64,
        raw: JsBuffer { array },
    })
}

fn response_out(out: HttpResponseOut) -> worker::Result<worker::worker_sys::web_sys::Response> {
    match out {
        HttpResponseOut::Http {
            status,
            headers,
            body,
        } => {
            let h = worker::Headers::new();
            for (name, value) in &headers {
                h.append(name, value)?;
            }
            Ok(worker::Response::from_bytes(body)?
                .with_status(status)
                .with_headers(h)
                .into())
        }
        HttpResponseOut::Upgrade(up) => Ok(up.response),
        HttpResponseOut::Stream {
            status,
            headers,
            body,
        } => {
            use wasm_bindgen::JsCast as _;
            let h = worker::Headers::new();
            for (name, value) in &headers {
                h.append(name, value)?;
            }
            let init = super::js::object();
            let set = |k: &str, v: &wasm_bindgen::JsValue| {
                super::js::set(&init, k, v).map_err(|_| worker::Error::RustError(k.to_owned()))
            };
            set("status", &wasm_bindgen::JsValue::from(status))?;
            set("headers", &h.0.clone().into())?;
            let r = super::js::construct("Response", &[&body.readable, &init])
                .map_err(|_| worker::Error::RustError("stream response".into()))?;
            r.dyn_into()
                .map_err(|_| worker::Error::RustError("stream response".into()))
        }
    }
}

/// The response for a request or response the glue could not convert, as worker-macros 0.8.7
/// answers it. The log line carries no request content (Security § 12).
fn internal_error(handler: &str) -> worker::worker_sys::web_sys::Response {
    worker::console_error!("event=glue_error handler={handler}");
    worker::Response::error("INTERNAL SERVER ERROR", 500)
        .expect("a fixed body and a valid status always build a response")
        .into()
}

/// `503 unavailable` with `details.config_invalid` naming the variable.
fn config_invalid(e: ConfigInvalid) -> worker::worker_sys::web_sys::Response {
    let body = serde_json::json!({
        "error": {
            "code": "unavailable",
            "message": "The deployment's configuration is invalid.",
            "retryable": true,
            "fix": "The operator must correct the variable named in details.config_invalid and redeploy.",
            "docs": format!("{}/reference/errors#code-unavailable", crate::config::DEFAULT_DOCS_URL),
            "request_id": CfIds.new_id(IdPrefix::Request),
            "details": { "config_invalid": e.name },
        }
    });
    let headers = worker::Headers::new();
    let _ = headers.set("content-type", "application/json");
    let _ = headers.set("retry-after", "60");
    match worker::Response::from_bytes(body.to_string().into_bytes()) {
        Ok(r) => r.with_status(503).with_headers(headers).into(),
        Err(_) => internal_error("config_invalid"),
    }
}
