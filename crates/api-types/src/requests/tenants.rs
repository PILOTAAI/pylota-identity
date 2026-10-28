//! Request bodies of the `tenants` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Creates the workspace's console owner and emails them a sign-in link. Without it, a platform or partner key can add an owner later with an invitation and an ownership transfer in the console.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantCreateRequestOwner {
    pub email: EmailAddress,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub name: Option<String>,
}

fn schema_tenant_create_request_owner_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Creates the workspace's console owner and emails them a sign-in link. Without it, a platform or partner key can add an owner later with an invitation and an ownership transfer in the console.", "properties": {"email": {"$ref": "#/components/schemas/EmailAddress"}, "name": {"examples": ["Sam Patel"], "minLength": 1, "type": "string"}}, "required": ["email"], "type": "object"}"###).expect("the contract's schema")
}

/// The workspace's billing account. Platform keys only: a partner key gets `403 scope_denied`, and its tenants take the partner's `default_billing_mode`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantCreateRequestBilling {
    /// Defaults to `metered` (plan `free`) on a deployment with billing on, and to `disabled` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = BillingMode)]
    pub mode: Option<BillingMode>,
}

fn schema_tenant_create_request_billing_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "The workspace's billing account. Platform keys only: a partner key gets `403 scope_denied`, and its tenants take the partner's `default_billing_mode`.", "properties": {"mode": {"$ref": "#/components/schemas/BillingMode", "description": "Defaults to `metered` (plan `free`) on a deployment with billing on, and to `disabled` otherwise."}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantCreateRequest {
    pub slug: TenantSlug,
    pub name: String,
    pub mode: Mode,
    /// IANA time zone name (for example `Europe/London`). An unknown name is refused with `400 invalid_request` (path `timezone`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = AddressSuffix)]
    pub address_suffix: Option<AddressSuffix>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = TenantPolicyPatch)]
    pub policy: Option<TenantPolicyPatch>,
    /// Creates the workspace's console owner and emails them a sign-in link. Without it, a platform or partner key can add an owner later with an invitation and an ownership transfer in the console.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_tenant_create_request_owner_property)]
    pub owner: Option<TenantCreateRequestOwner>,
    /// The workspace's billing account. Platform keys only: a partner key gets `403 scope_denied`, and its tenants take the partner's `default_billing_mode`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_tenant_create_request_billing_property)]
    pub billing: Option<TenantCreateRequestBilling>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantUpdateRequestStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "suspended")]
    Suspended,
}

fn schema_tenant_update_request_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`suspended` suspends the tenant (FR-TEN-3) and records `suspended_by`; `active` resumes it, except that a partner key cannot resume a tenant a platform key suspended (`403 scope_denied`).", "enum": ["active", "suspended"], "type": "string"}"###).expect("the contract's schema")
}

fn schema_tenant_update_request_sending_paused_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`false` lifts an automatic sending pause ([G12]), audit-logged. Platform keys only: any other key gets `403 scope_denied` with `details.field = \"sending_paused\"`. A pause is never set by hand.", "enum": [false], "type": "boolean"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub name: Option<String>,
    /// IANA time zone name (for example `Europe/London`). An unknown name is refused with `400 invalid_request` (path `timezone`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = TenantPolicyPatch)]
    pub policy: Option<TenantPolicyPatch>,
    /// `suspended` suspends the tenant (FR-TEN-3) and records `suspended_by`; `active` resumes it, except that a partner key cannot resume a tenant a platform key suspended (`403 scope_denied`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_tenant_update_request_status_property)]
    pub status: Option<TenantUpdateRequestStatus>,
    /// `false` lifts an automatic sending pause ([G12]), audit-logged. Platform keys only: any other key gets `403 scope_denied` with `details.field = "sending_paused"`. A pause is never set by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_tenant_update_request_sending_paused_property)]
    pub sending_paused: Option<bool>,
}
