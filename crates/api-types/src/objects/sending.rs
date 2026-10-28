//! Objects first used by the `sending` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// A recipient, as a bare address or as an object with a display name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum Recipient {
    EmailAddress(EmailAddress),
    Variant2 {
        address: EmailAddress,
        /// Display name. No CR or LF.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schema(value_type = String)]
        name: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AttachmentInputDisposition {
    #[serde(rename = "attachment")]
    Attachment,
    #[serde(rename = "inline")]
    Inline,
}

fn schema_attachment_input_disposition_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"default": "attachment", "enum": ["attachment", "inline"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

/// An inline attachment, its bytes in base64. For a large file, create an upload and attach it by `upload_id` instead (`AttachmentUploadInput`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AttachmentInput {
    pub filename: String,
    pub content_type: String,
    /// The file bytes, base64-encoded.
    pub content_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_attachment_input_disposition_property)]
    pub disposition: Option<AttachmentInputDisposition>,
    /// The Content-ID for an `inline` attachment referenced as `cid:` from the HTML body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub content_id: Option<String>,
}

/// One-click unsubscribe (RFC 8058) for marketing mail. Sets the List-Unsubscribe headers and a visible link.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Unsubscribe {
    pub url: String,
    /// An unsubscribe mailbox, for the `mailto:` form of List-Unsubscribe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub mailto: Option<String>,
}

/// The tenant's attestation of the recipient's consent to marketing mail.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Consent {
    pub basis: String,
    pub recorded_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SendHeadersImportance {
    #[serde(rename = "high")]
    High,
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "low")]
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SendHeadersPriority {
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "non-urgent")]
    NonUrgent,
    #[serde(rename = "urgent")]
    Urgent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SendHeadersSensitivity {
    #[serde(rename = "personal")]
    Personal,
    #[serde(rename = "private")]
    Private,
    #[serde(rename = "company-confidential")]
    CompanyConfidential,
}

/// Custom headers, checked when the request arrives, never later at the transport. Names are matched case-insensitively, as Cloudflare matches them. A name must be an `X-` name matching `^X-[A-Za-z0-9_-]+$`, the prefix in either case (at most 100 bytes, sent as given; `X-Pylota-*` and `X-AI-Generated` are reserved in any case), or one of `Importance`, `Priority`, `Sensitivity`, `Keywords`, `Comments` and `Organization` in any case, which is sent in that casing; any other name is `400 header_not_allowed` (the schema leaves names unconstrained, so that this code and not a generic `invalid_request` is returned). Two names that differ only in case are `400 invalid_request`. The enums below are listed under the canonical names and apply to every casing. `Importance` takes only `high`, `normal` or `low`, `Priority` only `normal`, `non-urgent` or `urgent`, and `Sensitivity` only `personal`, `private` or `company-confidential`; another value, an empty value or one with CR or LF is `400 invalid_request`. Everything else is set by the service. At most 16 KB in total; each value at most 2,048 bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SendHeaders {
    /// Every other entry.
    #[serde(flatten)]
    pub extra: BTreeMap<String, String>,
    #[serde(rename = "Importance")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<SendHeadersImportance>,
    #[serde(rename = "Priority")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<SendHeadersPriority>,
    #[serde(rename = "Sensitivity")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitivity: Option<SendHeadersSensitivity>,
}

impl utoipa::PartialSchema for SendHeaders {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"additionalProperties": {"maxLength": 2048, "minLength": 1, "pattern": "^[^\\r\\n]+$", "type": "string"}, "description": "Custom headers, checked when the request arrives, never later at the transport. Names are matched case-insensitively, as Cloudflare matches them. A name must be an `X-` name matching `^X-[A-Za-z0-9_-]+$`, the prefix in either case (at most 100 bytes, sent as given; `X-Pylota-*` and `X-AI-Generated` are reserved in any case), or one of `Importance`, `Priority`, `Sensitivity`, `Keywords`, `Comments` and `Organization` in any case, which is sent in that casing; any other name is `400 header_not_allowed` (the schema leaves names unconstrained, so that this code and not a generic `invalid_request` is returned). Two names that differ only in case are `400 invalid_request`. The enums below are listed under the canonical names and apply to every casing. `Importance` takes only `high`, `normal` or `low`, `Priority` only `normal`, `non-urgent` or `urgent`, and `Sensitivity` only `personal`, `private` or `company-confidential`; another value, an empty value or one with CR or LF is `400 invalid_request`. Everything else is set by the service. At most 16 KB in total; each value at most 2,048 bytes.", "examples": [{"X-Booking-Ref": "BK-2291"}], "properties": {"Importance": {"enum": ["high", "normal", "low"], "type": "string"}, "Priority": {"enum": ["normal", "non-urgent", "urgent"], "type": "string"}, "Sensitivity": {"enum": ["personal", "private", "company-confidential"], "type": "string"}}, "type": "object"}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SendHeaders {}

