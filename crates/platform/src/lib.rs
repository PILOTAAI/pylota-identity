//! Traits for every Cloudflare capability, their Cloudflare implementations and in-memory fakes
//! ([Rust workspace §6]). This is the only crate that imports `worker`; every other crate reaches
//! Cloudflare through it.
//!
//! - One module per capability holds its trait and the types it speaks ([`clock`], [`rng`], [`ids`],
//!   [`d1`], [`durable`], [`r2`], [`queues`], [`ai`], [`vectorize`], [`email`], [`ratelimit`], [`dns`],
//!   [`http`], [`tcp`], [`metrics`], [`background`]), and [`Platform`] bundles them, so logic names one
//!   type parameter.
//! - [`config`] reads and validates the variables and secrets once per isolate (§6.1).
//! - `cf` (wasm32 only) implements the traits over the runtime and holds the entry glue behind
//!   [`export_worker!`]; `fakes` (native only) implements them in memory for worker-logic tests.
//!
//! [Rust workspace §6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#![allow(async_fn_in_trait)] // single-threaded Worker: futures are never Send (§5 rule 1)

pub mod ai;
pub mod app;
pub mod background;
pub mod base64;
pub mod buffer;
pub mod clock;
pub mod config;
pub mod console;
pub mod d1;
pub mod dns;
pub mod durable;
pub mod email;
pub mod http;
pub mod ids;
pub mod metrics;
pub mod queues;
pub mod r2;
pub mod ratelimit;
pub mod rng;
pub mod stream;
pub mod tcp;
pub mod vectorize;

#[cfg(target_arch = "wasm32")]
pub mod cf;
#[cfg(not(target_arch = "wasm32"))]
pub mod fakes;

pub use ai::Ai;
pub use background::Background;
pub use clock::Clock;
pub use config::{Config, Secrets};
pub use d1::ControlDb;
pub use dns::Dns;
pub use durable::{ObjectClient, ObjectState, Sql};
pub use email::MailSender;
pub use http::HttpClient;
pub use ids::Ids;
pub use metrics::Metrics;
pub use queues::QueueProducer;
pub use r2::BlobStore;
pub use ratelimit::RateLimiter;
pub use rng::Rng;
pub use tcp::TcpConnect;
pub use vectorize::VectorIndex;

/// What failed, as the handlers map it to an API error ([Design conventions]).
///
/// [Design conventions]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/index.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformErrorKind {
    Unavailable,
    Timeout,
    Upstream,
    NotFound,
    Conflict,
    Invalid,
    Corrupt,
    Internal,
}

/// A failed platform call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformError {
    pub kind: PlatformErrorKind,
    /// The binding or service: `"DB"`, `"BLOBS"`, `"MAILBOX"`, `"AI"`, `"cf-api"`, …
    pub binding: &'static str,
    /// Machine text; never content or clear-text addresses (Security § 12).
    pub detail: String,
}

pub type PResult<T> = Result<T, PlatformError>;

impl PlatformError {
    pub fn new(kind: PlatformErrorKind, binding: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind,
            binding,
            detail: detail.into(),
        }
    }

    pub fn invalid(binding: &'static str, detail: impl Into<String>) -> Self {
        Self::new(PlatformErrorKind::Invalid, binding, detail)
    }

    pub fn unavailable(binding: &'static str, detail: impl Into<String>) -> Self {
        Self::new(PlatformErrorKind::Unavailable, binding, detail)
    }

    pub fn internal(binding: &'static str, detail: impl Into<String>) -> Self {
        Self::new(PlatformErrorKind::Internal, binding, detail)
    }

    /// The R2 error code the failure carries (`10013`, `10037`, `10058`, …), for [`r2::BlobStore`]
    /// callers that map them ([Uploads]). R2 errors are written `r2 {code}: {text}`.
    ///
    /// [Uploads]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/uploads.md
    pub fn r2_code(&self) -> Option<u32> {
        let rest = self.detail.strip_prefix("r2 ")?;
        rest.split(':').next()?.trim().parse().ok()
    }
}

impl core::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?} ({}): {}", self.kind, self.binding, self.detail)
    }
}

impl std::error::Error for PlatformError {}

