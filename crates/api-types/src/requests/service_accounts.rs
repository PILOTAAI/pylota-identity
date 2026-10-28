//! Request bodies of the `service_accounts` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ServiceAccountCreateRequestVaultLoginPasswordGenerateClassesItem {
    #[serde(rename = "lower")]
    Lower,
    #[serde(rename = "upper")]
    Upper,
    #[serde(rename = "digit")]
    Digit,
    #[serde(rename = "symbol")]
    Symbol,
}

fn schema_service_account_create_request_vault_login_password_generate_classes_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"enum": ["lower", "upper", "digit", "symbol"], "type": "string"}, "type": "array", "uniqueItems": true}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ServiceAccountCreateRequestVaultLoginPasswordGenerate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub length: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_service_account_create_request_vault_login_password_generate_classes_property)]
    pub classes: Option<Vec<ServiceAccountCreateRequestVaultLoginPasswordGenerateClassesItem>>,
}

fn schema_service_account_create_request_vault_login_password_generate_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"additionalProperties": false, "properties": {"classes": {"items": {"enum": ["lower", "upper", "digit", "symbol"], "type": "string"}, "type": "array", "uniqueItems": true}, "length": {"default": 24, "maximum": 128, "minimum": 16, "type": "integer"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ServiceAccountCreateRequestVaultLoginPassword {
    #[schema(schema_with = schema_service_account_create_request_vault_login_password_generate_property)]
    pub generate: ServiceAccountCreateRequestVaultLoginPasswordGenerate,
}

fn schema_service_account_create_request_vault_login_password_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"additionalProperties": false, "properties": {"generate": {"additionalProperties": false, "properties": {"classes": {"items": {"enum": ["lower", "upper", "digit", "symbol"], "type": "string"}, "type": "array", "uniqueItems": true}, "length": {"default": 24, "maximum": 128, "minimum": 16, "type": "integer"}}, "type": "object"}}, "required": ["generate"], "type": "object"}"###).expect("the contract's schema")
}

/// Optional: when the entry is approved, the same batch creates an identity-scoped vault login with a generated password and an active `fill` grant to the requesting identity for 90 days (docs › Agent credential vault § 3.5). Not stored on the entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ServiceAccountCreateRequestVaultLogin {
    pub origins: Vec<VaultOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_service_account_create_request_vault_login_password_property)]
    pub password: Option<ServiceAccountCreateRequestVaultLoginPassword>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub email_otp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultApprovalLevel)]
    pub approval: Option<VaultApprovalLevel>,
}

fn schema_service_account_create_request_vault_login_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"additionalProperties": false, "description": "Optional: when the entry is approved, the same batch creates an identity-scoped vault login with a generated password and an active `fill` grant to the requesting identity for 90 days (docs › Agent credential vault § 3.5). Not stored on the entry.", "properties": {"approval": {"$ref": "#/components/schemas/VaultApprovalLevel"}, "email_otp": {"default": false, "type": "boolean"}, "origins": {"items": {"$ref": "#/components/schemas/VaultOrigin"}, "maxItems": 10, "minItems": 1, "type": "array", "uniqueItems": true}, "password": {"additionalProperties": false, "properties": {"generate": {"additionalProperties": false, "properties": {"classes": {"items": {"enum": ["lower", "upper", "digit", "symbol"], "type": "string"}, "type": "array", "uniqueItems": true}, "length": {"default": 24, "maximum": 128, "minimum": 16, "type": "integer"}}, "type": "object"}}, "required": ["generate"], "type": "object"}}, "required": ["origins"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ServiceAccountCreateRequest {
    /// The service's domain; it is reduced to its organisational domain. A public suffix or this deployment's own domain is refused.
    pub service_domain: String,
    /// Other domains the service's mail comes from, each reduced to its organisational domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub sender_domains: Option<Vec<String>>,
    /// The username or account email the agent will use at the service.
    pub account_identifier: String,
    /// Why the agent needs the account, shown to the operator.
    pub purpose: String,
    /// An active or retiring address of the identity; defaults to its primary address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = EmailAddress)]
    pub address: Option<EmailAddress>,
    /// Up to 5 organisational domains, besides `service_domain`, where the account signs in. Approving them lets a vault fill happen there, so the approve page lists them apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub login_domains: Option<Vec<String>>,
    /// The identity's number a service texts codes to. An entry naming no number matches SMS only when the identity has one live SMS number.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<PhoneNumberId>)]
    pub phone_number_id: Option<Option<PhoneNumberId>>,
    /// Senders the service's codes come from: E.164 numbers, short codes or alphanumeric sender IDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub sms_senders: Option<Vec<String>>,
    /// Optional: when the entry is approved, the same batch creates an identity-scoped vault login with a generated password and an active `fill` grant to the requesting identity for 90 days (docs › Agent credential vault § 3.5). Not stored on the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_service_account_create_request_vault_login_property)]
    pub vault_login: Option<ServiceAccountCreateRequestVaultLogin>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ServiceAccountNoteRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub note: Option<String>,
}
