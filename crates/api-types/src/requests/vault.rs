//! Request bodies of the `vault` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemCreateRequestKind {
    #[serde(rename = "login")]
    Login,
    #[serde(rename = "api_key")]
    ApiKey,
    #[serde(rename = "session")]
    Session,
}

fn schema_vault_item_create_request_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["login", "api_key", "session"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemCreateRequestInjectScheme {
    #[serde(rename = "bearer")]
    Bearer,
    #[serde(rename = "basic")]
    Basic,
    #[serde(rename = "header")]
    Header,
}

fn schema_vault_item_create_request_inject_scheme_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["bearer", "basic", "header"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemCreateRequestInject {
    #[schema(schema_with = schema_vault_item_create_request_inject_scheme_property)]
    pub scheme: VaultItemCreateRequestInjectScheme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub header_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub username: Option<String>,
}

fn schema_vault_item_create_request_inject_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"header_name": {"maxLength": 128, "type": "string"}, "scheme": {"enum": ["bearer", "basic", "header"], "type": "string"}, "username": {"maxLength": 254, "type": "string"}}, "required": ["scheme"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemCreateRequestSignScheme {
    #[serde(rename = "hmac_sha256")]
    HmacSha256,
    #[serde(rename = "aws_sigv4")]
    AwsSigv4,
}

fn schema_vault_item_create_request_sign_scheme_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["hmac_sha256", "aws_sigv4"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemCreateRequestSignEncoding {
    #[serde(rename = "hex")]
    Hex,
    #[serde(rename = "base64")]
    Base64,
}

fn schema_vault_item_create_request_sign_encoding_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["hex", "base64"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// P1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemCreateRequestSign {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_create_request_sign_scheme_property)]
    pub scheme: Option<VaultItemCreateRequestSignScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_create_request_sign_encoding_property)]
    pub encoding: Option<VaultItemCreateRequestSignEncoding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub access_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub service: Option<String>,
}

fn schema_vault_item_create_request_sign_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "P1.", "properties": {"access_key_id": {"type": "string"}, "encoding": {"enum": ["hex", "base64"], "type": "string"}, "region": {"type": "string"}, "scheme": {"enum": ["hmac_sha256", "aws_sigv4"], "type": "string"}, "service": {"type": "string"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemCreateRequestSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub ttl_seconds: Option<i64>,
}

fn schema_vault_item_create_request_session_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"ttl_seconds": {"minimum": 3600, "type": "integer"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemCreateRequestImportSource {
    #[serde(rename = "pylota_sealed_session")]
    PylotaSealedSession,
}

fn schema_vault_item_create_request_import_source_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["pylota_sealed_session"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemCreateRequestImport {
    #[schema(schema_with = schema_vault_item_create_request_import_source_property)]
    pub source: VaultItemCreateRequestImportSource,
    pub source_ref: String,
}

fn schema_vault_item_create_request_import_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"source": {"enum": ["pylota_sealed_session"], "type": "string"}, "source_ref": {"maxLength": 64, "type": "string"}}, "required": ["source", "source_ref"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemCreateRequestRotation {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub remind_after_days: Option<Option<i64>>,
}

fn schema_vault_item_create_request_rotation_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"remind_after_days": {"maximum": 730, "minimum": 7, "type": ["integer", "null"]}}, "type": "object"}"###).expect("the contract's schema")
}

/// One request shape for the three kinds; fields that do not belong to `kind` are refused. Values (`password`, `totp`, `recovery_codes`, `secret`, `state`) are write-only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemCreateRequest {
    #[schema(schema_with = schema_vault_item_create_request_kind_property)]
    pub kind: VaultItemCreateRequestKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = ServiceAccountId)]
    pub account_id: Option<ServiceAccountId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub username: Option<String>,
    pub origins: Vec<VaultOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultApprovalLevel)]
    pub approval: Option<VaultApprovalLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultPasswordInput)]
    pub password: Option<VaultPasswordInput>,
    /// An `otpauth://totp/` URI. Write-only; never returned, not even to a person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub totp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub recovery_codes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultOtpChannel)]
    pub otp_channel: Option<VaultOtpChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_create_request_inject_property)]
    pub inject: Option<VaultItemCreateRequestInject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub rotate_after_use: Option<bool>,
    /// P1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_create_request_sign_property)]
    pub sign: Option<VaultItemCreateRequestSign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultSessionState)]
    pub state: Option<VaultSessionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_create_request_session_property)]
    pub session: Option<VaultItemCreateRequestSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_create_request_import_property)]
    pub import: Option<VaultItemCreateRequestImport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_create_request_rotation_property)]
    pub rotation: Option<VaultItemCreateRequestRotation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultItemUpdateRequestState {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "archived")]
    Archived,
}

