//! Request bodies of the `identities` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct IdentityCreateRequest {
    pub username: UsernameInput,
    pub display_name: DisplayName,
    /// A free tag, for example `bookings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentityOwner)]
    pub owner: Option<IdentityOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentitySignature)]
    pub signature: Option<IdentitySignature>,
    /// A healthy tenant domain for the primary address (`{username}@{domain}`). Omitted: the platform domain (`{username}{suffix}@{platform}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = DomainId)]
    pub domain_id: Option<DomainId>,
    /// Makes the create idempotent within the tenant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub client_id: Option<String>,
    /// The identity's own send policy, as on update. From any key but a platform key, a `daily_cap` above the tenant's effective `identity_daily_send_cap` gets `403 scope_denied` with `details.field = "send_policy.daily_cap"`, and nothing is created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentitySendPolicy)]
    pub send_policy: Option<IdentitySendPolicy>,
    /// The identity's own message retention; a number needs `erasure:manage` as well.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentityRetention)]
    pub retention: Option<IdentityRetention>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Metadata)]
    pub metadata: Option<Metadata>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum IdentityUpdateRequestStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "paused")]
    Paused,
}

fn schema_identity_update_request_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`paused` pauses with reason `manual`; `active` resumes (an `abuse_threshold` pause on a tenant a partner's key created needs a platform key).", "enum": ["active", "paused"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct IdentityUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = DisplayName)]
    pub display_name: Option<DisplayName>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub purpose: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentityOwner)]
    pub owner: Option<IdentityOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentitySignature)]
    pub signature: Option<IdentitySignature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Metadata)]
    pub metadata: Option<Metadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentitySendPolicy)]
    pub send_policy: Option<IdentitySendPolicy>,
    /// Setting `message_hours` where it was `null`, or lowering it, needs `erasure:manage` as well; raising it or setting `null` needs `identities:write` only. Each change writes the audit row `identity.retention_update`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentityRetention)]
    pub retention: Option<IdentityRetention>,
    /// `paused` pauses with reason `manual`; `active` resumes (an `abuse_threshold` pause on a tenant a partner's key created needs a platform key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_identity_update_request_status_property)]
    pub status: Option<IdentityUpdateRequestStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ErasureRequest {
    pub id: ErasureRequestId,
    pub tenant_id: TenantId,
    pub scope: ErasureScope,
    pub status: ErasureStatus,
    /// Set for message, thread and identity scope.
    #[schema(required = true)]
    pub identity_id: Option<IdentityId>,
    /// The `msg_` or `thr_` ID, for message and thread scope. The counterparty address is never stored or returned.
    #[schema(required = true)]
    pub target_id: Option<String>,
    /// The failed request this one resumes, or the `completed_with_holds` request it continues.
    #[schema(required = true)]
    pub resumes_id: Option<ErasureRequestId>,
    pub created_at: String,
    /// When the 24 hours of NFR-PRV-1 end, counted from the first request (copied when resuming a failed one).
    pub deadline_at: String,
    #[schema(required = true)]
    pub completed_at: Option<String>,
    /// Set once the request has completed.
    #[schema(required = true)]
    pub receipt: Option<ErasureReceipt>,
    /// The API key that started the request (`null` when a person started it in the console or the system started it, for example after a legal hold ended). Lets an operator list what a compromised key did ([J6]).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub created_by_key_id: Option<Option<String>>,
}
