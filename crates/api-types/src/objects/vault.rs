//! Objects first used by the `vault` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Vault consumer ID (`vcn_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct VaultConsumerId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultOkpJwkKty {
    #[serde(rename = "OKP")]
    #[default]
    OKP,
}

fn schema_vault_okp_jwk_kty_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["OKP"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultOkpJwkCrv {
    #[serde(rename = "Ed25519")]
    #[default]
    Ed25519,
    #[serde(rename = "X25519")]
    X25519,
}

fn schema_vault_okp_jwk_crv_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["Ed25519", "X25519"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// A public OKP JWK. A JWK with a private member (`d`) is refused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct VaultOkpJwk {
    #[schema(schema_with = schema_vault_okp_jwk_kty_property)]
    pub kty: VaultOkpJwkKty,
    #[schema(schema_with = schema_vault_okp_jwk_crv_property)]
    pub crv: VaultOkpJwkCrv,
    pub x: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VaultPasswordInput(pub serde_json::Value);

impl utoipa::PartialSchema for VaultPasswordInput {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"oneOf": [{"description": "The password. Write-only.", "maxLength": 1024, "minLength": 1, "type": "string"}, {"properties": {"generate": {"properties": {"classes": {"items": {"enum": ["lower", "upper", "digit", "symbol"], "type": "string"}, "type": "array"}, "length": {"default": 24, "maximum": 128, "minimum": 16, "type": "integer"}}, "type": "object"}}, "required": ["generate"], "type": "object"}]}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultPasswordInput {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultSessionStateCookiesItemSameSite {
    #[serde(rename = "Strict")]
    Strict,
    #[serde(rename = "Lax")]
    Lax,
    #[serde(rename = "None")]
    None,
}

fn schema_vault_session_state_cookies_item_same_site_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["Strict", "Lax", "None"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultSessionStateCookiesItem {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = f64)]
    pub expires: Option<f64>,
    #[serde(rename = "httpOnly")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub http_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub secure: Option<bool>,
    #[serde(rename = "sameSite")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_session_state_cookies_item_same_site_property)]
    pub same_site: Option<VaultSessionStateCookiesItemSameSite>,
}

fn schema_vault_session_state_cookies_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"domain": {"type": "string"}, "expires": {"type": "number"}, "httpOnly": {"type": "boolean"}, "name": {"type": "string"}, "path": {"type": "string"}, "sameSite": {"enum": ["Strict", "Lax", "None"], "type": "string"}, "secure": {"type": "boolean"}, "value": {"type": "string"}}, "required": ["name", "value", "domain", "path"], "type": "object"}, "maxItems": 500, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultSessionStateOriginsItemLocalStorageItem {
    pub name: String,
    pub value: String,
}

fn schema_vault_session_state_origins_item_local_storage_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"name": {"type": "string"}, "value": {"type": "string"}}, "required": ["name", "value"], "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultSessionStateOriginsItem {
    pub origin: VaultOrigin,
    #[schema(schema_with = schema_vault_session_state_origins_item_local_storage_property)]
    pub local_storage: Vec<VaultSessionStateOriginsItemLocalStorageItem>,
}

fn schema_vault_session_state_origins_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"local_storage": {"items": {"properties": {"name": {"type": "string"}, "value": {"type": "string"}}, "required": ["name", "value"], "type": "object"}, "type": "array"}, "origin": {"$ref": "#/components/schemas/VaultOrigin"}}, "required": ["origin", "local_storage"], "type": "object"}, "maxItems": 10, "type": "array"}"###).expect("the contract's schema")
}

