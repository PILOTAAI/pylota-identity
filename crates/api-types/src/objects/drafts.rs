//! Objects first used by the `drafts` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Fixed at creation from `in_reply_to`, `reply_all` and `forward_of`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftKind {
    #[serde(rename = "new")]
    New,
    #[serde(rename = "reply")]
    Reply,
    #[serde(rename = "reply_all")]
    ReplyAll,
    #[serde(rename = "forward")]
    Forward,
}

/// `requested` by the writer, or the policy level that needed approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftApprovalReason {
    #[serde(rename = "requested")]
    Requested,
    #[serde(rename = "new_recipients")]
    NewRecipients,
    #[serde(rename = "external")]
    External,
    #[serde(rename = "always")]
    Always,
}

/// `failure.reason` of a `failed` draft: the error code the send would have returned for a refusal that is not transient (every code the send pipeline can return for a stored draft is in this list), or `authority_revoked` (the key or person that scheduled it can no longer send), `sender_address_unavailable` (its explicit `from_address` is no longer usable), `send_interrupted` (three attempts died before the message was stored) or `send_window_missed` (not sent within an hour of `send_at`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftFailureReason {
    #[serde(rename = "identity_paused")]
    IdentityPaused,
    #[serde(rename = "tenant_suspended")]
    TenantSuspended,
    #[serde(rename = "partner_suspended")]
    PartnerSuspended,
    #[serde(rename = "sending_paused")]
    SendingPaused,
    #[serde(rename = "identity_owner_required")]
    IdentityOwnerRequired,
    #[serde(rename = "billing_limit")]
    BillingLimit,
    #[serde(rename = "daily_cap_reached")]
    DailyCapReached,
    #[serde(rename = "authority_revoked")]
    AuthorityRevoked,
    #[serde(rename = "sender_address_unavailable")]
    SenderAddressUnavailable,
    #[serde(rename = "domain_not_ready")]
    DomainNotReady,
    #[serde(rename = "transport_unavailable")]
    TransportUnavailable,
    #[serde(rename = "test_mode_recipient")]
    TestModeRecipient,
    #[serde(rename = "loop_detected")]
    LoopDetected,
    #[serde(rename = "auto_reply_not_allowed")]
    AutoReplyNotAllowed,
    #[serde(rename = "message_too_large")]
    MessageTooLarge,
    #[serde(rename = "thread_not_found")]
    ThreadNotFound,
    #[serde(rename = "source_unavailable")]
    SourceUnavailable,
    #[serde(rename = "identity_not_found")]
    IdentityNotFound,
    #[serde(rename = "too_many_recipients")]
    TooManyRecipients,
    #[serde(rename = "policy_denied")]
    PolicyDenied,
    #[serde(rename = "attachment_risky")]
    AttachmentRisky,
    #[serde(rename = "calendar_invalid")]
    CalendarInvalid,
    #[serde(rename = "calendar_too_large")]
    CalendarTooLarge,
    #[serde(rename = "calendar_organizer_mismatch")]
    CalendarOrganizerMismatch,
    #[serde(rename = "calendar_attendee_mismatch")]
    CalendarAttendeeMismatch,
    #[serde(rename = "calendar_recipient_mismatch")]
    CalendarRecipientMismatch,
    #[serde(rename = "calendar_sequence_stale")]
    CalendarSequenceStale,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "address_invalid")]
    AddressInvalid,
    #[serde(rename = "address_unsupported")]
    AddressUnsupported,
    #[serde(rename = "header_not_allowed")]
    HeaderNotAllowed,
    #[serde(rename = "marketing_requirements_missing")]
    MarketingRequirementsMissing,
    #[serde(rename = "send_interrupted")]
    SendInterrupted,
    #[serde(rename = "send_window_missed")]
    SendWindowMissed,
}

/// Inline (`filename`, `content_type`, `content_base64`) or a reference to a `ready` upload of the same identity (`upload_id`), whose bytes are copied when the revision is saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum DraftAttachmentInput {
    AttachmentInput(AttachmentInput),
    AttachmentUploadInput(AttachmentUploadInput),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftAttachmentDisposition {
    #[serde(rename = "attachment")]
    Attachment,
    #[serde(rename = "inline")]
    Inline,
}

fn schema_draft_attachment_disposition_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["attachment", "inline"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftAttachmentSource {
    #[serde(rename = "inline")]
    Inline,
    #[serde(rename = "upload")]
    Upload,
}

fn schema_draft_attachment_source_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`inline` (bytes given as base64) or `upload` (copied from an upload when the revision was saved).", "enum": ["inline", "upload"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftAttachment {
    pub id: AttachmentId,
    /// Sanitised. Untrusted content.
    pub filename: String,
    pub content_type: String,
    #[schema(schema_with = schema_draft_attachment_disposition_property)]
    pub disposition: DraftAttachmentDisposition,
    #[schema(required = true)]
    pub content_id: Option<String>,
    pub size: i64,
    pub sha256: String,
    /// `inline` (bytes given as base64) or `upload` (copied from an upload when the revision was saved).
    #[schema(schema_with = schema_draft_attachment_source_property)]
    pub source: DraftAttachmentSource,
    /// The upload the bytes were copied from; `null` for inline parts. It stays after the upload is gone.
    #[schema(required = true)]
    pub upload_id: Option<UploadId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftKind2 {
    #[serde(rename = "transactional")]
    Transactional,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "auto_reply")]
    AutoReply,
}

