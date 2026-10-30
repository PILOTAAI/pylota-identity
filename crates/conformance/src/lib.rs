//! The MIME conformance corpus, its expected outputs and the RFC conformance runners (build plan M2,
//! Testing § 5). The corpus lives in `corpus/`; `corpus/authoring/make_corpus.py` writes it. The golden
//! set of the quality gates is [`golden`] (build plan M18, Testing § 9).
#![forbid(unsafe_code)]

/// The inbound pipeline's pure steps, which live in `core` because the Worker runs them too.
pub use pylota_identity_core::inbound as pipeline;

pub mod golden;

#[cfg(test)]
mod corpus;
#[cfg(test)]
mod loader;
