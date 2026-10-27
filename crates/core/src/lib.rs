//! Pure logic for Pylota Identity: MIME parsing, sanitising, threading, verdicts and every other rule
//! that can be expressed without effects ([Rust workspace §2]). No I/O, no clock, no randomness, and
//! no `worker` dependency; it builds for the host and for `wasm32-unknown-unknown`.
//!
//! [Rust workspace §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#![forbid(unsafe_code)]

pub mod httpsig;
pub mod jwk;
pub mod ses;
pub mod sns;

#[cfg(test)]
mod tests {
    /// The crate builds and its test harness runs (build plan M0).
    #[test]
    fn workspace_member_builds() {}
}
