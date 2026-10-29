//! The Rust client for the Pylota Identity REST API (build plan M16, [Rust workspace §11]): one async
//! method per REST operation of `openapi.yaml`, typed with the `api-types` objects, plus webhook
//! signature verification.
//!
//! ```rust,no_run
//! use pylota_identity::Client;
//!
//! # async fn run() -> Result<(), pylota_identity::Error> {
//! let client = Client::builder()
//!     .base_url("https://mail.example.com")
//!     .api_key("pmk_live_…")
//!     .build()?;
//! let me = client.get_me().await?;
//! println!("{}", me.key_id);
//! # Ok(())
//! # }
//! ```
//!
//! [Rust workspace §11]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#![forbid(unsafe_code)]

mod client;
mod generated;
pub mod webhooks;

pub use client::{
    Body, Client, ClientBuilder, IdempotencyKey, Pages, Raw, Req, Response, RetryPolicy, segment,
};
pub use generated::*;

/// The request and response objects of the API, named as the component schemas of `openapi.yaml`.
pub mod types {
    pub use pylota_identity_api_types::*;
}

use serde_json::Value;

/// The error envelope of a failed call ([Errors]). `code` is the code as sent, so a code newer than
/// this SDK still arrives; [`ApiError::error_code`] reads it as a known one.
///
/// [Errors]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/errors.md
#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub fix: Option<String>,
    pub docs: Option<String>,
    pub details: Option<Value>,
    pub request_id: Option<String>,
    pub retryable: bool,
}

impl ApiError {
    /// The code, when this SDK knows it.
    pub fn error_code(&self) -> Option<types::ErrorCode> {
        serde_json::from_value(Value::String(self.code.clone())).ok()
    }
}

/// Why a call failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The service answered with an error envelope (boxed, so `Result<_, Error>` stays small).
    Api(Box<ApiError>),
    /// A non-success status without an error envelope (a proxy, a missing route on another host).
    Status { status: u16, body: String },
    /// Connect, TLS or timeout. `idempotency_key` is the key the call sent, to retry with.
    Transport {
        message: String,
        idempotency_key: Option<String>,
    },
    /// The answer did not decode.
    Decode(String),
    /// A value the SDK refuses before sending.
    Config(String),
}

impl Error {
    /// A non-success answer: the envelope when there is one.
    pub fn from_response(status: u16, body: &[u8]) -> Self {
        let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        let e = &v["error"];
        match e["code"].as_str() {
            Some(code) => Error::Api(Box::new(ApiError {
                status,
                code: code.to_owned(),
                message: e["message"].as_str().unwrap_or("").to_owned(),
                fix: e["fix"].as_str().map(str::to_owned),
                docs: e["docs"].as_str().map(str::to_owned),
                details: e.get("details").cloned(),
                request_id: e["request_id"].as_str().map(str::to_owned),
                retryable: e["retryable"].as_bool().unwrap_or(false),
            })),
            None => Error::Status {
                status,
                body: String::from_utf8_lossy(body).chars().take(512).collect(),
            },
        }
    }

    /// The envelope, for an `Api` error.
    pub fn api(&self) -> Option<&ApiError> {
        match self {
            Error::Api(e) => Some(e),
            _ => None,
        }
    }

    /// The envelope's code, when this SDK knows it.
    pub fn error_code(&self) -> Option<types::ErrorCode> {
        self.api().and_then(ApiError::error_code)
    }

    /// The HTTP status, for `Api` and `Status`.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api(e) => Some(e.status),
            Error::Status { status, .. } => Some(*status),
            _ => None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Api(e) => write!(f, "{} {}: {}", e.status, e.code, e.message),
            Error::Status { status, .. } => write!(f, "HTTP {status} without an error envelope"),
            Error::Transport { message, .. } => write!(f, "transport: {message}"),
            Error::Decode(m) => write!(f, "decode: {m}"),
            Error::Config(m) => write!(f, "config: {m}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sdk::errors::envelope_round_trip`: every error code decodes with its `retryable` flag.
    #[test]
    fn envelope_round_trip() {
        let body = br#"{"error":{"code":"rate_limited","message":"Too many requests.","retryable":true,"fix":"Wait.","docs":"https://x/errors#code-rate_limited","request_id":"req_1","details":{"bucket":"RL_API"}}}"#;
        let e = Error::from_response(429, body);
        assert_eq!(e.error_code(), Some(types::ErrorCode::RateLimited));
        assert!(e.api().is_some_and(|a| a.retryable && a.status == 429));
        let newer = Error::from_response(
            400,
            br#"{"error":{"code":"from_the_future","message":"x"}}"#,
        );
        assert!(newer.api().is_some_and(|a| a.code == "from_the_future"));
        assert_eq!(newer.error_code(), None);
        assert!(matches!(
            Error::from_response(502, b"<html>"),
            Error::Status { status: 502, .. }
        ));
    }
}

/// The Rust examples of the quickstart compile against this crate (`sdk::doctest::published_examples`
/// checks the landing page's copy; `cargo test --doc` compiles these).
#[cfg(doctest)]
#[doc = include_str!("../../../docs/src/quickstart.md")]
mod quickstart {}
