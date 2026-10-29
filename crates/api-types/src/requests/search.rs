//! Request bodies of the `search` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SearchRequestMode {
    #[serde(rename = "keyword")]
    Keyword,
    #[serde(rename = "semantic")]
    Semantic,
    #[serde(rename = "hybrid")]
    Hybrid,
}

fn schema_search_request_mode_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"default": "hybrid", "enum": ["keyword", "semantic", "hybrid"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SearchRequestGroupBy {
    #[serde(rename = "message")]
    Message,
    #[serde(rename = "thread")]
    Thread,
}

fn schema_search_request_group_by_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "message", "description": "`thread` returns one `ThreadHit` per thread.", "enum": ["message", "thread"], "type": "string"}"###).expect("the contract's schema")
}

/// A `keyword`, `semantic` or `hybrid` search.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct SearchRequest {
    /// The query, in the query language described on `searchIdentity`. At most 1,024 characters; `""` matches all messages.
    pub q: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_search_request_mode_property)]
    pub mode: Option<SearchRequestMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SearchFilters)]
    pub filters: Option<SearchFilters>,
    /// `thread` returns one `ThreadHit` per thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_search_request_group_by_property)]
    pub group_by: Option<SearchRequestGroupBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub limit: Option<i64>,
    /// Maximum characters per snippet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub snippet_chars: Option<i64>,
    /// Include `facets` in the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub facets: Option<bool>,
    /// Include quarantined mail and mail still being screened. Needs `quarantine:review`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub include_quarantined: Option<bool>,
    /// The `next_cursor` of the previous page.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub cursor: Option<Option<String>>,
    /// Fail with `503 search_degraded` instead of degrading when the requested mode is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub require_mode: Option<bool>,
}

/// A `keyword`, `semantic` or `hybrid` search across a tenant's identities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct TenantSearchRequest {
    #[serde(flatten)]
    pub base: SearchRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentityIdFilter)]
    pub identity_ids: Option<IdentityIdFilter>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum AgenticRequestMode {
    #[serde(rename = "agentic")]
    #[default]
    Agentic,
}

fn schema_agentic_request_mode_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["agentic"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// An agentic search. The planner's tools are read-only and cannot widen the caller's scope or filters ([F10]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct AgenticRequest {
    /// The question, in natural language, at most 1,024 characters.
    pub q: String,
    #[schema(schema_with = schema_agentic_request_mode_property)]
    pub mode: AgenticRequestMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = AgenticBudget)]
    pub budget: Option<AgenticBudget>,
    /// With `Accept: text/event-stream`, stream progress as server-sent events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SearchFilters)]
    pub filters: Option<SearchFilters>,
    /// Include quarantined mail and mail still being screened. Needs `quarantine:review`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub include_quarantined: Option<bool>,
    /// Fail with `503 search_degraded` instead of degrading to hybrid results when the model is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub require_mode: Option<bool>,
}

/// An agentic search across a tenant's identities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct TenantAgenticRequest {
    #[serde(flatten)]
    pub base: AgenticRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentityIdFilter)]
    pub identity_ids: Option<IdentityIdFilter>,
}