fn schema_vault_item_update_request_state_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["active", "archived"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemUpdateRequestRotation {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub remind_after_days: Option<Option<i64>>,
}

fn schema_vault_item_update_request_rotation_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"remind_after_days": {"maximum": 730, "minimum": 7, "type": ["integer", "null"]}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<VaultOrigin>)]
    pub origins: Option<Vec<VaultOrigin>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultApprovalLevel)]
    pub approval: Option<VaultApprovalLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_update_request_state_property)]
    pub state: Option<VaultItemUpdateRequestState>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<VaultOtpChannel>)]
    pub otp_channel: Option<Option<VaultOtpChannel>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_item_update_request_rotation_property)]
    pub rotation: Option<VaultItemUpdateRequestRotation>,
}

/// The kind's values (write-only) and the overlap for the previous version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemVersionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultPasswordInput)]
    pub password: Option<VaultPasswordInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub totp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub recovery_codes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultSessionState)]
    pub state: Option<VaultSessionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub overlap_hours: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultGrantRequest {
    pub item_id: VaultItemId,
    pub operations: Vec<VaultOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<VaultOrigin>)]
    pub origins: Option<Vec<VaultOrigin>>,
    /// Untrusted text shown to the approver.
    pub purpose: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_uses: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub approve: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub expires_in: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultUseCreateRequestVersion {
    #[serde(rename = "current")]
    Current,
    #[serde(rename = "previous")]
    Previous,
}

fn schema_vault_use_create_request_version_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "current", "description": "`previous` only for `fill` and `inject`, while the item's previous version is inside its overlap; otherwise `403 vault_refused` with `details.reason = \"previous_version_unavailable\"`. Fixed at creation and part of the digest.", "enum": ["current", "previous"], "type": "string"}"###).expect("the contract's schema")
}

/// P1, for `operation = sign` only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultUseCreateRequestSignRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub message_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = BTreeMap<String, String>)]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub body_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub timestamp: Option<String>,
}

