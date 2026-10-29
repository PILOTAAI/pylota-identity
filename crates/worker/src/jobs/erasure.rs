//! The `erasure` job (Privacy and erasure § 6).

use pylota_identity_platform::Platform;
use pylota_identity_platform::durable::Sql;
use serde_json::{Map, Value, json};

use super::{Ctx, Slice, StepResult};
use crate::errors::ApiError;

pub fn steps(_params: &Value) -> Vec<&'static str> {
    vec!["init"]
}

pub async fn run<P: Platform, S: Sql>(
    _c: &Ctx<'_, P, S>,
    _step: &str,
    _cursor: Option<&str>,
) -> StepResult {
    Ok(Slice::done())
}

pub async fn on_end<P: Platform, S: Sql>(
    _c: &Ctx<'_, P, S>,
    _outcome: &str,
    _counts: &Map<String, Value>,
    _failed: Option<(&str, &str)>,
) -> Result<Value, ApiError> {
    Ok(json!({}))
}
