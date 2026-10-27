//! The functions behind [`export_worker!`](crate::export_worker): each converts the runtime's JS
//! values, calls the application trait and converts the result back, as the code `worker-macros`
//! 0.8.7 generates for `#[event(...)]` and `#[durable_object]` does, including its answer to a
//! conversion error (a plain `500`).
//!
//! Spike S1 wrote the `email`, `queue` and `scheduled` exports and the Durable Object classes and ran
//! them against the deployed runtime ([Rust workspace §6.3]); M4 hardens them.
//!
//! Two deliberate differences from `worker-macros` 0.8.7, which panics when an `email` or `queue`
//! handler fails: here a failure is a rejected promise. The runtime treats both as a thrown exception,
//! and a rejection leaves the wasm instance usable.
//!
//! [Rust workspace §6.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

pub use js_sys;
pub use wasm_bindgen;
pub use wasm_bindgen::JsValue;
pub use wasm_bindgen_futures;

use wasm_bindgen::JsCast as _;

/// The argument types of the exports, named so the macro expansion needs no `worker` path.
pub type RawRequest = worker::worker_sys::web_sys::Request;
pub type RawEnv = worker::Env;
pub type RawContext = worker::worker_sys::Context;
pub type RawEmail = worker::ForwardableEmailMessage;
pub type RawBatch = worker::worker_sys::MessageBatch;
pub type RawScheduledEvent = worker::worker_sys::ScheduledEvent;
pub type RawScheduleContext = worker::worker_sys::ScheduleContext;
pub type RawObjectState = worker::worker_sys::DurableObjectState;

/// The Cloudflare platform handed to the application for one invocation. M4 gives it the trait set.
pub struct CfPlatform {
    env: worker::Env,
}

impl CfPlatform {
    /// The raw bindings, for the spike only; M4 replaces this with the platform traits.
    #[cfg(feature = "spike-s1")]
    pub fn env(&self) -> &worker::Env {
        &self.env
    }
}

/// A Durable Object's state, handed to [`ObjectApp::new`].
pub struct CfObjectState {
    state: worker::State,
}

impl CfObjectState {
    /// The raw state, for the spike only; M4 replaces this with the platform traits.
    #[cfg(feature = "spike-s1")]
    pub fn state(&self) -> &worker::State {
        &self.state
    }
}

/// An HTTP request as the application sees it.
pub struct HttpRequestIn {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub cf_connecting_ip: Option<String>,
}

/// The application's answer. M30 adds the WebSocket upgrade.
pub enum HttpResponseOut {
    Http {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
}

/// An inbound message from Email Routing.
pub struct EmailIn {
    pub from: String,
    pub to: String,
    pub headers: Vec<(String, String)>,
    pub raw: Vec<u8>,
    pub raw_size: u64,
}

/// What the email export does with the message ([Inbound › Interface]).
///
/// [Inbound › Interface]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
pub enum EmailOutcome {
    Accept,
    /// `setReject`: a permanent SMTP failure with this text.
    Reject {
        smtp: String,
    },
    /// The export rejects its promise, which the runtime reports to the sender as a temporary failure.
    TempFail,
}

/// A cron trigger.
pub struct ScheduledIn {
    pub cron: String,
    pub scheduled_time_ms: f64,
}

/// One queue message. Unhandled messages follow the runtime's default: the batch succeeds and is acked.
pub struct Incoming {
    pub id: String,
    /// When the message was first sent, in milliseconds since the epoch.
    pub timestamp_ms: f64,
    /// Delivery attempts so far, starting at 1.
    pub attempts: u32,
    /// The body as JSON text.
    pub body_json: String,
    raw: worker::worker_sys::Message,
}

impl Incoming {
    pub fn ack(&self) {
        let _ = self.raw.ack();
    }

