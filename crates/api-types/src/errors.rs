//! The error envelope and the error codes (FR-API-2, [Errors]).
//!
//! [Errors]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/errors.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErrorError {
    pub code: ErrorCode,
    /// Human-readable. It can change. Never parse it.
    pub message: String,
    /// `true`: the same request can succeed later unchanged (retry with backoff, and with the same `Idempotency-Key` for sends). `false`: the request has to change.
    pub retryable: bool,
    /// One sentence telling a developer, or an agent, what to do.
    pub fix: String,
    /// The code's entry on the published errors page, `{PM_DOCS_URL}/reference/errors#code-{code}`.
    pub docs: String,
    pub request_id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = ErrorDetails)]
    pub details: Option<ErrorDetails>,
}

fn schema_error_error_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"code": {"$ref": "#/components/schemas/ErrorCode"}, "details": {"$ref": "#/components/schemas/ErrorDetails"}, "docs": {"description": "The code's entry on the published errors page, `{PM_DOCS_URL}/reference/errors#code-{code}`.", "examples": ["https://pylotamail.com/docs/reference/errors#code-idempotency_conflict"], "format": "uri", "type": "string"}, "fix": {"description": "One sentence telling a developer, or an agent, what to do.", "type": "string"}, "message": {"description": "Human-readable. It can change. Never parse it.", "type": "string"}, "request_id": {"$ref": "#/components/schemas/RequestId"}, "retryable": {"description": "`true`: the same request can succeed later unchanged (retry with backoff, and with the same `Idempotency-Key` for sends). `false`: the request has to change.", "type": "boolean"}}, "required": ["code", "message", "retryable", "fix", "docs", "request_id"], "type": "object"}"###).expect("the contract's schema")
}

/// The envelope of every non-2xx response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Error {
    #[schema(schema_with = schema_error_error_property)]
    pub error: ErrorError,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErrorDetailsErrorsItem {
    /// Where in the request the problem is.
    pub path: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErrorDetailsCeilingSource {
    #[serde(rename = "deployment")]
    Deployment,
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "partner")]
    Partner,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ErrorDetailsStatus {
    ServiceAccountStatus(ServiceAccountStatus),
    DraftStatus(DraftStatus),
    UploadStatus(UploadStatus),
    MessageStatus(MessageStatus),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErrorDetailsRecordsItemType {
    #[serde(rename = "A")]
    A,
    #[serde(rename = "AAAA")]
    AAAA,
    #[serde(rename = "MX")]
    MX,
    #[serde(rename = "CNAME")]
    CNAME,
}

fn schema_error_details_records_item_type_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["A", "AAAA", "MX", "CNAME"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErrorDetailsRecordsItem {
    #[schema(schema_with = schema_error_details_records_item_type_property)]
    pub r#type: ErrorDetailsRecordsItemType,
    pub name: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErrorDetailsCap {
    #[serde(rename = "identity")]
    Identity,
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "invitations")]
    Invitations,
    #[serde(rename = "shared_domain")]
    SharedDomain,
    #[serde(rename = "billing_dispute")]
    BillingDispute,
}

/// Optional, code-specific details. Known keys are listed; other keys can appear.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorDetails {
    /// Every other entry.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
    /// `permission_denied`: always an array: every permission the key lacks (one element when it lacks one); on a route that takes any one of several permissions (the upload routes), all of them, with `any_of: true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<Permission>>,
    /// `permission_denied`: `true` when the key needs any one of the permissions in `required`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub any_of: Option<bool>,
    /// `invalid_request`: one entry per schema violation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<ErrorDetailsErrorsItem>>,
    /// `invalid_query`: the character offset in `q` where parsing failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    /// `invalid_query`: what the parser expected at `position`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<serde_json::Value>,
    /// `idempotency_conflict`: the message created by the original request, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_message_id: Option<MessageId>,
    /// `identity_paused`: why the identity is paused. `transport_unavailable`: why the deployment or the domain's method cannot do what was asked. `invalid_request` on `POST /v1/keys`: `permission_not_allowed_for_level` when `permissions` lists one the new key's level cannot hold. `scope_denied` on a domain create: `zone_not_allowed` when a non-platform key names a Cloudflare zone its tenant may not use (`cloudflare_zone`, `replace_mx`; see `TenantPolicy.domains`). `scope_denied` on a policy write: `not_writable` or `above_ceiling`. `permission_denied`: `person_required` when only a signed-in person may take the decision (a guard-field loosening by a tenant key, a service-account approval by an API key, a draft approval by an API key), or `self_approval` when a key approves draft content it wrote, `consumer_owner_only` on a durable event consumer, `signup_unverified` for a key of a provisional agent sign-up. `policy_denied` on `wait`: `account_not_approved`; on a send: `signup_unverified`, `signup_human_unreachable` or `signup_paused`. `invalid_request` on `requestServiceAccount`: `own_deployment`; on the event feed: `cursor_ahead`, `credential_in_url`, `two_credentials`, `subprotocol_required`; on `POST …/feedback`: `feedback_not_applicable`; on `batch-update`: `label_limit`; on a list or retention change of the system identity: `system_identity`; on a draft's `send_at`: `beyond_retention`; on `POST /v1/keys`: `unknown_mcp_tool`, `mcp_tool_not_permitted`. `approval_required`: the approval level (`new_recipients`, `external`, `always`). `draft_revision_mismatch`: `digest`. `source_unavailable`: `erased`, `not_visible`, `raw_expired`, `attachment_missing`. `cursor_expired` on the event feed: `retention`, `size_limit`, `log_reset`. `too_many_recipients`: `calendar_single_recipient`. `header_not_allowed` on a webhook endpoint: `webhook_header`. `message_too_large`: `attachments_total`, `transport_limit`. `upload_size_mismatch`: `content_length`, `encoding`, `incomplete`. `upload_expired`: `expired`, `too_many_attempts`, `object_missing`. `attachment_risky`: `infected`. `signup_closed`: `disabled`, `paused`. `signup_cannot_verify`: `workspace_limit`, `human_email_hosted`, `person_disabled`. `signup_code_invalid`: `wrong`, `burned`, `locked`, `expired`, `superseded`, `none`. `invalid_request` on `POST /v1/agent-signup`: `human_email_not_allowed`, `no_mail_domain`. `calendar_invalid` and the other codes list their own values in docs › Errors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// `scope_denied`: the request field the key may not set to that value, as a dotted path: a platform-only or lower-only policy field written by a partner key (`policy.tenant_daily_send_cap` is reported as `tenant_daily_send_cap`), `send_policy.daily_cap` above the tenant's identity cap, or `status` when a partner key tries to lift a platform suspension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// `partner_tenant_limit`: the partner's `max_tenants`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tenants: Option<i64>,
    /// `scope_denied` with `above_ceiling`: the loosest value the caller may set on `field`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<serde_json::Value>,
    /// `scope_denied` with `above_ceiling`: which bound decided the ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_source: Option<ErrorDetailsCeilingSource>,
    /// `policy_denied` with `account_not_approved`: the organisational domain that has no approved entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_domain: Option<String>,
    /// `account_exists`: the existing entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<ServiceAccountId>,
    /// `account_exists`, `account_not_pending` and `account_limit_reached`: the state of the entry, or the state that hit the limit. `draft_not_editable`, `draft_not_sendable` and `draft_not_pending`: the draft's status. `upload_not_ready`: the upload's status. `not_quarantined`: the message's status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ErrorDetailsStatus>,
    /// `account_limit_reached`: the limit that was reached (10 pending, or 200 in any state). The other `*_limit_reached` codes: the limit; `upload_limit_reached` and `webhook_header_limit_reached` name which limit (`count` or `bytes`) and give `max`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<serde_json::Value>,
    /// `domain_not_dedicated`: the records found at the name (A, AAAA, MX) and at `www` (CNAME, A) that moving the nameservers would take over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<Vec<ErrorDetailsRecordsItem>>,
    /// `rate_limited` and `thread_busy`: seconds to wait before retrying. `upstream_rate_limited`: as in `Retry-After`: `10800` (3 hours) after a Cloudflare 1105, or the wait for the next SES control-plane slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<i64>,
    /// `daily_cap_reached` and `billing_limit`: when the cap or allowance resets (`null` for counts such as `inboxes`, and for `daily_cap_reached` with `cap: billing_dispute`, which ends when the dispute does).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    pub resets_at: Option<Option<String>>,
    /// `daily_cap_reached`: which cap refused the request. `identity` and `tenant` are the daily send caps of tenant policy (the tenant one lowered by the new-workspace ramp); `invitations` is the tenant's 50 invitation emails a day; `shared_domain` is the deployment's shared-domain breaker; `billing_dispute` stops a workspace's sends while one of its payments is disputed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<ErrorDetailsCap>,
    /// `billing_limit`: the spent allowance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<UsageFeatureName>,
    /// `billing_limit`: the allowance, top-ups included.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    pub granted: Option<Option<i64>>,
    /// `billing_limit`: how much of it is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<i64>,
    /// `billing_limit`: the console plan page, or `null` when the console is off.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    pub upgrade_url: Option<Option<String>>,
    /// `partner_has_tenants`: how many of the partner's tenants are not erased yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenants: Option<i64>,
    /// `spf_lookup_limit`: the DNS lookups the merged SPF record would need.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookups: Option<i64>,
}