/// The outbound Message object (`direction: "outbound"`, `status: "queued"` on first acceptance) plus `deduplicated`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SendResponse {
    #[serde(flatten)]
    pub base: Message,
    /// `true` when this is the stored response of an earlier request with the same `Idempotency-Key`.
    pub deduplicated: bool,
}

fn schema_dry_run_result_would_send_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Always `true` in a `200`; a send that would be refused returns its error instead.", "enum": [true], "type": "boolean"}"###).expect("the contract's schema")
}

/// The result of a dry run (`dry_run=true`). Nothing was sent or stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DryRunResult {
    /// Always `true` in a `200`; a send that would be refused returns its error instead.
    #[schema(schema_with = schema_dry_run_result_would_send_property)]
    pub would_send: bool,
    /// Each recipient after duplicates are removed, with what a real send would do with it.
    pub recipients: Vec<DryRunRecipient>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DryRunRecipientField {
    #[serde(rename = "to")]
    To,
    #[serde(rename = "cc")]
    Cc,
    #[serde(rename = "bcc")]
    Bcc,
}

fn schema_dry_run_recipient_field_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["to", "cc", "bcc"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DryRunRecipientStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "suppressed")]
    Suppressed,
}

fn schema_dry_run_recipient_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`queued` (it would be sent) or `suppressed` (it would be skipped; see `reason`).", "enum": ["queued", "suppressed"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DryRunRecipientScope {
    #[serde(rename = "identity")]
    Identity,
    #[serde(rename = "tenant")]
    Tenant,
}

fn schema_dry_run_recipient_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Only with `reason: \"send_block\"`: whether the identity's own entry or the tenant's blocked it.", "enum": ["identity", "tenant"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DryRunRecipient {
    pub address: EmailAddress,
    #[schema(schema_with = schema_dry_run_recipient_field_property)]
    pub field: DryRunRecipientField,
    /// `queued` (it would be sent) or `suppressed` (it would be skipped; see `reason`).
    #[schema(schema_with = schema_dry_run_recipient_status_property)]
    pub status: DryRunRecipientStatus,
    /// Only for `suppressed`: the suppression reason (`hard_bounce`, `complaint`, `unsubscribe`, `manual`, `provider`), or `send_block`, `not_on_allowlist` or `unknown_recipient`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub reason: Option<String>,
    /// Only with `reason: "send_block"`: whether the identity's own entry or the tenant's blocked it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_dry_run_recipient_scope_property)]
    pub scope: Option<DryRunRecipientScope>,
}

/// An iCalendar object to send as iMIP (docs › Calendar invitations § 5). It needs exactly one `METHOD`, one `UID`, a `DTSTAMP` in each `VEVENT` and the structure RFC 5546 gives its method (`400 calendar_invalid`). For `PUBLISH`, `REQUEST`, `ADD`, `CANCEL` and `DECLINECOUNTER` the `ORGANIZER` must be the address the message is sent from (`400 calendar_organizer_mismatch`), and for all of them but `PUBLISH`, which has no attendees and may go to anyone, every recipient must be an `ATTENDEE`; for `REPLY`, `REFRESH` and `COUNTER` the replying `ATTENDEE` must be the `From` (`400 calendar_attendee_mismatch`) and the one recipient the `ORGANIZER` (`400 calendar_recipient_mismatch`). `kind` must be `transactional`. A lower `SEQUENCE` than one this identity already sent for the `UID` is `409 calendar_sequence_stale`. A calendar message never falls back to the platform domain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CalendarSendInput {
    /// UTF-8 iCalendar text; the limit is 262,144 bytes, checked by the server (`413 calendar_too_large`).
    pub ics: String,
    /// Attach an `invite.ics` copy. Default `true` for organizer methods, `false` for attendee methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub attach_copy: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AttachmentUploadInputDisposition {
    #[serde(rename = "attachment")]
    Attachment,
    #[serde(rename = "inline")]
    Inline,
}

fn schema_attachment_upload_input_disposition_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"default": "attachment", "enum": ["attachment", "inline"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AttachmentUploadInput {
    pub upload_id: UploadId,
    /// Defaults to the upload's filename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_attachment_upload_input_disposition_property)]
    pub disposition: Option<AttachmentUploadInputDisposition>,
    /// The Content-ID for an `inline` attachment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub content_id: Option<String>,
}

/// An attachment of a send, reply, reply-all or forward, inline or from an upload of the same identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum SendAttachment {
    AttachmentInput(AttachmentInput),
    AttachmentUploadInput(AttachmentUploadInput),
}
