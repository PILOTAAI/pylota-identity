//! Objects first used by the `identities` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Address ID (`adr_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct AddressId(pub String);

/// Domain ID (`dom_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct DomainId(pub String);

/// Thread ID (`thr_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ThreadId(pub String);

/// Erasure request ID (`era_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ErasureRequestId(pub String);

/// Integrator metadata. At most 16 keys; each value is a string of at most 512 bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Metadata(pub BTreeMap<String, String>);

impl utoipa::PartialSchema for Metadata {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"additionalProperties": {"maxLength": 512, "type": "string"}, "description": "Integrator metadata. At most 16 keys; each value is a string of at most 512 bytes.", "maxProperties": 16, "type": "object"}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for Metadata {}

/// `deleting` and `deleted` are set by identity deletion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum IdentityStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "deleting")]
    Deleting,
    #[serde(rename = "deleted")]
    Deleted,
}

/// The stored form of a username: the lower-case local-part stem. Requests send a `UsernameInput`, which the service validates and lower-cases into this form.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct Username(pub String);

/// A username as sent in a request. The schema does not constrain it: the service validates it with the username rules ([A1], [A3], [A4], [A12]) and returns their own codes, never a generic `invalid_request`. A reserved name, a confusable one (homoglyphs, mixed scripts, right-to-left characters) or a name that folds to a reserved one gets `400 address_reserved`; any other non-ASCII character `400 address_unsupported`; a name whose lower-cased form does not match the stored form `^[a-z0-9][a-z0-9._-]{0,23}$` (at most 24 characters, no `..`, not ending in `.`) gets `400 address_invalid`; username plus tenant suffix over 40 characters gets `400 local_part_too_long`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct UsernameInput(pub String);

/// At most 78 characters, no CR or LF. Unicode is allowed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct DisplayName(pub String);

/// The accountable human for the identity (FR-IDN-2). Required before the identity can send.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityOwner {
    pub name: String,
    pub email: EmailAddress,
}

/// Appended to outbound messages according to policy. `html` is sanitised on write.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentitySignature {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum IdentitySendPolicyAutoReply {
    #[serde(rename = "allowed")]
    Allowed,
    #[serde(rename = "denied")]
    Denied,
}

fn schema_identity_send_policy_auto_reply_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "allowed", "description": "Whether the identity may send `kind: \"auto_reply\"` messages. `denied` refuses them (`409 auto_reply_not_allowed`); `allowed`, the default, leaves the decision to the tenant's `policy.auto_reply.allowed` and the exchange cap.", "enum": ["allowed", "denied"], "type": "string"}"###).expect("the contract's schema")
}

/// Per-identity sending rules, applied on top of the tenant policy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentitySendPolicy {
    /// Sends per day for this identity. Defaults to the tenant's `identity_daily_send_cap`. Only a platform key may set it above that cap (`403 scope_denied` with `details.field`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub daily_cap: Option<i64>,
    /// Whether the identity may send `kind: "auto_reply"` messages. `denied` refuses them (`409 auto_reply_not_allowed`); `allowed`, the default, leaves the decision to the tenant's `policy.auto_reply.allowed` and the exchange cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_identity_send_policy_auto_reply_property)]
    pub auto_reply: Option<IdentitySendPolicyAutoReply>,
    /// Suppress recipients that are not known ([E2], FR-OUT-13): a recipient is known when the identity has sent to it before (a delivery that was not suppressed), when the effective send-allow entry for this identity (its own or the tenant's) names it or its domain, for a reply or reply-all when it is the reply target of a `received` message whose verdict is `pass` or `arc_rescued`, for a send from a draft whose current revision carries a valid approval when that approval covers it, and for a calendar reply when it is the `ORGANIZER` of the verified invitation answered. Mail received from an address does not make it known. On by default for every identity; the system identity is created with `false`. Turn it off for an identity that sends to new people by design (booking confirmations).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub require_known_recipient: Option<bool>,
    /// The identity's own approval level for sends; it can only make the tenant's `drafts.require_approval` stricter, never looser. `null` follows the tenant. A looser value than the identity's current one gets `403 scope_denied` from an identity key and `403 permission_denied` (`person_required`) from a tenant key where people decide.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<RequireApproval>)]
    pub require_approval: Option<Option<RequireApproval>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityRetention {
    /// Each message of the identity, with its attachments, extracted text, index rows, vectors and drafts, is erased this many hours after it is stored, except in a thread under a legal hold. `null`: the tenant's retention policy alone applies. Setting a number where there was `null`, or lowering it, needs `erasure:manage` as well (`403 permission_denied`, `details.required: ["erasure:manage"]`); raising it or setting `null` needs `identities:write` only and brings nothing back.
    #[schema(required = true)]
    pub message_hours: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Identity {
    pub id: IdentityId,
    pub tenant_id: TenantId,
    pub username: Username,
    pub display_name: DisplayName,
    /// A free tag, for example `bookings`.
    #[schema(required = true)]
    pub purpose: Option<String>,
    pub status: IdentityStatus,
    /// Set when `status` is `paused`.
    #[schema(required = true)]
    pub pause_reason: Option<PauseReason>,
    pub primary_address: EmailAddress,
    /// Every address of the identity, in every status.
    pub addresses: Vec<Address>,
    #[schema(required = true)]
    pub owner: Option<IdentityOwner>,
    pub signature: IdentitySignature,
    pub send_policy: IdentitySendPolicy,
    pub retention: IdentityRetention,
    pub metadata: Metadata,
    /// The integrator's idempotent-create key.
    #[schema(required = true)]
    pub client_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AddressRole {
    #[serde(rename = "primary")]
    Primary,
    #[serde(rename = "alias")]
    Alias,
}

/// `pending` until the domain is healthy; `active`; `retiring` (still receives, until `retire_at`); `retired` (inbound refused with `550 5.1.6`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AddressStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "retiring")]
    Retiring,
    #[serde(rename = "retired")]
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AddressForwardingValue {
    #[serde(rename = "unverified")]
    Unverified,
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "failed")]
    Failed,
}

