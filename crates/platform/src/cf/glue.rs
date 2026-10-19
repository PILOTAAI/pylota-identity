//! The functions behind [`export_worker!`](crate::export_worker): each converts the runtime's JS
//! values, calls the application trait and converts the result back, as the code `worker-macros`
//! 0.8.7 generates for `#[event(fetch)]` does, including its answer to a conversion error (a plain
//! `500`).
//!
//! M0 carries the subset of [Rust workspace §6.3] that a `fetch` answering `200` needs. Spike S1
//! extends it to the other handlers and validates it against the real runtime; M4 hardens it.
//!
//! [Rust workspace §6.3]: https://github.com/PILOTAAI/pylota-mail/blob/main/docs/src/project/design/rust-workspace.md

pub use js_sys;
pub use wasm_bindgen;
pub use wasm_bindgen::JsValue;
pub use wasm_bindgen_futures;

/// The argument types of the exported `fetch`, named so the macro expansion needs no `worker` path.
pub type RawRequest = worker::worker_sys::web_sys::Request;
pub type RawEnv = worker::Env;
pub type RawContext = worker::worker_sys::Context;

/// The Cloudflare platform handed to the application for one invocation. M4 gives it the trait set.
pub struct CfPlatform {
    #[expect(dead_code, reason = "read by the platform traits from M4 on")]
    env: worker::Env,
}

/// An HTTP request as the application sees it. S1 adds the body.
pub struct HttpRequestIn {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub cf_connecting_ip: Option<String>,
}

/// The application's answer. S1 adds the WebSocket upgrade.
pub enum HttpResponseOut {
    Http {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
}

/// What the Worker implements. Futures are not `Send`: the Worker is single-threaded
/// ([Rust workspace §5], rule 1).
///
/// [Rust workspace §5]: https://github.com/PILOTAAI/pylota-mail/blob/main/docs/src/project/design/rust-workspace.md
#[allow(async_fn_in_trait)]
pub trait WorkerApp: Sized + 'static {
    /// Called once per invocation.
    fn new(platform: CfPlatform) -> Self;
    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut;
}

#[doc(hidden)]
pub async fn fetch<A: WorkerApp>(
    req: RawRequest,
    env: RawEnv,
    _ctx: RawContext,
) -> Result<JsValue, JsValue> {
    let app = A::new(CfPlatform { env });
    let response = match request_in(worker::Request::from(req)) {
        Ok(input) => match response_out(app.fetch(input).await) {
            Ok(response) => response,
            Err(_) => internal_error(),
        },
        Err(_) => internal_error(),
    };
    Ok(JsValue::from(response))
}

fn request_in(req: worker::Request) -> worker::Result<HttpRequestIn> {
    let headers = req.headers();
    Ok(HttpRequestIn {
        method: req.method().to_string(),
        url: req.url()?.to_string(),
        cf_connecting_ip: headers.get("cf-connecting-ip")?,
        headers: headers.entries().collect(),
    })
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
fn internal_error() -> worker::worker_sys::web_sys::Response {
    worker::console_error!("event=glue_error handler=fetch");
    worker::Response::error("INTERNAL SERVER ERROR", 500)
        .expect("a fixed body and a valid status always build a response")
        .into()
}