/// Playwright `storageState` shape. Write-only; filtered to the item's origins before sealing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultSessionState {
    #[schema(schema_with = schema_vault_session_state_cookies_property)]
    pub cookies: Vec<VaultSessionStateCookiesItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_session_state_origins_property)]
    pub origins: Option<Vec<VaultSessionStateOriginsItem>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemCreated {
    #[serde(flatten)]
    pub base: VaultItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub cookies_dropped: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub clearance_cookies_dropped: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub origins_dropped: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultItemPage {
    pub data: Vec<VaultItem>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultGrantDecision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub expires_in: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultGrantPage {
    pub data: Vec<VaultGrant>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultUseCreated {
    pub r#use: VaultUse,
    /// Shown once, here or by `mintVaultTicket`. Never replayed.
    #[schema(required = true)]
    pub ticket: Option<String>,
    #[schema(required = true)]
    pub ticket_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub ticket_replayed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultSignature)]
    pub signature: Option<VaultSignature>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultUsePage {
    pub data: Vec<VaultUse>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultTicket {
    pub ticket: String,
    pub ticket_expires_at: String,
    pub phases: Vec<VaultPhase>,
    pub use_id: VaultUseId,
}

/// P1. A signature is not the secret.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultSignature {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub mac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub authorization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub x_amz_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub x_amz_content_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultConsumerDeclarationsSessionRecording {
    #[serde(rename = "off")]
    #[default]
    Off,
}

fn schema_vault_consumer_declarations_session_recording_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultConsumerDeclarationsSessionLogging {
    #[serde(rename = "off")]
    #[default]
    Off,
}

fn schema_vault_consumer_declarations_session_logging_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultConsumerDeclarationsCaptchaSolving {
    #[serde(rename = "off")]
    #[default]
    Off,
}

fn schema_vault_consumer_declarations_captcha_solving_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultConsumerDeclarationsExtensions {
    #[serde(rename = "none")]
    #[default]
    None,
}

fn schema_vault_consumer_declarations_extensions_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["none"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultConsumerDeclarationsHonestIdentity {
    #[serde(rename = "web_bot_auth")]
    #[default]
    WebBotAuth,
    #[serde(rename = "user_agent")]
    UserAgent,
    #[serde(rename = "none")]
    None,
}

fn schema_vault_consumer_declarations_honest_identity_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["web_bot_auth", "user_agent", "none"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

fn schema_vault_consumer_declarations_observation_paused_during_fill_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [true], "type": "boolean"}"###)
        .expect("the contract's schema")
}

fn schema_vault_consumer_declarations_post_fill_check_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [true], "type": "boolean"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerDeclarationsModesItem {
    #[serde(rename = "cdp")]
    Cdp,
    #[serde(rename = "proxy")]
    Proxy,
}

fn schema_vault_consumer_declarations_modes_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"enum": ["cdp", "proxy"], "type": "string"}, "minItems": 1, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct VaultConsumerDeclarations {
    #[schema(schema_with = schema_vault_consumer_declarations_session_recording_property)]
    pub session_recording: VaultConsumerDeclarationsSessionRecording,
    #[schema(schema_with = schema_vault_consumer_declarations_session_logging_property)]
    pub session_logging: VaultConsumerDeclarationsSessionLogging,
    #[schema(schema_with = schema_vault_consumer_declarations_captcha_solving_property)]
    pub captcha_solving: VaultConsumerDeclarationsCaptchaSolving,
    #[schema(schema_with = schema_vault_consumer_declarations_extensions_property)]
    pub extensions: VaultConsumerDeclarationsExtensions,
    #[schema(schema_with = schema_vault_consumer_declarations_honest_identity_property)]
    pub honest_identity: VaultConsumerDeclarationsHonestIdentity,
    #[schema(schema_with = schema_vault_consumer_declarations_observation_paused_during_fill_property)]
    pub observation_paused_during_fill: bool,
    #[schema(schema_with = schema_vault_consumer_declarations_post_fill_check_property)]
    pub post_fill_check: bool,
    #[schema(schema_with = schema_vault_consumer_declarations_modes_property)]
    pub modes: Vec<VaultConsumerDeclarationsModesItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerKind {
    #[serde(rename = "browser")]
    Browser,
    #[serde(rename = "egress_proxy")]
    EgressProxy,
    #[serde(rename = "executor")]
    Executor,
}

fn schema_vault_consumer_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"enum": ["browser", "egress_proxy", "executor"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerLevel {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "partner")]
    Partner,
    #[serde(rename = "tenant")]
    Tenant,
}

fn schema_vault_consumer_level_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["platform", "partner", "tenant"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerHosting {
    #[serde(rename = "self_hosted")]
    SelfHosted,
    #[serde(rename = "third_party")]
    ThirdParty,
}

fn schema_vault_consumer_hosting_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["self_hosted", "third_party"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultConsumerStatus {
    #[serde(rename = "pending_activation")]
    PendingActivation,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "revoked")]
    Revoked,
}

fn schema_vault_consumer_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["pending_activation", "active", "suspended", "revoked"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultConsumer {
    pub id: VaultConsumerId,
    #[schema(schema_with = schema_vault_consumer_kind_property)]
    pub kind: VaultConsumerKind,
    pub name: String,
    #[schema(schema_with = schema_vault_consumer_level_property)]
    pub level: VaultConsumerLevel,
    #[schema(required = true)]
    pub tenant_id: Option<String>,
    #[schema(required = true)]
    pub partner_id: Option<String>,
    #[schema(schema_with = schema_vault_consumer_hosting_property)]
    pub hosting: VaultConsumerHosting,
    #[schema(required = true)]
    pub provider: Option<String>,
    pub contract: String,
    #[schema(required = true)]
    pub signing_kid: Option<String>,
    #[schema(required = true)]
    pub encryption_kid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultOkpJwk)]
    pub signing_jwk: Option<VaultOkpJwk>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = VaultOkpJwk)]
    pub encryption_jwk: Option<VaultOkpJwk>,
    pub declarations: VaultConsumerDeclarations,
    #[schema(schema_with = schema_vault_consumer_status_property)]
    pub status: VaultConsumerStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub activated_at: Option<Option<String>>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultConsumerPage {
    pub data: Vec<VaultConsumer>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultStatusCustody {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "partner")]
    Partner,
}

