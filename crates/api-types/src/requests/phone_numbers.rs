//! Request bodies of the `phone_numbers` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// A draft's fields; no other property is accepted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSetupDraftRequest {
    #[serde(flatten)]
    pub base: PhoneSetupDraftFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneBudgetRequest {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub countries: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<PhoneNumberType>)]
    pub number_types: Option<Vec<PhoneNumberType>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_numbers_per_identity: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_new_numbers_per_month: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_setup_pence_per_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_monthly_pence_per_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub expiring_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneBuyRequestPrimaryForItem {
    #[serde(rename = "sms")]
    Sms,
    #[serde(rename = "voice")]
    Voice,
}

fn schema_phone_buy_request_primary_for_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"items": {"enum": ["sms", "voice"], "type": "string"}, "type": "array"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneBuyRequest {
    pub country: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = PhoneNumberType)]
    pub r#type: Option<PhoneNumberType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub area_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub locality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = E164)]
    pub number: Option<E164>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub quote_token: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_buy_request_primary_for_property)]
    pub primary_for: Option<Vec<PhoneBuyRequestPrimaryForItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub acknowledge_not_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub client_reference: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhonePortabilityRequest {
    pub numbers: Vec<E164>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberUpdateRequestPrimaryForItem {
    #[serde(rename = "sms")]
    Sms,
    #[serde(rename = "voice")]
    Voice,
}

fn schema_phone_number_update_request_primary_for_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"items": {"enum": ["sms", "voice"], "type": "string"}, "type": "array"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_number_update_request_primary_for_property)]
    pub primary_for: Option<Vec<PhoneNumberUpdateRequestPrimaryForItem>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberMoveRequest {
    pub to_identity_id: IdentityId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestApproveRequest {
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestRejectRequest {
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneRequestResolveRequestOutcome {
    #[serde(rename = "ordered")]
    Ordered,
    #[serde(rename = "not_ordered")]
    NotOrdered,
}

fn schema_phone_request_resolve_request_outcome_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["ordered", "not_ordered"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestResolveRequest {
    #[schema(schema_with = schema_phone_request_resolve_request_outcome_property)]
    pub outcome: PhoneRequestResolveRequestOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub provider_order_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneExportRequest {
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneOtpRequest {
    pub pin: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneAdoptRequest {
    pub identity_id: IdentityId,
    pub phone_number: E164,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub vapi_phone_number_id: Option<String>,
}
