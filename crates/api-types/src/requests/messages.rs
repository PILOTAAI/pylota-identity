//! Request bodies of the `messages` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_add: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_remove: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub read: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BatchGetRequest {
    pub message_ids: BatchMessageIds,
    /// `extracted_text` of each message is cut at a UTF-8 character boundary to this many bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub extracted_text_bytes: Option<i64>,
}

/// At least one of `labels_add`, `labels_remove`, `read` and `archived` is required (checked by the service).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BatchUpdateRequest {
    pub message_ids: BatchMessageIds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_add: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_remove: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub read: Option<bool>,
    /// Sets the archived flag of each thread that holds a listed message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub archived: Option<bool>,
    /// A message carrying any of these labels is not changed and is listed in `conflicts`. Needs `labels_add` or `labels_remove`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub require_absent: Option<Vec<Label>>,
}
