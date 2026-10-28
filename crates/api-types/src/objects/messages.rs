//! Objects first used by the `messages` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Message IDs of one identity. A repeated ID is processed once, at its first position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BatchMessageIds(pub Vec<MessageId>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BatchMessage {
    #[serde(flatten)]
    pub base: Message,
    pub extracted_text_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BatchGetResponse {
    pub data: Vec<BatchMessage>,
    /// Missing, erased, of another identity, or not visible to the key; never told apart.
    pub not_found: Vec<MessageId>,
    /// Found messages left out to keep the body within 1,048,576 bytes; ask for them again.
    pub remaining: Vec<MessageId>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BatchUpdateResponse {
    pub updated: Vec<MessageId>,
    pub unchanged: Vec<MessageId>,
    pub conflicts: Vec<MessageId>,
    pub not_found: Vec<MessageId>,
    pub threads_archived: i64,
    pub threads_unarchived: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AttachmentTextPagesItem {
    pub page: i64,
    pub text: String,
}

fn schema_attachment_text_pages_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "The requested pages (untrusted content). Empty unless `status` is `ready`.", "items": {"properties": {"page": {"minimum": 1, "type": "integer"}, "text": {"type": "string"}}, "required": ["page", "text"], "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AttachmentText {
    pub status: AttachmentTextStatus,
    /// The requested pages (untrusted content). Empty unless `status` is `ready`.
    #[schema(schema_with = schema_attachment_text_pages_property)]
    pub pages: Vec<AttachmentTextPagesItem>,
    #[schema(required = true)]
    pub total_pages: Option<i64>,
    /// `true` when the text was cut at 200 KB.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessagePage {
    pub data: Vec<Message>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
