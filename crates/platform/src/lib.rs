//! Traits for every Cloudflare capability, their Cloudflare implementations and in-memory fakes
//! ([Rust workspace §6]). This is the only crate that imports `worker`; every other crate reaches
//! Cloudflare through it.
//!
//! M0 holds the entry glue only: [`export_worker!`] and, on wasm32, `cf::glue`, with a `fetch`
//! export. Spike S1 adds the `email`, `queue` and `scheduled` exports and the Durable Object classes;
//! M4 adds the trait set, the fakes and `Config`.
//!
//! [Rust workspace §6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

#[cfg(target_arch = "wasm32")]
pub mod cf;

/// Exports the Worker's entry points from `crates/worker` without the `worker` attribute macros,
/// which expand to `::worker::…` paths and so compile only in a crate that depends on `worker`
/// directly ([Rust workspace §2], "Entry points").
///
/// The expansion holds only `#[wasm_bindgen]` exports whose paths go through `$crate`, each a
/// one-line call into `cf::glue`. M0 exports `fetch` alone:
///
/// ```ignore
/// pylota_identity_platform::export_worker! {
///     app: crate::App, // impl pylota_identity_platform::cf::glue::WorkerApp
/// }
/// ```
///
/// [Rust workspace §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
// rustfmt re-indents attributes inside macro_rules! bodies on every run, so it never settles.
#[rustfmt::skip]
#[macro_export]
macro_rules! export_worker {
    (app: $app:path $(,)?) => {
        #[doc(hidden)]
        mod __pylota_identity_export_fetch {
            #[allow(unused_imports)]
            use $crate::cf::glue::wasm_bindgen;
            use $crate::cf::glue::wasm_bindgen::prelude::wasm_bindgen;

            #[wasm_bindgen(
                wasm_bindgen = $crate::cf::glue::wasm_bindgen,
                wasm_bindgen_futures = $crate::cf::glue::wasm_bindgen_futures,
                js_sys = $crate::cf::glue::js_sys
            )]
            pub async fn fetch(
                req: $crate::cf::glue::RawRequest,
                env: $crate::cf::glue::RawEnv,
                ctx: $crate::cf::glue::RawContext,
            ) -> ::core::result::Result<$crate::cf::glue::JsValue, $crate::cf::glue::JsValue> {
                // As worker-macros 0.8.7 does: wasm-bindgen requires an unwind-safe export future.
                ::core::panic::AssertUnwindSafe($crate::cf::glue::fetch::<$app>(req, env, ctx)).await
            }
        }
    };
}

#[cfg(test)]
mod tests {
    /// The crate builds and its test harness runs (build plan M0).
    #[test]
    fn workspace_member_builds() {}
}
