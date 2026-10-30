//! The golden set for the quality gates (build plan M18, Testing § 9): the generated mailbox, the
//! labelled queries, questions and triage set, and the metrics they are scored with.

pub mod generator;
pub mod metrics;
pub mod policy;
pub mod sets;

/// `crates/conformance/golden/`.
pub fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden")
}
