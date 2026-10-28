//! `JobRunner` (Privacy and erasure § 4) as a stub until M14: it accepts `JobRequest::Start` and runs
//! no step, so a job stays `queued`. M14's runner restarts jobs left `queued` by the every-minute cron.

use pylota_identity_api_types::ErrorCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::ApiError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum JobRequest {
    Start {
        job_id: String,
        kind: String,
        tenant_id: Option<String>,
        params: Value,
        resume_job_id: Option<String>,
    },
}

/// The stub's answer to one envelope: `Start` is accepted, anything else is `unavailable`.
pub fn stub(envelope: &Value) -> Result<Value, ApiError> {
    match serde_json::from_value::<JobRequest>(envelope["req"].clone()) {
        Ok(JobRequest::Start { .. }) => Ok(Value::Null),
        Err(_) => Err(ApiError::new(
            ErrorCode::Unavailable,
            "JobRunner is not built yet.",
        )),
    }
}
