//! Pure logic for Pylota Identity: MIME parsing, sanitising, threading, verdicts and every other rule
//! that can be expressed without effects ([Rust workspace §2]). No I/O, no clock, no randomness, and
//! no `worker` dependency; it builds for the host and for `wasm32-unknown-unknown`.
//!
//! [Rust workspace §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#![forbid(unsafe_code)]

pub mod address;
pub mod attach;
pub mod classify;
pub mod httpsig;
pub mod ids;
pub mod jwk;
pub mod mime;
pub mod policy;
pub mod quote;
pub mod reply;
pub mod sanitize;
pub mod ses;
pub mod sns;
pub mod text;
pub mod thread;
pub mod thread_token;

#[cfg(test)]
mod tests {
    /// The crate builds and its test harness runs (build plan M0).
    #[test]
    fn workspace_member_builds() {}
}
