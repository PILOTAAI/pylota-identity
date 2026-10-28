//! Objects first used by the `usage` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UsageDay {
    /// The UTC day.
    pub day: String,
    pub inbound: i64,
    pub outbound: i64,
    /// Metered sends (one per recipient the transport accepted).
    pub sends: i64,
    /// Stored triage analyses.
    pub triage: i64,
    pub search: i64,
    pub agentic: i64,
    /// Agent assertions minted (`POST …/assertions`). Counted only; not metered against a plan allowance.
    pub assertions: i64,
    /// Web Bot Auth HTTP signatures made (`POST …/http-signatures`). Counted only; not metered against a plan allowance.
    pub http_signatures: i64,
    /// Workers AI usage in neurons.
    pub ai_neurons: i64,
    pub storage_bytes: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DailyUsage {
    pub data: Vec<UsageDay>,
}

/// The state of one allowance in the current period.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UsageFeature {
    pub feature: UsageFeatureName,
    /// The allowance, top-ups included. `null` when unlimited.
    #[schema(required = true)]
    pub granted: Option<i64>,
    /// For `storage_gb`, measured, rounded up, and refreshed at least hourly.
    pub used: i64,
    /// `null` when unlimited.
    #[schema(required = true)]
    pub remaining: Option<i64>,
    pub unlimited: bool,
    /// When a monthly allowance resets; `null` for counts.
    #[schema(required = true)]
    pub resets_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlanStateStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "trialing")]
    Trialing,
    #[serde(rename = "past_due")]
    PastDue,
    #[serde(rename = "canceled")]
    Canceled,
    #[serde(rename = "incomplete")]
    Incomplete,
}

fn schema_plan_state_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["active", "trialing", "past_due", "canceled", "incomplete"], "type": "string"}"###).expect("the contract's schema")
}

/// The workspace's current plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlanState {
    pub plan_id: String,
    #[schema(schema_with = schema_plan_state_status_property)]
    pub status: PlanStateStatus,
    pub current_period_end: String,
    pub cancel_at_period_end: bool,
}

/// The allowance of each feature; `null` means unlimited.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlanIncluded {
    #[schema(required = true)]
    pub inboxes: Option<i64>,
    #[schema(required = true)]
    pub sends: Option<i64>,
    #[schema(required = true)]
    pub triage: Option<i64>,
    #[schema(required = true)]
    pub custom_domains: Option<i64>,
    #[schema(required = true)]
    pub storage_gb: Option<i64>,
    #[schema(required = true)]
    pub seats: Option<i64>,
}

fn schema_plan_included_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "The allowance of each feature; `null` means unlimited.", "properties": {"custom_domains": {"minimum": 0, "type": ["integer", "null"]}, "inboxes": {"minimum": 0, "type": ["integer", "null"]}, "seats": {"minimum": 0, "type": ["integer", "null"]}, "sends": {"minimum": 0, "type": ["integer", "null"]}, "storage_gb": {"minimum": 0, "type": ["integer", "null"]}, "triage": {"minimum": 0, "type": ["integer", "null"]}}, "required": ["inboxes", "sends", "triage", "custom_domains", "storage_gb", "seats"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlanSupport {
    #[serde(rename = "github_issues")]
    GithubIssues,
    #[serde(rename = "email")]
    Email,
    #[serde(rename = "priority_email")]
    PriorityEmail,
}

fn schema_plan_support_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["github_issues", "email", "priority_email"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

/// A plan of the catalog (`PM_PLAN_CATALOG`). Stripe price IDs are never returned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Plan {
    pub plan_id: String,
    pub name: String,
    /// Display only.
    pub price: f64,
    pub currency: String,
    pub interval: String,
    /// The allowance of each feature; `null` means unlimited.
    #[schema(schema_with = schema_plan_included_property)]
    pub included: PlanIncluded,
    /// Whether top-ups can be bought on this plan.
    pub topups: bool,
    #[schema(schema_with = schema_plan_support_property)]
    pub support: PlanSupport,
}

/// Top-up units held, per feature that has top-ups.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Topups {
    pub inboxes: i64,
    pub sends: i64,
    pub triage: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UsageSummary {
    pub billing: BillingMode,
    pub plan: PlanState,
    pub features: Vec<UsageFeature>,
    pub topups: Topups,
    /// The plan catalog.
    pub plans: Vec<Plan>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlanCatalog {
    /// `false` on a deployment without billing, with an empty `data`.
    pub billing_enabled: bool,
    pub data: Vec<Plan>,
}

/// A workspace's billing account.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BillingAccount {
    pub tenant_id: TenantId,
    pub mode: BillingMode,
    pub plan: PlanState,
    pub topups: Topups,
}
