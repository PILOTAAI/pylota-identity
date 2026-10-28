//! Cron triggers (Configuration › Cron triggers). M5 runs the `rpc_intents` re-send every minute; the
//! other steps arrive with their milestones.

pub mod intents;

use pylota_identity_platform::Platform;

/// One `scheduled` invocation.
pub async fn run<P: Platform>(p: &P, cron: &str, request_id: &str) {
    if cron == "* * * * *" {
        intents::resend(p, request_id).await;
    }
}
