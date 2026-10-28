//! Objects first used by the `uploads` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum UploadStatusReason {
    #[serde(rename = "risk")]
    Risk,
    #[serde(rename = "infected")]
    Infected,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "too_many_attempts")]
    TooManyAttempts,
    #[serde(rename = "object_missing")]
    ObjectMissing,
}

fn schema_upload_status_reason_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["risk", "infected", "expired", "too_many_attempts", "object_missing", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum UploadScanStatus {
    #[serde(rename = "skipped")]
    Skipped,
    #[serde(rename = "clean")]
    Clean,
    #[serde(rename = "infected")]
    Infected,
    #[serde(rename = "error")]
    Error,
}

fn schema_upload_scan_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["skipped", "clean", "infected", "error"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Upload {
    pub id: UploadId,
    pub identity_id: IdentityId,
    /// Sanitised. Untrusted content.
    pub filename: String,
    pub content_type: String,
    pub size: i64,
    pub sha256: String,
    pub status: UploadStatus,
    #[schema(required = true, schema_with = schema_upload_status_reason_property)]
    pub status_reason: Option<UploadStatusReason>,
    #[schema(required = true)]
    pub sniffed_type: Option<String>,
    #[schema(required = true)]
    pub risk: Option<AttachmentRisk>,
    #[schema(schema_with = schema_upload_scan_status_property)]
    pub scan_status: UploadScanStatus,
    pub use_count: i64,
    #[schema(required = true)]
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    /// Only in the create response and in `getUpload` while `pending` or `uploading`. A bearer URL; valid until `upload_url_expires_at`.
    #[schema(required = true)]
    pub upload_url: Option<String>,
    #[schema(required = true)]
    pub upload_url_expires_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UploadPage {
    pub data: Vec<Upload>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
