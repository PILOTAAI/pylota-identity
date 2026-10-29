//! Request bodies of the `agent_sign_up` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

fn schema_agent_signup_create_request_accept_terms_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [true], "type": "boolean"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct AgentSignupCreateRequest {
    /// 2–32 characters of `[a-z0-9-]` after lower-casing; checked by the address-suffix rules, not a schema pattern. The inbox is `agent.{username}@{platform domain}`.
    pub username: String,
    /// The person responsible for the agent. They get a six-digit code and a link.
    pub human_email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub agent_name: Option<String>,
    /// The client that signs up, for example an SDK and its version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub source: Option<String>,
    #[schema(schema_with = schema_agent_signup_create_request_accept_terms_property)]
    pub accept_terms: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct AgentSignupVerifyRequest {
    pub code: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct AgentSignupHumanRequest {
    pub human_email: String,
}
