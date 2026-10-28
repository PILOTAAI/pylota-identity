//! What the entry glue hands the application and takes back ([Rust workspace §6.3]): one HTTP request,
//! one inbound message, one cron tick. Defined outside `cf` so that native worker-logic tests build them
//! too.
//!
//! [Rust workspace §6.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

use crate::buffer::JsBuffer;

/// An HTTP request as the application sees it.
#[derive(Clone, Debug)]
pub struct HttpRequestIn {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// `None` for `GET` and `HEAD`, and for a request without a body.
    pub body: Option<JsBuffer>,
    pub cf_connecting_ip: Option<String>,
}

impl HttpRequestIn {
    /// The first value of a header, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// The application's answer.
pub enum HttpResponseOut {
    Http {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// The runtime's `101` from a Durable Object (`ObjectClient::upgrade`), returned unchanged.
    Upgrade(UpgradeResponse),
}

impl HttpResponseOut {
    pub fn status(&self) -> u16 {
        match self {
            Self::Http { status, .. } => *status,
            Self::Upgrade(_) => 101,
        }
    }
}

/// A WebSocket upgrade response: the runtime's `Response` with its `webSocket` on wasm32, a marker
/// natively.
pub struct UpgradeResponse {
    #[cfg(target_arch = "wasm32")]
    pub(crate) response: web_sys::Response,
    #[cfg(not(target_arch = "wasm32"))]
    pub object_id: String,
}

/// An inbound message from Email Routing. `raw` stays in the JS heap until parsing (§5 rule 5).
pub struct EmailIn {
    pub from: String,
    pub to: String,
    pub headers: Vec<(String, String)>,
    pub raw: JsBuffer,
    pub raw_size: u64,
}

/// What the email export does with the message ([Inbound › Interface]).
///
/// [Inbound › Interface]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug)]
pub struct ScheduledIn {
    pub cron: String,
    pub scheduled_time_ms: i64,
}

/// A WebSocket message delivered to a hibernating object (`webSocketMessage`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
}