fn schema_draft_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["transactional", "marketing", "auto_reply"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftApprovalState {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "requested")]
    Requested,
    #[serde(rename = "approved")]
    Approved,
}

fn schema_draft_approval_state_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["none", "requested", "approved"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftApproval {
    #[schema(schema_with = schema_draft_approval_state_property)]
    pub state: DraftApprovalState,
    #[schema(required = true)]
    pub reason: Option<DraftApprovalReason>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub requested_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub requested_by_key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub approved_revision: Option<Option<i64>>,
    /// The digest the approval covers; a send needs it to equal the current digest.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub approved_digest: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub approved_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub approved_by_key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub approved_by_user_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub on_behalf_of: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub note: Option<Option<String>>,
}

fn schema_draft_approval_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"approved_at": {"format": "date-time", "type": ["string", "null"]}, "approved_by_key_id": {"type": ["string", "null"]}, "approved_by_user_id": {"type": ["string", "null"]}, "approved_digest": {"description": "The digest the approval covers; a send needs it to equal the current digest.", "type": ["string", "null"]}, "approved_revision": {"type": ["integer", "null"]}, "note": {"type": ["string", "null"]}, "on_behalf_of": {"type": ["string", "null"]}, "reason": {"oneOf": [{"$ref": "#/components/schemas/DraftApprovalReason"}, {"type": "null"}]}, "requested_at": {"format": "date-time", "type": ["string", "null"]}, "requested_by_key_id": {"type": ["string", "null"]}, "state": {"enum": ["none", "requested", "approved"], "type": "string"}}, "required": ["state", "reason"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftSendOrigin {
    #[serde(rename = "api")]
    Api,
    #[serde(rename = "approval")]
    Approval,
    #[serde(rename = "console")]
    Console,
}

fn schema_draft_send_origin_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["api", "approval", "console"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// The pending or last send request; `null` when none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftSend {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_draft_send_origin_property)]
    pub origin: Option<DraftSendOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub requested_at: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub requested_by_key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub requested_by_user_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub due_at: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_add: Option<Vec<Label>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_remove: Option<Vec<Label>>,
}

fn schema_draft_send_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "The pending or last send request; `null` when none.", "properties": {"due_at": {"format": "date-time", "type": ["string", "null"]}, "labels_add": {"items": {"$ref": "#/components/schemas/Label"}, "type": "array"}, "labels_remove": {"items": {"$ref": "#/components/schemas/Label"}, "type": "array"}, "origin": {"enum": ["api", "approval", "console"], "type": "string"}, "requested_at": {"format": "date-time", "type": "string"}, "requested_by_key_id": {"type": ["string", "null"]}, "requested_by_user_id": {"type": ["string", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftRejectionReason {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "expired")]
    Expired,
}

fn schema_draft_rejection_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["operator", "expired"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// Set while `rejected`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftRejection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_draft_rejection_reason_property)]
    pub reason: Option<DraftRejectionReason>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub note: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub rejected_at: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub rejected_by_key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub rejected_by_user_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub on_behalf_of: Option<Option<String>>,
}

fn schema_draft_rejection_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Set while `rejected`.", "properties": {"note": {"type": ["string", "null"]}, "on_behalf_of": {"type": ["string", "null"]}, "reason": {"enum": ["operator", "expired"], "type": "string"}, "rejected_at": {"format": "date-time", "type": "string"}, "rejected_by_key_id": {"type": ["string", "null"]}, "rejected_by_user_id": {"type": ["string", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

/// Set while `failed`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftFailure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = DraftFailureReason)]
    pub reason: Option<DraftFailureReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub detail: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub failed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub attempts: Option<i64>,
}

fn schema_draft_failure_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Set while `failed`.", "properties": {"attempts": {"minimum": 1, "type": "integer"}, "detail": {"type": "object"}, "failed_at": {"format": "date-time", "type": "string"}, "reason": {"$ref": "#/components/schemas/DraftFailureReason"}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

/// A draft (docs › Drafts, approvals and scheduled send § 2). Every text field was written by an agent that mail may have steered: it is untrusted content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Draft {
    pub id: DraftId,
    pub tenant_id: TenantId,
    pub identity_id: IdentityId,
    #[schema(required = true)]
    pub client_id: Option<String>,
    pub draft_kind: DraftKind,
    #[schema(schema_with = schema_draft_kind_property)]
    pub kind: DraftKind2,
    pub status: DraftStatus,
    pub revision: i64,
    /// The entity tag of the current revision, `"{revision}-{8 hex}"`.
    pub etag: String,
    /// The content digest of the current revision; `null` once the content is cleared.
    #[schema(required = true)]
    pub digest: Option<String>,
    #[schema(required = true)]
    pub in_reply_to: Option<MessageId>,
    #[schema(required = true)]
    pub forward_of: Option<MessageId>,
    #[schema(required = true)]
    pub thread_id: Option<ThreadId>,
    #[schema(required = true)]
    pub from_address: Option<EmailAddress>,
    pub to: Vec<MailboxAddress>,
    pub cc: Vec<MailboxAddress>,
    pub bcc: Vec<MailboxAddress>,
    #[schema(required = true)]
    pub subject: Option<String>,
    #[schema(required = true)]
    pub text: Option<String>,
    #[schema(required = true)]
    pub html: Option<String>,
    /// Lists only, where `text` and `html` are `null`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub preview: Option<Option<String>>,
    pub attachments: Vec<DraftAttachment>,
    #[schema(required = true)]
    pub include_attachments: Option<bool>,
    #[schema(required = true)]
    pub calendar: Option<CalendarSendInput>,
    pub headers: BTreeMap<String, String>,
    #[schema(required = true)]
    pub unsubscribe: Option<Unsubscribe>,
    #[schema(required = true)]
    pub consent: Option<Consent>,
    pub labels: Vec<Label>,
    pub metadata: Metadata,
    #[schema(required = true)]
    pub send_at: Option<String>,
    #[schema(schema_with = schema_draft_approval_property)]
    pub approval: DraftApproval,
    /// The pending or last send request; `null` when none.
    #[schema(required = true, schema_with = schema_draft_send_property)]
    pub send: Option<DraftSend>,
    /// Set while `rejected`.
    #[schema(required = true, schema_with = schema_draft_rejection_property)]
    pub rejection: Option<DraftRejection>,
    /// Set while `failed`.
    #[schema(required = true, schema_with = schema_draft_failure_property)]
    pub failure: Option<DraftFailure>,
    /// Set when the draft became a message.
    #[schema(required = true)]
    pub message_id: Option<MessageId>,
    #[schema(required = true)]
    pub sent_at: Option<String>,
    #[schema(required = true)]
    pub created_by_key_id: Option<String>,
    /// The key that wrote the current revision (`null` for a console write).
    #[schema(required = true)]
    pub updated_by_key_id: Option<String>,
    /// The key that last changed the content: the create, or the last `PATCH` that changed the digest. It may not approve the draft (`self_approval`); a change of only `send_at`, `labels` or `metadata` leaves it.
    #[schema(required = true)]
    pub content_by_key_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Present on write responses; `true` on an idempotent replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub deduplicated: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftDryRunOutcome {
    #[serde(rename = "send")]
    Send,
    #[serde(rename = "schedule")]
    Schedule,
    #[serde(rename = "pending_approval")]
    PendingApproval,
}

fn schema_draft_dry_run_outcome_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"enum": ["send", "schedule", "pending_approval"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

/// The result of a draft send's dry run (`dry_run=true`). Nothing was sent, scheduled or stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftDryRun {
    /// `false` only for `pending_approval`.
    pub would_send: bool,
    #[schema(schema_with = schema_draft_dry_run_outcome_property)]
    pub outcome: DraftDryRunOutcome,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<DraftApprovalReason>)]
    pub approval_reason: Option<Option<DraftApprovalReason>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub send_at: Option<Option<String>>,
    /// Each recipient as a real send would treat it (after an approval, for `pending_approval`).
    pub recipients: Vec<DryRunRecipient>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftIndexEntryStatus {
    #[serde(rename = "pending_approval")]
    PendingApproval,
    #[serde(rename = "scheduled")]
    Scheduled,
    #[serde(rename = "failed")]
    Failed,
}

fn schema_draft_index_entry_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["pending_approval", "scheduled", "failed"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

/// An entry of the tenant's queue of drafts. No content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftIndexEntry {
    pub draft_id: DraftId,
    pub identity_id: IdentityId,
    #[schema(schema_with = schema_draft_index_entry_status_property)]
    pub status: DraftIndexEntryStatus,
    pub draft_kind: DraftKind,
    pub revision: i64,
    #[schema(required = true)]
    pub approval_reason: Option<DraftApprovalReason>,
    pub recipient_count: i64,
    pub attachment_count: i64,
    #[schema(required = true)]
    pub send_at: Option<String>,
    pub status_at: String,
    #[schema(required = true)]
    pub failure_reason: Option<DraftFailureReason>,
    #[schema(required = true)]
    pub requested_by_key_id: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftPage {
    pub data: Vec<Draft>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftIndexPage {
    pub data: Vec<DraftIndexEntry>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