fn schema_vault_use_create_request_sign_request_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "P1, for `operation = sign` only.", "properties": {"body_sha256": {"type": "string"}, "headers": {"additionalProperties": {"type": "string"}, "maxProperties": 50, "type": "object"}, "message_base64": {"maxLength": 87384, "type": "string"}, "method": {"type": "string"}, "timestamp": {"format": "date-time", "type": "string"}, "url": {"format": "uri", "type": "string"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultUseCreateRequest {
    pub item_id: VaultItemId,
    pub operation: VaultOperation,
    /// At least one phase of the operation, without repeats; for `session_capture`, which has no phase, an empty array (any other value is `400 invalid_request`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<VaultPhase>)]
    pub phases: Option<Vec<VaultPhase>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultConsumerId)]
    pub consumer_id: Option<VaultConsumerId>,
    pub run_id: String,
    pub origin: VaultOrigin,
    pub trigger: VaultTrigger,
    pub purpose: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub capture_into: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub run_window_seconds: Option<Option<i64>>,
    /// `previous` only for `fill` and `inject`, while the item's previous version is inside its overlap; otherwise `403 vault_refused` with `details.reason = "previous_version_unavailable"`. Fixed at creation and part of the digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_use_create_request_version_property)]
    pub version: Option<VaultUseCreateRequestVersion>,
    /// P1, for `operation = sign` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_use_create_request_sign_request_property)]
    pub sign_request: Option<VaultUseCreateRequestSignRequest>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultUseApproveRequest {
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerCreateRequestKind {
    #[serde(rename = "browser")]
    Browser,
    #[serde(rename = "egress_proxy")]
    EgressProxy,
    #[serde(rename = "executor")]
    Executor,
}

fn schema_vault_consumer_create_request_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["browser", "egress_proxy", "executor"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerCreateRequestLevel {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "partner")]
    Partner,
    #[serde(rename = "tenant")]
    Tenant,
}

fn schema_vault_consumer_create_request_level_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["platform", "partner", "tenant"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerCreateRequestHosting {
    #[serde(rename = "self_hosted")]
    SelfHosted,
    #[serde(rename = "third_party")]
    ThirdParty,
}

fn schema_vault_consumer_create_request_hosting_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["self_hosted", "third_party"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerCreateRequestContract {
    #[serde(rename = "vault-consumer/1")]
    VaultConsumer1,
}

fn schema_vault_consumer_create_request_contract_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["vault-consumer/1"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultConsumerCreateRequest {
    #[schema(schema_with = schema_vault_consumer_create_request_kind_property)]
    pub kind: VaultConsumerCreateRequestKind,
    pub name: String,
    #[schema(schema_with = schema_vault_consumer_create_request_level_property)]
    pub level: VaultConsumerCreateRequestLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = TenantId)]
    pub tenant_id: Option<TenantId>,
    #[schema(schema_with = schema_vault_consumer_create_request_hosting_property)]
    pub hosting: VaultConsumerCreateRequestHosting,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub provider: Option<Option<String>>,
    #[schema(schema_with = schema_vault_consumer_create_request_contract_property)]
    pub contract: VaultConsumerCreateRequestContract,
    pub signing_jwk: VaultOkpJwk,
    pub encryption_jwk: VaultOkpJwk,
    pub declarations: VaultConsumerDeclarations,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultConsumerActivateRequest {
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultConsumerKeysRequest {
    pub signing_jwk: VaultOkpJwk,
    pub encryption_jwk: VaultOkpJwk,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub overlap_seconds: Option<i64>,
}

fn schema_vault_custody_request_slots_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [1, 2], "type": "integer"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultCustodyRequest {
    #[schema(schema_with = schema_vault_custody_request_slots_property)]
    pub slots: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultRevokeAllRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub shred_sessions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub archive_items: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultRedeemRequest {
    pub ticket: String,
    pub phase: VaultPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub wait_seconds: Option<i64>,
    pub context: VaultRedeemContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultReportRequestOutcome {
    #[serde(rename = "filled")]
    Filled,
    #[serde(rename = "submitted")]
    Submitted,
    #[serde(rename = "signed_in")]
    SignedIn,
    #[serde(rename = "signed_up")]
    SignedUp,
    #[serde(rename = "credentials_rejected")]
    CredentialsRejected,
    #[serde(rename = "otp_rejected")]
    OtpRejected,
    #[serde(rename = "session_rejected")]
    SessionRejected,
    #[serde(rename = "value_exposed")]
    ValueExposed,
    #[serde(rename = "value_exposed_unscannable")]
    ValueExposedUnscannable,
    #[serde(rename = "placeholder_misplaced")]
    PlaceholderMisplaced,
    #[serde(rename = "origin_mismatch")]
    OriginMismatch,
    #[serde(rename = "frame_mismatch")]
    FrameMismatch,
    #[serde(rename = "form_not_found")]
    FormNotFound,
    #[serde(rename = "aborted")]
    Aborted,
    #[serde(rename = "password_changed")]
    PasswordChanged,
    #[serde(rename = "password_change_failed")]
    PasswordChangeFailed,
}

fn schema_vault_report_request_outcome_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["filled", "submitted", "signed_in", "signed_up", "credentials_rejected", "otp_rejected", "session_rejected", "value_exposed", "value_exposed_unscannable", "placeholder_misplaced", "origin_mismatch", "frame_mismatch", "form_not_found", "aborted", "password_changed", "password_change_failed"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultReportRequest {
    pub ticket: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultPhase)]
    pub phase: Option<VaultPhase>,
    #[schema(schema_with = schema_vault_report_request_outcome_property)]
    pub outcome: VaultReportRequestOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub r#final: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultWriteBackRequestKind {
    #[serde(rename = "session")]
    Session,
    #[serde(rename = "secret")]
    Secret,
    #[serde(rename = "password_confirmed")]
    PasswordConfirmed,
}

fn schema_vault_write_back_request_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["session", "secret", "password_confirmed"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultWriteBackRequest {
    pub ticket: String,
    #[schema(schema_with = schema_vault_write_back_request_kind_property)]
    pub kind: VaultWriteBackRequestKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultSessionState)]
    pub state: Option<VaultSessionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub secret: Option<String>,
}
