//! The Pylota Identity Worker (build plan M5: routing, auth, tenants, partners, keys and the policy
//! service). The logic is generic over `platform::Platform`, so native tests run it on
//! `platform::fakes`; on wasm32 `entry` wires it to the runtime.
//!
//! The `worker` attribute macros are never used here; the entry points come from
//! `pylota_identity_platform::export_worker!` ([Rust workspace §2]).
//!
//! [Rust workspace §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#![forbid(unsafe_code)]

pub mod app;
pub mod auth;
pub mod crons;
pub mod ctx;
pub mod db;
pub mod errors;
pub mod handlers;
pub mod keyring;
pub mod log;
pub mod metrics;
pub mod objects;
pub mod policy;
pub mod quota;
pub mod ratelimit;
pub mod request_id;
pub mod router;
pub mod rpc;

#[cfg(test)]
mod tests;

/// The workspace version, or the release tag without its `v` (`cargo xtask build-worker`).
pub const BUILD_VERSION: &str = env!("PM_BUILD_VERSION");
/// `git rev-parse --short=7 HEAD` at build time, `unknown` in native builds.
pub const BUILD_COMMIT: &str = env!("PM_BUILD_COMMIT");
/// The UTC build date, `1970-01-01` in native builds.
pub const BUILD_DATE: &str = env!("PM_BUILD_DATE");

#[cfg(target_arch = "wasm32")]
mod entry {
    use pylota_identity_platform::app::{HttpRequestIn, HttpResponseOut, ScheduledIn};
    use pylota_identity_platform::cf::glue::{CfPlatform, WorkerApp};

    use crate::app::App;

    /// The Worker application for one invocation.
    pub struct Worker(App<CfPlatform>);

    impl WorkerApp for Worker {
        fn new(platform: CfPlatform) -> Self {
            Worker(App::new(platform))
        }

        async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
            self.0.fetch(req).await
        }

        async fn scheduled(&self, ev: ScheduledIn) {
            let request_id = crate::request_id::new(self.0.p.as_ref());
            crate::crons::run(self.0.p.as_ref(), &ev.cron, &request_id).await;
        }
    }

    pylota_identity_platform::export_worker! {
        app: crate::entry::Worker,
        durable_objects: {
            IdentityMailbox => crate::objects::stubs::MailboxObject,
            DomainMonitor => crate::objects::stubs::MonitorObject,
            JobRunner => crate::objects::stubs::RunnerObject,
            TenantQuota => crate::objects::stubs::QuotaObject,
            SesControl => crate::objects::stubs::SesControlObject,
            Notifier => crate::objects::stubs::NotifierObject,
            TenantEvents => crate::objects::stubs::EventsObject { websocket },
        }
    }
}
