//! Objects first used by the `privacy` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// An erasure request. `scope` selects which other fields are required.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ErasureRequestCreate {
    ErasureRequestCreateMessage(ErasureRequestCreateMessage),
    ErasureRequestCreateThread(ErasureRequestCreateThread),
    ErasureRequestCreateCounterparty(ErasureRequestCreateCounterparty),
    ErasureRequestCreateIdentity(ErasureRequestCreateIdentity),
    ErasureRequestCreateTenant(ErasureRequestCreateTenant),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErasureRequestCreateMessageScope {
    #[serde(rename = "message")]
    Message,
}

fn schema_erasure_request_create_message_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["message"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureRequestCreateMessage {
    pub tenant_id: TenantId,
    #[schema(schema_with = schema_erasure_request_create_message_scope_property)]
    pub scope: ErasureRequestCreateMessageScope,
    pub identity_id: IdentityId,
    pub message_id: MessageId,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErasureRequestCreateThreadScope {
    #[serde(rename = "thread")]
    Thread,
}

fn schema_erasure_request_create_thread_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["thread"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureRequestCreateThread {
    pub tenant_id: TenantId,
    #[schema(schema_with = schema_erasure_request_create_thread_scope_property)]
    pub scope: ErasureRequestCreateThreadScope,
    pub identity_id: IdentityId,
    pub thread_id: ThreadId,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErasureRequestCreateCounterpartyScope {
    #[serde(rename = "counterparty")]
    Counterparty,
}

fn schema_erasure_request_create_counterparty_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["counterparty"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureRequestCreateCounterparty {
    pub tenant_id: TenantId,
    #[schema(schema_with = schema_erasure_request_create_counterparty_scope_property)]
    pub scope: ErasureRequestCreateCounterpartyScope,
    pub counterparty_address: EmailAddress,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErasureRequestCreateIdentityScope {
    #[serde(rename = "identity")]
    Identity,
}

fn schema_erasure_request_create_identity_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["identity"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureRequestCreateIdentity {
    pub tenant_id: TenantId,
    #[schema(schema_with = schema_erasure_request_create_identity_scope_property)]
    pub scope: ErasureRequestCreateIdentityScope,
    pub identity_id: IdentityId,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ErasureRequestCreateTenantScope {
    #[serde(rename = "tenant")]
    Tenant,
}

fn schema_erasure_request_create_tenant_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["tenant"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureRequestCreateTenant {
    pub tenant_id: TenantId,
    #[schema(schema_with = schema_erasure_request_create_tenant_scope_property)]
    pub scope: ErasureRequestCreateTenantScope,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ExportScope {
    #[serde(rename = "counterparty")]
    Counterparty,
    #[serde(rename = "identity")]
    Identity,
}

/// `expired` once the download is no longer available (7 days after creation, or earlier when an erasure of the tenant expired it). `canceled`: a tenant erasure superseded it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ExportStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "canceled")]
    Canceled,
    #[serde(rename = "expired")]
    Expired,
}

/// A subject-access export. `scope` selects which other field is required.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ExportCreate {
    ExportCreateCounterparty(ExportCreateCounterparty),
    ExportCreateIdentity(ExportCreateIdentity),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ExportCreateCounterpartyScope {
    #[serde(rename = "counterparty")]
    Counterparty,
}

fn schema_export_create_counterparty_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["counterparty"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ExportCreateCounterparty {
    pub tenant_id: TenantId,
    #[schema(schema_with = schema_export_create_counterparty_scope_property)]
    pub scope: ExportCreateCounterpartyScope,
    pub counterparty_address: EmailAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ExportCreateIdentityScope {
    #[serde(rename = "identity")]
    Identity,
}

fn schema_export_create_identity_scope_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["identity"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ExportCreateIdentity {
    pub tenant_id: TenantId,
    #[schema(schema_with = schema_export_create_identity_scope_property)]
    pub scope: ExportCreateIdentityScope,
    pub identity_id: IdentityId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Export {
    pub id: ExportId,
    pub tenant_id: TenantId,
    pub scope: ExportScope,
    /// The identity, for identity scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<IdentityId>)]
    pub identity_id: Option<Option<IdentityId>>,
    pub status: ExportStatus,
    /// Bytes of the ZIP, once `completed`.
    #[schema(required = true)]
    pub size: Option<i64>,
    /// Set when `completed`: a signed link (`getSignedLink`) to a ZIP holding one `.eml` per message plus `messages.json`. Minted again on each `GET`, and valid for one hour from it, or until `expires_at` if that is sooner.
    #[schema(required = true)]
    pub download_url: Option<String>,
    /// When the export and its ZIP are deleted (7 days after it completed). No link works after it.
    #[schema(required = true)]
    pub expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErasureRequestPage {
    pub data: Vec<ErasureRequest>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
