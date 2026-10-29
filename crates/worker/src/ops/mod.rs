//! Operations (Observability § 5, Security § 6.2): the state alert evaluator and the master-key re-seal
//! sweep, both run by the every-minute cron.

pub mod alerts;
pub mod reseal;
