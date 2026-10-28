//! Request bodies of the `suppressions` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SuppressionCreateRequest {
    pub address: EmailAddress,
    /// Defaults to `manual`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SuppressionReason)]
    pub reason: Option<SuppressionReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SuppressionDeleteRequest {
    /// Required (`true`) to remove a `complaint` suppression. Audit-logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub confirm_complaint_removal: Option<bool>,
}
