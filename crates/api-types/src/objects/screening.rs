//! Objects first used by the `screening` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningFeedbackVerdict {
    #[serde(rename = "correct")]
    Correct,
    #[serde(rename = "false_positive")]
    FalsePositive,
    #[serde(rename = "missed_threat")]
    MissedThreat,
    #[serde(rename = "legitimate_instruction")]
    LegitimateInstruction,
    #[serde(rename = "spam")]
    Spam,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningFeedbackScreeningStatus {
    #[serde(rename = "clear")]
    Clear,
    #[serde(rename = "suspect")]
    Suspect,
    #[serde(rename = "unavailable")]
    Unavailable,
}

fn schema_screening_feedback_screening_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["clear", "suspect", "unavailable"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningFeedbackActorKind {
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "person")]
    Person,
}

fn schema_screening_feedback_actor_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["key", "person"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScreeningFeedbackActor {
    #[schema(schema_with = schema_screening_feedback_actor_kind_property)]
    pub kind: ScreeningFeedbackActorKind,
    #[schema(required = true)]
    pub key_id: Option<String>,
    #[schema(required = true)]
    pub user_id: Option<String>,
}

fn schema_screening_feedback_actor_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"key_id": {"type": ["string", "null"]}, "kind": {"enum": ["key", "person"], "type": "string"}, "user_id": {"type": ["string", "null"]}}, "required": ["kind", "key_id", "user_id"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScreeningFeedback {
    /// Screening feedback ID (`sfb_` + ULID).
    pub id: String,
    pub message_id: MessageId,
    pub verdict: ScreeningFeedbackVerdict,
    #[schema(schema_with = schema_screening_feedback_screening_status_property)]
    pub screening_status: ScreeningFeedbackScreeningStatus,
    #[schema(required = true)]
    pub notes: Option<String>,
    #[schema(schema_with = schema_screening_feedback_actor_property)]
    pub actor: ScreeningFeedbackActor,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScreeningSummaryOutcomes {
    pub held: i64,
    pub delivered: i64,
    pub monitored: i64,
    pub released: i64,
}

fn schema_screening_summary_outcomes_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"delivered": {"type": "integer"}, "held": {"type": "integer"}, "monitored": {"type": "integer"}, "released": {"type": "integer"}}, "required": ["held", "delivered", "monitored", "released"], "type": "object"}"###).expect("the contract's schema")
}

fn schema_screening_summary_feedback_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"additionalProperties": {"minimum": 0, "type": "integer"}, "required": ["messages_reviewed", "correct", "false_positive", "missed_threat", "legitimate_instruction", "spam"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScreeningSummaryQuality {
    pub true_positive: i64,
    pub false_positive: i64,
    pub false_negative: i64,
    pub true_negative: i64,
    #[schema(required = true)]
    pub precision: Option<f64>,
    #[schema(required = true)]
    pub recall: Option<f64>,
}

fn schema_screening_summary_quality_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"false_negative": {"type": "integer"}, "false_positive": {"type": "integer"}, "precision": {"maximum": 1, "minimum": 0, "type": ["number", "null"]}, "recall": {"maximum": 1, "minimum": 0, "type": ["number", "null"]}, "true_negative": {"type": "integer"}, "true_positive": {"type": "integer"}}, "required": ["true_positive", "false_positive", "false_negative", "true_negative", "precision", "recall"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScreeningSummary {
    pub from: String,
    pub to: String,
    pub partial: bool,
    pub failed_identities: Vec<IdentityId>,
    pub verdicts: BTreeMap<String, i64>,
    pub reasons: BTreeMap<String, i64>,
    #[schema(schema_with = schema_screening_summary_outcomes_property)]
    pub outcomes: ScreeningSummaryOutcomes,
    #[schema(schema_with = schema_screening_summary_feedback_property)]
    pub feedback: BTreeMap<String, i64>,
    #[schema(schema_with = schema_screening_summary_quality_property)]
    pub quality: ScreeningSummaryQuality,
}
