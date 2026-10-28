//! Counters (Observability § 3): an `event = "metric"` log line and an Analytics Engine data point,
//! indexed by tenant (or `platform`). M17 Foundation adds the catalogue of names and the alert evaluator.

use pylota_identity_platform::metrics::DataPoint;
use pylota_identity_platform::{Metrics as _, Platform};

/// Counts `n` of `name` for `tenant` (or the platform).
pub fn count<P: Platform>(p: &P, name: &'static str, tenant: Option<&str>, n: u32) {
    let index = tenant.unwrap_or("platform").to_owned();
    crate::log::info(
        "metric",
        &[
            ("name", name.into()),
            ("tenant", index.clone().into()),
            ("value", i64::from(n).into()),
        ],
    );
    p.metrics().write(DataPoint {
        indexes: [index],
        blobs: vec![name.to_owned()],
        doubles: vec![f64::from(n)],
    });
}
