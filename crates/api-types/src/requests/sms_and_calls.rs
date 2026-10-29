//! Request bodies of the `sms_and_calls` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SmsSendRequestKind {
    #[serde(rename = "transactional")]
    Transactional,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "verification")]
    Verification,
}

fn schema_sms_send_request_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": "transactional", "enum": ["transactional", "marketing", "verification"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct SmsSendRequest {
    /// A number ID or E.164 of this identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub from: Option<String>,
    pub to: E164,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_sms_send_request_kind_property)]
    pub kind: Option<SmsSendRequestKind>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub conversation_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub client_reference: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum SmsResolveRequestOutcome {
    #[serde(rename = "sent")]
    #[default]
    Sent,
    #[serde(rename = "not_sent")]
    NotSent,
}

fn schema_sms_resolve_request_outcome_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["sent", "not_sent"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct SmsResolveRequest {
    #[schema(schema_with = schema_sms_resolve_request_outcome_property)]
    pub outcome: SmsResolveRequestOutcome,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct CallCreateRequestAssistant {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub assistant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub variables: Option<serde_json::Value>,
}

fn schema_call_create_request_assistant_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"assistant_id": {"type": "string"}, "variables": {"type": "object"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct CallCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub from: Option<String>,
    pub to: E164,
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_call_create_request_assistant_property)]
    pub assistant: Option<CallCreateRequestAssistant>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum CallOrRequest {
    Call(Box<Call>),
    PhoneRequest(Box<PhoneRequest>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum PhoneConsentRequestState {
    #[serde(rename = "opted_out")]
    #[default]
    OptedOut,
    #[serde(rename = "opted_in")]
    OptedIn,
}

fn schema_phone_consent_request_state_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["opted_out", "opted_in"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneConsentRequestPurpose {
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "all")]
    All,
}

fn schema_phone_consent_request_purpose_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["marketing", "all"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct PhoneConsentRequest {
    pub number: E164,
    #[schema(schema_with = schema_phone_consent_request_state_property)]
    pub state: PhoneConsentRequestState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_consent_request_purpose_property)]
    pub purpose: Option<PhoneConsentRequestPurpose>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub evidence: Option<String>,
}
