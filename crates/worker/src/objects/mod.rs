//! The seven Durable Object classes. `TenantQuota` answers every `QuotaRequest` (the M5 stub); the
//! other six are shells that refuse every request until their milestones build them.

#[cfg(target_arch = "wasm32")]
pub mod stubs;
