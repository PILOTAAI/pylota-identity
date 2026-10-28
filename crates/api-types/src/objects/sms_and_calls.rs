//! Objects first used by the `sms_and_calls` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SmsDirection {
    #[serde(rename = "inbound")]
    Inbound,
    #[serde(rename = "outbound")]
    Outbound,
}

fn schema_sms_direction_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["inbound", "outbound"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Sms {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub conversation_id: Option<String>,
    pub number_id: PhoneNumberId,
    #[schema(schema_with = schema_sms_direction_property)]
    pub direction: SmsDirection,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub to: Option<String>,
    /// Untrusted; codes appear as `[code]`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub text: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub segments: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub flags: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub reason: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub client_reference: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub received_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub sent_at: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsDryRun {
    pub would_send: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub segments: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub price_pence: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsPage {
    pub data: Vec<Sms>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsConversationPageDataItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub number_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub counterparty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub last_message_at: Option<String>,
}

fn schema_sms_conversation_page_data_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"counterparty": {"type": "string"}, "id": {"type": "string"}, "last_message_at": {"format": "date-time", "type": "string"}, "number_id": {"type": "string"}}, "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsConversationPage {
    #[schema(schema_with = schema_sms_conversation_page_data_property)]
    pub data: Vec<SmsConversationPageDataItem>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsWaitResultVerification {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub value: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub account_id: Option<Option<String>>,
}

fn schema_sms_wait_result_verification_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"account_id": {"type": ["string", "null"]}, "kind": {"type": "string"}, "value": {"type": "string"}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmsWaitResult {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Sms>)]
    pub sms: Option<Option<Sms>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_sms_wait_result_verification_property)]
    pub verification: Option<Option<SmsWaitResultVerification>>,
    pub timed_out: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CallPage {
    pub data: Vec<Call>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneUsageNumbersItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub number_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub sms_out_segments: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub sms_in_segments: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub calls: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub voice_minutes: Option<i64>,
}

fn schema_phone_usage_numbers_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"items": {"properties": {"calls": {"type": "integer"}, "number_id": {"type": "string"}, "sms_in_segments": {"type": "integer"}, "sms_out_segments": {"type": "integer"}, "voice_minutes": {"type": "integer"}}, "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub month: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_usage_numbers_property)]
    pub numbers: Option<Vec<PhoneUsageNumbersItem>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneConsentState {
    #[serde(rename = "opted_out")]
    OptedOut,
    #[serde(rename = "opted_in")]
    OptedIn,
}

fn schema_phone_consent_state_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"enum": ["opted_out", "opted_in", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneConsent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub number_hint: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_consent_state_property)]
    pub state: Option<Option<PhoneConsentState>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub purpose: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub source: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub updated_at: Option<Option<String>>,
}
