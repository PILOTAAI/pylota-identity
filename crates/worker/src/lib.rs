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
pub mod console;
pub mod consumers;
pub mod crons;
pub mod ctx;
pub mod db;
pub mod domains;
pub mod email;
pub mod errors;
pub mod handlers;
#[cfg(feature = "itest-hooks")]
pub mod hooks;
pub mod inbound;
pub mod intents;
pub mod jobs;
pub mod keyring;
pub mod log;
pub mod mailbox;
pub mod mcp;
pub mod members;
pub mod metrics;
pub mod net;
pub mod objects;
pub mod ops;
pub mod policy;
pub mod quota;
pub mod ratelimit;
pub mod request_id;
pub mod router;
pub mod rpc;
pub mod search;
pub mod send;
pub mod system_mail;
pub mod transport;
pub mod triage;
pub mod webhooks;

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
            let r = self.0.fetch(req).await;
            crate::ops::end_invocation(self.0.p.as_ref()).await;
            r
        }

        async fn email(
            &self,
            msg: pylota_identity_platform::app::EmailIn,
        ) -> pylota_identity_platform::app::EmailOutcome {
            let r = crate::email::handle(self.0.p.as_ref(), &msg).await;
            crate::ops::end_invocation(self.0.p.as_ref()).await;
            r
        }

        /// The itest build taps the messages of two dead-letter queues to the fake server first, so
        /// the suite can redrive a dead-lettered pointer and see that nothing was dead-lettered.
        async fn queue(
            &self,
            queue: pylota_identity_platform::queues::QueueName,
            dead_letter: bool,
            batch: Vec<pylota_identity_platform::queues::Incoming<serde_json::Value>>,
        ) {
            #[cfg(feature = "itest-hooks")]
            if dead_letter
                && matches!(
                    queue,
                    pylota_identity_platform::queues::QueueName::Inbound
                        | pylota_identity_platform::queues::QueueName::Index
                )
            {
                let name = format!("{}-dlq", queue.queue_name());
                crate::hooks::tap_queue(self.0.p.as_ref(), &name, &batch).await;
            }
            crate::consumers::run(self.0.p.as_ref(), queue, dead_letter, batch).await;
            crate::ops::end_invocation(self.0.p.as_ref()).await;
        }

        async fn scheduled(&self, ev: ScheduledIn) {
            let request_id = crate::request_id::new(self.0.p.as_ref());
            crate::crons::run(self.0.p.as_ref(), &ev.cron, &request_id).await;
            crate::ops::end_invocation(self.0.p.as_ref()).await;
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
