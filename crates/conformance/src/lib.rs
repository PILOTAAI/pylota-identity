//! The MIME conformance corpus, its expected outputs and the RFC conformance runners (build plan M2,
//! Testing § 5). The corpus lives in `corpus/`; `corpus/authoring/make_corpus.py` writes it.
#![forbid(unsafe_code)]

pub mod pipeline;

#[cfg(test)]
mod corpus;
#[cfg(test)]
mod loader;
