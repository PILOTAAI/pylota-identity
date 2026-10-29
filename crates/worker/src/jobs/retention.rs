//! The `retention` job (Privacy and erasure). Filled in below.

use pylota_identity_platform::Platform;
use pylota_identity_platform::durable::Sql;
use serde_json::Value;

use super::{Ctx, Slice, StepResult};

pub const STEPS: &[&str] = &["run"];

pub fn steps(_params: &Value) -> Vec<&'static str> {
    STEPS.to_vec()
}

pub async fn run<P: Platform, S: Sql>(
    _c: &Ctx<'_, P, S>,
    _step: &str,
    _cursor: Option<&str>,
) -> StepResult {
    Ok(Slice::done())
}

/// The `*/15` cron's retention scheduling (Privacy § 5.1). Filled in below.
pub async fn schedule<P: Platform>(_p: &P) {}
