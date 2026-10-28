//! Objects first used by the `metrics` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MetricEventType {
    #[serde(rename = "received")]
    Received,
    #[serde(rename = "quarantined")]
    Quarantined,
    #[serde(rename = "sent")]
    Sent,
    #[serde(rename = "delivered")]
    Delivered,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "bounced")]
    Bounced,
    #[serde(rename = "complained")]
    Complained,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "uncertain")]
    Uncertain,
    #[serde(rename = "suppressed")]
    Suppressed,
    #[serde(rename = "canceled")]
    Canceled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RateWindowId {
    #[serde(rename = "identity_complaints")]
    IdentityComplaints,
    #[serde(rename = "identity_bounces")]
    IdentityBounces,
    #[serde(rename = "tenant_complaints_7d")]
    TenantComplaints7d,
    #[serde(rename = "tenant_bounces_7d")]
    TenantBounces7d,
    #[serde(rename = "domain_complaints_7d")]
    DomainComplaints7d,
    #[serde(rename = "domain_bounces_7d")]
    DomainBounces7d,
    #[serde(rename = "burst_bounces_1h")]
    BurstBounces1h,
    #[serde(rename = "burst_complaints_24h")]
    BurstComplaints24h,
    #[serde(rename = "alert_bounces_1h")]
    AlertBounces1h,
    #[serde(rename = "alert_complaints_24h")]
    AlertComplaints24h,
}

