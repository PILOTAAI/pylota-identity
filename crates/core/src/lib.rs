//! Pure logic for Pylota Identity: MIME parsing, sanitising, threading, verdicts and every other rule
//! that can be expressed without effects ([Rust workspace §2]). No I/O, no clock, no randomness, and
//! no `worker` dependency; it builds for the host and for `wasm32-unknown-unknown`.
//!
//! [Rust workspace §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#![forbid(unsafe_code)]

pub mod address;
pub mod attach;
pub mod attach_text;
pub mod auth;
pub mod citations;
pub mod classify;
pub mod compose;
pub mod crypto;
pub mod cursor;
pub mod days;
pub mod dns;
pub mod domain_fsm;
pub mod fusion;
pub mod fuzz;
pub mod httpsig;
pub mod ids;
pub mod inbound;
pub mod injection;
pub mod jobs;
pub mod jwk;
pub mod keys;
pub mod links;
pub mod lists;
pub mod mime;
pub mod policy;
pub mod query;
pub mod quote;
pub mod refs;
pub mod reply;
pub mod sanitize;
pub mod sealed;
pub mod search;
pub mod ses;
pub mod slo;
pub mod sns;
pub mod ssrf;
pub mod standard_webhooks;
pub mod text;
pub mod thread;
pub mod thread_token;
pub mod triage_rules;
pub mod trust;
pub mod visibility;
pub mod zip;

#[cfg(test)]
mod tests {
    /// The crate builds and its test harness runs (build plan M0).
    #[test]
    fn workspace_member_builds() {}
}
