//! Objects first used by the `agent_sign_up` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Agent sign-up ID (`asu_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct AgentSignupId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupState {
    #[serde(rename = "provisional")]
    Provisional,
    #[serde(rename = "verified")]
    Verified,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "abandoned")]
    Abandoned,
    #[serde(rename = "refused")]
    Refused,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignupCreatedKey {
    pub id: KeyId,
    /// Shown once; never stored or replayed.
    pub secret: String,
}

fn schema_agent_signup_created_key_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"id": {"$ref": "#/components/schemas/KeyId"}, "secret": {"description": "Shown once; never stored or replayed.", "type": "string"}}, "required": ["id", "secret"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupCreatedVerificationEmail {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "not_sent")]
    NotSent,
}

fn schema_agent_signup_created_verification_email_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["queued", "not_sent"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignupCreatedVerification {
    #[schema(schema_with = schema_agent_signup_created_verification_email_property)]
    pub email: AgentSignupCreatedVerificationEmail,
    pub code_expires_at: String,
}

fn schema_agent_signup_created_verification_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"code_expires_at": {"format": "date-time", "type": "string"}, "email": {"enum": ["queued", "not_sent"], "type": "string"}}, "required": ["email", "code_expires_at"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupCreatedLimitsRecipients {
    #[serde(rename = "human_only")]
    HumanOnly,
}

fn schema_agent_signup_created_limits_recipients_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["human_only"], "type": "string"}"###)
        .expect("the contract's schema")
}

fn schema_agent_signup_created_limits_daily_sends_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [10], "type": "integer"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignupCreatedLimits {
    #[schema(schema_with = schema_agent_signup_created_limits_recipients_property)]
    pub recipients: AgentSignupCreatedLimitsRecipients,
    #[schema(schema_with = schema_agent_signup_created_limits_daily_sends_property)]
    pub daily_sends: i64,
}

fn schema_agent_signup_created_limits_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"daily_sends": {"enum": [10], "type": "integer"}, "recipients": {"enum": ["human_only"], "type": "string"}}, "required": ["recipients", "daily_sends"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignupCreated {
    pub signup_id: AgentSignupId,
    pub state: AgentSignupState,
    pub tenant_id: TenantId,
    pub identity_id: IdentityId,
    pub address: EmailAddress,
    #[schema(schema_with = schema_agent_signup_created_key_property)]
    pub key: AgentSignupCreatedKey,
    /// The human's address, masked.
    pub human_email_hint: String,
    /// When an unverified sign-up is erased (7 days).
    pub expires_at: String,
    #[schema(schema_with = schema_agent_signup_created_verification_property)]
    pub verification: AgentSignupCreatedVerification,
    #[schema(schema_with = schema_agent_signup_created_limits_property)]
    pub limits: AgentSignupCreatedLimits,
    /// What the agent should do next, in one sentence.
    pub next: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupHumanStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "delivered")]
    Delivered,
    #[serde(rename = "bounced")]
    Bounced,
}

fn schema_agent_signup_human_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["pending", "delivered", "bounced"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupCodeEmail {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "not_sent")]
    NotSent,
}

fn schema_agent_signup_code_email_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["queued", "not_sent"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignupCode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub attempts_left: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub failures_left: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub emails_left_this_hour: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_agent_signup_code_email_property)]
    pub email: Option<AgentSignupCodeEmail>,
}

fn schema_agent_signup_code_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"attempts_left": {"maximum": 10, "minimum": 0, "type": "integer"}, "email": {"enum": ["queued", "not_sent"], "type": "string"}, "emails_left_this_hour": {"maximum": 3, "minimum": 0, "type": "integer"}, "expires_at": {"format": "date-time", "type": "string"}, "failures_left": {"maximum": 30, "minimum": 0, "type": "integer"}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupVerifiedVia {
    #[serde(rename = "code")]
    Code,
    #[serde(rename = "link")]
    Link,
    #[serde(rename = "claim")]
    Claim,
}

fn schema_agent_signup_verified_via_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["code", "link", "claim", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignup {
    pub signup_id: AgentSignupId,
    pub state: AgentSignupState,
    pub tenant_id: TenantId,
    pub identity_id: IdentityId,
    pub address: EmailAddress,
    pub human_email_hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_agent_signup_human_status_property)]
    pub human_status: Option<AgentSignupHumanStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub replacements_left: Option<i64>,
    pub expires_at: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_agent_signup_code_property)]
    pub code: Option<Option<AgentSignupCode>>,
    /// The permissions the key acts with now (the provisional set until verified).
    pub permissions: Vec<Permission>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub verified_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_agent_signup_verified_via_property)]
    pub verified_via: Option<Option<AgentSignupVerifiedVia>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub already_verified: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub source: Option<Option<String>>,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupEndedState {
    #[serde(rename = "abandoned")]
    Abandoned,
}

fn schema_agent_signup_ended_state_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["abandoned"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignupEnded {
    #[schema(schema_with = schema_agent_signup_ended_state_property)]
    pub state: AgentSignupEndedState,
    pub erasure_request_id: ErasureRequestId,
}

impl std::fmt::Display for AgentSignupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
