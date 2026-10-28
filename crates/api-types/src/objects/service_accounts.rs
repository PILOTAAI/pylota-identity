//! Objects first used by the `service_accounts` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Console user ID (`usr_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UserId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ServiceAccountRejectedReason {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "expired")]
    Expired,
}

fn schema_service_account_rejected_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "While `rejected`: `operator`, or `expired` after 7 days undecided.", "enum": ["operator", "expired", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

/// An entry of the service sign-up ledger: a third-party account an identity's agent asked to create (FR-IDN-10). `purpose` and `account_identifier` were written by an agent: show them as untrusted text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ServiceAccount {
    pub id: ServiceAccountId,
    pub tenant_id: TenantId,
    pub identity_id: IdentityId,
    /// The service's organisational domain (A-label, lower case).
    pub service_domain: String,
    /// Organisational domains the service's verification mail may come from; always includes `service_domain`.
    pub sender_domains: Vec<String>,
    pub account_identifier: String,
    pub address: EmailAddress,
    pub purpose: String,
    pub status: ServiceAccountStatus,
    /// While `rejected`: `operator`, or `expired` after 7 days undecided.
    #[schema(required = true, schema_with = schema_service_account_rejected_reason_property)]
    pub rejected_reason: Option<ServiceAccountRejectedReason>,
    /// The note given with the decision or the closure.
    #[schema(required = true)]
    pub note: Option<String>,
    #[schema(required = true)]
    pub requested_by_key_id: Option<KeyId>,
    #[schema(required = true)]
    pub decided_by_key_id: Option<KeyId>,
    /// The person who decided in the console, or `null`.
    #[schema(required = true)]
    pub decided_by_user_id: Option<UserId>,
    #[schema(required = true)]
    pub decided_at: Option<String>,
    #[schema(required = true)]
    pub closed_by_key_id: Option<KeyId>,
    #[schema(required = true)]
    pub closed_by_user_id: Option<UserId>,
    #[schema(required = true)]
    pub closed_at: Option<String>,
    /// While `pending_approval`, when it is rejected as `expired`; otherwise `null`.
    #[schema(required = true)]
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Up to 5 organisational domains, besides `service_domain`, where the account signs in. Approving them lets a vault fill happen there, so the approve page lists them apart.
    pub login_domains: Vec<String>,
    /// The identity's number a service texts codes to. An entry naming no number matches SMS only when the identity has one live SMS number.
    #[schema(required = true)]
    pub phone_number_id: Option<PhoneNumberId>,
    /// Senders the service's codes come from: E.164 numbers, short codes or alphanumeric sender IDs.
    pub sms_senders: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ServiceAccountPage {
    pub data: Vec<ServiceAccount>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberId(pub String);

/// An exact `https` origin, canonicalised (lower case, IDNA A-label, no path, port 443 dropped).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultOrigin(pub String);