fn schema_rate_window_id_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["identity_complaints", "identity_bounces", "tenant_complaints_7d", "tenant_bounces_7d", "domain_complaints_7d", "domain_bounces_7d", "burst_bounces_1h", "burst_complaints_24h", "alert_bounces_1h", "alert_complaints_24h"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RateWindowMetric {
    #[serde(rename = "complaint_rate")]
    ComplaintRate,
    #[serde(rename = "bounce_rate")]
    BounceRate,
}

fn schema_rate_window_metric_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["complaint_rate", "bounce_rate"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RateWindowAction {
    #[serde(rename = "pause_identity")]
    PauseIdentity,
    #[serde(rename = "pause_tenant")]
    PauseTenant,
    #[serde(rename = "pause_domain")]
    PauseDomain,
    #[serde(rename = "operator_alert")]
    OperatorAlert,
}

fn schema_rate_window_action_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["pause_identity", "pause_tenant", "pause_domain", "operator_alert"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RateWindowWindowKind {
    #[serde(rename = "last_outcomes")]
    LastOutcomes,
    #[serde(rename = "utc_days")]
    UtcDays,
    #[serde(rename = "utc_hours")]
    UtcHours,
}

fn schema_rate_window_window_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["last_outcomes", "utc_days", "utc_hours"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RateWindowWindow {
    #[schema(schema_with = schema_rate_window_window_kind_property)]
    pub kind: RateWindowWindowKind,
    /// Outcomes, for `last_outcomes`.
    #[schema(required = true)]
    pub size: Option<i64>,
    #[schema(required = true)]
    pub from: Option<String>,
    #[schema(required = true)]
    pub to: Option<String>,
}

fn schema_rate_window_window_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"from": {"format": "date-time", "type": ["string", "null"]}, "kind": {"enum": ["last_outcomes", "utc_days", "utc_hours"], "type": "string"}, "size": {"description": "Outcomes, for `last_outcomes`.", "type": ["integer", "null"]}, "to": {"format": "date-time", "type": ["string", "null"]}}, "required": ["kind", "size", "from", "to"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RateWindowComparison {
    #[serde(rename = "gt")]
    Gt,
    #[serde(rename = "ge")]
    Ge,
}

fn schema_rate_window_comparison_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["gt", "ge"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RateWindow {
    #[schema(schema_with = schema_rate_window_id_property)]
    pub id: RateWindowId,
    #[schema(schema_with = schema_rate_window_metric_property)]
    pub metric: RateWindowMetric,
    #[schema(schema_with = schema_rate_window_action_property)]
    pub action: RateWindowAction,
    /// The domain of a domain or burst window. `null` for identity and tenant windows, and, for an identity key, on the burst windows of a domain none of its addresses use.
    #[schema(required = true)]
    pub domain_id: Option<DomainId>,
    #[schema(schema_with = schema_rate_window_window_property)]
    pub window: RateWindowWindow,
    #[schema(schema_with = schema_rate_window_comparison_property)]
    pub comparison: RateWindowComparison,
    /// Compared in parts per million.
    pub threshold: f64,
    pub threshold_source: String,
    #[schema(required = true)]
    pub min_outcomes: Option<i64>,
    pub outcomes: i64,
    pub events: i64,
    pub denominator: i64,
    /// `events / denominator`, rounded to 6 decimal places.
    pub rate: f64,
    pub evaluated: bool,
    pub tripped: bool,
    /// The fewest further complaints or bounces that would trip the rule at `evaluated_at`; fewer further outcomes of any kinds cannot. It can fall between reads (a delivery below the window's minimum, an hour or day rolling out), so read it just before a batch. `null` when no number could trip it: a threshold of 1 with `gt`, or with `ge` once the window holds an outcome not of its kind.
    #[schema(required = true)]
    pub headroom: Option<i64>,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SendingStopScope {
    #[serde(rename = "identity")]
    Identity,
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "domain")]
    Domain,
}

fn schema_sending_stop_scope_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["identity", "tenant", "domain"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SendingStop {
    #[schema(schema_with = schema_sending_stop_scope_property)]
    pub scope: SendingStopScope,
    pub id: String,
    pub reason: String,
    #[schema(required = true)]
    pub paused_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RatesScope {
    #[serde(rename = "identity")]
    Identity,
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "domain")]
    Domain,
}

fn schema_rates_scope_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["identity", "tenant", "domain"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RatesSending {
    pub paused: bool,
    pub stops: Vec<SendingStop>,
}

fn schema_rates_sending_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"paused": {"type": "boolean"}, "stops": {"items": {"$ref": "#/components/schemas/SendingStop"}, "type": "array"}}, "required": ["paused", "stops"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Rates {
    #[schema(schema_with = schema_rates_scope_property)]
    pub scope: RatesScope,
    pub tenant_id: TenantId,
    #[schema(required = true)]
    pub identity_id: Option<IdentityId>,
    #[schema(required = true)]
    pub domain_id: Option<DomainId>,
    pub evaluated_at: String,
    pub exempt: bool,
    #[schema(schema_with = schema_rates_sending_property)]
    pub sending: RatesSending,
    pub windows: Vec<RateWindow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum EventCountsScope {
    #[serde(rename = "identity")]
    Identity,
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "domain")]
    Domain,
}

fn schema_event_counts_scope_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["identity", "tenant", "domain"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum EventCountsBucket {
    #[serde(rename = "hour")]
    Hour,
    #[serde(rename = "day")]
    Day,
}

fn schema_event_counts_bucket_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["hour", "day"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventCountsDataItem {
    pub start: String,
    pub counts: BTreeMap<String, i64>,
}

fn schema_event_counts_data_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"counts": {"additionalProperties": {"minimum": 0, "type": "integer"}, "type": "object"}, "start": {"format": "date-time", "type": "string"}}, "required": ["start", "counts"], "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventCounts {
    #[schema(schema_with = schema_event_counts_scope_property)]
    pub scope: EventCountsScope,
    pub tenant_id: TenantId,
    #[schema(required = true)]
    pub identity_id: Option<IdentityId>,
    #[schema(required = true)]
    pub domain_id: Option<DomainId>,
    #[schema(schema_with = schema_event_counts_bucket_property)]
    pub bucket: EventCountsBucket,
    pub from: String,
    pub to: String,
    pub types: Vec<MetricEventType>,
    pub identities: i64,
    pub partial: bool,
    pub failed_identities: Vec<IdentityId>,
    #[schema(schema_with = schema_event_counts_data_property)]
    pub data: Vec<EventCountsDataItem>,
    pub totals: BTreeMap<String, i64>,
}
