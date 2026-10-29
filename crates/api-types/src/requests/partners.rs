//! Request bodies of the `partners` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct PartnerCreateRequest {
    pub name: String,
    /// Defaults to `metered`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = PartnerBillingMode)]
    pub default_billing_mode: Option<PartnerBillingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = PartnerMaxTenants)]
    pub max_tenants: Option<PartnerMaxTenants>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = PartnerRampExempt)]
    pub ramp_exempt: Option<PartnerRampExempt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PartnerUpdateRequestStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "suspended")]
    Suspended,
}

fn schema_partner_update_request_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`deleted` cannot be set here: use `DELETE /v1/partners/{partner_id}`.", "enum": ["active", "suspended"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct PartnerUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub name: Option<String>,
    /// `deleted` cannot be set here: use `DELETE /v1/partners/{partner_id}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_partner_update_request_status_property)]
    pub status: Option<PartnerUpdateRequestStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = PartnerBillingMode)]
    pub default_billing_mode: Option<PartnerBillingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = PartnerMaxTenants)]
    pub max_tenants: Option<PartnerMaxTenants>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = PartnerRampExempt)]
    pub ramp_exempt: Option<PartnerRampExempt>,
}