impl utoipa::PartialSchema for ErrorDetails {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"additionalProperties": true, "description": "Optional, code-specific details. Known keys are listed; other keys can appear.", "properties": {"account_id": {"$ref": "#/components/schemas/ServiceAccountId", "description": "`account_exists`: the existing entry."}, "any_of": {"description": "`permission_denied`: `true` when the key needs any one of the permissions in `required`.", "type": "boolean"}, "cap": {"description": "`daily_cap_reached`: which cap refused the request. `identity` and `tenant` are the daily send caps of tenant policy (the tenant one lowered by the new-workspace ramp); `invitations` is the tenant's 50 invitation emails a day; `shared_domain` is the deployment's shared-domain breaker; `billing_dispute` stops a workspace's sends while one of its payments is disputed.", "enum": ["identity", "tenant", "invitations", "shared_domain", "billing_dispute"], "type": "string"}, "ceiling": {"description": "`scope_denied` with `above_ceiling`: the loosest value the caller may set on `field`.", "type": ["number", "boolean"]}, "ceiling_source": {"description": "`scope_denied` with `above_ceiling`: which bound decided the ceiling.", "enum": ["deployment", "platform", "partner"], "type": "string"}, "errors": {"description": "`invalid_request`: one entry per schema violation.", "items": {"properties": {"message": {"type": "string"}, "path": {"description": "Where in the request the problem is.", "type": "string"}}, "required": ["path", "message"], "type": "object"}, "type": "array"}, "expected": {"description": "`invalid_query`: what the parser expected at `position`.", "type": ["string", "number", "boolean", "object", "array", "null"]}, "feature": {"$ref": "#/components/schemas/UsageFeatureName", "description": "`billing_limit`: the spent allowance."}, "field": {"description": "`scope_denied`: the request field the key may not set to that value, as a dotted path: a platform-only or lower-only policy field written by a partner key (`policy.tenant_daily_send_cap` is reported as `tenant_daily_send_cap`), `send_policy.daily_cap` above the tenant's identity cap, or `status` when a partner key tries to lift a platform suspension.", "examples": ["tenant_daily_send_cap", "send_policy.daily_cap", "status"], "type": "string"}, "granted": {"description": "`billing_limit`: the allowance, top-ups included.", "minimum": 0, "type": ["integer", "null"]}, "limit": {"anyOf": [{"minimum": 1, "type": "integer"}, {"enum": ["count", "bytes"], "type": "string"}], "description": "`account_limit_reached`: the limit that was reached (10 pending, or 200 in any state). The other `*_limit_reached` codes: the limit; `upload_limit_reached` and `webhook_header_limit_reached` name which limit (`count` or `bytes`) and give `max`."}, "lookups": {"description": "`spf_lookup_limit`: the DNS lookups the merged SPF record would need.", "minimum": 0, "type": "integer"}, "max_tenants": {"description": "`partner_tenant_limit`: the partner's `max_tenants`.", "minimum": 0, "type": "integer"}, "original_message_id": {"$ref": "#/components/schemas/MessageId", "description": "`idempotency_conflict`: the message created by the original request, when known."}, "position": {"description": "`invalid_query`: the character offset in `q` where parsing failed.", "minimum": 0, "type": "integer"}, "reason": {"anyOf": [{"$ref": "#/components/schemas/PauseReason"}, {"$ref": "#/components/schemas/TransportUnavailableReason"}, {"$ref": "#/components/schemas/RequireApproval"}, {"enum": ["permission_not_allowed_for_level"], "type": "string"}, {"enum": ["zone_not_allowed"], "type": "string"}, {"enum": ["not_writable", "above_ceiling", "person_required", "account_not_approved", "own_deployment"], "type": "string"}, {"enum": ["self_approval", "consumer_owner_only", "signup_unverified", "signup_human_unreachable", "signup_paused", "cursor_ahead", "credential_in_url", "two_credentials", "subprotocol_required", "feedback_not_applicable", "label_limit", "system_identity", "beyond_retention", "unknown_mcp_tool", "mcp_tool_not_permitted", "digest", "erased", "not_visible", "raw_expired", "attachment_missing", "retention", "size_limit", "log_reset", "calendar_single_recipient", "webhook_header", "attachments_total", "transport_limit", "content_length", "encoding", "incomplete", "expired", "too_many_attempts", "object_missing", "infected", "disabled", "paused", "workspace_limit", "human_email_hosted", "person_disabled", "wrong", "burned", "locked", "superseded", "none", "human_email_not_allowed", "no_mail_domain"], "type": "string"}, {"description": "Any other code-specific reason listed in docs › Errors.", "type": "string"}], "description": "`identity_paused`: why the identity is paused. `transport_unavailable`: why the deployment or the domain's method cannot do what was asked. `invalid_request` on `POST /v1/keys`: `permission_not_allowed_for_level` when `permissions` lists one the new key's level cannot hold. `scope_denied` on a domain create: `zone_not_allowed` when a non-platform key names a Cloudflare zone its tenant may not use (`cloudflare_zone`, `replace_mx`; see `TenantPolicy.domains`). `scope_denied` on a policy write: `not_writable` or `above_ceiling`. `permission_denied`: `person_required` when only a signed-in person may take the decision (a guard-field loosening by a tenant key, a service-account approval by an API key, a draft approval by an API key), or `self_approval` when a key approves draft content it wrote, `consumer_owner_only` on a durable event consumer, `signup_unverified` for a key of a provisional agent sign-up. `policy_denied` on `wait`: `account_not_approved`; on a send: `signup_unverified`, `signup_human_unreachable` or `signup_paused`. `invalid_request` on `requestServiceAccount`: `own_deployment`; on the event feed: `cursor_ahead`, `credential_in_url`, `two_credentials`, `subprotocol_required`; on `POST …/feedback`: `feedback_not_applicable`; on `batch-update`: `label_limit`; on a list or retention change of the system identity: `system_identity`; on a draft's `send_at`: `beyond_retention`; on `POST /v1/keys`: `unknown_mcp_tool`, `mcp_tool_not_permitted`. `approval_required`: the approval level (`new_recipients`, `external`, `always`). `draft_revision_mismatch`: `digest`. `source_unavailable`: `erased`, `not_visible`, `raw_expired`, `attachment_missing`. `cursor_expired` on the event feed: `retention`, `size_limit`, `log_reset`. `too_many_recipients`: `calendar_single_recipient`. `header_not_allowed` on a webhook endpoint: `webhook_header`. `message_too_large`: `attachments_total`, `transport_limit`. `upload_size_mismatch`: `content_length`, `encoding`, `incomplete`. `upload_expired`: `expired`, `too_many_attempts`, `object_missing`. `attachment_risky`: `infected`. `signup_closed`: `disabled`, `paused`. `signup_cannot_verify`: `workspace_limit`, `human_email_hosted`, `person_disabled`. `signup_code_invalid`: `wrong`, `burned`, `locked`, `expired`, `superseded`, `none`. `invalid_request` on `POST /v1/agent-signup`: `human_email_not_allowed`, `no_mail_domain`. `calendar_invalid` and the other codes list their own values in docs › Errors."}, "records": {"description": "`domain_not_dedicated`: the records found at the name (A, AAAA, MX) and at `www` (CNAME, A) that moving the nameservers would take over.", "items": {"properties": {"name": {"examples": ["brightwell-agents.example"], "type": "string"}, "type": {"enum": ["A", "AAAA", "MX", "CNAME"], "type": "string"}, "value": {"examples": ["203.0.113.10"], "type": "string"}}, "required": ["type", "name", "value"], "type": "object"}, "type": "array"}, "required": {"description": "`permission_denied`: always an array: every permission the key lacks (one element when it lacks one); on a route that takes any one of several permissions (the upload routes), all of them, with `any_of: true`.", "items": {"$ref": "#/components/schemas/Permission"}, "minItems": 1, "type": "array"}, "resets_at": {"description": "`daily_cap_reached` and `billing_limit`: when the cap or allowance resets (`null` for counts such as `inboxes`, and for `daily_cap_reached` with `cap: billing_dispute`, which ends when the dispute does).", "format": "date-time", "type": ["string", "null"]}, "retry_after": {"description": "`rate_limited` and `thread_busy`: seconds to wait before retrying. `upstream_rate_limited`: as in `Retry-After`: `10800` (3 hours) after a Cloudflare 1105, or the wait for the next SES control-plane slot.", "minimum": 0, "type": "integer"}, "service_domain": {"description": "`policy_denied` with `account_not_approved`: the organisational domain that has no approved entry.", "type": "string"}, "status": {"anyOf": [{"$ref": "#/components/schemas/ServiceAccountStatus"}, {"$ref": "#/components/schemas/DraftStatus"}, {"$ref": "#/components/schemas/UploadStatus"}, {"$ref": "#/components/schemas/MessageStatus"}], "description": "`account_exists`, `account_not_pending` and `account_limit_reached`: the state of the entry, or the state that hit the limit. `draft_not_editable`, `draft_not_sendable` and `draft_not_pending`: the draft's status. `upload_not_ready`: the upload's status. `not_quarantined`: the message's status."}, "tenants": {"description": "`partner_has_tenants`: how many of the partner's tenants are not erased yet.", "minimum": 1, "type": "integer"}, "upgrade_url": {"description": "`billing_limit`: the console plan page, or `null` when the console is off.", "format": "uri", "type": ["string", "null"]}, "used": {"description": "`billing_limit`: how much of it is used.", "minimum": 0, "type": "integer"}}, "type": "object"}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for ErrorDetails {}

