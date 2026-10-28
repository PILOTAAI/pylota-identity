//! Request bodies of the `drafts` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftCreateRequestKind {
    #[serde(rename = "transactional")]
    Transactional,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "auto_reply")]
    AutoReply,
}

fn schema_draft_create_request_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "transactional", "enum": ["transactional", "marketing", "auto_reply"], "type": "string"}"###).expect("the contract's schema")
}

/// The fields of `SendRequest`, or `in_reply_to` (with `reply_all`) or `forward_of` with the fields of a reply or forward, plus `send_at`, `client_id` and `request_approval`. A field the kind does not take is `400 invalid_request`: `new` takes every content field; `reply` takes `text`, `html`, `attachments`, `kind` (`transactional` or `auto_reply`), `headers`, `calendar`, `labels`, `metadata` and `send_at`; `reply_all` the same without `calendar`; `forward` takes `to`, `cc`, `bcc`, `text`, `html`, `attachments`, `include_attachments`, `from_address`, `headers`, `labels`, `metadata` and `send_at`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftCreateRequest {
    /// Makes the create idempotent per identity. Printable ASCII without `@`; it appears in events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub client_id: Option<String>,
    /// Makes a `reply` draft (with `reply_all: true`, a `reply_all` draft) of this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = MessageId)]
    pub in_reply_to: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub reply_all: Option<bool>,
    /// Makes a `forward` draft of this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = MessageId)]
    pub forward_of: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub to: Option<Vec<Recipient>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub cc: Option<Vec<Recipient>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub bcc: Option<Vec<Recipient>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub subject: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub text: Option<Option<String>>,
    /// Kept as written; sanitised only when shown in the console and when the message is stored.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub html: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<DraftAttachmentInput>)]
    pub attachments: Option<Vec<DraftAttachmentInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_draft_create_request_kind_property)]
    pub kind: Option<DraftCreateRequestKind>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<ThreadId>)]
    pub thread_id: Option<Option<ThreadId>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<EmailAddress>)]
    pub from_address: Option<Option<EmailAddress>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SendHeaders)]
    pub headers: Option<SendHeaders>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Unsubscribe)]
    pub unsubscribe: Option<Unsubscribe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Consent)]
    pub consent: Option<Consent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = CalendarSendInput)]
    pub calendar: Option<CalendarSendInput>,
    /// Forward drafts only. Default `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub include_attachments: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Metadata)]
    pub metadata: Option<Metadata>,
    /// The time the draft should be sent. Setting it alone schedules nothing.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub send_at: Option<Option<String>>,
    /// `true` puts the draft in `pending_approval` at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub request_approval: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftUpdateRequestKind {
    #[serde(rename = "transactional")]
    Transactional,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "auto_reply")]
    AutoReply,
}

fn schema_draft_update_request_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["transactional", "marketing", "auto_reply"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

/// A field left out is unchanged; `null` clears a nullable field. `to`, `cc`, `bcc`, `headers` and `metadata` are replaced whole; attachments change only through `attachments_add` and `attachments_remove`, labels through `labels`, or `labels_add` and `labels_remove`. `client_id`, `in_reply_to`, `reply_all` and `forward_of` cannot change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub to: Option<Vec<Recipient>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub cc: Option<Vec<Recipient>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub bcc: Option<Vec<Recipient>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub subject: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub text: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub html: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_draft_update_request_kind_property)]
    pub kind: Option<DraftUpdateRequestKind>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<ThreadId>)]
    pub thread_id: Option<Option<ThreadId>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<EmailAddress>)]
    pub from_address: Option<Option<EmailAddress>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SendHeaders)]
    pub headers: Option<SendHeaders>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Unsubscribe>)]
    pub unsubscribe: Option<Option<Unsubscribe>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Consent>)]
    pub consent: Option<Option<Consent>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<CalendarSendInput>)]
    pub calendar: Option<Option<CalendarSendInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub include_attachments: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Metadata)]
    pub metadata: Option<Metadata>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub send_at: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub request_approval: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<DraftAttachmentInput>)]
    pub attachments_add: Option<Vec<DraftAttachmentInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<AttachmentId>)]
    pub attachments_remove: Option<Vec<AttachmentId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_add: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_remove: Option<Vec<Label>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftSendRequest {
    /// A time schedules; `null` sends now; left out, the draft's future `send_at` is used, or it is sent now.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub send_at: Option<Option<String>>,
    /// When given, the draft must still be at this revision (`409 draft_revision_mismatch`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_add: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_remove: Option<Vec<Label>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftApproveRequest {
    pub revision: i64,
    /// When given, the revision's content digest must match (`409 draft_revision_mismatch`, `details.reason = "digest"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub digest: Option<String>,
    /// `true` also sends now, or at a future `send_at`; it needs `messages:send` as well.
    pub send: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub note: Option<String>,
    /// API keys only. Your own user's ID, recorded with the decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftRejectRequest {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}
