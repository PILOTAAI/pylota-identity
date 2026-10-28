//! Request bodies of the `screening` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScreeningFeedbackRequest {
    /// Must fit the record's status (`400 invalid_request`, `details.reason = "feedback_not_applicable"`).
    pub verdict: ScreeningFeedbackVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub notes: Option<String>,
}
