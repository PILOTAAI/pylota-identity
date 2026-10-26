//! The Rust client for the Pylota Identity REST API, the event stream and the agent-assertion verifier
//! (build plan M16, [Rust workspace §11]).
//!
//! [Rust workspace §11]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    /// The crate builds and its test harness runs (build plan M0).
    #[test]
    fn workspace_member_builds() {}
}
