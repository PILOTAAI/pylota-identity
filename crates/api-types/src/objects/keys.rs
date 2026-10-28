//! Objects first used by the `keys` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ApiKey {
    pub id: KeyId,
    pub name: String,
    pub level: KeyLevel,
    pub mode: Mode,
    /// `null` unless the key is partner-level.
    #[schema(required = true)]
    pub partner_id: Option<PartnerId>,
    #[schema(required = true)]
    pub tenant_id: Option<TenantId>,
    #[schema(required = true)]
    pub identity_id: Option<IdentityId>,
    pub permissions: Vec<Permission>,
    pub mcp_tools: McpToolList,
    #[schema(required = true)]
    pub expires_at: Option<String>,
    #[schema(required = true)]
    pub revoked_at: Option<String>,
    /// Updated at most once a minute.
    #[schema(required = true)]
    pub last_used_at: Option<String>,
    /// The console user behind the key: the person whose console session minted it, copied to every key minted with it. `null` for a key minted by a platform or partner key with no person behind it. Removing that person from the workspace, or lowering their role, revokes the key ([W36]).
    #[schema(required = true)]
    pub created_by_user_id: Option<UserId>,
    pub created_at: String,
}

fn schema_api_key_created_secret_replayed_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present only on an idempotent replay, which carries no secret. Rotate or revoke the key if the secret was lost.", "enum": [false], "type": "boolean"}"###).expect("the contract's schema")
}

/// A key with its secret. The secret is shown only in this response: an idempotent replay (`Idempotent-Replayed: true`) returns the same body without `secret` and with `"secret_replayed": false`, because the stored response never holds it (FR-KEY-2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyCreated {
    #[serde(flatten)]
    pub base: ApiKey,
    /// `pmk_live_<lookup12>_<secret>` or `pmk_test_<lookup12>_<secret>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub secret: Option<String>,
    /// Present only on an idempotent replay, which carries no secret. Rotate or revoke the key if the secret was lost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_api_key_created_secret_replayed_property)]
    pub secret_replayed: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyPage {
    pub data: Vec<ApiKey>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
