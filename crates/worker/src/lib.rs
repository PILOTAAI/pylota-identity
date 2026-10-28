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
pub mod consumers;
pub mod crons;
pub mod ctx;
pub mod db;
pub mod email;
pub mod errors;
pub mod handlers;
#[cfg(feature = "itest-hooks")]
pub mod hooks;
pub mod intents;
pub mod jobs;
pub mod keyring;
pub mod log;
pub mod mailbox;
pub mod metrics;
pub mod objects;
pub mod policy;
pub mod quota;
pub mod ratelimit;
pub mod request_id;
pub mod router;
pub mod rpc;
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
            self.0.fetch(req).await
        }

        async fn email(
            &self,
            msg: pylota_identity_platform::app::EmailIn,
        ) -> pylota_identity_platform::app::EmailOutcome {
            crate::email::handle(self.0.p.as_ref(), &msg).await
        }

        /// Until M8 consumes `pm-webhooks`, its messages wait in the queue. The itest build hands
        /// them, and those of two dead-letter queues, to the fake server instead, so the suite can see them
        /// (Testing § 6.2).
        async fn queue(
            &self,
            queue: pylota_identity_platform::queues::QueueName,
            dead_letter: bool,
            batch: Vec<pylota_identity_platform::queues::Incoming<serde_json::Value>>,
        ) {
            #[cfg(feature = "itest-hooks")]
            // `pm-inbound-dlq` and `pm-index-dlq` too: J7's test redrives a dead-lettered pointer,
            // and the index tests check that nothing was dead-lettered.
            if (queue == pylota_identity_platform::queues::QueueName::Webhooks && !dead_letter)
                || (dead_letter
                    && matches!(
                        queue,
                        pylota_identity_platform::queues::QueueName::Inbound
                            | pylota_identity_platform::queues::QueueName::Index
                    ))
            {
                crate::hooks::capture_queue(self.0.p.as_ref(), queue, dead_letter, batch).await;
                return;
            }
            crate::consumers::run(self.0.p.as_ref(), queue, dead_letter, batch).await;
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
