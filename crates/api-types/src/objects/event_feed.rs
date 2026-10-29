//! Objects first used by the `event_feed` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Export ID (`exp_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ExportId(pub String);

/// Invitation ID (`inv_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct InvitationId(pub String);

/// The roll-up status of an outbound message. Per-recipient status is in `deliveries`.
///
/// | Status | Meaning | Terminal |
/// |---|---|---|
/// | `queued` | Accepted, waiting for the transport | no |
/// | `submitted` | The transport accepted it; `provider_message_id` is set | no |
/// | `delivered` | Every recipient is delivered | yes |
/// | `deferred` | At least one recipient has a temporary failure and the provider is still retrying | no |
/// | `bounced` | At least one recipient bounced and none remains in flight | yes |
/// | `complained` | A recipient reported spam (can follow `delivered`) | yes |
/// | `rejected` | The transport refused it before sending (validation, policy) | yes |
/// | `failed` | It could not be sent (quota exhausted after retries, or resolved as not sent) | yes |
/// | `uncertain` | The outcome is unknown. It is never resent automatically | until resolved |
/// | `suppressed` | Every recipient is suppressed. Nothing was sent | yes |
/// | `canceled` | Cancelled while queued | yes |
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum OutboundStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "submitted")]
    Submitted,
    #[serde(rename = "delivered")]
    Delivered,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "bounced")]
    Bounced,
    #[serde(rename = "complained")]
    Complained,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "uncertain")]
    Uncertain,
    #[serde(rename = "suppressed")]
    Suppressed,
    #[serde(rename = "canceled")]
    Canceled,
}

/// The thin message summary carried in webhook events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageSummary {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub direction: Direction,
    pub status: MessageStatus,
    #[schema(required = true)]
    pub from: Option<MailboxAddress>,
    pub to: Vec<MailboxAddress>,
    pub cc: Vec<MailboxAddress>,
    #[schema(required = true)]
    pub delivered_to: Option<EmailAddress>,
    /// Inbound only. `true` on exactly one copy when one message reached several identities of the tenant; act only on that copy ([A9]).
    #[schema(required = true)]
    pub is_primary_recipient: Option<bool>,
    #[schema(required = true)]
    pub subject: Option<String>,
    #[schema(required = true)]
    pub sent_at: Option<String>,
    pub received_at: String,
    pub kind: MessageKind,
    pub labels: Vec<Label>,
    #[schema(required = true)]
    pub in_reply_to: Option<String>,
    /// The draft an outbound message was sent from, or `null`.
    #[schema(required = true)]
    pub draft_id: Option<DraftId>,
    pub flags: Vec<MessageFlag>,
}

/// Durable event consumer ID (`evc_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct EventConsumerId(pub String);

/// Opaque event-log cursor. Valid until retention removes the events behind it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct EventCursor(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventFeedItem {
    pub cursor: EventCursor,
    /// The webhook envelope, byte for byte.
    pub event: Event,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventFeedPage {
    pub data: Vec<EventFeedItem>,
    /// Never `null`; pass it as `after` to continue.
    pub next_cursor: EventCursor,
    pub has_more: bool,
    /// Event types the request covers that this key may not see; left out, never refused.
    pub withheld_types: Vec<String>,
    /// `true` only on a request with `since` when retention or the size budget had already removed events appended at or after that time, so the page does not start where the client asked. Erasure never sets it. Always `false` on consumer reads, which take no `since`.
    pub since_truncated: bool,
    /// Only on consumer reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = EventCursor)]
    pub committed_cursor: Option<EventCursor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum EventConsumerState {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "reset_required")]
    ResetRequired,
}

fn schema_event_consumer_state_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"enum": ["active", "expired", "reset_required"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventConsumer {
    pub id: EventConsumerId,
    pub name: String,
    pub tenant_id: TenantId,
    pub owner_key_id: KeyId,
    pub types: Vec<String>,
    #[schema(required = true)]
    pub identity_ids: Option<Vec<IdentityId>>,
    pub committed_cursor: EventCursor,
    #[schema(required = true)]
    pub committed_at: Option<String>,
    #[schema(required = true)]
    pub last_read_at: Option<String>,
    pub positions_behind: i64,
    #[schema(schema_with = schema_event_consumer_state_property)]
    pub state: EventConsumerState,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct StreamToken {
    /// Single use, valid for 60 seconds. A secret.
    pub token: String,
    pub expires_at: String,
    /// The `wss://` URL of `GET /v1/events/stream`, without any credential.
    pub url: String,
    /// The subprotocols to offer, `pylota-identity.events.v1` and `pylota-identity.token.{token}`.
    pub protocols: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SuppressionReason {
    #[serde(rename = "hard_bounce")]
    HardBounce,
    #[serde(rename = "complaint")]
    Complaint,
    #[serde(rename = "unsubscribe")]
    Unsubscribe,
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "provider")]
    Provider,
}

/// The owner is set at workspace creation or by an ownership transfer in the console.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MemberRole {
    #[serde(rename = "owner")]
    Owner,
    #[serde(rename = "admin")]
    Admin,
    #[serde(rename = "member")]
    Member,
    #[serde(rename = "viewer")]
    Viewer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum InvitationRole {
    #[serde(rename = "admin")]
    #[default]
    Admin,
    #[serde(rename = "member")]
    Member,
    #[serde(rename = "viewer")]
    Viewer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventConsumerPage {
    pub data: Vec<EventConsumer>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SmsEventSummaryDirection {
    #[serde(rename = "inbound")]
    Inbound,
    #[serde(rename = "outbound")]
    Outbound,
}

fn schema_sms_event_summary_direction_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["inbound", "outbound"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// The SMS summary in phone mailbox events. Verification codes appear as `[code]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsEventSummary {
    pub id: String,
    pub conversation_id: String,
    /// The number ID (`pnm_`).
    pub number_id: String,
    #[schema(schema_with = schema_sms_event_summary_direction_property)]
    pub direction: SmsEventSummaryDirection,
    pub status: String,
    pub from: String,
    pub to: String,
    /// Untrusted content; absent on `sms.quarantined`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub text: Option<String>,
    pub segments: i64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub client_reference: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub received_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub sent_at: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub flags: Option<Vec<String>>,
}

/// The head of the tenant's hash-chained vault audit after the change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultEventAudit {
    pub seq: i64,
    pub row_hash: String,
}

fn schema_sms_received_data_trust_sender_unverified_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [true], "type": "boolean"}"###)
        .expect("the contract's schema")
}

fn schema_sms_received_data_trust_screening_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsReceivedDataTrust {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_sms_received_data_trust_sender_unverified_property)]
    pub sender_unverified: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_sms_received_data_trust_screening_property)]
    pub screening: Option<Option<serde_json::Value>>,
}

fn schema_sms_received_data_trust_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"screening": {"type": ["object", "null"]}, "sender_unverified": {"enum": [true], "type": "boolean"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsReceivedDataMediaItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub n: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub size: Option<i64>,
}

fn schema_sms_received_data_media_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"content_type": {"type": "string"}, "n": {"type": "integer"}, "size": {"type": "integer"}}, "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsReceivedData {
    pub sms: SmsEventSummary,
    pub conversation_id: String,
    #[schema(schema_with = schema_sms_received_data_trust_property)]
    pub trust: SmsReceivedDataTrust,
    #[schema(schema_with = schema_sms_received_data_media_property)]
    pub media: Vec<SmsReceivedDataMediaItem>,
}

fn schema_sms_quarantined_data_trust_sender_unverified_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [true], "type": "boolean"}"###)
        .expect("the contract's schema")
}

fn schema_sms_quarantined_data_trust_screening_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsQuarantinedDataTrust {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_sms_quarantined_data_trust_sender_unverified_property)]
    pub sender_unverified: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_sms_quarantined_data_trust_screening_property)]
    pub screening: Option<Option<serde_json::Value>>,
}

fn schema_sms_quarantined_data_trust_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"screening": {"type": ["object", "null"]}, "sender_unverified": {"enum": [true], "type": "boolean"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsQuarantinedDataMediaItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub n: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub size: Option<i64>,
}