/// Stable, machine-readable error code. New codes can be added, so handle unknown codes by HTTP status. The catalogue, with HTTP status and retryability, is in `docs/src/reference/errors.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErrorCode {
    #[serde(rename = "unauthenticated")]
    Unauthenticated,
    #[serde(rename = "key_expired")]
    KeyExpired,
    #[serde(rename = "key_revoked")]
    KeyRevoked,
    #[serde(rename = "permission_denied")]
    PermissionDenied,
    #[serde(rename = "scope_denied")]
    ScopeDenied,
    #[serde(rename = "key_scope_exceeded")]
    KeyScopeExceeded,
    #[serde(rename = "tenant_suspended")]
    TenantSuspended,
    #[serde(rename = "partner_suspended")]
    PartnerSuspended,
    #[serde(rename = "partner_tenant_limit")]
    PartnerTenantLimit,
    #[serde(rename = "test_mode_recipient")]
    TestModeRecipient,
    #[serde(rename = "invalid_signature")]
    InvalidSignature,
    #[serde(rename = "recipient_opted_out")]
    RecipientOptedOut,
    #[serde(rename = "consent_required")]
    ConsentRequired,
    #[serde(rename = "a2p_not_registered")]
    A2pNotRegistered,
    #[serde(rename = "destination_blocked")]
    DestinationBlocked,
    #[serde(rename = "consumer_unauthenticated")]
    ConsumerUnauthenticated,
    #[serde(rename = "vault_not_granted")]
    VaultNotGranted,
    #[serde(rename = "vault_refused")]
    VaultRefused,
    #[serde(rename = "consumer_not_active")]
    ConsumerNotActive,
    #[serde(rename = "vault_unlock_required")]
    VaultUnlockRequired,
    #[serde(rename = "vault_unlock_invalid")]
    VaultUnlockInvalid,
    #[serde(rename = "policy_denied")]
    PolicyDenied,
    #[serde(rename = "approval_required")]
    ApprovalRequired,
    #[serde(rename = "signup_closed")]
    SignupClosed,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "idempotency_key_required")]
    IdempotencyKeyRequired,
    #[serde(rename = "invalid_idempotency_key")]
    InvalidIdempotencyKey,
    #[serde(rename = "invalid_query")]
    InvalidQuery,
    #[serde(rename = "address_invalid")]
    AddressInvalid,
    #[serde(rename = "address_unsupported")]
    AddressUnsupported,
    #[serde(rename = "address_reserved")]
    AddressReserved,
    #[serde(rename = "local_part_too_long")]
    LocalPartTooLong,
    #[serde(rename = "header_not_allowed")]
    HeaderNotAllowed,
    #[serde(rename = "marketing_requirements_missing")]
    MarketingRequirementsMissing,
    #[serde(rename = "too_many_recipients")]
    TooManyRecipients,
    #[serde(rename = "smtp_port_not_allowed")]
    SmtpPortNotAllowed,
    #[serde(rename = "spf_lookup_limit")]
    SpfLookupLimit,
    #[serde(rename = "message_too_large")]
    MessageTooLarge,
    #[serde(rename = "payload_too_large")]
    PayloadTooLarge,
    #[serde(rename = "scope_too_large")]
    ScopeTooLarge,
    #[serde(rename = "calendar_invalid")]
    CalendarInvalid,
    #[serde(rename = "calendar_organizer_mismatch")]
    CalendarOrganizerMismatch,
    #[serde(rename = "calendar_attendee_mismatch")]
    CalendarAttendeeMismatch,
    #[serde(rename = "calendar_recipient_mismatch")]
    CalendarRecipientMismatch,
    #[serde(rename = "calendar_too_large")]
    CalendarTooLarge,
    #[serde(rename = "upload_size_mismatch")]
    UploadSizeMismatch,
    #[serde(rename = "upload_checksum_mismatch")]
    UploadChecksumMismatch,
    #[serde(rename = "phone_number_invalid")]
    PhoneNumberInvalid,
    #[serde(rename = "query_range_too_wide")]
    QueryRangeTooWide,
    #[serde(rename = "upgrade_required")]
    UpgradeRequired,
    #[serde(rename = "tenant_not_found")]
    TenantNotFound,
    #[serde(rename = "identity_not_found")]
    IdentityNotFound,
    #[serde(rename = "address_not_found")]
    AddressNotFound,
    #[serde(rename = "domain_not_found")]
    DomainNotFound,
    #[serde(rename = "thread_not_found")]
    ThreadNotFound,
    #[serde(rename = "message_not_found")]
    MessageNotFound,
    #[serde(rename = "attachment_not_found")]
    AttachmentNotFound,
    #[serde(rename = "webhook_not_found")]
    WebhookNotFound,
    #[serde(rename = "key_not_found")]
    KeyNotFound,
    #[serde(rename = "erasure_not_found")]
    ErasureNotFound,
    #[serde(rename = "export_not_found")]
    ExportNotFound,
    #[serde(rename = "suppression_not_found")]
    SuppressionNotFound,
    #[serde(rename = "list_entry_not_found")]
    ListEntryNotFound,
    #[serde(rename = "member_not_found")]
    MemberNotFound,
    #[serde(rename = "invitation_not_found")]
    InvitationNotFound,
    #[serde(rename = "job_not_found")]
    JobNotFound,
    #[serde(rename = "dlq_item_not_found")]
    DlqItemNotFound,
    #[serde(rename = "partner_not_found")]
    PartnerNotFound,
    #[serde(rename = "route_not_found")]
    RouteNotFound,
    #[serde(rename = "method_not_allowed")]
    MethodNotAllowed,
    #[serde(rename = "account_not_found")]
    AccountNotFound,
    #[serde(rename = "draft_not_found")]
    DraftNotFound,
    #[serde(rename = "event_consumer_not_found")]
    EventConsumerNotFound,
    #[serde(rename = "signup_not_found")]
    SignupNotFound,
    #[serde(rename = "phone_number_not_found")]
    PhoneNumberNotFound,
    #[serde(rename = "phone_request_not_found")]
    PhoneRequestNotFound,
    #[serde(rename = "sms_not_found")]
    SmsNotFound,
    #[serde(rename = "call_not_found")]
    CallNotFound,
    #[serde(rename = "phone_document_not_found")]
    PhoneDocumentNotFound,
    #[serde(rename = "port_out_not_found")]
    PortOutNotFound,
    #[serde(rename = "voice_connector_not_found")]
    VoiceConnectorNotFound,
    #[serde(rename = "vault_item_not_found")]
    VaultItemNotFound,
    #[serde(rename = "vault_grant_not_found")]
    VaultGrantNotFound,
    #[serde(rename = "vault_use_not_found")]
    VaultUseNotFound,
    #[serde(rename = "vault_consumer_not_found")]
    VaultConsumerNotFound,
    #[serde(rename = "vault_ticket_not_found")]
    VaultTicketNotFound,
    #[serde(rename = "upload_not_found")]
    UploadNotFound,
    #[serde(rename = "idempotency_conflict")]
    IdempotencyConflict,
    #[serde(rename = "request_in_progress")]
    RequestInProgress,
    #[serde(rename = "client_id_conflict")]
    ClientIdConflict,
    #[serde(rename = "username_taken")]
    UsernameTaken,
    #[serde(rename = "slug_taken")]
    SlugTaken,
    #[serde(rename = "suffix_taken")]
    SuffixTaken,
    #[serde(rename = "not_quarantined")]
    NotQuarantined,
    #[serde(rename = "not_screened")]
    NotScreened,
    #[serde(rename = "screening_pending")]
    ScreeningPending,
    #[serde(rename = "domain_not_suspended")]
    DomainNotSuspended,
    #[serde(rename = "existing_mx")]
    ExistingMx,
    #[serde(rename = "domain_not_dedicated")]
    DomainNotDedicated,
    #[serde(rename = "zone_hold")]
    ZoneHold,
    #[serde(rename = "owner_required")]
    OwnerRequired,
    #[serde(rename = "partner_has_tenants")]
    PartnerHasTenants,
    #[serde(rename = "system_protected")]
    SystemProtected,
    #[serde(rename = "tenant_erased")]
    TenantErased,
    #[serde(rename = "plan_managed_by_stripe")]
    PlanManagedByStripe,
    #[serde(rename = "address_taken")]
    AddressTaken,
    #[serde(rename = "address_is_primary")]
    AddressIsPrimary,
    #[serde(rename = "address_in_use")]
    AddressInUse,
    #[serde(rename = "domain_not_ready")]
    DomainNotReady,
    #[serde(rename = "domain_in_use")]
    DomainInUse,
    #[serde(rename = "domain_exists")]
    DomainExists,
    #[serde(rename = "domain_claim_pending")]
    DomainClaimPending,
    #[serde(rename = "identity_paused")]
    IdentityPaused,
    #[serde(rename = "identity_owner_required")]
    IdentityOwnerRequired,
    #[serde(rename = "thread_busy")]
    ThreadBusy,
    #[serde(rename = "not_cancelable")]
    NotCancelable,
    #[serde(rename = "not_uncertain")]
    NotUncertain,
    #[serde(rename = "auto_reply_not_allowed")]
    AutoReplyNotAllowed,
    #[serde(rename = "account_exists")]
    AccountExists,
    #[serde(rename = "account_not_pending")]
    AccountNotPending,
    #[serde(rename = "draft_revision_mismatch")]
    DraftRevisionMismatch,
    #[serde(rename = "draft_not_editable")]
    DraftNotEditable,
    #[serde(rename = "draft_not_sendable")]
    DraftNotSendable,
    #[serde(rename = "draft_not_pending")]
    DraftNotPending,
    #[serde(rename = "source_unavailable")]
    SourceUnavailable,
    #[serde(rename = "event_consumer_name_taken")]
    EventConsumerNameTaken,
    #[serde(rename = "calendar_sequence_stale")]
    CalendarSequenceStale,
    #[serde(rename = "calendar_not_invitation")]
    CalendarNotInvitation,
    #[serde(rename = "calendar_not_verified")]
    CalendarNotVerified,
    #[serde(rename = "calendar_not_attendee")]
    CalendarNotAttendee,
    #[serde(rename = "calendar_cancelled")]
    CalendarCancelled,
    #[serde(rename = "calendar_superseded")]
    CalendarSuperseded,
    #[serde(rename = "calendar_event_past")]
    CalendarEventPast,
    #[serde(rename = "calendar_attendee_address_unavailable")]
    CalendarAttendeeAddressUnavailable,
    #[serde(rename = "upload_not_ready")]
    UploadNotReady,
    #[serde(rename = "upload_in_progress")]
    UploadInProgress,
    #[serde(rename = "upload_in_use")]
    UploadInUse,
    #[serde(rename = "signup_not_pending")]
    SignupNotPending,
    #[serde(rename = "signup_cannot_verify")]
    SignupCannotVerify,
    #[serde(rename = "signup_human_unreachable")]
    SignupHumanUnreachable,
    #[serde(rename = "phone_number_not_ready")]
    PhoneNumberNotReady,
    #[serde(rename = "phone_number_taken")]
    PhoneNumberTaken,
    #[serde(rename = "phone_request_not_pending")]
    PhoneRequestNotPending,
    #[serde(rename = "phone_request_digest_mismatch")]
    PhoneRequestDigestMismatch,
    #[serde(rename = "compliance_not_approved")]
    ComplianceNotApproved,
    #[serde(rename = "quiet_hours")]
    QuietHours,
    #[serde(rename = "identity_has_phone_numbers")]
    IdentityHasPhoneNumbers,
    #[serde(rename = "voice_route_unsupported")]
    VoiceRouteUnsupported,
    #[serde(rename = "phone_document_in_use")]
    PhoneDocumentInUse,
    #[serde(rename = "port_in_not_cancelable")]
    PortInNotCancelable,
    #[serde(rename = "port_out_closed")]
    PortOutClosed,
    #[serde(rename = "vault_grant_exists")]
    VaultGrantExists,
    #[serde(rename = "vault_not_pending")]
    VaultNotPending,
    #[serde(rename = "vault_phase_spent")]
    VaultPhaseSpent,
    #[serde(rename = "vault_totp_step_used")]
    VaultTotpStepUsed,
    #[serde(rename = "loop_detected")]
    LoopDetected,
    #[serde(rename = "sending_paused")]
    SendingPaused,
    #[serde(rename = "triage_not_eligible")]
    TriageNotEligible,
    #[serde(rename = "raw_expired")]
    RawExpired,
    #[serde(rename = "message_expired")]
    MessageExpired,
    #[serde(rename = "upload_expired")]
    UploadExpired,
    #[serde(rename = "phone_quote_expired")]
    PhoneQuoteExpired,
    #[serde(rename = "vault_ticket_expired")]
    VaultTicketExpired,
    #[serde(rename = "cursor_expired")]
    CursorExpired,
    #[serde(rename = "legal_hold")]
    LegalHold,
    #[serde(rename = "not_frozen")]
    NotFrozen,
    #[serde(rename = "erasure_not_completed")]
    ErasureNotCompleted,
    #[serde(rename = "all_recipients_suppressed")]
    AllRecipientsSuppressed,
    #[serde(rename = "recipient_blocked")]
    RecipientBlocked,
    #[serde(rename = "transport_unavailable")]
    TransportUnavailable,
    #[serde(rename = "smtp_tls_required")]
    SmtpTlsRequired,
    #[serde(rename = "smtp_auth_failed")]
    SmtpAuthFailed,
    #[serde(rename = "cf_token_required")]
    CfTokenRequired,
    #[serde(rename = "agentic_disabled")]
    AgenticDisabled,
    #[serde(rename = "address_limit_reached")]
    AddressLimitReached,
    #[serde(rename = "unverified_domain_limit")]
    UnverifiedDomainLimit,
    #[serde(rename = "zone_domain_limit")]
    ZoneDomainLimit,
    #[serde(rename = "account_limit_reached")]
    AccountLimitReached,
    #[serde(rename = "draft_limit_reached")]
    DraftLimitReached,
    #[serde(rename = "event_consumer_limit_reached")]
    EventConsumerLimitReached,
    #[serde(rename = "feedback_limit_reached")]
    FeedbackLimitReached,
    #[serde(rename = "upload_limit_reached")]
    UploadLimitReached,
    #[serde(rename = "attachment_risky")]
    AttachmentRisky,
    #[serde(rename = "signup_code_invalid")]
    SignupCodeInvalid,
    #[serde(rename = "signup_replace_limit")]
    SignupReplaceLimit,
    #[serde(rename = "signup_limit_reached")]
    SignupLimitReached,
    #[serde(rename = "webhook_limit_reached")]
    WebhookLimitReached,
    #[serde(rename = "list_limit_reached")]
    ListLimitReached,
    #[serde(rename = "webhook_header_limit_reached")]
    WebhookHeaderLimitReached,
    #[serde(rename = "key_limit_reached")]
    KeyLimitReached,
    #[serde(rename = "web_bot_auth_disabled")]
    WebBotAuthDisabled,
    #[serde(rename = "country_not_offered")]
    CountryNotOffered,
    #[serde(rename = "local_presence_required")]
    LocalPresenceRequired,
    #[serde(rename = "compliance_incomplete")]
    ComplianceIncomplete,
    #[serde(rename = "phone_registration_otp_invalid")]
    PhoneRegistrationOtpInvalid,
    #[serde(rename = "phone_number_limit_reached")]
    PhoneNumberLimitReached,
    #[serde(rename = "phone_request_limit_reached")]
    PhoneRequestLimitReached,
    #[serde(rename = "compliance_address_invalid")]
    ComplianceAddressInvalid,
    #[serde(rename = "vault_disabled")]
    VaultDisabled,
    #[serde(rename = "vault_kind_refused")]
    VaultKindRefused,
    #[serde(rename = "vault_item_limit_reached")]
    VaultItemLimitReached,
    #[serde(rename = "vault_grant_limit_reached")]
    VaultGrantLimitReached,
    #[serde(rename = "vault_consumer_limit_reached")]
    VaultConsumerLimitReached,
    #[serde(rename = "vault_session_too_large")]
    VaultSessionTooLarge,
    #[serde(rename = "billing_limit")]
    BillingLimit,
    #[serde(rename = "spend_cap_reached")]
    SpendCapReached,
    #[serde(rename = "rate_limited")]
    RateLimited,
    #[serde(rename = "daily_cap_reached")]
    DailyCapReached,
    #[serde(rename = "agentic_budget_exhausted")]
    AgenticBudgetExhausted,
    #[serde(rename = "upstream_rate_limited")]
    UpstreamRateLimited,
    #[serde(rename = "internal_error")]
    InternalError,
    #[serde(rename = "upstream_error")]
    UpstreamError,
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "search_degraded")]
    SearchDegraded,
    #[serde(rename = "timeout")]
    Timeout,
}

