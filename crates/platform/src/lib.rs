//! Traits for every Cloudflare capability, their Cloudflare implementations and in-memory fakes
//! ([Rust workspace §6]). This is the only crate that imports `worker`; every other crate reaches
//! Cloudflare through it.
//!
//! Today it holds the entry glue: [`export_worker!`] and, on wasm32, `cf::glue` and `cf::externs`,
//! written and run against the runtime by spike S1. M4 adds the trait set, the fakes and `Config`.
//!
//! [Rust workspace §6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

#[cfg(target_arch = "wasm32")]
pub mod cf;

/// Exports the Worker's entry points from `crates/worker` without the `worker` attribute macros,
/// which expand to `::worker::…` paths and so compile only in a crate that depends on `worker`
/// directly ([Rust workspace §2], "Entry points").
///
/// The expansion holds only `#[wasm_bindgen]` exports whose paths go through `$crate`, each a
/// one-line call into `cf::glue`: the `fetch`, `email`, `queue` and `scheduled` functions, and one
/// class per Durable Object with its constructor, `fetch` and `alarm`.
///
/// ```ignore
/// pylota_identity_platform::export_worker! {
///     app: crate::App, // impl pylota_identity_platform::cf::glue::WorkerApp
///     durable_objects: {
///         IdentityMailbox => crate::mailbox::MailboxObject, // impl cf::glue::ObjectApp
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
    (app: $app:path, durable_objects: { $($class:ident => $object:path),* $(,)? } $(,)?) => {
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
                    object: $object,
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
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    /// The crate builds and its test harness runs (build plan M0).
    #[test]
    fn workspace_member_builds() {}
}