/// `null` when the address's domain does not use `inbound: forward`. Otherwise `unverified` (no forwarding test and no forwarded message has arrived yet), `ok` (the last test passed, or mail arrived through forwarding) or `failed` (the last test timed out after 10 minutes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddressForwarding(pub Option<AddressForwardingValue>);

impl utoipa::PartialSchema for AddressForwarding {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"description": "`null` when the address's domain does not use `inbound: forward`. Otherwise `unverified` (no forwarding test and no forwarded message has arrived yet), `ok` (the last test passed, or mail arrived through forwarding) or `failed` (the last test timed out after 10 minutes).", "enum": ["unverified", "ok", "failed", null], "type": ["string", "null"]}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for AddressForwarding {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Address {
    pub id: AddressId,
    pub identity_id: IdentityId,
    pub address: EmailAddress,
    pub local_part: String,
    pub domain_id: DomainId,
    pub role: AddressRole,
    pub status: AddressStatus,
    /// When a `retiring` address becomes `retired`.
    #[schema(required = true)]
    pub retire_at: Option<String>,
    #[schema(required = true)]
    pub retired_at: Option<String>,
    pub forwarding: AddressForwarding,
    /// When `forwarding` last changed.
    #[schema(required = true)]
    pub forwarding_checked_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum ErasureScope {
    #[serde(rename = "message")]
    #[default]
    Message,
    #[serde(rename = "thread")]
    Thread,
    #[serde(rename = "counterparty")]
    Counterparty,
    #[serde(rename = "identity")]
    Identity,
    #[serde(rename = "tenant")]
    Tenant,
}

/// `completed_with_holds`: the erasure finished but skipped held threads, listed in the receipt's `held` (a tenant then stays `erasing` until no hold remains). `failed`: a step still failed 20 hours after the first request. `canceled`: a tenant erasure superseded it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum ErasureStatus {
    #[serde(rename = "queued")]
    #[default]
    Queued,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "completed_with_holds")]
    CompletedWithHolds,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "canceled")]
    Canceled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureHeldItem {
    pub thread_id: ThreadId,
    pub reason: String,
}

/// Results of keyword and semantic probe queries run after the erasure, and the event-log probe. All must be 0 (FR-SRCH-11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureReceiptProbe {
    pub keyword_hits: i64,
    pub semantic_hits: i64,
    pub log_hits: i64,
}

fn schema_erasure_receipt_probe_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "Results of keyword and semantic probe queries run after the erasure, and the event-log probe. All must be 0 (FR-SRCH-11).", "properties": {"keyword_hits": {"minimum": 0, "type": "integer"}, "log_hits": {"minimum": 0, "type": "integer"}, "semantic_hits": {"minimum": 0, "type": "integer"}}, "required": ["keyword_hits", "semantic_hits", "log_hits"], "type": "object"}"###).expect("the contract's schema")
}

/// What was deleted in each store (FR-PRV-3), the held items skipped (FR-PRV-4), and the post-erasure probe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureReceipt {
    pub messages_deleted: i64,
    pub attachments_deleted: i64,
    pub r2_objects_deleted: i64,
    pub fts_rows_deleted: i64,
    pub refs_deleted: i64,
    pub vectors_deleted: i64,
    pub events_deleted: i64,
    /// Rows deleted from the tenant's event log.
    pub log_events_deleted: i64,
    /// Drafts deleted (including by `WipeAll`).
    pub drafts_deleted: i64,
    /// Upload rows deleted (including by `WipeAll`).
    pub uploads_deleted: i64,
    /// Screening records deleted with their messages.
    pub screenings_deleted: i64,
    /// Screening feedback rows deleted with their messages.
    pub screening_feedback_deleted: i64,
    /// Allow and block list entries deleted (identity and tenant scope).
    pub list_entries_deleted: i64,
    pub identities_affected: Vec<IdentityId>,
    pub held: Vec<ErasureHeldItem>,
    /// Results of keyword and semantic probe queries run after the erasure, and the event-log probe. All must be 0 (FR-SRCH-11).
    #[schema(schema_with = schema_erasure_receipt_probe_property)]
    pub probe: ErasureReceiptProbe,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityPage {
    pub data: Vec<Identity>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

impl std::fmt::Display for AddressId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for DomainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for ErasureRequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for Username {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for UsernameInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for DisplayName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
