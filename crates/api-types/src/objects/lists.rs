//! Objects first used by the `lists` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ListDirection {
    #[serde(rename = "receive")]
    Receive,
    #[serde(rename = "send")]
    Send,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ListKind {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "block")]
    Block,
}

/// An address (`user@example.com`) or a whole domain (`@example.com`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ListEntryValue(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ListEntryScope {
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "identity")]
    Identity,
}

fn schema_list_entry_scope_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["tenant", "identity"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ListEntry {
    pub direction: ListDirection,
    pub kind: ListKind,
    pub entry: ListEntryValue,
    #[schema(schema_with = schema_list_entry_scope_property)]
    pub scope: ListEntryScope,
    /// The identity of an `identity`-scope entry; `null` for the tenant scope.
    #[schema(required = true)]
    pub identity_id: Option<IdentityId>,
    #[schema(required = true)]
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ListEntryPage {
    pub data: Vec<ListEntry>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

impl std::fmt::Display for ListEntryValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
