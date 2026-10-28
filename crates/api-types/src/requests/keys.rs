//! Request bodies of the `keys` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// `tenant_id` is required for `tenant` and `identity` keys, and `identity_id` for `identity` keys; neither is allowed on `platform` or `partner` keys. `partner_id` is required for `partner` keys, which only a platform key may create, and refused on every other level. `permissions` is required at every level, `platform` included (no implicit full set). Level, tenant, identity and permissions must lie within the caller's own (`403 key_scope_exceeded`), and each permission must be one the level can hold (`400 invalid_request`, `details.reason = "permission_not_allowed_for_level"`; see `Permission`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyCreateRequest {
    pub name: String,
    pub level: KeyLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = PartnerId)]
    pub partner_id: Option<PartnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = TenantId)]
    pub tenant_id: Option<TenantId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentityId)]
    pub identity_id: Option<IdentityId>,
    pub permissions: Vec<Permission>,
    /// Optional. The MCP tools the key may see, at most 64 distinct names, narrowing what its permissions and level allow. Absent or `null`: every allowed tool. An unknown name is `400 invalid_request` with `details.reason = "unknown_mcp_tool"`; a tool the new key could not see is `details.reason = "mcp_tool_not_permitted"` (both with `details.tool`). Rotation keeps the list; keys minted by this key do not inherit it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = McpToolList)]
    pub mcp_tools: Option<Option<McpToolList>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<Option<String>>,
}
