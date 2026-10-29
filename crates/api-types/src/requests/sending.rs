//! Request bodies of the `sending` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SendRequestKind {
    #[serde(rename = "transactional")]
    Transactional,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "auto_reply")]
    AutoReply,
}

fn schema_send_request_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "transactional", "enum": ["transactional", "marketing", "auto_reply"], "type": "string"}"###).expect("the contract's schema")
}

/// A new message. At most `policy.max_recipients` (default 10, hard maximum 49) across `to`, `cc` and `bcc` (`400 too_many_recipients`); duplicates are removed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct SendRequest {
    pub to: Vec<Recipient>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub cc: Option<Vec<Recipient>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub bcc: Option<Vec<Recipient>>,
    pub subject: String,
    /// Plain-text body. Derived from `html` when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub text: Option<String>,
    /// HTML body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub html: Option<String>,
    /// At most 32, each inline or from an upload of the same identity; 26,214,400 decoded bytes in total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<SendAttachment>)]
    pub attachments: Option<Vec<SendAttachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_send_request_kind_property)]
    pub kind: Option<SendRequestKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Unsubscribe)]
    pub unsubscribe: Option<Unsubscribe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Consent)]
    pub consent: Option<Consent>,
    /// Continue this thread without quoting; `References` are set from the thread.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<ThreadId>)]
    pub thread_id: Option<Option<ThreadId>>,
    /// An `active` address of the identity, or a `retiring` one on a thread that already uses it ([G7]; with `thread_id`). Otherwise `400 invalid_request` with `details.errors[0].path = "from_address"`. Default the primary.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<EmailAddress>)]
    pub from_address: Option<Option<EmailAddress>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SendHeaders)]
    pub headers: Option<SendHeaders>,
    /// Sends iMIP (docs › Calendar invitations § 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = CalendarSendInput)]
    pub calendar: Option<CalendarSendInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Metadata)]
    pub metadata: Option<Metadata>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ReplyRequestKind {
    #[serde(rename = "transactional")]
    Transactional,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "auto_reply")]
    AutoReply,
}

fn schema_reply_request_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "transactional", "enum": ["transactional", "marketing", "auto_reply"], "type": "string"}"###).expect("the contract's schema")
}

/// The body of a reply or reply-all. Recipients, subject, `From`, `In-Reply-To` and `References` are derived from the original message. At least one of `text` and `html` is required. `calendar` is accepted on a reply and refused on a reply-all (`400 invalid_request`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ReplyRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub html: Option<String>,
    /// At most 32, each inline or from an upload of the same identity; 26,214,400 decoded bytes in total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<SendAttachment>)]
    pub attachments: Option<Vec<SendAttachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_reply_request_kind_property)]
    pub kind: Option<ReplyRequestKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Unsubscribe)]
    pub unsubscribe: Option<Unsubscribe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Consent)]
    pub consent: Option<Consent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SendHeaders)]
    pub headers: Option<SendHeaders>,
    /// Sends iMIP on a reply (docs › Calendar invitations § 5). Refused on a reply-all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = CalendarSendInput)]
    pub calendar: Option<CalendarSendInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Metadata)]
    pub metadata: Option<Metadata>,
}

/// The body of a forward. The original message is attached with its `References` kept. Recipients count against `policy.max_recipients` as for a new message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ForwardRequest {
    pub to: Vec<Recipient>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub cc: Option<Vec<Recipient>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Recipient>)]
    pub bcc: Option<Vec<Recipient>>,
    /// A note placed above the forwarded message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub html: Option<String>,
    /// Attach the original's attachments (never those with a `risk`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub include_attachments: Option<bool>,
    /// Extra attachments, at most 32 as on a send, each inline or from an upload of the same identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<SendAttachment>)]
    pub attachments: Option<Vec<SendAttachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SendHeaders)]
    pub headers: Option<SendHeaders>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Metadata)]
    pub metadata: Option<Metadata>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum ResolveRequestOutcome {
    #[serde(rename = "sent")]
    #[default]
    Sent,
    #[serde(rename = "not_sent")]
    NotSent,
}

fn schema_resolve_request_outcome_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`sent` moves the message and its uncertain deliveries to `submitted` and emits `message.sent`; `not_sent` marks the message `failed` with reason `resolved_not_sent`.", "enum": ["sent", "not_sent"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ResolveRequest {
    /// `sent` moves the message and its uncertain deliveries to `submitted` and emits `message.sent`; `not_sent` marks the message `failed` with reason `resolved_not_sent`.
    #[schema(schema_with = schema_resolve_request_outcome_property)]
    pub outcome: ResolveRequestOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum CalendarReplyRequestPartstat {
    #[serde(rename = "accepted")]
    #[default]
    Accepted,
    #[serde(rename = "declined")]
    Declined,
    #[serde(rename = "tentative")]
    Tentative,
}

fn schema_calendar_reply_request_partstat_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["accepted", "declined", "tentative"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct CalendarReplyRequest {
    #[schema(schema_with = schema_calendar_reply_request_partstat_property)]
    pub partstat: CalendarReplyRequestPartstat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub comment: Option<String>,
    /// Answers one instance, in the form of the invitation's DTSTART: an RFC 3339 instant for a UTC or zoned date-time, a local `YYYY-MM-DDTHH:MM:SS` without offset for a floating one, `YYYY-MM-DD` for a date. An invitation whose zone is unknown cannot be answered per instance (`400 invalid_request`, `instance_zone_unknown`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub recurrence_id: Option<Option<String>>,
}