fn schema_sms_quarantined_data_media_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"content_type": {"type": "string"}, "n": {"type": "integer"}, "size": {"type": "integer"}}, "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SmsQuarantinedDataQuarantineReason {
    #[serde(rename = "account_unapproved")]
    AccountUnapproved,
    #[serde(rename = "otp_unsolicited")]
    OtpUnsolicited,
    #[serde(rename = "steering_suspected")]
    SteeringSuspected,
    #[serde(rename = "screening_unavailable")]
    ScreeningUnavailable,
    #[serde(rename = "risky_attachment")]
    RiskyAttachment,
}

fn schema_sms_quarantined_data_quarantine_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["account_unapproved", "otp_unsolicited", "steering_suspected", "screening_unavailable", "risky_attachment"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsQuarantinedData {
    pub sms: SmsEventSummary,
    pub conversation_id: String,
    #[schema(schema_with = schema_sms_quarantined_data_trust_property)]
    pub trust: SmsQuarantinedDataTrust,
    #[schema(schema_with = schema_sms_quarantined_data_media_property)]
    pub media: Vec<SmsQuarantinedDataMediaItem>,
    #[schema(schema_with = schema_sms_quarantined_data_quarantine_reason_property)]
    pub quarantine_reason: SmsQuarantinedDataQuarantineReason,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsReleasedData {
    pub sms_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub released_by_key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub released_by_user_id: Option<Option<String>>,
}

fn schema_sms_screened_data_screening_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsScreenedData {
    pub sms_id: String,
    pub status: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub quarantine_reason: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_sms_screened_data_screening_property)]
    pub screening: Option<Option<serde_json::Value>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SmsVerificationReceivedDataKind {
    #[serde(rename = "code")]
    Code,
}

fn schema_sms_verification_received_data_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["code"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsVerificationReceivedData {
    pub sms_id: String,
    /// The number ID (`pnm_`).
    pub number_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub sender_hint: Option<String>,
    #[schema(schema_with = schema_sms_verification_received_data_kind_property)]
    pub kind: SmsVerificationReceivedDataKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub account_id: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsSentData {
    pub sms_id: String,
    pub segments: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub sender_used: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub sender_substituted: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsDeliveredData {
    pub sms_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsFailedData {
    pub sms_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsDeliveryUnconfirmedData {
    pub sms_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsUncertainData {
    pub sms_id: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub fix: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsReconciledData {
    pub sms_id: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CallEventData {
    pub call: Call,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneRequestRequestedDataReason {
    #[serde(rename = "created")]
    Created,
    #[serde(rename = "quote_changed")]
    QuoteChanged,
}

fn schema_phone_request_requested_data_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["created", "quote_changed"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestRequestedData {
    pub phone_request: PhoneRequest,
    #[schema(schema_with = schema_phone_request_requested_data_reason_property)]
    pub reason: PhoneRequestRequestedDataReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneRequestApprovedDataApprovedBy {
    #[serde(rename = "person")]
    Person,
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "standing_budget")]
    StandingBudget,
    #[serde(rename = "purchase")]
    Purchase,
}

fn schema_phone_request_approved_data_approved_by_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["person", "key", "standing_budget", "purchase"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestApprovedData {
    pub phone_request: PhoneRequest,
    #[schema(schema_with = schema_phone_request_approved_data_approved_by_property)]
    pub approved_by: PhoneRequestApprovedDataApprovedBy,
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
    #[schema(value_type = Option<i64>)]
    pub budget_version: Option<Option<i64>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneRequestRejectedDataReason {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "expired")]
    Expired,
}

fn schema_phone_request_rejected_data_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["operator", "expired"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestRejectedData {
    pub phone_request: PhoneRequest,
    #[schema(schema_with = schema_phone_request_rejected_data_reason_property)]
    pub reason: PhoneRequestRejectedDataReason,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestCanceledData {
    pub phone_request: PhoneRequest,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestCompletedData {
    pub phone_request: PhoneRequest,
    #[schema(value_type = Object)]
    pub result: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestFailedData {
    pub phone_request: PhoneRequest,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestUncertainData {
    pub phone_request: PhoneRequest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberStatusChangedDataProvisioning {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub status: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub code: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub retryable: Option<bool>,
}

fn schema_phone_number_status_changed_data_provisioning_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"code": {"type": ["string", "null"]}, "retryable": {"type": "boolean"}, "status": {"type": "string"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberStatusChangedData {
    /// The number ID (`pnm_`).
    pub number_id: String,
    pub identity_id: String,
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub setup_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_number_status_changed_data_provisioning_property)]
    pub provisioning: Option<PhoneNumberStatusChangedDataProvisioning>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub reason: Option<Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberCapabilityChangedDataCapability {
    #[serde(rename = "outbound_sms")]
    OutboundSms,
    #[serde(rename = "inbound_sms")]
    InboundSms,
    #[serde(rename = "inbound_calls")]
    InboundCalls,
    #[serde(rename = "outbound_calls")]
    OutboundCalls,
}

fn schema_phone_number_capability_changed_data_capability_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["outbound_sms", "inbound_sms", "inbound_calls", "outbound_calls"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberCapabilityChangedDataFrom {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "activating")]
    Activating,
    #[serde(rename = "registration_required")]
    RegistrationRequired,
    #[serde(rename = "not_available")]
    NotAvailable,
}

fn schema_phone_number_capability_changed_data_from_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["enabled", "activating", "registration_required", "not_available"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberCapabilityChangedDataTo {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "activating")]
    Activating,
    #[serde(rename = "registration_required")]
    RegistrationRequired,
    #[serde(rename = "not_available")]
    NotAvailable,
}

fn schema_phone_number_capability_changed_data_to_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["enabled", "activating", "registration_required", "not_available"], "type": "string"}"###).expect("the contract's schema")
}

fn schema_phone_number_capability_changed_data_next_step_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberCapabilityChangedData {
    /// The number ID (`pnm_`).
    pub number_id: String,
    pub identity_id: String,
    #[schema(schema_with = schema_phone_number_capability_changed_data_capability_property)]
    pub capability: PhoneNumberCapabilityChangedDataCapability,
    #[schema(schema_with = schema_phone_number_capability_changed_data_from_property)]
    pub from: PhoneNumberCapabilityChangedDataFrom,
    #[schema(schema_with = schema_phone_number_capability_changed_data_to_property)]
    pub to: PhoneNumberCapabilityChangedDataTo,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub reason: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_number_capability_changed_data_next_step_property)]
    pub next_step: Option<Option<serde_json::Value>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberExpiringData {
    /// The number ID (`pnm_`).
    pub number_id: String,
    pub identity_id: String,
    pub release_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberActionNeededDataActionNeeded {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub kind: Option<String>,
    /// Untrusted text from the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub message: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub deadline_at: Option<Option<String>>,
}

fn schema_phone_number_action_needed_data_action_needed_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"deadline_at": {"format": "date-time", "type": ["string", "null"]}, "kind": {"type": "string"}, "message": {"description": "Untrusted text from the provider.", "type": "string"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberActionNeededData {
    /// The number ID (`pnm_`).
    pub number_id: String,
    #[schema(schema_with = schema_phone_number_action_needed_data_action_needed_property)]
    pub action_needed: PhoneNumberActionNeededDataActionNeeded,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberPortOutRequestedData {
    /// The number ID (`pnm_`).
    pub number_id: String,
    pub port_out_id: String,
    pub respond_by: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberPortOutDecidedDataDecision {
    #[serde(rename = "authorized")]
    Authorized,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "provider_authorized")]
    ProviderAuthorized,
}

fn schema_phone_number_port_out_decided_data_decision_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["authorized", "rejected", "provider_authorized"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberPortOutDecidedData {
    /// The number ID (`pnm_`).
    pub number_id: String,
    pub port_out_id: String,
    #[schema(schema_with = schema_phone_number_port_out_decided_data_decision_property)]
    pub decision: PhoneNumberPortOutDecidedDataDecision,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub decided_by_key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub decided_by_user_id: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRegistrationStatusChangedData {
    pub registration_id: String,
    pub kind: String,
    pub from: String,
    pub to: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub failure_reason: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneComplianceStatusChangedData {
    pub country: String,
    pub number_type: PhoneNumberType,
    pub action: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneConsentChangedData {
    /// Masked.
    pub number_hint: String,
    pub state: String,
    pub purpose: String,
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneSpendWarningDataScope {
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "identity")]
    Identity,
    #[serde(rename = "registration")]
    Registration,
}

fn schema_phone_spend_warning_data_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["tenant", "identity", "registration"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSpendWarningData {
    #[schema(schema_with = schema_phone_spend_warning_data_scope_property)]
    pub scope: PhoneSpendWarningDataScope,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub identity_id: Option<Option<String>>,
    pub percent: i64,
    pub used_pence: i64,
    pub cap_pence: i64,
    pub resets_at: String,
}

fn schema_phone_setup_status_changed_data_next_step_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSetupStatusChangedData {
    pub country: String,
    pub from: String,
    pub to: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_setup_status_changed_data_next_step_property)]
    pub next_step: Option<Option<serde_json::Value>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneBudgetChangedDataChange {
    #[serde(rename = "set")]
    Set,
    #[serde(rename = "loosened")]
    Loosened,
    #[serde(rename = "tightened")]
    Tightened,
    #[serde(rename = "disabled")]
    Disabled,
}

fn schema_phone_budget_changed_data_change_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["set", "loosened", "tightened", "disabled"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneBudgetChangedDataDecidedBy {
    #[serde(rename = "person")]
    Person,
    #[serde(rename = "key")]
    Key,
}

fn schema_phone_budget_changed_data_decided_by_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["person", "key"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneBudgetChangedData {
    pub version: i64,
    pub enabled: bool,
    #[schema(schema_with = schema_phone_budget_changed_data_change_property)]
    pub change: PhoneBudgetChangedDataChange,
    #[schema(schema_with = schema_phone_budget_changed_data_decided_by_property)]
    pub decided_by: PhoneBudgetChangedDataDecidedBy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemEventData {
    pub item: VaultItem,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemDeletedData {
    pub item_id: String,
    pub receipt: VaultShredReceipt,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemRevealedData {
    pub item_id: String,
    pub actor_user_id: String,
    pub rotated: bool,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemHealthChangedData {
    pub item_id: String,
    pub health: VaultHealth,
    #[schema(required = true)]
    pub health_reason: Option<String>,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultGrantEventData {
    pub grant: VaultGrant,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultUseEventData {
    pub r#use: VaultUse,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultTicketIssuedData {
    pub use_id: String,
    pub item_id: String,
    pub consumer_id: String,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultTicketRedeemedData {
    pub use_id: String,
    pub item_id: String,
    pub consumer_id: String,
    pub phase: VaultPhase,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultTicketRefusedData {
    pub use_id: String,
    pub item_id: String,
    pub consumer_id: String,
    pub phase: VaultPhase,
    pub reason: String,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultTicketReportedData {
    pub use_id: String,
    pub item_id: String,
    pub consumer_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<VaultPhase>)]
    pub phase: Option<Option<VaultPhase>>,
    pub outcome: String,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerChangedDataChange {
    #[serde(rename = "registered")]
    Registered,
    #[serde(rename = "activated")]
    Activated,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "resumed")]
    Resumed,
    #[serde(rename = "keys_rotated")]
    KeysRotated,
    #[serde(rename = "revoked")]
    Revoked,
}

fn schema_vault_consumer_changed_data_change_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["registered", "activated", "suspended", "resumed", "keys_rotated", "revoked"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultConsumerChangedData {
    pub consumer_id: String,
    #[schema(schema_with = schema_vault_consumer_changed_data_change_property)]
    pub change: VaultConsumerChangedDataChange,
    pub audit: VaultEventAudit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultKeyChangedDataChange {
    #[serde(rename = "rotated")]
    Rotated,
    #[serde(rename = "custody_changed")]
    CustodyChanged,
    #[serde(rename = "revoke_all")]
    RevokeAll,
}

fn schema_vault_key_changed_data_change_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["rotated", "custody_changed", "revoke_all"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultKeyChangedDataCustody {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "partner")]
    Partner,
}

fn schema_vault_key_changed_data_custody_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["operator", "partner"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultKeyChangedData {
    #[schema(schema_with = schema_vault_key_changed_data_change_property)]
    pub change: VaultKeyChangedDataChange,
    #[schema(schema_with = schema_vault_key_changed_data_custody_property)]
    pub custody: VaultKeyChangedDataCustody,
    pub audit: VaultEventAudit,
}

/// The envelope of every webhook event. Payloads are thin: IDs, a summary, verdicts and at most `policy.webhook_text_bytes` of `extracted_text` (FR-WH-4). Fetch anything else through the API. Events can arrive out of order: use `occurred_at` and `sequence` to discard stale updates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventBase {
    pub id: EventId,
    pub r#type: EventType,
    /// The payload schema date. A breaking payload change gets a new date; endpoints can pin the old one for 12 months.
    pub api_version: String,
    pub occurred_at: String,
    /// `null` only for deployment-level platform events.
    #[schema(required = true)]
    pub tenant_id: Option<TenantId>,
    /// `null` for events that do not belong to one identity (domain, job and platform events).
    #[schema(required = true)]
    pub identity_id: Option<IdentityId>,
    /// Increases strictly per owner: per identity for mailbox events (`message.*`, `identity.*`, `verification.received`, `calendar.received`, `draft.*`, `suppression.created`, `quota.warning`, `sms.*`, `call.*`), per domain for `domain.*` events, and per job for `erasure.*` and `export.*` events. `null` for platform events (`webhook.disabled`, `webhook.test`, `member.*`, `billing.*`, `tenant.policy_updated`, `signup.verified`, `account.*`, `phone_request.*`, `phone_number.*`, `phone_registration.*`, `phone_compliance.*`, `phone_consent.*`, `phone_spend.*`, `phone_setup.*`, `phone_budget.*` and `vault.*`). `message.screened` precedes, by one `sequence`, the `message.received` or `message.quarantined` it causes, and `message.released` precedes its `message.received`.
    #[schema(required = true)]
    pub sequence: Option<i64>,
    /// The payload; its schema depends on `type`.
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
}

/// Why an accepted send did not go out normally (`data.reason` of `message.rejected`, `message.failed`
/// and `message.uncertain`).
///
/// | Reason | Status | Meaning |
/// |---|---|---|
/// | `provider_validation` | `rejected` | The transport refused the content (header, size, format) |
/// | `sender_domain_unavailable` | `rejected` | The domain is not onboarded with the transport |
/// | `recipient_suppressed_by_provider` | per recipient `suppressed` | The provider's own suppression list; synced into ours, except a suppression scoped to the shared platform domain, which may come from another workspace and is not copied ([G4]) |
/// | `quota_exhausted` | `failed` | The provider's daily limit, still exhausted after 24 hours of backoff, or an SMTP relay still unavailable after 24 hours of retries |
/// | `transport_timeout` | `uncertain` | No answer from the transport. It may have been sent |
/// | `transport_connection_lost` | `uncertain` | The connection dropped after the request was written |
/// | `provider_outcome_unknown` | `uncertain` | The transport reported a delivery failure for a message with several recipients without saying which; some may have received it ([G2]) |
/// | `resolved_not_sent` | `failed` | A human resolved an uncertain send as not sent |
/// | `domain_failing_no_fallback` | `failed` | The domain failed and fallback was disabled by policy |
/// | `marketing_needs_ses` | `rejected` | A `kind: marketing` message whose sending domain uses the `cloudflare` transport when it reached the transport (the domain's transport changed, or fallback moved it to the platform domain, after it was accepted). Cloudflare Email Service is for transactional mail only |
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SendFailureReason {
    #[serde(rename = "provider_validation")]
    ProviderValidation,
    #[serde(rename = "sender_domain_unavailable")]
    SenderDomainUnavailable,
    #[serde(rename = "recipient_suppressed_by_provider")]
    RecipientSuppressedByProvider,
    #[serde(rename = "quota_exhausted")]
    QuotaExhausted,
    #[serde(rename = "transport_timeout")]
    TransportTimeout,
    #[serde(rename = "transport_connection_lost")]
    TransportConnectionLost,
    #[serde(rename = "provider_outcome_unknown")]
    ProviderOutcomeUnknown,
    #[serde(rename = "resolved_not_sent")]
    ResolvedNotSent,
    #[serde(rename = "domain_failing_no_fallback")]
    DomainFailingNoFallback,
    #[serde(rename = "marketing_needs_ses")]
    MarketingNeedsSes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventAttachment {
    pub id: AttachmentId,
    #[schema(required = true)]
    pub filename: Option<String>,
    pub content_type: String,
    pub size: i64,
}

fn schema_message_received_data_reprocessed_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present only on an event re-emitted by a re-parse ([J3]); it is not new mail.", "enum": [true], "type": "boolean"}"###).expect("the contract's schema")
}

fn schema_message_received_data_released_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present only when a release emitted the event: it is the first time this message is visible. `message.received` is emitted exactly once per message (re-parse re-emissions excepted).", "enum": [true], "type": "boolean"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageReceivedData {
    pub message: MessageSummary,
    pub thread_id: ThreadId,
    pub trust: Trust,
    /// Up to `policy.webhook_text_bytes` (default 16 KB, maximum 64 KB) of `extracted_text`. Untrusted content.
    #[schema(required = true)]
    pub extracted_text: Option<String>,
    pub extracted_text_truncated: bool,
    pub attachments: Vec<EventAttachment>,
    /// Present only on an event re-emitted by a re-parse ([J3]); it is not new mail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_received_data_reprocessed_property)]
    pub reprocessed: Option<bool>,
    /// The screening record, or `null` when the message was not screened.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Screening>)]
    pub screening: Option<Option<Screening>>,
    /// Present only when a release emitted the event: it is the first time this message is visible. `message.received` is emitted exactly once per message (re-parse re-emissions excepted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_received_data_released_property)]
    pub released: Option<bool>,
}

fn schema_message_quarantined_data_reprocessed_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present only on an event re-emitted by a re-parse ([J3]).", "enum": [true], "type": "boolean"}"###).expect("the contract's schema")
}

fn schema_message_quarantined_data_rescreened_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present only when a late signal moved a delivered message back to quarantine. A re-parse that is also such a late signal emits one `message.quarantined` with both `reprocessed` and `rescreened`.", "enum": [true], "type": "boolean"}"###).expect("the contract's schema")
}

/// As `message.received`, plus `quarantine_reason`, and without `extracted_text`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageQuarantinedData {
    pub message: MessageSummary,
    pub thread_id: ThreadId,
    pub trust: Trust,
    pub attachments: Vec<EventAttachment>,
    pub quarantine_reason: QuarantineReason,
    /// Present only on an event re-emitted by a re-parse ([J3]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_quarantined_data_reprocessed_property)]
    pub reprocessed: Option<bool>,
    /// The screening record, or `null` when the message was not screened.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Screening>)]
    pub screening: Option<Option<Screening>>,
    /// Present only when a late signal moved a delivered message back to quarantine. A re-parse that is also such a late signal emits one `message.quarantined` with both `reprocessed` and `rescreened`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_quarantined_data_rescreened_property)]
    pub rescreened: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageReleasedData {
    pub message: MessageSummary,
    /// The API key that released the message, or `null` for a release by a person in the console.
    #[schema(required = true)]
    pub released_by_key_id: Option<KeyId>,
    /// The console user who released the message, or `null` for a release by API key.
    #[schema(required = true)]
    pub released_by_user_id: Option<UserId>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageTriagedData {
    pub message_id: MessageId,
    pub thread_id: ThreadId,
    pub triage: Triage,
}

/// No subject, address or text. An endpoint subscribed to `message.screened` needs `quarantine:review`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageScreenedData {
    pub message_id: MessageId,
    pub thread_id: ThreadId,
    pub message_status: MessageStatus,
    #[schema(required = true)]
    pub quarantine_reason: Option<QuarantineReason>,
    pub late: bool,
    pub screening: Screening,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageSentDataProvider {
    #[serde(rename = "cloudflare")]
    Cloudflare,
    #[serde(rename = "ses")]
    Ses,
    #[serde(rename = "smtp")]
    Smtp,
    #[serde(rename = "simulator")]
    Simulator,
    #[serde(rename = "loopback")]
    Loopback,
}

fn schema_message_sent_data_provider_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`smtp` for a domain that sends through its own SMTP relay (`provider_message_id` is then `smtp:{host}:{Message-ID}`).", "enum": ["cloudflare", "ses", "smtp", "simulator", "loopback"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageSentData {
    pub message: MessageSummary,
    /// `smtp` for a domain that sends through its own SMTP relay (`provider_message_id` is then `smtp:{host}:{Message-ID}`).
    #[schema(schema_with = schema_message_sent_data_provider_property)]
    pub provider: MessageSentDataProvider,
    /// `null` when a person resolved an uncertain send as sent (`resolveMessage` with `outcome: "sent"`).
    #[schema(required = true)]
    pub provider_message_id: Option<String>,
    /// Sent from the identity's platform address because the domain was failing.
    pub sent_via_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageDeliveredData {
    pub message_id: MessageId,
    pub recipient: EmailAddress,
    #[schema(required = true)]
    pub smtp_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageDeferredData {
    pub message_id: MessageId,
    pub recipient: EmailAddress,
    #[schema(required = true)]
    pub smtp_code: Option<String>,
    #[schema(required = true)]
    pub smtp_response: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageBouncedDataBounceType {
    #[serde(rename = "hard")]
    Hard,
    #[serde(rename = "soft")]
    Soft,
}

fn schema_message_bounced_data_bounce_type_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["hard", "soft"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageBouncedData {
    pub message_id: MessageId,
    pub recipient: EmailAddress,
    #[schema(schema_with = schema_message_bounced_data_bounce_type_property)]
    pub bounce_type: MessageBouncedDataBounceType,
    #[schema(required = true)]
    pub smtp_code: Option<String>,
    #[schema(required = true)]
    pub smtp_response: Option<String>,
    /// The recipient was added to the suppression list (hard bounces).
    pub suppressed: bool,
}

fn schema_message_complained_data_suppressed_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "A complaint always creates a permanent suppression.", "enum": [true], "type": "boolean"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageComplainedData {
    pub message_id: MessageId,
    pub recipient: EmailAddress,
    /// A complaint always creates a permanent suppression.
    #[schema(schema_with = schema_message_complained_data_suppressed_property)]
    pub suppressed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageRejectedData {
    pub message_id: MessageId,
    pub reason: SendFailureReason,
    /// The transport's explanation, for example its error code.
    #[schema(required = true)]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageFailedData {
    pub message_id: MessageId,
    pub reason: SendFailureReason,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageUncertainData {
    pub message_id: MessageId,
    pub reason: SendFailureReason,
    /// What to do, for example to check the recipient's mailbox and then call `resolve`.
    pub fix: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageReconciledData {
    pub message_id: MessageId,
    /// The real status the uncertain send moved to.
    pub status: OutboundStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageSuppressedData {
    pub message_id: MessageId,
    pub recipients: Vec<EmailAddress>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageCanceledData {
    pub message_id: MessageId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageLabelsChangedDataMessagesItem {
    pub message_id: MessageId,
    pub thread_id: ThreadId,
}

fn schema_message_labels_changed_data_messages_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"message_id": {"$ref": "#/components/schemas/MessageId"}, "thread_id": {"$ref": "#/components/schemas/ThreadId"}}, "required": ["message_id", "thread_id"], "type": "object"}, "maxItems": 100, "minItems": 1, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageLabelsChangedDataActorKind {
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "person")]
    Person,
}

fn schema_message_labels_changed_data_actor_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["key", "person"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageLabelsChangedDataActor {
    #[schema(schema_with = schema_message_labels_changed_data_actor_kind_property)]
    pub kind: MessageLabelsChangedDataActorKind,
    #[schema(required = true)]
    pub key_id: Option<KeyId>,
    #[schema(required = true)]
    pub user_id: Option<UserId>,
}

fn schema_message_labels_changed_data_actor_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"key_id": {"oneOf": [{"$ref": "#/components/schemas/KeyId"}, {"type": "null"}]}, "kind": {"enum": ["key", "person"], "type": "string"}, "user_id": {"oneOf": [{"$ref": "#/components/schemas/UserId"}, {"type": "null"}]}}, "required": ["kind", "key_id", "user_id"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageLabelsChangedDataSource {
    #[serde(rename = "message")]
    Message,
    #[serde(rename = "thread")]
    Thread,
    #[serde(rename = "batch")]
    Batch,
}

fn schema_message_labels_changed_data_source_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["message", "thread", "batch"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageLabelsChangedData {
    #[schema(schema_with = schema_message_labels_changed_data_messages_property)]
    pub messages: Vec<MessageLabelsChangedDataMessagesItem>,
    pub added: Vec<Label>,
    pub removed: Vec<Label>,
    #[schema(schema_with = schema_message_labels_changed_data_actor_property)]
    pub actor: MessageLabelsChangedDataActor,
    #[schema(schema_with = schema_message_labels_changed_data_source_property)]
    pub source: MessageLabelsChangedDataSource,
    pub request_id: RequestId,
    pub part: i64,
    pub parts: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VerificationReceivedDataKind {
    #[serde(rename = "code")]
    Code,
    #[serde(rename = "link")]
    Link,
}

fn schema_verification_received_data_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["code", "link"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// The code or link itself is only available through `wait`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VerificationReceivedData {
    pub message_id: MessageId,
    pub sender_domain: String,
    #[schema(schema_with = schema_verification_received_data_kind_property)]
    pub kind: VerificationReceivedDataKind,
    /// The approved service-ledger entry that matched, or `null` when the tenant does not require approval.
    #[schema(required = true)]
    pub account_id: Option<ServiceAccountId>,
}

/// The `uid` is text the sender chose; read the object with `getMessage`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CalendarReceivedData {
    pub message_id: MessageId,
    pub thread_id: ThreadId,
    pub object_status: String,
    #[schema(required = true)]
    pub method: Option<String>,
    #[schema(required = true)]
    pub uid: Option<String>,
    #[schema(required = true)]
    pub sequence: Option<i64>,
    #[schema(required = true)]
    pub recurrence_id: Option<CalendarTime>,
    pub verified: bool,
    #[schema(required = true)]
    pub verified_by: Option<String>,
    #[schema(required = true)]
    pub reason: Option<String>,
    pub organized_here: bool,
    /// Present, `true`, when a release from quarantine or screening emitted the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub released: Option<bool>,
    /// Present, `true`, when a re-parse changed the calendar object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub reprocessed: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftSummaryKind {
    #[serde(rename = "transactional")]
    Transactional,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "auto_reply")]
    AutoReply,
}

fn schema_draft_summary_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["transactional", "marketing", "auto_reply"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

/// The draft summary carried by `draft.*` events. No subject, body, address, filename or note, so an endpoint needs only `webhooks:manage` to receive them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftSummary {
    pub id: DraftId,
    pub identity_id: IdentityId,
    pub draft_kind: DraftKind,
    #[schema(schema_with = schema_draft_summary_kind_property)]
    pub kind: DraftSummaryKind,
    pub status: DraftStatus,
    pub revision: i64,
    #[schema(required = true)]
    pub digest: Option<String>,
    pub recipient_count: i64,
    pub attachment_count: i64,
    #[schema(required = true)]
    pub send_at: Option<String>,
    #[schema(required = true)]
    pub in_reply_to: Option<MessageId>,
    #[schema(required = true)]
    pub forward_of: Option<MessageId>,
    #[schema(required = true)]
    pub thread_id: Option<ThreadId>,
    #[schema(required = true)]
    pub client_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftApprovalRequestedData {
    pub draft: DraftSummary,
    pub reason: DraftApprovalReason,
    #[schema(required = true)]
    pub requested_by_key_id: Option<KeyId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftApprovedData {
    pub draft: DraftSummary,
    pub send: bool,
    /// The API key that approved, or `null` for a person in the console.
    #[schema(required = true)]
    pub approved_by_key_id: Option<KeyId>,
    /// The console user who approved, or `null` for an API key.
    #[schema(required = true)]
    pub approved_by_user_id: Option<UserId>,
    /// The caller's opaque user ID, when an API key gave one.
    #[schema(required = true)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftRejectedDataReason {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "expired")]
    Expired,
}

fn schema_draft_rejected_data_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["operator", "expired"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftRejectedData {
    pub draft: DraftSummary,
    #[schema(schema_with = schema_draft_rejected_data_reason_property)]
    pub reason: DraftRejectedDataReason,
    #[schema(required = true)]
    pub rejected_by_key_id: Option<KeyId>,
    #[schema(required = true)]
    pub rejected_by_user_id: Option<UserId>,
    #[schema(required = true)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftSentDataMessageStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "suppressed")]
    Suppressed,
}

fn schema_draft_sent_data_message_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`suppressed` when every recipient was suppressed.", "enum": ["queued", "suppressed"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftSentData {
    pub draft: DraftSummary,
    pub message_id: MessageId,
    pub thread_id: ThreadId,
    /// `suppressed` when every recipient was suppressed.
    #[schema(schema_with = schema_draft_sent_data_message_status_property)]
    pub message_status: DraftSentDataMessageStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftFailedData {
    pub draft: DraftSummary,
    pub reason: DraftFailureReason,
    pub attempts: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityCreatedData {
    pub identity: Identity,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityUpdatedData {
    pub identity: Identity,
    /// The names of the fields that changed.
    pub changed: Vec<String>,
}

fn schema_identity_paused_data_metrics_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "For `abuse_threshold`, the complaint and bounce figures that triggered the pause; otherwise `null`.", "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityPausedData {
    pub identity_id: IdentityId,
    pub reason: PauseReason,
    /// For `abuse_threshold`, the complaint and bounce figures that triggered the pause; otherwise `null`.
    #[schema(required = true, schema_with = schema_identity_paused_data_metrics_property)]
    pub metrics: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityResumedData {
    pub identity_id: IdentityId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityDeletedData {
    pub identity_id: IdentityId,
    pub erasure_request_id: ErasureRequestId,
}

/// Payload of `identity.address_added`, `identity.address_activated` and `identity.address_retired`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityAddressData {
    pub address: Address,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityAddressPromotedData {
    pub address: Address,
    /// The previous primary, now an alias with status `retiring` and its `retire_at`.
    pub previous_primary: Address,
}

/// Payload of `identity.key_created` and `identity.key_revoked`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityKeyEventData {
    pub identity_id: IdentityId,
    pub kid: IdentityKeyKid,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityKeyRotatedData {
    pub identity_id: IdentityId,
    /// The new active key.
    pub kid: IdentityKeyKid,
    /// The key now `retiring`, or `null` when the rotation created the identity's first key.
    #[schema(required = true)]
    pub previous_kid: Option<IdentityKeyKid>,
}

/// Payload of `domain.created` and `domain.verified`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainEventData {
    pub domain: Domain,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainDegradedData {
    pub domain_id: DomainId,
    pub issues: Vec<DomainIssue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainFailingData {
    pub domain_id: DomainId,
    pub issues: Vec<DomainIssue>,
    pub fallback_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainSuspendedDataReason {
    #[serde(rename = "failing_14_days")]
    Failing14Days,
    #[serde(rename = "nameservers_changed")]
    NameserversChanged,
    #[serde(rename = "ownership_record_missing")]
    OwnershipRecordMissing,
    #[serde(rename = "registration_changed")]
    RegistrationChanged,
}

fn schema_domain_suspended_data_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["failing_14_days", "nameservers_changed", "ownership_record_missing", "registration_changed"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainSuspendedData {
    pub domain_id: DomainId,
    #[schema(schema_with = schema_domain_suspended_data_reason_property)]
    pub reason: DomainSuspendedDataReason,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainRecoveredData {
    pub domain_id: DomainId,
    pub from_state: DomainState,
}

fn schema_domain_reminder_data_hours_in_state_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "24, 72 and 168 (7 days); `288` (12 days) for a domain never verified, two days before its unverified expiry; `504` (21 days) is the final reminder for a `nameservers` domain still `pending` before Cloudflare deletes its zone at 28 days.", "enum": [24, 72, 168, 288, 504], "type": "integer"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainReminderData {
    pub domain_id: DomainId,
    pub state: DomainState,
    /// 24, 72 and 168 (7 days); `288` (12 days) for a domain never verified, two days before its unverified expiry; `504` (21 days) is the final reminder for a `nameservers` domain still `pending` before Cloudflare deletes its zone at 28 days.
    #[schema(schema_with = schema_domain_reminder_data_hours_in_state_property)]
    pub hours_in_state: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainRemovedDataReason {
    #[serde(rename = "requested")]
    Requested,
    #[serde(rename = "zone_expired")]
    ZoneExpired,
    #[serde(rename = "evicted")]
    Evicted,
    #[serde(rename = "unverified_expired")]
    UnverifiedExpired,
}

fn schema_domain_removed_data_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`requested`: removed through `DELETE /v1/domains/{domain_id}`. `zone_expired`: a `nameservers` zone was never activated and Cloudflare deleted it; the domain can be added again. `evicted`: the domain was never verified, and another tenant proved control of its DNS with a claim record. `unverified_expired`: the domain was never verified within 14 days of being added (or of its zone's activation).", "enum": ["requested", "zone_expired", "evicted", "unverified_expired"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainRemovedData {
    pub domain_id: DomainId,
    /// `requested`: removed through `DELETE /v1/domains/{domain_id}`. `zone_expired`: a `nameservers` zone was never activated and Cloudflare deleted it; the domain can be added again. `evicted`: the domain was never verified, and another tenant proved control of its DNS with a claim record. `unverified_expired`: the domain was never verified within 14 days of being added (or of its zone's activation).
    #[schema(schema_with = schema_domain_removed_data_reason_property)]
    pub reason: DomainRemovedDataReason,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureCompletedData {
    pub erasure_request: ErasureRequest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureFailedData {
    pub erasure_request_id: ErasureRequestId,
    /// The job step that failed, for example `erase_mailboxes`. The step was retried until 20 hours after the first request.
    pub step: String,
    pub error: String,
}

/// Fetch the download link with `getExport`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ExportCompletedData {
    pub export_id: ExportId,
    pub expires_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SuppressionCreatedData {
    pub address_hint: String,
    pub reason: SuppressionReason,
    #[schema(required = true)]
    pub source_message_id: Option<MessageId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum QuotaWarningDataMetric {
    #[serde(rename = "sends")]
    Sends,
}

fn schema_quota_warning_data_metric_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "The daily send cap that was reached. v1 emits this event only for `sends` (the tenant cap and each identity's cap); the agentic budget answers `429 agentic_budget_exhausted` without a warning event.", "enum": ["sends"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum QuotaWarningDataScope {
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "identity")]
    Identity,
}

fn schema_quota_warning_data_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["tenant", "identity"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct QuotaWarningData {
    /// The daily send cap that was reached. v1 emits this event only for `sends` (the tenant cap and each identity's cap); the agentic budget answers `429 agentic_budget_exhausted` without a warning event.
    #[schema(schema_with = schema_quota_warning_data_metric_property)]
    pub metric: QuotaWarningDataMetric,
    pub used: i64,
    pub limit: i64,
    #[schema(schema_with = schema_quota_warning_data_scope_property)]
    pub scope: QuotaWarningDataScope,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WebhookDisabledData {
    pub webhook_id: WebhookId,
    /// Why the endpoint was disabled, for example `failing`.
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum WebhookTestDataMessage {
    #[serde(rename = "hello")]
    Hello,
}

fn schema_webhook_test_data_message_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["hello"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WebhookTestData {
    #[schema(schema_with = schema_webhook_test_data_message_property)]
    pub message: WebhookTestDataMessage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MemberInvitedData {
    pub invitation_id: InvitationId,
    /// The invited address, masked.
    pub email_hint: String,
    pub role: InvitationRole,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MemberJoinedData {
    pub user_id: UserId,
    pub role: MemberRole,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MemberRoleChangedData {
    pub user_id: UserId,
    pub from: MemberRole,
    pub to: MemberRole,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MemberRemovedData {
    pub user_id: UserId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum BillingPlanChangedDataReason {
    #[serde(rename = "checkout")]
    Checkout,
    #[serde(rename = "portal")]
    Portal,
    #[serde(rename = "scheduled_downgrade")]
    ScheduledDowngrade,
    #[serde(rename = "payment_failed_grace_ended")]
    PaymentFailedGraceEnded,
    #[serde(rename = "payment_recovered")]
    PaymentRecovered,
    #[serde(rename = "canceled")]
    Canceled,
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "dispute")]
    Dispute,
}

fn schema_billing_plan_changed_data_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`portal`: an upgrade confirmed in the Stripe Customer Portal. `scheduled_downgrade`: a downgrade the owner scheduled took effect at the period end. `payment_recovered`: the plan was restored after a late payment, or when a dispute was closed in the workspace's favour. `dispute`: a payment was disputed, so the default plan applies until the dispute closes.", "enum": ["checkout", "portal", "scheduled_downgrade", "payment_failed_grace_ended", "payment_recovered", "canceled", "operator", "dispute"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BillingPlanChangedData {
    pub from_plan: String,
    pub to_plan: String,
    /// `portal`: an upgrade confirmed in the Stripe Customer Portal. `scheduled_downgrade`: a downgrade the owner scheduled took effect at the period end. `payment_recovered`: the plan was restored after a late payment, or when a dispute was closed in the workspace's favour. `dispute`: a payment was disputed, so the default plan applies until the dispute closes.
    #[schema(schema_with = schema_billing_plan_changed_data_reason_property)]
    pub reason: BillingPlanChangedDataReason,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BillingPaymentFailedData {
    /// The workspace keeps its plan until then.
    pub grace_until: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BillingLimitReachedData {
    pub feature: UsageFeatureName,
    #[schema(required = true)]
    pub granted: Option<i64>,
    #[schema(required = true)]
    pub resets_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyUpdatedDataBy {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "partner")]
    Partner,
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "console")]
    Console,
}

fn schema_tenant_policy_updated_data_by_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Who wrote it.", "enum": ["platform", "partner", "tenant", "console"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyUpdatedData {
    /// The dotted paths of the fields written.
    pub fields: Vec<String>,
    /// Who wrote it.
    #[schema(schema_with = schema_tenant_policy_updated_data_by_property)]
    pub by: TenantPolicyUpdatedDataBy,
    #[schema(required = true)]
    pub actor_key_id: Option<KeyId>,
    /// The person, for a console save.
    #[schema(required = true)]
    pub actor_user_id: Option<UserId>,
    pub policy_version: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SignupVerifiedDataVerifiedVia {
    #[serde(rename = "code")]
    Code,
    #[serde(rename = "link")]
    Link,
    #[serde(rename = "claim")]
    Claim,
}

fn schema_signup_verified_data_verified_via_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["code", "link", "claim"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// A platform event with `tenant_id` and `identity_id` set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SignupVerifiedData {
    pub signup_id: AgentSignupId,
    pub address: EmailAddress,
    #[schema(schema_with = schema_signup_verified_data_verified_via_property)]
    pub verified_via: SignupVerifiedDataVerifiedVia,
    pub user_id: UserId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AccountEventData {
    pub account: ServiceAccount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AccountRejectedDataReason {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "expired")]
    Expired,
}

fn schema_account_rejected_data_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["operator", "expired"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AccountRejectedData {
    pub account: ServiceAccount,
    #[schema(schema_with = schema_account_rejected_data_reason_property)]
    pub reason: AccountRejectedDataReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum NextStepWho {
    #[serde(rename = "person")]
    Person,
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "agent")]
    Agent,
}

fn schema_next_step_who_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["person", "operator", "agent"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NextStep {
    #[schema(schema_with = schema_next_step_who_property)]
    pub who: NextStepWho,
    /// For example `register_us_a2p`, `enable_emergency`, `set_voice_route`, `submit_compliance`, `platform_prerequisite`.
    pub kind: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub request_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub console_url: Option<Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum FollowOnCapability {
    #[serde(rename = "outbound_sms")]
    OutboundSms,
    #[serde(rename = "inbound_sms")]
    InboundSms,
    #[serde(rename = "inbound_calls")]
    InboundCalls,
    #[serde(rename = "outbound_calls")]
    OutboundCalls,
}

fn schema_follow_on_capability_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["outbound_sms", "inbound_sms", "inbound_calls", "outbound_calls"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum FollowOnKind {
    #[serde(rename = "register_us_a2p")]
    RegisterUsA2p,
    #[serde(rename = "verify_toll_free")]
    VerifyTollFree,
    #[serde(rename = "enable_emergency")]
    EnableEmergency,
    #[serde(rename = "set_voice_route")]
    SetVoiceRoute,
    #[serde(rename = "not_available")]
    NotAvailable,
}

fn schema_follow_on_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["register_us_a2p", "verify_toll_free", "enable_emergency", "set_voice_route", "not_available"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum FollowOnWho {
    #[serde(rename = "person")]
    Person,
    #[serde(rename = "operator")]
    Operator,
}

fn schema_follow_on_who_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["person", "operator"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum FollowOnOncePer {
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "number")]
    Number,
}

fn schema_follow_on_once_per_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["tenant", "number", null], "type": ["string", "null"]}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FollowOn {
    #[schema(schema_with = schema_follow_on_capability_property)]
    pub capability: FollowOnCapability,
    #[schema(schema_with = schema_follow_on_kind_property)]
    pub kind: FollowOnKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub reason: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub setup_pence: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub monthly_pence: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<i64>>)]
    pub typical_wait_days: Option<Option<Vec<i64>>>,
    #[schema(schema_with = schema_follow_on_who_property)]
    pub who: FollowOnWho,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_follow_on_once_per_property)]
    pub once_per: Option<Option<FollowOnOncePer>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneQuoteCurrency {
    #[serde(rename = "GBP")]
    GBP,
}

fn schema_phone_quote_currency_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["GBP"], "type": "string"}"###)
        .expect("the contract's schema")
}

fn schema_phone_quote_provider_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "Provider USD costs and the exchange rate; platform and partner keys only.", "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneQuote {
    #[schema(schema_with = schema_phone_quote_currency_property)]
    pub currency: PhoneQuoteCurrency,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub setup_pence: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub monthly_pence: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_setup_pence: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_monthly_pence: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub usage: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<FollowOn>)]
    pub follow_on: Option<Vec<FollowOn>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub months_charged_before_expiry: Option<Option<i64>>,
    pub rate_card_version: i64,
    pub valid_until: String,
    /// Provider USD costs and the exchange rate; platform and partner keys only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_quote_provider_property)]
    pub provider: Option<Option<serde_json::Value>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum PhoneRequestKind {
    #[serde(rename = "purchase_number")]
    #[default]
    PurchaseNumber,
    #[serde(rename = "port_in")]
    PortIn,
    #[serde(rename = "release_number")]
    ReleaseNumber,
    #[serde(rename = "submit_compliance")]
    SubmitCompliance,
    #[serde(rename = "register_us_a2p")]
    RegisterUsA2p,
    #[serde(rename = "verify_toll_free")]
    VerifyTollFree,
    #[serde(rename = "enable_emergency")]
    EnableEmergency,
    #[serde(rename = "set_voice_route")]
    SetVoiceRoute,
    #[serde(rename = "outbound_call")]
    OutboundCall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum PhoneRequestStatus {
    #[serde(rename = "pending_approval")]
    #[default]
    PendingApproval,
    #[serde(rename = "approved")]
    Approved,
    #[serde(rename = "executing")]
    Executing,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "action_needed")]
    ActionNeeded,
    #[serde(rename = "uncertain")]
    Uncertain,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "canceled")]
    Canceled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum CallDirection {
    #[serde(rename = "inbound")]
    Inbound,
    #[serde(rename = "outbound")]
    Outbound,
}

fn schema_call_direction_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["inbound", "outbound"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum CallSource {
    #[serde(rename = "webhook")]
    Webhook,
    #[serde(rename = "cdr")]
    Cdr,
}

fn schema_call_source_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["webhook", "cdr"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Call {
    pub id: String,
    pub number_id: PhoneNumberId,
    #[schema(schema_with = schema_call_direction_property)]
    pub direction: CallDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub counterparty: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub route_kind: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub status: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub outcome: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub disclosure_applied: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub started_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub ended_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub duration_seconds: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub cost_pence: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_call_source_property)]
    pub source: Option<CallSource>,
}

/// Vault item ID (`vit_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct VaultItemId(pub String);

/// Vault grant ID (`vgr_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct VaultGrantId(pub String);

/// Vault use ID (`vus_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct VaultUseId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemKind {
    #[serde(rename = "login")]
    Login,
    #[serde(rename = "api_key")]
    ApiKey,
    #[serde(rename = "session")]
    Session,
    #[serde(rename = "managed_secret")]
    ManagedSecret,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultOperation {
    #[serde(rename = "fill")]
    #[default]
    Fill,
    #[serde(rename = "rotate_fill")]
    RotateFill,
    #[serde(rename = "session_load")]
    SessionLoad,
    #[serde(rename = "session_capture")]
    SessionCapture,
    #[serde(rename = "inject")]
    Inject,
    #[serde(rename = "sign")]
    Sign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultPhase {
    #[serde(rename = "credentials")]
    #[default]
    Credentials,
    #[serde(rename = "totp")]
    Totp,
    #[serde(rename = "email_otp")]
    EmailOtp,
    #[serde(rename = "sms_otp")]
    SmsOtp,
    #[serde(rename = "session")]
    Session,
    #[serde(rename = "secret")]
    Secret,
    #[serde(rename = "new_password")]
    NewPassword,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultTrigger {
    #[serde(rename = "login_form")]
    #[default]
    LoginForm,
    #[serde(rename = "signup_form")]
    SignupForm,
    #[serde(rename = "reauth_prompt")]
    ReauthPrompt,
    #[serde(rename = "otp_field")]
    OtpField,
    #[serde(rename = "session_start")]
    SessionStart,
    #[serde(rename = "person_handoff")]
    PersonHandoff,
    #[serde(rename = "api_request")]
    ApiRequest,
    #[serde(rename = "password_change")]
    PasswordChange,
    #[serde(rename = "sign_request")]
    SignRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultHealth {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "needs_auth")]
    NeedsAuth,
    #[serde(rename = "needs_attention")]
    NeedsAttention,
    #[serde(rename = "needs_rotation")]
    NeedsRotation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultOtpChannelVariant1Kind {
    #[serde(rename = "email")]
    Email,
}

fn schema_vault_otp_channel_variant1_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["email"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultOtpChannelVariant2Kind {
    #[serde(rename = "phone")]
    Phone,
}

fn schema_vault_otp_channel_variant2_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["phone"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum VaultOtpChannel {
    Variant1 {
        #[schema(schema_with = schema_vault_otp_channel_variant1_kind_property)]
        kind: VaultOtpChannelVariant1Kind,
        address: EmailAddress,
        sender_domains: Vec<String>,
    },
    Variant2 {
        #[schema(schema_with = schema_vault_otp_channel_variant2_kind_property)]
        kind: VaultOtpChannelVariant2Kind,
        number_id: String,
        senders: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemState {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "archived")]
    Archived,
}

fn schema_vault_item_state_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["active", "archived"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemHealthReason {
    #[serde(rename = "session_rejected")]
    SessionRejected,
    #[serde(rename = "session_expired")]
    SessionExpired,
    #[serde(rename = "credentials_rejected")]
    CredentialsRejected,
    #[serde(rename = "otp_rejected")]
    OtpRejected,
    #[serde(rename = "value_exposed")]
    ValueExposed,
    #[serde(rename = "rotation_due")]
    RotationDue,
    #[serde(rename = "rotation_unconfirmed")]
    RotationUnconfirmed,
    #[serde(rename = "manual")]
    Manual,
}

fn schema_vault_item_health_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["session_rejected", "session_expired", "credentials_rejected", "otp_rejected", "value_exposed", "rotation_due", "rotation_unconfirmed", "manual", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemHas {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub password: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub totp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub recovery_codes: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub email_otp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub sms_otp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub secret: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub session_state: Option<bool>,
}

fn schema_vault_item_has_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"email_otp": {"type": "boolean"}, "password": {"type": "boolean"}, "recovery_codes": {"type": "boolean"}, "secret": {"type": "boolean"}, "session_state": {"type": "boolean"}, "sms_otp": {"type": "boolean"}, "totp": {"type": "boolean"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemRotation {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub remind_after_days: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub last_rotated_at: Option<Option<String>>,
}

fn schema_vault_item_rotation_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"properties": {"last_rotated_at": {"format": "date-time", "type": ["string", "null"]}, "remind_after_days": {"type": ["integer", "null"]}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub ttl_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub cookie_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub origin_count: Option<i64>,
}

fn schema_vault_item_session_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"cookie_count": {"type": "integer"}, "expires_at": {"format": "date-time", "type": "string"}, "origin_count": {"type": "integer"}, "size_bytes": {"type": "integer"}, "ttl_seconds": {"type": "integer"}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemManagedBy {
    #[serde(rename = "phone")]
    Phone,
}

fn schema_vault_item_managed_by_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["phone", null], "type": ["string", "null"]}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemCreatedBy {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub user_id: Option<Option<String>>,
}

fn schema_vault_item_created_by_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"properties": {"key_id": {"type": ["string", "null"]}, "user_id": {"type": ["string", "null"]}}, "type": "object"}"###).expect("the contract's schema")
}

/// A vault item. Never carries a value. `title` and `username` are not secrets; show `title` as untrusted text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItem {
    pub id: VaultItemId,
    pub tenant_id: TenantId,
    #[schema(required = true)]
    pub identity_id: Option<IdentityId>,
    pub kind: VaultItemKind,
    pub title: String,
    #[schema(required = true)]
    pub account_id: Option<String>,
    #[schema(required = true)]
    pub username: Option<String>,
    pub origins: Vec<VaultOrigin>,
    pub approval: VaultApprovalLevel,
    #[schema(schema_with = schema_vault_item_state_property)]
    pub state: VaultItemState,
    pub health: VaultHealth,
    #[schema(required = true, schema_with = schema_vault_item_health_reason_property)]
    pub health_reason: Option<VaultItemHealthReason>,
    pub current_version: i64,
    #[schema(schema_with = schema_vault_item_has_property)]
    pub has: VaultItemHas,
    #[schema(required = true)]
    pub otp_channel: Option<VaultOtpChannel>,
    #[schema(schema_with = schema_vault_item_rotation_property)]
    pub rotation: VaultItemRotation,
    #[schema(required = true, schema_with = schema_vault_item_session_property)]
    pub session: Option<VaultItemSession>,
    #[schema(required = true, schema_with = schema_vault_item_managed_by_property)]
    pub managed_by: Option<VaultItemManagedBy>,
    #[schema(schema_with = schema_vault_item_created_by_property)]
    pub created_by: VaultItemCreatedBy,
    #[schema(required = true)]
    pub first_used_at: Option<String>,
    #[schema(required = true)]
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn schema_vault_shred_receipt_deleted_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [true], "type": "boolean"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultShredReceiptCustody {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "partner")]
    Partner,
}

fn schema_vault_shred_receipt_custody_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["operator", "partner"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultShredReceipt {
    pub item_id: VaultItemId,
    #[schema(schema_with = schema_vault_shred_receipt_deleted_property)]
    pub deleted: bool,
    pub versions_shredded: i64,
    pub r2_objects_deleted: i64,
    pub uses_closed: i64,
    /// Uses deleted with the item, closed ones and the ones this deletion closed.
    pub uses_deleted: i64,
    pub grants_deleted: i64,
    #[schema(required = true)]
    pub restorable_until: Option<String>,
    #[schema(schema_with = schema_vault_shred_receipt_custody_property)]
    pub custody: VaultShredReceiptCustody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultGrantStatus {
    #[serde(rename = "pending_approval")]
    PendingApproval,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "denied")]
    Denied,
    #[serde(rename = "revoked")]
    Revoked,
    #[serde(rename = "expired")]
    Expired,
}

fn schema_vault_grant_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["pending_approval", "active", "denied", "revoked", "expired"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultGrant {
    pub id: VaultGrantId,
    pub tenant_id: TenantId,
    pub item_id: VaultItemId,
    pub identity_id: IdentityId,
    pub operations: Vec<VaultOperation>,
    pub origins: Vec<VaultOrigin>,
    #[schema(schema_with = schema_vault_grant_status_property)]
    pub status: VaultGrantStatus,
    pub purpose: String,
    #[schema(required = true)]
    pub max_uses: Option<i64>,
    pub use_count: i64,
    #[schema(required = true)]
    pub requested_by_key_id: Option<String>,
    #[schema(required = true)]
    pub decided_by_key_id: Option<String>,
    #[schema(required = true)]
    pub decided_by_user_id: Option<String>,
    #[schema(required = true)]
    pub on_behalf_of: Option<String>,
    #[schema(required = true)]
    pub note: Option<String>,
    #[schema(required = true)]
    pub decided_at: Option<String>,
    pub expires_at: String,
    #[schema(required = true)]
    pub revoked_at: Option<String>,
    #[schema(required = true)]
    pub revoke_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultUseVersion {
    #[serde(rename = "current")]
    Current,
    #[serde(rename = "previous")]
    Previous,
}

fn schema_vault_use_version_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "The version the use names; fixed at creation.", "enum": ["current", "previous"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultUseStatus2 {
    #[serde(rename = "pending_approval")]
    PendingApproval,
    #[serde(rename = "approved")]
    Approved,
    #[serde(rename = "in_use")]
    InUse,
    #[serde(rename = "closed")]
    Closed,
    #[serde(rename = "denied")]
    Denied,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "canceled")]
    Canceled,
}

fn schema_vault_use_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["pending_approval", "approved", "in_use", "closed", "denied", "expired", "canceled"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultUseApprovalReason {
    #[serde(rename = "per_use")]
    PerUse,
    #[serde(rename = "per_run")]
    PerRun,
    #[serde(rename = "operation")]
    Operation,
    #[serde(rename = "tenant_floor")]
    TenantFloor,
}

fn schema_vault_use_approval_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["per_use", "per_run", "operation", "tenant_floor", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultUseApprovedBy {
    #[serde(rename = "person")]
    Person,
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "grant")]
    Grant,
}

fn schema_vault_use_approved_by_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"enum": ["person", "key", "run", "grant", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

/// A use never carries its ticket, its hash or a sealed key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultUse {
    pub id: VaultUseId,
    pub tenant_id: TenantId,
    pub identity_id: IdentityId,
    pub item_id: VaultItemId,
    pub grant_id: VaultGrantId,
    pub operation: VaultOperation,
    pub phases: Vec<VaultPhase>,
    pub phases_redeemed: Vec<VaultPhase>,
    #[schema(required = true)]
    pub consumer_id: Option<String>,
    pub run_id: String,
    pub origin: VaultOrigin,
    pub trigger: VaultTrigger,
    pub purpose: String,
    /// The version the use names; fixed at creation.
    #[schema(schema_with = schema_vault_use_version_property)]
    pub version: VaultUseVersion,
    #[schema(schema_with = schema_vault_use_status_property)]
    pub status: VaultUseStatus2,
    #[schema(required = true, schema_with = schema_vault_use_approval_reason_property)]
    pub approval_reason: Option<VaultUseApprovalReason>,
    #[schema(required = true, schema_with = schema_vault_use_approved_by_property)]
    pub approved_by: Option<VaultUseApprovedBy>,
    pub digest: String,
    #[schema(required = true)]
    pub deadline_at: Option<String>,
    #[schema(required = true)]
    pub redeem_by: Option<String>,
    #[schema(required = true)]
    pub closes_at: Option<String>,
    #[schema(required = true)]
    pub closed_at: Option<String>,
    #[schema(required = true)]
    pub close_reason: Option<String>,
    #[schema(required = true)]
    pub outcome: Option<String>,
    pub created_by_key_id: String,
    #[schema(required = true)]
    pub decided_by_key_id: Option<String>,
    #[schema(required = true)]
    pub decided_by_user_id: Option<String>,
    #[schema(required = true)]
    pub on_behalf_of: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl std::fmt::Display for ExportId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for InvitationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for EventConsumerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for EventCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for VaultItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for VaultGrantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for VaultUseId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
