//! Request bodies of the `event_feed` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct EventConsumerCreateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub types: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<IdentityId>>)]
    pub identity_ids: Option<Option<Vec<IdentityId>>>,
    /// `earliest`, `latest` or a cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub start: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct EventConsumerCommitRequest {
    pub cursor: EventCursor,
}

/// Exactly one of `to` and `since`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct EventConsumerResetRequest {
    /// `earliest`, `latest` or a cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub since: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneRequestBlockedByItem {
    #[serde(rename = "compliance")]
    Compliance,
    #[serde(rename = "platform_prerequisite")]
    PlatformPrerequisite,
}

fn schema_phone_request_blocked_by_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"enum": ["compliance", "platform_prerequisite"], "type": "string"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneRequestApprovedBy {
    #[serde(rename = "person")]
    Person,
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "standing_budget")]
    StandingBudget,
    #[serde(rename = "purchase")]
    Purchase,
}

fn schema_phone_request_approved_by_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["person", "key", "standing_budget", "purchase", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct PhoneRequestDetails {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub outside_budget: Option<Option<String>>,
}

fn schema_phone_request_details_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"properties": {"outside_budget": {"type": ["string", "null"]}}, "type": "object"}"###,
    )
    .expect("the contract's schema")
}

fn schema_phone_request_result_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct PhoneRequest {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = IdentityId)]
    pub identity_id: Option<IdentityId>,
    pub kind: PhoneRequestKind,
    pub status: PhoneRequestStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub reason: Option<Option<String>>,
    #[schema(value_type = Object)]
    pub payload: serde_json::Value,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<PhoneQuote>)]
    pub quote: Option<Option<PhoneQuote>>,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_request_blocked_by_property)]
    pub blocked_by: Option<Vec<PhoneRequestBlockedByItem>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_request_approved_by_property)]
    pub approved_by: Option<Option<PhoneRequestApprovedBy>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub budget_version: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_request_details_property)]
    pub details: Option<PhoneRequestDetails>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<NextStep>)]
    pub next_step: Option<Option<NextStep>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub release_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub decided_by_key_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub decided_by_user_id: Option<Option<String>>,
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
    pub expires_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_request_result_property)]
    pub result: Option<Option<serde_json::Value>>,
}