fn schema_vault_status_custody_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["operator", "partner"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultStatus {
    pub enabled: bool,
    #[schema(schema_with = schema_vault_status_custody_property)]
    pub custody: VaultStatusCustody,
    #[schema(required = true)]
    pub kid: Option<String>,
    pub items: i64,
    pub active_grants: i64,
    pub pending_grants: i64,
    pub pending_uses: i64,
    pub retiring_versions: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultCustodyResponseCustody {
    #[serde(rename = "partner")]
    Partner,
}

fn schema_vault_custody_response_custody_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["partner"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultCustodyResponse {
    #[schema(schema_with = schema_vault_custody_response_custody_property)]
    pub custody: VaultCustodyResponseCustody,
    /// Shown once; empty on a replay.
    pub unlock_keys: Vec<String>,
    pub secret_replayed: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub operator_copy_restorable_until: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultRevokeAllResult {
    pub grants_revoked: i64,
    pub uses_closed: i64,
    pub session_versions_deleted: i64,
    pub items_archived: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultAuditRow {
    pub id: String,
    pub tenant_id: TenantId,
    pub seq: i64,
    pub prev_hash: String,
    pub row_hash: String,
    pub action: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub item_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub version: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub grant_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub use_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub consumer_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub identity_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub actor_key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub actor_user_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub actor_consumer_id: Option<Option<String>>,
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
    #[schema(value_type = Option<String>)]
    pub operation: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub phase: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub outcome: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub reason: Option<Option<String>>,
    /// The registrable domain of the origin concerned; never a URL.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub site: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub run_id: Option<Option<String>>,
    /// Counts and enumerations only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub details: Option<serde_json::Value>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultAuditPage {
    pub data: Vec<VaultAuditRow>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultRedeemContextFormKind {
    #[serde(rename = "login")]
    Login,
    #[serde(rename = "signup")]
    Signup,
    #[serde(rename = "password_change")]
    PasswordChange,
    #[serde(rename = "otp")]
    Otp,
    #[serde(rename = "reauth")]
    Reauth,
}

fn schema_vault_redeem_context_form_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["login", "signup", "password_change", "otp", "reauth", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum VaultRedeemContextMode {
    #[serde(rename = "cdp")]
    #[default]
    Cdp,
    #[serde(rename = "proxy")]
    Proxy,
}

fn schema_vault_redeem_context_mode_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["cdp", "proxy"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultRedeemContextBrowserHosting {
    #[serde(rename = "self_hosted")]
    SelfHosted,
    #[serde(rename = "third_party")]
    ThirdParty,
}

fn schema_vault_redeem_context_browser_hosting_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["self_hosted", "third_party"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultRedeemContextBrowser {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_redeem_context_browser_hosting_property)]
    pub hosting: Option<VaultRedeemContextBrowserHosting>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub provider: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub session_recording: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub session_logging: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub captcha_solving: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub extensions: Option<String>,
}

fn schema_vault_redeem_context_browser_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"captcha_solving": {"type": "string"}, "extensions": {"type": "string"}, "hosting": {"enum": ["self_hosted", "third_party"], "type": "string"}, "provider": {"type": ["string", "null"]}, "session_logging": {"type": "string"}, "session_recording": {"type": "string"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct VaultRedeemContext {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub top_level_origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub frame_origin: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_vault_redeem_context_form_kind_property)]
    pub form_kind: Option<Option<VaultRedeemContextFormKind>>,
    pub trigger: VaultTrigger,
    #[schema(schema_with = schema_vault_redeem_context_mode_property)]
    pub mode: VaultRedeemContextMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_vault_redeem_context_browser_property)]
    pub browser: Option<VaultRedeemContextBrowser>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultSealedPacketSuite {
    #[serde(rename = "x25519-hkdfsha256-aes256gcm")]
    X25519Hkdfsha256Aes256gcm,
}

fn schema_vault_sealed_packet_suite_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["x25519-hkdfsha256-aes256gcm"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// HPKE base mode, DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, AES-256-GCM, sealed to the consumer's encryption key named in `enc_kid`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultSealedPacket {
    #[schema(schema_with = schema_vault_sealed_packet_suite_property)]
    pub suite: VaultSealedPacketSuite,
    /// base64url
    pub enc: String,
    /// base64url
    pub ct: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultRedeemResponse {
    pub use_id: VaultUseId,
    pub phase: VaultPhase,
    pub enc_kid: String,
    #[schema(required = true)]
    pub packet: Option<VaultSealedPacket>,
    pub remaining_phases: Vec<VaultPhase>,
    pub closes_at: String,
    pub timed_out: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultConsumerAck {
    pub use_id: VaultUseId,
    pub closed: bool,
    #[schema(required = true)]
    pub close_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub cookies_dropped: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub clearance_cookies_dropped: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VaultUseStatus {
    pub use_id: VaultUseId,
    pub status: String,
    pub closed: bool,
    #[schema(required = true)]
    pub close_reason: Option<String>,
    #[schema(required = true)]
    pub closes_at: Option<String>,
    pub remaining_phases: Vec<VaultPhase>,
}

impl std::fmt::Display for VaultConsumerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
