//! The Cloudflare side of the platform, compiled for wasm32 only.

pub mod externs;
pub mod glue;
#[cfg(feature = "spike-s1")]
pub mod spike_s1;
