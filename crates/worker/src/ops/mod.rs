//! Operations (Observability § 5, Security § 6.2): the state alert evaluator and the master-key re-seal
//! sweep, both run by the every-minute cron.

pub mod alert_email;
pub mod alerts;
pub mod containment;
pub mod reseal;
pub mod switches;

pub use containment::end_invocation;