/// Every capability of one invocation, so handlers and objects name a single type parameter
/// ([Rust workspace §6.2]). `cf::CfPlatform` implements it over the runtime's `Env`;
/// `fakes::FakePlatform` natively.
///
/// [Rust workspace §6.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
pub trait Platform: 'static {
    type Clock: Clock;
    type Rng: Rng;
    type Ids: Ids;
    type Db: ControlDb;
    type Objects: ObjectClient;
    type Blobs: BlobStore;
    type Queues: QueueProducer;
    type Ai: Ai;
    type Vectors: VectorIndex;
    type Mail: MailSender;
    type Limits: RateLimiter;
    type Dns: Dns;
    type Http: HttpClient;
    type Tcp: TcpConnect;
    type Metrics: Metrics;
    type Background: Background;

    fn clock(&self) -> &Self::Clock;
    fn rng(&self) -> &Self::Rng;
    fn ids(&self) -> &Self::Ids;
    fn db(&self) -> &Self::Db;
    fn objects(&self) -> &Self::Objects;
    /// `BLOBS`.
    fn blobs(&self) -> &Self::Blobs;
    /// `BACKUP`: bound only when `PM_BACKUP_BUCKET` is set.
    fn backup(&self) -> Option<&Self::Blobs>;
    fn queues(&self) -> &Self::Queues;
    fn ai(&self) -> &Self::Ai;
    /// `VECTORS`.
    fn vectors(&self) -> &Self::Vectors;
    /// `VECTORS_NEXT`: bound only while a re-embed runs (Search § 7.3).
    fn vectors_next(&self) -> Option<&Self::Vectors>;
    fn mail(&self) -> &Self::Mail;
    fn limits(&self) -> &Self::Limits;
    fn dns(&self) -> &Self::Dns;
    fn http(&self) -> &Self::Http;
    fn tcp(&self) -> &Self::Tcp;
    fn metrics(&self) -> &Self::Metrics;
    fn background(&self) -> &Self::Background;
    /// Read once per isolate (§6.1).
    fn config(&self) -> &Config;
    fn secrets(&self) -> &Secrets {
        &self.config().secrets
    }
}