    /// Retries the message, after `delay_seconds` when given.
    pub fn retry(&self, delay_seconds: Option<u32>) {
        let options = js_sys::Object::new();
        if let Some(delay) = delay_seconds {
            let _ = js_sys::Reflect::set(&options, &"delaySeconds".into(), &delay.into());
        }
        let _ = self.raw.retry(options.into());
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
    async fn queue(&self, _queue: String, batch: Vec<Incoming>) {
        for message in &batch {
            message.retry(None);
        }
    }
    async fn scheduled(&self, _ev: ScheduledIn) {}
}

/// What each Durable Object class implements.
#[allow(async_fn_in_trait)]
pub trait ObjectApp: Sized + 'static {
    fn new(state: CfObjectState, platform: CfPlatform) -> Self;
    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut;
    async fn alarm(&self) {}
}

#[doc(hidden)]
pub async fn fetch<A: WorkerApp>(
    req: RawRequest,
    env: RawEnv,
    _ctx: RawContext,
) -> Result<JsValue, JsValue> {
    let app = A::new(CfPlatform { env });
    Ok(JsValue::from(
        handle_http(req, "fetch", |input| app.fetch(input)).await,
    ))
}

#[doc(hidden)]
pub async fn email<A: WorkerApp>(
    message: RawEmail,
    env: RawEnv,
    _ctx: RawContext,
) -> Result<JsValue, JsValue> {
    let app = A::new(CfPlatform { env });
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
    _ctx: RawContext,
) -> Result<JsValue, JsValue> {
    let app = A::new(CfPlatform { env });
    let name = batch.queue()?.as_string().unwrap_or_default();
    let mut messages = Vec::new();
    for raw in batch.messages()?.iter() {
        messages.push(incoming(raw.unchecked_into())?);
    }
    app.queue(name, messages).await;
    Ok(JsValue::UNDEFINED)
}

#[doc(hidden)]
pub async fn scheduled<A: WorkerApp>(
    event: RawScheduledEvent,
    env: RawEnv,
    _ctx: RawScheduleContext,
) -> Result<JsValue, JsValue> {
    let app = A::new(CfPlatform { env });
    let event = worker::ScheduledEvent::from(event);
    app.scheduled(ScheduledIn {
        cron: event.cron(),
        scheduled_time_ms: event.schedule(),
    })
    .await;
    Ok(JsValue::UNDEFINED)
}

#[doc(hidden)]
pub fn object_new<O: ObjectApp>(state: RawObjectState, env: RawEnv) -> O {
    O::new(
        CfObjectState {
            state: worker::State::from(state),
        },
        CfPlatform { env },
    )
}

#[doc(hidden)]
pub async fn object_fetch<O: ObjectApp>(object: &O, req: RawRequest) -> Result<JsValue, JsValue> {
    Ok(JsValue::from(
        handle_http(req, "object_fetch", |input| object.fetch(input)).await,
    ))
}

#[doc(hidden)]
pub async fn object_alarm<O: ObjectApp>(object: &O) -> Result<JsValue, JsValue> {
    object.alarm().await;
    Ok(JsValue::UNDEFINED)
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
    let input = match request_in(worker::Request::from(req)).await {
        Ok(input) => input,
        Err(_) => return internal_error(handler),
    };
    match response_out(run(input).await) {
        Ok(response) => response,
        Err(_) => internal_error(handler),
    }
}

async fn request_in(mut req: worker::Request) -> worker::Result<HttpRequestIn> {
    let headers = req.headers().clone();
    let body = match req.method() {
        worker::Method::Get | worker::Method::Head => Vec::new(),
        _ => req.bytes().await?,
    };
    Ok(HttpRequestIn {
        method: req.method().to_string(),
        url: req.url()?.to_string(),
        cf_connecting_ip: headers.get("cf-connecting-ip")?,
        headers: headers.entries().collect(),
        body,
    })
}

async fn email_in(message: &RawEmail) -> worker::Result<EmailIn> {
    let from = js_string(message, "from")?;
    let to = js_string(message, "to")?;
    let headers = worker::Headers(message.headers()).entries().collect();
    Ok(EmailIn {
        from,
        to,
        headers,
        raw_size: message.raw_size() as u64,
        raw: message.raw_bytes().await?,
    })
}

fn incoming(raw: worker::worker_sys::Message) -> Result<Incoming, JsValue> {
    let attempts = js_sys::Reflect::get(&raw, &"attempts".into())?
        .as_f64()
        .unwrap_or(1.0) as u32;
    let body_json = js_sys::JSON::stringify(&raw.body()?)?
        .as_string()
        .unwrap_or_default();
    Ok(Incoming {
        id: raw.id()?.as_string().unwrap_or_default(),
        timestamp_ms: raw.timestamp()?.get_time(),
        attempts,
        body_json,
        raw,
    })
}

/// `from` and `to` are getters inherited from the runtime's `EmailMessage`.
fn js_string(value: &JsValue, name: &str) -> worker::Result<String> {
    js_sys::Reflect::get(value, &name.into())?
        .as_string()
        .ok_or_else(|| worker::Error::RustError(format!("email message has no `{name}`")))
}

fn response_out(out: HttpResponseOut) -> worker::Result<worker::worker_sys::web_sys::Response> {
    let HttpResponseOut::Http {
        status,
        headers,
        body,
    } = out;
    let h = worker::Headers::new();
    for (name, value) in &headers {
        h.append(name, value)?;
    }
    Ok(worker::Response::from_bytes(body)?
        .with_status(status)
        .with_headers(h)
        .into())
}

/// The response for a request or response the glue could not convert, as worker-macros 0.8.7
/// answers it. The log line carries no request content (Security § 12).
fn internal_error(handler: &str) -> worker::worker_sys::web_sys::Response {
    worker::console_error!("event=glue_error handler={handler}");
    worker::Response::error("INTERNAL SERVER ERROR", 500)
        .expect("a fixed body and a valid status always build a response")
        .into()
}
