//! Domains: the Cloudflare API client (M9 builds only the Email Sending suppression lookup of G4; M13
//! owns the module from then on).

pub mod cloudflare_api;
pub mod records;
pub mod ses_api;
pub mod ses_control;
