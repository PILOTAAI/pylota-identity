//! Objects first used by the `audit` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Audit entry ID (`aud_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct AuditEventId(pub String);

fn schema_audit_event_details_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "Never message content or clear-text addresses.", "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AuditEvent {
    pub id: AuditEventId,
    #[schema(required = true)]
    pub tenant_id: Option<TenantId>,
    /// The key that acted, or `null` for a console user or the system (for example retention purges).
    #[schema(required = true)]
    pub actor_key_id: Option<KeyId>,
    /// The console user who acted, or `null` for an API key or the system.
    #[schema(required = true)]
    pub actor_user_id: Option<UserId>,
    pub action: String,
    #[schema(required = true)]
    pub target_type: Option<String>,
    #[schema(required = true)]
    pub target_id: Option<String>,
    /// Never message content or clear-text addresses.
    #[schema(required = true, schema_with = schema_audit_event_details_property)]
    pub details: Option<serde_json::Value>,
    #[schema(required = true)]
    pub request_id: Option<RequestId>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AuditEventPage {
    pub data: Vec<AuditEvent>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

impl std::fmt::Display for AuditEventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