/// Exports the Worker's entry points from `crates/worker` without the `worker` attribute macros,
/// which expand to `::worker::…` paths and so compile only in a crate that depends on `worker`
/// directly ([Rust workspace §2], "Entry points").
///
/// The expansion holds only `#[wasm_bindgen]` exports whose paths go through `$crate`, each a
/// one-line call into `cf::glue`: the `fetch`, `email`, `queue` and `scheduled` functions, and one
/// class per Durable Object with its constructor, `fetch` and `alarm`. A class marked `{ websocket }`
/// also exports `webSocketMessage`, `webSocketClose` and `webSocketError` (`TenantEvents`, Event
/// feed § 7.9).
///
/// ```ignore
/// pylota_identity_platform::export_worker! {
///     app: crate::App, // impl pylota_identity_platform::cf::glue::WorkerApp
///     durable_objects: {
///         IdentityMailbox => crate::mailbox::MailboxObject, // impl cf::glue::ObjectApp
///         TenantEvents => crate::events::EventsObject { websocket },
///     }
/// }
/// ```
///
/// [Rust workspace §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
// rustfmt re-indents attributes inside macro_rules! bodies on every run, so it never settles.
#[rustfmt::skip]
#[macro_export]
macro_rules! export_worker {
    (app: $app:path $(,)?) => {
        $crate::export_worker! { app: $app, durable_objects: {} }
    };
    (app: $app:path, durable_objects: { $($class:ident => $object:path $({ $ws:ident })?),* $(,)? } $(,)?) => {
        #[doc(hidden)]
        mod __pylota_identity_exports {
            #[allow(unused_imports)]
            use $crate::cf::glue::wasm_bindgen;
            use $crate::cf::glue::wasm_bindgen::prelude::wasm_bindgen;
            use $crate::cf::glue as g;

            // As worker-macros 0.8.7 does: wasm-bindgen requires an unwind-safe export future.
            #[wasm_bindgen(
                wasm_bindgen = $crate::cf::glue::wasm_bindgen,
                wasm_bindgen_futures = $crate::cf::glue::wasm_bindgen_futures,
                js_sys = $crate::cf::glue::js_sys
            )]
            pub async fn fetch(
                req: g::RawRequest,
                env: g::RawEnv,
                ctx: g::RawContext,
            ) -> ::core::result::Result<g::JsValue, g::JsValue> {
                ::core::panic::AssertUnwindSafe(g::fetch::<$app>(req, env, ctx)).await
            }

            #[wasm_bindgen(
                wasm_bindgen = $crate::cf::glue::wasm_bindgen,
                wasm_bindgen_futures = $crate::cf::glue::wasm_bindgen_futures,
                js_sys = $crate::cf::glue::js_sys
            )]
            pub async fn email(
                message: g::RawEmail,
                env: g::RawEnv,
                ctx: g::RawContext,
            ) -> ::core::result::Result<g::JsValue, g::JsValue> {
                ::core::panic::AssertUnwindSafe(g::email::<$app>(message, env, ctx)).await
            }

            #[wasm_bindgen(
                wasm_bindgen = $crate::cf::glue::wasm_bindgen,
                wasm_bindgen_futures = $crate::cf::glue::wasm_bindgen_futures,
                js_sys = $crate::cf::glue::js_sys
            )]
            pub async fn queue(
                batch: g::RawBatch,
                env: g::RawEnv,
                ctx: g::RawContext,
            ) -> ::core::result::Result<g::JsValue, g::JsValue> {
                ::core::panic::AssertUnwindSafe(g::queue::<$app>(batch, env, ctx)).await
            }

            #[wasm_bindgen(
                wasm_bindgen = $crate::cf::glue::wasm_bindgen,
                wasm_bindgen_futures = $crate::cf::glue::wasm_bindgen_futures,
                js_sys = $crate::cf::glue::js_sys
            )]
            pub async fn scheduled(
                event: g::RawScheduledEvent,
                env: g::RawEnv,
                ctx: g::RawScheduleContext,
            ) -> ::core::result::Result<g::JsValue, g::JsValue> {
                ::core::panic::AssertUnwindSafe(g::scheduled::<$app>(event, env, ctx)).await
            }

            $(
                #[wasm_bindgen(wasm_bindgen = $crate::cf::glue::wasm_bindgen)]
                pub struct $class {
                    object: g::Object<$object>,
                }

                #[wasm_bindgen(
                    wasm_bindgen = $crate::cf::glue::wasm_bindgen,
                    wasm_bindgen_futures = $crate::cf::glue::wasm_bindgen_futures,
                    js_sys = $crate::cf::glue::js_sys
                )]
                impl $class {
                    #[wasm_bindgen(constructor)]
                    pub fn new(state: g::RawObjectState, env: g::RawEnv) -> Self {
                        Self { object: g::object_new::<$object>(state, env) }
                    }

                    #[wasm_bindgen(js_name = fetch)]
                    pub async fn fetch(
                        &self,
                        req: g::RawRequest,
                    ) -> ::core::result::Result<g::JsValue, g::JsValue> {
                        ::core::panic::AssertUnwindSafe(g::object_fetch(&self.object, req)).await
                    }

                    #[wasm_bindgen(js_name = alarm)]
                    pub async fn alarm(&self) -> ::core::result::Result<g::JsValue, g::JsValue> {
                        ::core::panic::AssertUnwindSafe(g::object_alarm(&self.object)).await
                    }
                }

                $(
                    #[doc = concat!("Hibernation handlers (`", stringify!($ws), "`).")]
                    #[wasm_bindgen(
                        wasm_bindgen = $crate::cf::glue::wasm_bindgen,
                        wasm_bindgen_futures = $crate::cf::glue::wasm_bindgen_futures,
                        js_sys = $crate::cf::glue::js_sys
                    )]
                    impl $class {
                        #[wasm_bindgen(js_name = webSocketMessage)]
                        pub async fn websocket_message(
                            &self,
                            ws: g::RawWebSocket,
                            message: g::JsValue,
                        ) -> ::core::result::Result<g::JsValue, g::JsValue> {
                            ::core::panic::AssertUnwindSafe(
                                g::object_websocket_message(&self.object, ws, message),
                            )
                            .await
                        }

                        #[wasm_bindgen(js_name = webSocketClose)]
                        pub async fn websocket_close(
                            &self,
                            ws: g::RawWebSocket,
                            code: u16,
                            reason: String,
                            was_clean: bool,
                        ) -> ::core::result::Result<g::JsValue, g::JsValue> {
                            ::core::panic::AssertUnwindSafe(
                                g::object_websocket_close(&self.object, ws, code, reason, was_clean),
                            )
                            .await
                        }

                        #[wasm_bindgen(js_name = webSocketError)]
                        pub async fn websocket_error(
                            &self,
                            ws: g::RawWebSocket,
                            error: g::JsValue,
                        ) -> ::core::result::Result<g::JsValue, g::JsValue> {
                            ::core::panic::AssertUnwindSafe(
                                g::object_websocket_error(&self.object, ws, error),
                            )
                            .await
                        }
                    }
                )?
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The crate builds and its test harness runs (build plan M0).
    #[test]
    fn workspace_member_builds() {}

    #[test]
    fn r2_codes_are_read_from_the_detail() {
        let e = PlatformError::new(PlatformErrorKind::Invalid, "BLOBS", "r2 10037: BadDigest");
        assert_eq!(e.r2_code(), Some(10037));
        assert_eq!(PlatformError::invalid("BLOBS", "other").r2_code(), None);
    }
}