impl ErrorCode {
    /// The HTTP status the code is returned with ([Errors › Catalogue]).
    ///
    /// [Errors › Catalogue]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/errors.md#catalogue
    pub const fn http_status(self) -> u16 {
        match self {
            ErrorCode::Unauthenticated => 401,
            ErrorCode::KeyExpired => 401,
            ErrorCode::KeyRevoked => 401,
            ErrorCode::PermissionDenied => 403,
            ErrorCode::ScopeDenied => 403,
            ErrorCode::KeyScopeExceeded => 403,
            ErrorCode::TenantSuspended => 403,
            ErrorCode::PartnerSuspended => 403,
            ErrorCode::PartnerTenantLimit => 403,
            ErrorCode::TestModeRecipient => 403,
            ErrorCode::PolicyDenied => 403,
            ErrorCode::ApprovalRequired => 403,
            ErrorCode::SignupClosed => 403,
            ErrorCode::InvalidSignature => 403,
            ErrorCode::RecipientOptedOut => 403,
            ErrorCode::ConsentRequired => 403,
            ErrorCode::A2pNotRegistered => 403,
            ErrorCode::DestinationBlocked => 403,
            ErrorCode::ConsumerUnauthenticated => 401,
            ErrorCode::VaultNotGranted => 403,
            ErrorCode::VaultRefused => 403,
            ErrorCode::ConsumerNotActive => 403,
            ErrorCode::VaultUnlockRequired => 403,
            ErrorCode::VaultUnlockInvalid => 403,
            ErrorCode::InvalidRequest => 400,
            ErrorCode::IdempotencyKeyRequired => 400,
            ErrorCode::InvalidIdempotencyKey => 400,
            ErrorCode::InvalidQuery => 400,
            ErrorCode::AddressInvalid => 400,
            ErrorCode::AddressUnsupported => 400,
            ErrorCode::AddressReserved => 400,
            ErrorCode::LocalPartTooLong => 400,
            ErrorCode::HeaderNotAllowed => 400,
            ErrorCode::MarketingRequirementsMissing => 400,
            ErrorCode::TooManyRecipients => 400,
            ErrorCode::SmtpPortNotAllowed => 400,
            ErrorCode::SpfLookupLimit => 400,
            ErrorCode::CalendarInvalid => 400,
            ErrorCode::CalendarOrganizerMismatch => 400,
            ErrorCode::CalendarAttendeeMismatch => 400,
            ErrorCode::CalendarRecipientMismatch => 400,
            ErrorCode::UploadSizeMismatch => 400,
            ErrorCode::UploadChecksumMismatch => 400,
            ErrorCode::PhoneNumberInvalid => 400,
            ErrorCode::QueryRangeTooWide => 400,
            ErrorCode::MessageTooLarge => 413,
            ErrorCode::CalendarTooLarge => 413,
            ErrorCode::PayloadTooLarge => 413,
            ErrorCode::ScopeTooLarge => 422,
            ErrorCode::UpgradeRequired => 426,
            ErrorCode::TenantNotFound => 404,
            ErrorCode::IdentityNotFound => 404,
            ErrorCode::AddressNotFound => 404,
            ErrorCode::DomainNotFound => 404,
            ErrorCode::ThreadNotFound => 404,
            ErrorCode::MessageNotFound => 404,
            ErrorCode::AttachmentNotFound => 404,
            ErrorCode::WebhookNotFound => 404,
            ErrorCode::KeyNotFound => 404,
            ErrorCode::ErasureNotFound => 404,
            ErrorCode::ExportNotFound => 404,
            ErrorCode::SuppressionNotFound => 404,
            ErrorCode::ListEntryNotFound => 404,
            ErrorCode::MemberNotFound => 404,
            ErrorCode::InvitationNotFound => 404,
            ErrorCode::JobNotFound => 404,
            ErrorCode::DlqItemNotFound => 404,
            ErrorCode::PartnerNotFound => 404,
            ErrorCode::AccountNotFound => 404,
            ErrorCode::DraftNotFound => 404,
            ErrorCode::EventConsumerNotFound => 404,
            ErrorCode::UploadNotFound => 404,
            ErrorCode::SignupNotFound => 404,
            ErrorCode::PhoneNumberNotFound => 404,
            ErrorCode::PhoneRequestNotFound => 404,
            ErrorCode::SmsNotFound => 404,
            ErrorCode::CallNotFound => 404,
            ErrorCode::PhoneDocumentNotFound => 404,
            ErrorCode::PortOutNotFound => 404,
            ErrorCode::VoiceConnectorNotFound => 404,
            ErrorCode::VaultItemNotFound => 404,
            ErrorCode::VaultGrantNotFound => 404,
            ErrorCode::VaultUseNotFound => 404,
            ErrorCode::VaultConsumerNotFound => 404,
            ErrorCode::VaultTicketNotFound => 404,
            ErrorCode::RouteNotFound => 404,
            ErrorCode::MethodNotAllowed => 405,
            ErrorCode::IdempotencyConflict => 409,
            ErrorCode::RequestInProgress => 409,
            ErrorCode::ClientIdConflict => 409,
            ErrorCode::UsernameTaken => 409,
            ErrorCode::SlugTaken => 409,
            ErrorCode::SuffixTaken => 409,
            ErrorCode::NotQuarantined => 409,
            ErrorCode::NotScreened => 409,
            ErrorCode::ScreeningPending => 409,
            ErrorCode::DomainNotSuspended => 409,
            ErrorCode::ExistingMx => 409,
            ErrorCode::DomainNotDedicated => 409,
            ErrorCode::ZoneHold => 409,
            ErrorCode::OwnerRequired => 409,
            ErrorCode::PlanManagedByStripe => 409,
            ErrorCode::PartnerHasTenants => 409,
            ErrorCode::TenantErased => 409,
            ErrorCode::SystemProtected => 409,
            ErrorCode::AddressTaken => 409,
            ErrorCode::AddressIsPrimary => 409,
            ErrorCode::AddressInUse => 409,
            ErrorCode::DomainNotReady => 409,
            ErrorCode::DomainInUse => 409,
            ErrorCode::DomainExists => 409,
            ErrorCode::DomainClaimPending => 409,
            ErrorCode::IdentityPaused => 409,
            ErrorCode::IdentityOwnerRequired => 409,
            ErrorCode::ThreadBusy => 409,
            ErrorCode::NotCancelable => 409,
            ErrorCode::NotUncertain => 409,
            ErrorCode::AutoReplyNotAllowed => 409,
            ErrorCode::AccountExists => 409,
            ErrorCode::AccountNotPending => 409,
            ErrorCode::DraftRevisionMismatch => 409,
            ErrorCode::DraftNotEditable => 409,
            ErrorCode::DraftNotSendable => 409,
            ErrorCode::DraftNotPending => 409,
            ErrorCode::SourceUnavailable => 409,
            ErrorCode::LoopDetected => 409,
            ErrorCode::SendingPaused => 409,
            ErrorCode::TriageNotEligible => 409,
            ErrorCode::ErasureNotCompleted => 409,
            ErrorCode::NotFrozen => 409,
            ErrorCode::EventConsumerNameTaken => 409,
            ErrorCode::CalendarSequenceStale => 409,
            ErrorCode::CalendarNotInvitation => 409,
            ErrorCode::CalendarNotVerified => 409,
            ErrorCode::CalendarNotAttendee => 409,
            ErrorCode::CalendarCancelled => 409,
            ErrorCode::CalendarSuperseded => 409,
            ErrorCode::CalendarEventPast => 409,
            ErrorCode::CalendarAttendeeAddressUnavailable => 409,
            ErrorCode::UploadNotReady => 409,
            ErrorCode::UploadInProgress => 409,
            ErrorCode::UploadInUse => 409,
            ErrorCode::SignupNotPending => 409,
            ErrorCode::SignupCannotVerify => 409,
            ErrorCode::SignupHumanUnreachable => 409,
            ErrorCode::PhoneNumberNotReady => 409,
            ErrorCode::PhoneNumberTaken => 409,
            ErrorCode::PhoneRequestNotPending => 409,
            ErrorCode::PhoneRequestDigestMismatch => 409,
            ErrorCode::ComplianceNotApproved => 409,
            ErrorCode::QuietHours => 409,
            ErrorCode::IdentityHasPhoneNumbers => 409,
            ErrorCode::VoiceRouteUnsupported => 409,
            ErrorCode::PhoneDocumentInUse => 409,
            ErrorCode::PortInNotCancelable => 409,
            ErrorCode::PortOutClosed => 409,
            ErrorCode::VaultGrantExists => 409,
            ErrorCode::VaultNotPending => 409,
            ErrorCode::VaultPhaseSpent => 409,
            ErrorCode::VaultTotpStepUsed => 409,
            ErrorCode::RawExpired => 410,
            ErrorCode::CursorExpired => 410,
            ErrorCode::UploadExpired => 410,
            ErrorCode::PhoneQuoteExpired => 410,
            ErrorCode::VaultTicketExpired => 410,
            ErrorCode::MessageExpired => 410,
            ErrorCode::LegalHold => 423,
            ErrorCode::AllRecipientsSuppressed => 422,
            ErrorCode::RecipientBlocked => 422,
            ErrorCode::TransportUnavailable => 422,
            ErrorCode::SmtpTlsRequired => 422,
            ErrorCode::SmtpAuthFailed => 422,
            ErrorCode::CfTokenRequired => 422,
            ErrorCode::AgenticDisabled => 422,
            ErrorCode::AddressLimitReached => 422,
            ErrorCode::UnverifiedDomainLimit => 422,
            ErrorCode::ZoneDomainLimit => 422,
            ErrorCode::AccountLimitReached => 422,
            ErrorCode::DraftLimitReached => 422,
            ErrorCode::FeedbackLimitReached => 422,
            ErrorCode::EventConsumerLimitReached => 422,
            ErrorCode::UploadLimitReached => 422,
            ErrorCode::AttachmentRisky => 422,
            ErrorCode::SignupCodeInvalid => 422,
            ErrorCode::SignupReplaceLimit => 422,
            ErrorCode::WebhookLimitReached => 422,
            ErrorCode::ListLimitReached => 422,
            ErrorCode::WebhookHeaderLimitReached => 422,
            ErrorCode::KeyLimitReached => 422,
            ErrorCode::WebBotAuthDisabled => 422,
            ErrorCode::CountryNotOffered => 422,
            ErrorCode::LocalPresenceRequired => 422,
            ErrorCode::ComplianceIncomplete => 422,
            ErrorCode::PhoneRegistrationOtpInvalid => 422,
            ErrorCode::PhoneNumberLimitReached => 422,
            ErrorCode::PhoneRequestLimitReached => 422,
            ErrorCode::ComplianceAddressInvalid => 422,
            ErrorCode::VaultDisabled => 422,
            ErrorCode::VaultKindRefused => 422,
            ErrorCode::VaultItemLimitReached => 422,
            ErrorCode::VaultGrantLimitReached => 422,
            ErrorCode::VaultConsumerLimitReached => 422,
            ErrorCode::VaultSessionTooLarge => 422,
            ErrorCode::BillingLimit => 402,
            ErrorCode::SpendCapReached => 402,
            ErrorCode::RateLimited => 429,
            ErrorCode::DailyCapReached => 429,
            ErrorCode::AgenticBudgetExhausted => 429,
            ErrorCode::UpstreamRateLimited => 429,
            ErrorCode::SignupLimitReached => 429,
            ErrorCode::InternalError => 500,
            ErrorCode::UpstreamError => 502,
            ErrorCode::Unavailable => 503,
            ErrorCode::SearchDegraded => 503,
            ErrorCode::Timeout => 504,
        }
    }

