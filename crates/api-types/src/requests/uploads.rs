//! Request bodies of the `uploads` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct UploadCreateRequest {
    pub filename: String,
    pub content_type: String,
    /// Bytes.
    pub size: i64,
    /// SHA-256 of the bytes, hexadecimal.
    pub sha256: String,
    /// Seconds; capped by the identity's mail retention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub expires_in: Option<i64>,
}