    /// Whether the same request can succeed later unchanged. A code the catalogue marks retryable only in
    /// one state ("yes while sending") is `true` here; the error response sets `retryable` for its case.
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::RequestInProgress
                | ErrorCode::ScreeningPending
                | ErrorCode::DomainNotReady
                | ErrorCode::DomainClaimPending
                | ErrorCode::ThreadBusy
                | ErrorCode::DraftNotEditable
                | ErrorCode::DraftNotSendable
                | ErrorCode::UploadNotReady
                | ErrorCode::UploadInProgress
                | ErrorCode::UploadInUse
                | ErrorCode::PhoneNumberNotReady
                | ErrorCode::QuietHours
                | ErrorCode::VaultTotpStepUsed
                | ErrorCode::RateLimited
                | ErrorCode::DailyCapReached
                | ErrorCode::AgenticBudgetExhausted
                | ErrorCode::UpstreamRateLimited
                | ErrorCode::SignupLimitReached
                | ErrorCode::InternalError
                | ErrorCode::UpstreamError
                | ErrorCode::Unavailable
                | ErrorCode::SearchDegraded
                | ErrorCode::Timeout
        )
    }

    /// The code as it appears on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unauthenticated => "unauthenticated",
            ErrorCode::KeyExpired => "key_expired",
            ErrorCode::KeyRevoked => "key_revoked",
            ErrorCode::PermissionDenied => "permission_denied",
            ErrorCode::ScopeDenied => "scope_denied",
            ErrorCode::KeyScopeExceeded => "key_scope_exceeded",
            ErrorCode::TenantSuspended => "tenant_suspended",
            ErrorCode::PartnerSuspended => "partner_suspended",
            ErrorCode::PartnerTenantLimit => "partner_tenant_limit",
            ErrorCode::TestModeRecipient => "test_mode_recipient",
            ErrorCode::PolicyDenied => "policy_denied",
            ErrorCode::ApprovalRequired => "approval_required",
            ErrorCode::SignupClosed => "signup_closed",
            ErrorCode::InvalidSignature => "invalid_signature",
            ErrorCode::RecipientOptedOut => "recipient_opted_out",
            ErrorCode::ConsentRequired => "consent_required",
            ErrorCode::A2pNotRegistered => "a2p_not_registered",
            ErrorCode::DestinationBlocked => "destination_blocked",
            ErrorCode::ConsumerUnauthenticated => "consumer_unauthenticated",
            ErrorCode::VaultNotGranted => "vault_not_granted",
            ErrorCode::VaultRefused => "vault_refused",
            ErrorCode::ConsumerNotActive => "consumer_not_active",
            ErrorCode::VaultUnlockRequired => "vault_unlock_required",
            ErrorCode::VaultUnlockInvalid => "vault_unlock_invalid",
            ErrorCode::InvalidRequest => "invalid_request",
            ErrorCode::IdempotencyKeyRequired => "idempotency_key_required",
            ErrorCode::InvalidIdempotencyKey => "invalid_idempotency_key",
            ErrorCode::InvalidQuery => "invalid_query",
            ErrorCode::AddressInvalid => "address_invalid",
            ErrorCode::AddressUnsupported => "address_unsupported",
            ErrorCode::AddressReserved => "address_reserved",
            ErrorCode::LocalPartTooLong => "local_part_too_long",
            ErrorCode::HeaderNotAllowed => "header_not_allowed",
            ErrorCode::MarketingRequirementsMissing => "marketing_requirements_missing",
            ErrorCode::TooManyRecipients => "too_many_recipients",
            ErrorCode::SmtpPortNotAllowed => "smtp_port_not_allowed",
            ErrorCode::SpfLookupLimit => "spf_lookup_limit",
            ErrorCode::CalendarInvalid => "calendar_invalid",
            ErrorCode::CalendarOrganizerMismatch => "calendar_organizer_mismatch",
            ErrorCode::CalendarAttendeeMismatch => "calendar_attendee_mismatch",
            ErrorCode::CalendarRecipientMismatch => "calendar_recipient_mismatch",
            ErrorCode::UploadSizeMismatch => "upload_size_mismatch",
            ErrorCode::UploadChecksumMismatch => "upload_checksum_mismatch",
            ErrorCode::PhoneNumberInvalid => "phone_number_invalid",
            ErrorCode::QueryRangeTooWide => "query_range_too_wide",
            ErrorCode::MessageTooLarge => "message_too_large",
            ErrorCode::CalendarTooLarge => "calendar_too_large",
            ErrorCode::PayloadTooLarge => "payload_too_large",
            ErrorCode::ScopeTooLarge => "scope_too_large",
            ErrorCode::UpgradeRequired => "upgrade_required",
            ErrorCode::TenantNotFound => "tenant_not_found",
            ErrorCode::IdentityNotFound => "identity_not_found",
            ErrorCode::AddressNotFound => "address_not_found",
            ErrorCode::DomainNotFound => "domain_not_found",
            ErrorCode::ThreadNotFound => "thread_not_found",
            ErrorCode::MessageNotFound => "message_not_found",
            ErrorCode::AttachmentNotFound => "attachment_not_found",
            ErrorCode::WebhookNotFound => "webhook_not_found",
            ErrorCode::KeyNotFound => "key_not_found",
            ErrorCode::ErasureNotFound => "erasure_not_found",
            ErrorCode::ExportNotFound => "export_not_found",
            ErrorCode::SuppressionNotFound => "suppression_not_found",
            ErrorCode::ListEntryNotFound => "list_entry_not_found",
            ErrorCode::MemberNotFound => "member_not_found",
            ErrorCode::InvitationNotFound => "invitation_not_found",
            ErrorCode::JobNotFound => "job_not_found",
            ErrorCode::DlqItemNotFound => "dlq_item_not_found",
            ErrorCode::PartnerNotFound => "partner_not_found",
            ErrorCode::AccountNotFound => "account_not_found",
            ErrorCode::DraftNotFound => "draft_not_found",
            ErrorCode::EventConsumerNotFound => "event_consumer_not_found",
            ErrorCode::UploadNotFound => "upload_not_found",
            ErrorCode::SignupNotFound => "signup_not_found",
            ErrorCode::PhoneNumberNotFound => "phone_number_not_found",
            ErrorCode::PhoneRequestNotFound => "phone_request_not_found",
            ErrorCode::SmsNotFound => "sms_not_found",
            ErrorCode::CallNotFound => "call_not_found",
            ErrorCode::PhoneDocumentNotFound => "phone_document_not_found",
            ErrorCode::PortOutNotFound => "port_out_not_found",
            ErrorCode::VoiceConnectorNotFound => "voice_connector_not_found",
            ErrorCode::VaultItemNotFound => "vault_item_not_found",
            ErrorCode::VaultGrantNotFound => "vault_grant_not_found",
            ErrorCode::VaultUseNotFound => "vault_use_not_found",
            ErrorCode::VaultConsumerNotFound => "vault_consumer_not_found",
            ErrorCode::VaultTicketNotFound => "vault_ticket_not_found",
            ErrorCode::RouteNotFound => "route_not_found",
            ErrorCode::MethodNotAllowed => "method_not_allowed",
            ErrorCode::IdempotencyConflict => "idempotency_conflict",
            ErrorCode::RequestInProgress => "request_in_progress",
            ErrorCode::ClientIdConflict => "client_id_conflict",
            ErrorCode::UsernameTaken => "username_taken",
            ErrorCode::SlugTaken => "slug_taken",
            ErrorCode::SuffixTaken => "suffix_taken",
            ErrorCode::NotQuarantined => "not_quarantined",
            ErrorCode::NotScreened => "not_screened",
            ErrorCode::ScreeningPending => "screening_pending",
            ErrorCode::DomainNotSuspended => "domain_not_suspended",
            ErrorCode::ExistingMx => "existing_mx",
            ErrorCode::DomainNotDedicated => "domain_not_dedicated",
            ErrorCode::ZoneHold => "zone_hold",
            ErrorCode::OwnerRequired => "owner_required",
            ErrorCode::PlanManagedByStripe => "plan_managed_by_stripe",
            ErrorCode::PartnerHasTenants => "partner_has_tenants",
            ErrorCode::TenantErased => "tenant_erased",
            ErrorCode::SystemProtected => "system_protected",
            ErrorCode::AddressTaken => "address_taken",
            ErrorCode::AddressIsPrimary => "address_is_primary",
            ErrorCode::AddressInUse => "address_in_use",
            ErrorCode::DomainNotReady => "domain_not_ready",
            ErrorCode::DomainInUse => "domain_in_use",
            ErrorCode::DomainExists => "domain_exists",
            ErrorCode::DomainClaimPending => "domain_claim_pending",
            ErrorCode::IdentityPaused => "identity_paused",
            ErrorCode::IdentityOwnerRequired => "identity_owner_required",
            ErrorCode::ThreadBusy => "thread_busy",
            ErrorCode::NotCancelable => "not_cancelable",
            ErrorCode::NotUncertain => "not_uncertain",
            ErrorCode::AutoReplyNotAllowed => "auto_reply_not_allowed",
            ErrorCode::AccountExists => "account_exists",
            ErrorCode::AccountNotPending => "account_not_pending",
            ErrorCode::DraftRevisionMismatch => "draft_revision_mismatch",
            ErrorCode::DraftNotEditable => "draft_not_editable",
            ErrorCode::DraftNotSendable => "draft_not_sendable",
            ErrorCode::DraftNotPending => "draft_not_pending",
            ErrorCode::SourceUnavailable => "source_unavailable",
            ErrorCode::LoopDetected => "loop_detected",
            ErrorCode::SendingPaused => "sending_paused",
            ErrorCode::TriageNotEligible => "triage_not_eligible",
            ErrorCode::ErasureNotCompleted => "erasure_not_completed",
            ErrorCode::NotFrozen => "not_frozen",
            ErrorCode::EventConsumerNameTaken => "event_consumer_name_taken",
            ErrorCode::CalendarSequenceStale => "calendar_sequence_stale",
            ErrorCode::CalendarNotInvitation => "calendar_not_invitation",
            ErrorCode::CalendarNotVerified => "calendar_not_verified",
            ErrorCode::CalendarNotAttendee => "calendar_not_attendee",
            ErrorCode::CalendarCancelled => "calendar_cancelled",
            ErrorCode::CalendarSuperseded => "calendar_superseded",
            ErrorCode::CalendarEventPast => "calendar_event_past",
            ErrorCode::CalendarAttendeeAddressUnavailable => {
                "calendar_attendee_address_unavailable"
            }
            ErrorCode::UploadNotReady => "upload_not_ready",
            ErrorCode::UploadInProgress => "upload_in_progress",
            ErrorCode::UploadInUse => "upload_in_use",
            ErrorCode::SignupNotPending => "signup_not_pending",
            ErrorCode::SignupCannotVerify => "signup_cannot_verify",
            ErrorCode::SignupHumanUnreachable => "signup_human_unreachable",
            ErrorCode::PhoneNumberNotReady => "phone_number_not_ready",
            ErrorCode::PhoneNumberTaken => "phone_number_taken",
            ErrorCode::PhoneRequestNotPending => "phone_request_not_pending",
            ErrorCode::PhoneRequestDigestMismatch => "phone_request_digest_mismatch",
            ErrorCode::ComplianceNotApproved => "compliance_not_approved",
            ErrorCode::QuietHours => "quiet_hours",
            ErrorCode::IdentityHasPhoneNumbers => "identity_has_phone_numbers",
            ErrorCode::VoiceRouteUnsupported => "voice_route_unsupported",
            ErrorCode::PhoneDocumentInUse => "phone_document_in_use",
            ErrorCode::PortInNotCancelable => "port_in_not_cancelable",
            ErrorCode::PortOutClosed => "port_out_closed",
            ErrorCode::VaultGrantExists => "vault_grant_exists",
            ErrorCode::VaultNotPending => "vault_not_pending",
            ErrorCode::VaultPhaseSpent => "vault_phase_spent",
            ErrorCode::VaultTotpStepUsed => "vault_totp_step_used",
            ErrorCode::RawExpired => "raw_expired",
            ErrorCode::CursorExpired => "cursor_expired",
            ErrorCode::UploadExpired => "upload_expired",
            ErrorCode::PhoneQuoteExpired => "phone_quote_expired",
            ErrorCode::VaultTicketExpired => "vault_ticket_expired",
            ErrorCode::MessageExpired => "message_expired",
            ErrorCode::LegalHold => "legal_hold",
            ErrorCode::AllRecipientsSuppressed => "all_recipients_suppressed",
            ErrorCode::RecipientBlocked => "recipient_blocked",
            ErrorCode::TransportUnavailable => "transport_unavailable",
            ErrorCode::SmtpTlsRequired => "smtp_tls_required",
            ErrorCode::SmtpAuthFailed => "smtp_auth_failed",
            ErrorCode::CfTokenRequired => "cf_token_required",
            ErrorCode::AgenticDisabled => "agentic_disabled",
            ErrorCode::AddressLimitReached => "address_limit_reached",
            ErrorCode::UnverifiedDomainLimit => "unverified_domain_limit",
            ErrorCode::ZoneDomainLimit => "zone_domain_limit",
            ErrorCode::AccountLimitReached => "account_limit_reached",
            ErrorCode::DraftLimitReached => "draft_limit_reached",
            ErrorCode::FeedbackLimitReached => "feedback_limit_reached",
            ErrorCode::EventConsumerLimitReached => "event_consumer_limit_reached",
            ErrorCode::UploadLimitReached => "upload_limit_reached",
            ErrorCode::AttachmentRisky => "attachment_risky",
            ErrorCode::SignupCodeInvalid => "signup_code_invalid",
            ErrorCode::SignupReplaceLimit => "signup_replace_limit",
            ErrorCode::WebhookLimitReached => "webhook_limit_reached",
            ErrorCode::ListLimitReached => "list_limit_reached",
            ErrorCode::WebhookHeaderLimitReached => "webhook_header_limit_reached",
            ErrorCode::KeyLimitReached => "key_limit_reached",
            ErrorCode::WebBotAuthDisabled => "web_bot_auth_disabled",
            ErrorCode::CountryNotOffered => "country_not_offered",
            ErrorCode::LocalPresenceRequired => "local_presence_required",
            ErrorCode::ComplianceIncomplete => "compliance_incomplete",
            ErrorCode::PhoneRegistrationOtpInvalid => "phone_registration_otp_invalid",
            ErrorCode::PhoneNumberLimitReached => "phone_number_limit_reached",
            ErrorCode::PhoneRequestLimitReached => "phone_request_limit_reached",
            ErrorCode::ComplianceAddressInvalid => "compliance_address_invalid",
            ErrorCode::VaultDisabled => "vault_disabled",
            ErrorCode::VaultKindRefused => "vault_kind_refused",
            ErrorCode::VaultItemLimitReached => "vault_item_limit_reached",
            ErrorCode::VaultGrantLimitReached => "vault_grant_limit_reached",
            ErrorCode::VaultConsumerLimitReached => "vault_consumer_limit_reached",
            ErrorCode::VaultSessionTooLarge => "vault_session_too_large",
            ErrorCode::BillingLimit => "billing_limit",
            ErrorCode::SpendCapReached => "spend_cap_reached",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::DailyCapReached => "daily_cap_reached",
            ErrorCode::AgenticBudgetExhausted => "agentic_budget_exhausted",
            ErrorCode::UpstreamRateLimited => "upstream_rate_limited",
            ErrorCode::SignupLimitReached => "signup_limit_reached",
            ErrorCode::InternalError => "internal_error",
            ErrorCode::UpstreamError => "upstream_error",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::SearchDegraded => "search_degraded",
            ErrorCode::Timeout => "timeout",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One catalogue row: the codes it names, its HTTP status and whether it is retryable.
    fn catalogue() -> Vec<(String, u16, bool)> {
        let md = include_str!("../../../docs/src/reference/errors.md");
        let start = md.find("## Catalogue").expect("catalogue");
        let end = md.find("## Stream close codes").expect("end of catalogue");
        let mut out = Vec::new();
        for line in md[start..end].lines() {
            let cells: Vec<&str> = line.split(" | ").collect();
            let Some(status) = cells
                .first()
                .and_then(|c| c.trim_start_matches("| ").parse::<u16>().ok())
            else {
                continue;
            };
            // "yes while sending": retryable in the state the row names.
            let retryable = cells.get(2).is_some_and(|c| c.trim().starts_with("yes"));
            for part in cells
                .get(1)
                .copied()
                .unwrap_or("")
                .split("id=\"code-")
                .skip(1)
            {
                let code = part.split('"').next().unwrap_or("");
                out.push((code.to_owned(), status, retryable));
            }
        }
        out
    }

    /// Every code of the catalogue is an `ErrorCode` with its HTTP status and `retryable` flag, and every
    /// `ErrorCode` is in the catalogue (Testing § 3).
    /// Covers: FR-API-2
    #[test]
    fn catalogue_matches_reference() {
        let rows = catalogue();
        assert!(rows.len() >= 200, "{} rows", rows.len());
        for (code, status, retryable) in &rows {
            let c: ErrorCode = serde_json::from_value(serde_json::json!(code))
                .unwrap_or_else(|_| panic!("{code} is not an ErrorCode"));
            assert_eq!(c.as_str(), code);
            assert_eq!(c.http_status(), *status, "{code}");
            assert_eq!(c.retryable(), *retryable, "{code}");
        }
        let schema = serde_json::to_value(<ErrorCode as utoipa::PartialSchema>::schema()).unwrap();
        let variants: Vec<&str> = schema["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            variants.len(),
            rows.len(),
            "every ErrorCode is in the catalogue once"
        );
        for v in variants {
            assert!(
                rows.iter().any(|(c, _, _)| c == v),
                "{v} is not in errors.md"
            );
        }
    }
}
