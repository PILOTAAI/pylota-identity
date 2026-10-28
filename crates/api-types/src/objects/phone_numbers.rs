//! Objects first used by the `phone_numbers` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// A phone number in E.164 form.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct E164(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberStatus {
    #[serde(rename = "ordering")]
    Ordering,
    #[serde(rename = "uncertain")]
    Uncertain,
    #[serde(rename = "under_review")]
    UnderReview,
    #[serde(rename = "requirements_pending")]
    RequirementsPending,
    #[serde(rename = "configuring")]
    Configuring,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "port_out_pending")]
    PortOutPending,
    #[serde(rename = "porting_in")]
    PortingIn,
    #[serde(rename = "releasing")]
    Releasing,
    #[serde(rename = "released")]
    Released,
    #[serde(rename = "ported_out")]
    PortedOut,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum CapabilityState {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "activating")]
    Activating,
    #[serde(rename = "registration_required")]
    RegistrationRequired,
    #[serde(rename = "not_available")]
    NotAvailable,
}

fn schema_capability_state_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["enabled", "activating", "registration_required", "not_available"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Capability {
    #[schema(schema_with = schema_capability_state_property)]
    pub state: CapabilityState,
    /// With `not_available`: for example `brand_country_not_supported`, `no_us_emergency_address`, `legacy_unpapered`, `not_on_row`.
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
    #[schema(value_type = Option<NextStep>)]
    pub next_step: Option<Option<NextStep>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneCapabilities {
    pub outbound_sms: Capability,
    pub inbound_sms: Capability,
    pub inbound_calls: Capability,
    pub outbound_calls: Capability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneCountryStatus {
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "requires_local_presence")]
    RequiresLocalPresence,
}

fn schema_phone_country_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"enum": ["available", "requires_local_presence"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneCountryCapabilitiesItem {
    #[serde(rename = "sms")]
    Sms,
    #[serde(rename = "voice")]
    Voice,
    #[serde(rename = "mms_inbound")]
    MmsInbound,
}

fn schema_phone_country_capabilities_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"enum": ["sms", "voice", "mms_inbound"], "type": "string"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneCountrySmsUse {
    #[serde(rename = "two_way")]
    TwoWay,
    #[serde(rename = "a2p_only")]
    A2pOnly,
    #[serde(rename = "none")]
    None,
}

fn schema_phone_country_sms_use_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["two_way", "a2p_only", "none"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneCountryLocalPresence {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "address_in_country")]
    AddressInCountry,
    #[serde(rename = "address_in_area")]
    AddressInArea,
    #[serde(rename = "physical_presence")]
    PhysicalPresence,
}

fn schema_phone_country_local_presence_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["none", "address_in_country", "address_in_area", "physical_presence"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneCountryDestinationsValue {
    #[serde(rename = "tested_ok")]
    TestedOk,
    #[serde(rename = "untested")]
    Untested,
    #[serde(rename = "not_supported")]
    NotSupported,
}

fn schema_phone_country_destinations_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"additionalProperties": {"enum": ["tested_ok", "untested", "not_supported"], "type": "string"}, "type": "object"}"###).expect("the contract's schema")
}

fn schema_phone_country_registration_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneCountryReason {
    #[serde(rename = "local_presence_required")]
    LocalPresenceRequired,
    #[serde(rename = "compliance_missing")]
    ComplianceMissing,
    #[serde(rename = "not_allowed_by_policy")]
    NotAllowedByPolicy,
    #[serde(rename = "platform_prerequisite")]
    PlatformPrerequisite,
    #[serde(rename = "entity_type")]
    EntityType,
}

fn schema_phone_country_reason_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["local_presence_required", "compliance_missing", "not_allowed_by_policy", "platform_prerequisite", "entity_type", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneCountry {
    pub country: String,
    pub number_type: PhoneNumberType,
    #[schema(schema_with = schema_phone_country_status_property)]
    pub status: PhoneCountryStatus,
    #[schema(schema_with = schema_phone_country_capabilities_property)]
    pub capabilities: Vec<PhoneCountryCapabilitiesItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_country_sms_use_property)]
    pub sms_use: Option<PhoneCountrySmsUse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_country_local_presence_property)]
    pub local_presence: Option<PhoneCountryLocalPresence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub default_for_country: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_country_destinations_property)]
    pub destinations: Option<BTreeMap<String, PhoneCountryDestinationsValue>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_country_registration_property)]
    pub registration: Option<Option<serde_json::Value>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub review_hours: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub emergency: Option<serde_json::Value>,
    pub eligible: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_country_reason_property)]
    pub reason: Option<Option<PhoneCountryReason>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneCountryList {
    pub data: Vec<PhoneCountry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneSetupStepKind {
    #[serde(rename = "platform_prerequisite")]
    PlatformPrerequisite,
    #[serde(rename = "profile")]
    Profile,
    #[serde(rename = "documents")]
    Documents,
    #[serde(rename = "submit_compliance")]
    SubmitCompliance,
    #[serde(rename = "register_us_a2p")]
    RegisterUsA2p,
    #[serde(rename = "verify_toll_free")]
    VerifyTollFree,
    #[serde(rename = "enable_emergency")]
    EnableEmergency,
}

fn schema_phone_setup_step_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["platform_prerequisite", "profile", "documents", "submit_compliance", "register_us_a2p", "verify_toll_free", "enable_emergency"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneSetupStepStatus {
    #[serde(rename = "not_started")]
    NotStarted,
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "waiting_for_person")]
    WaitingForPerson,
    #[serde(rename = "in_review")]
    InReview,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "optional")]
    Optional,
    #[serde(rename = "not_applicable")]
    NotApplicable,
}

fn schema_phone_setup_step_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["not_started", "draft", "waiting_for_person", "in_review", "done", "rejected", "optional", "not_applicable"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneSetupStepWho {
    #[serde(rename = "person")]
    Person,
    #[serde(rename = "operator")]
    Operator,
}

fn schema_phone_setup_step_who_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"enum": ["person", "operator", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSetupStepFee {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub setup_pence: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub monthly_pence: Option<i64>,
}

fn schema_phone_setup_step_fee_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"properties": {"monthly_pence": {"type": "integer"}, "setup_pence": {"type": "integer"}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSetupStep {
    #[schema(schema_with = schema_phone_setup_step_kind_property)]
    pub kind: PhoneSetupStepKind,
    #[schema(schema_with = schema_phone_setup_step_status_property)]
    pub status: PhoneSetupStepStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_setup_step_who_property)]
    pub who: Option<Option<PhoneSetupStepWho>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub missing: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub request_id: Option<Option<String>>,
    /// Provider comments on a rejection; untrusted text.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub comments: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_setup_step_fee_property)]
    pub fee: Option<Option<PhoneSetupStepFee>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<i64>>)]
    pub typical_wait_days: Option<Option<Vec<i64>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneSetupStatus {
    #[serde(rename = "not_started")]
    NotStarted,
    #[serde(rename = "in_review")]
    InReview,
    #[serde(rename = "action_needed")]
    ActionNeeded,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "unavailable")]
    Unavailable,
}

fn schema_phone_setup_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["not_started", "in_review", "action_needed", "ready", "unavailable"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneSetupAfterSetupValue {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "activating")]
    Activating,
    #[serde(rename = "registration_required")]
    RegistrationRequired,
    #[serde(rename = "not_available")]
    NotAvailable,
}

fn schema_phone_setup_after_setup_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"additionalProperties": {"enum": ["enabled", "activating", "registration_required", "not_available"], "type": "string"}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSetup {
    pub country: String,
    #[schema(schema_with = schema_phone_setup_status_property)]
    pub status: PhoneSetupStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub reason: Option<Option<String>>,
    pub steps: Vec<PhoneSetupStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_setup_after_setup_property)]
    pub after_setup: Option<BTreeMap<String, PhoneSetupAfterSetupValue>>,
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
    pub draft_id: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSetupList {
    pub data: Vec<PhoneSetup>,
}

/// The fields of a phone onboarding draft, shared by the request and the stored draft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSetupDraftFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub business_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub use_case: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub sample_messages: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub consent_flow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub website: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub privacy_policy_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub terms_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub opt_in_image_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneSetupDraft {
    #[serde(flatten)]
    pub base: PhoneSetupDraftFields,
    pub id: String,
    pub country: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub drafted_by_key_id: Option<Option<String>>,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneBudgetThisMonth {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub covered: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub resets_at: Option<String>,
}

fn schema_phone_budget_this_month_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"covered": {"type": "integer"}, "resets_at": {"format": "date-time", "type": "string"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneBudget {
    #[serde(flatten)]
    pub base: PhoneBudgetRequest,
    pub version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub set_by: Option<serde_json::Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub set_at: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_budget_this_month_property)]
    pub this_month: Option<PhoneBudgetThisMonth>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberSetupState {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "carrier_pending")]
    CarrierPending,
    #[serde(rename = "configuring_sms")]
    ConfiguringSms,
    #[serde(rename = "configuring_voice")]
    ConfiguringVoice,
    #[serde(rename = "registration_pending")]
    RegistrationPending,
}

fn schema_phone_number_setup_state_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["ready", "carrier_pending", "configuring_sms", "configuring_voice", "registration_pending"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberPrimaryForItem {
    #[serde(rename = "sms")]
    Sms,
    #[serde(rename = "voice")]
    Voice,
}

fn schema_phone_number_primary_for_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"items": {"enum": ["sms", "voice"], "type": "string"}, "type": "array"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberProvisioning {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub status: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub code: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub retryable: Option<bool>,
}

fn schema_phone_number_provisioning_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"code": {"type": ["string", "null"]}, "retryable": {"type": "boolean"}, "status": {"type": "string"}}, "type": "object"}"###).expect("the contract's schema")
}

fn schema_phone_number_action_needed_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberPortPin {
    #[serde(rename = "set")]
    Set,
    #[serde(rename = "missing")]
    Missing,
}

fn schema_phone_number_port_pin_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["set", "missing"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberComplianceStatus {
    #[serde(rename = "own")]
    Own,
    #[serde(rename = "legacy_group")]
    LegacyGroup,
}

fn schema_phone_number_compliance_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["own", "legacy_group"], "type": "string"}"###)
        .expect("the contract's schema")
}

fn schema_phone_number_provider_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "Provider IDs; platform and partner keys only.", "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumber {
    pub id: PhoneNumberId,
    pub identity_id: IdentityId,
    pub phone_number: E164,
    pub country: String,
    pub number_type: PhoneNumberType,
    pub status: PhoneNumberStatus,
    #[schema(schema_with = schema_phone_number_setup_state_property)]
    pub setup_state: PhoneNumberSetupState,
    pub capabilities: PhoneCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_number_primary_for_property)]
    pub primary_for: Option<Vec<PhoneNumberPrimaryForItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_number_provisioning_property)]
    pub provisioning: Option<PhoneNumberProvisioning>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub requirements_status: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub deadline_at: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_number_action_needed_property)]
    pub action_needed: Option<Option<serde_json::Value>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_number_port_pin_property)]
    pub port_pin: Option<PhoneNumberPortPin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_number_compliance_status_property)]
    pub compliance_status: Option<PhoneNumberComplianceStatus>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub legacy_deadline: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub active_from: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub active_until: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub request_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub client_reference: Option<Option<String>>,
    /// Provider IDs; platform and partner keys only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_number_provider_property)]
    pub provider: Option<Option<serde_json::Value>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberPage {
    pub data: Vec<PhoneNumber>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneCandidate {
    pub phone_number: E164,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub locality: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub region: Option<Option<String>>,
    pub features: Vec<String>,
    pub quote: PhoneQuote,
    pub quote_token: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneCandidateList {
    pub data: Vec<PhoneCandidate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhonePortabilityListDataItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = E164)]
    pub phone_number: Option<E164>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub portable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub fast_portable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub messaging_capable: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub reason: Option<Option<String>>,
}

fn schema_phone_portability_list_data_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"fast_portable": {"type": "boolean"}, "messaging_capable": {"type": "boolean"}, "phone_number": {"$ref": "#/components/schemas/E164"}, "portable": {"type": "boolean"}, "reason": {"type": ["string", "null"]}}, "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhonePortabilityList {
    #[schema(schema_with = schema_phone_portability_list_data_property)]
    pub data: Vec<PhonePortabilityListDataItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneNumberLookup {
    pub identity_id: IdentityId,
    pub number_id: PhoneNumberId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneVoiceRouteKind {
    #[serde(rename = "vapi")]
    Vapi,
    #[serde(rename = "sip")]
    Sip,
    #[serde(rename = "forward")]
    Forward,
    #[serde(rename = "none")]
    None,
}

fn schema_phone_voice_route_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["vapi", "sip", "forward", "none"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneVoiceRouteStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "failed")]
    Failed,
}

fn schema_phone_voice_route_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["pending", "ready", "failed"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneVoiceRoute {
    #[schema(schema_with = schema_phone_voice_route_kind_property)]
    pub kind: PhoneVoiceRouteKind,
    /// Without secrets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub config: Option<serde_json::Value>,
    #[schema(schema_with = schema_phone_voice_route_status_property)]
    pub status: PhoneVoiceRouteStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub vapi_phone_number_id: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestCreate {
    pub kind: PhoneRequestKind,
    /// Per kind; for `purchase_number`, `selection` (`mode` `exact` with `phone_number` and `quote_token`, or `any` with `country`, `type` and optional location) and optional `expires_at`.
    #[schema(value_type = Object)]
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub client_reference: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRequestPage {
    pub data: Vec<PhoneRequest>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneComplianceProfileUpdateEntityType {
    #[serde(rename = "limited_company")]
    LimitedCompany,
    #[serde(rename = "llp")]
    Llp,
    #[serde(rename = "partnership")]
    Partnership,
    #[serde(rename = "public_company")]
    PublicCompany,
    #[serde(rename = "non_profit")]
    NonProfit,
    #[serde(rename = "sole_trader")]
    SoleTrader,
}

fn schema_phone_compliance_profile_update_entity_type_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["limited_company", "llp", "partnership", "public_company", "non_profit", "sole_trader"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneComplianceProfileUpdateCompanyRegistry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub country: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub region: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub name: Option<String>,
}

fn schema_phone_compliance_profile_update_company_registry_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"country": {"type": "string"}, "name": {"type": "string"}, "region": {"type": ["string", "null"]}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneComplianceProfileUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub legal_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub trading_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_compliance_profile_update_entity_type_property)]
    pub entity_type: Option<PhoneComplianceProfileUpdateEntityType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub registration_country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub company_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_compliance_profile_update_company_registry_property)]
    pub company_registry: Option<PhoneComplianceProfileUpdateCompanyRegistry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = BTreeMap<String, String>)]
    pub tax_ids: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub website: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub use_case: Option<String>,
    /// Name and role readable; email, phone and birth data write-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub representative: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Object>)]
    pub addresses: Option<Vec<serde_json::Value>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneComplianceProfileStatusItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub number_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub status: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<Option<String>>,
}

fn schema_phone_compliance_profile_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"action": {"type": "string"}, "country": {"type": "string"}, "expires_at": {"format": "date-time", "type": ["string", "null"]}, "number_type": {"type": "string"}, "status": {"type": "string"}}, "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneComplianceProfile {
    #[serde(flatten)]
    pub base: PhoneComplianceProfileUpdate,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_compliance_profile_status_property)]
    pub status: Option<Vec<PhoneComplianceProfileStatusItem>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneDocumentCreateKind {
    #[serde(rename = "company_registration_certificate")]
    CompanyRegistrationCertificate,
    #[serde(rename = "proof_of_address")]
    ProofOfAddress,
    #[serde(rename = "representative_id")]
    RepresentativeId,
    #[serde(rename = "registration_form")]
    RegistrationForm,
    #[serde(rename = "power_of_attorney")]
    PowerOfAttorney,
    #[serde(rename = "letter_of_authorisation")]
    LetterOfAuthorisation,
    #[serde(rename = "invoice")]
    Invoice,
    #[serde(rename = "other")]
    Other,
}

fn schema_phone_document_create_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["company_registration_certificate", "proof_of_address", "representative_id", "registration_form", "power_of_attorney", "letter_of_authorisation", "invoice", "other"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneDocumentCreateContentType {
    #[serde(rename = "application/pdf")]
    ApplicationPdf,
    #[serde(rename = "image/png")]
    ImagePng,
    #[serde(rename = "image/jpeg")]
    ImageJpeg,
}

fn schema_phone_document_create_content_type_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["application/pdf", "image/png", "image/jpeg"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneDocumentCreate {
    #[schema(schema_with = schema_phone_document_create_kind_property)]
    pub kind: PhoneDocumentCreateKind,
    pub filename: String,
    #[schema(schema_with = schema_phone_document_create_content_type_property)]
    pub content_type: PhoneDocumentCreateContentType,
    pub size: i64,
    pub sha256: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub valid_until: Option<Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneDocumentStatus {
    #[serde(rename = "pending_upload")]
    PendingUpload,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "deleted")]
    Deleted,
}

fn schema_phone_document_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(
        r###"{"enum": ["pending_upload", "ready", "deleted"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneDocument {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub size: Option<i64>,
    #[schema(schema_with = schema_phone_document_status_property)]
    pub status: PhoneDocumentStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub valid_until: Option<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneDocumentUpload {
    #[serde(flatten)]
    pub base: PhoneDocument,
    pub upload_url: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneDocumentList {
    pub data: Vec<PhoneDocument>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneExportLink {
    pub url: String,
    pub expires_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneRegistrationsBrandEntity {
    #[serde(rename = "standard")]
    Standard,
    #[serde(rename = "sole_proprietor")]
    SoleProprietor,
}

fn schema_phone_registrations_brand_entity_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["standard", "sole_proprietor", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneRegistrationsOtpStatus {
    #[serde(rename = "not_required")]
    NotRequired,
    #[serde(rename = "sent")]
    Sent,
    #[serde(rename = "verified")]
    Verified,
    #[serde(rename = "expired")]
    Expired,
}

fn schema_phone_registrations_otp_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["not_required", "sent", "verified", "expired", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneRegistrations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub configured: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_registrations_brand_entity_property)]
    pub brand_entity: Option<Option<PhoneRegistrationsBrandEntity>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub brand_status: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub campaign_status: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_registrations_otp_status_property)]
    pub otp_status: Option<Option<PhoneRegistrationsOtpStatus>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<PhoneNumberId>)]
    pub numbers_registered: Option<Vec<PhoneNumberId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub failure_reasons: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneConnectorCreateKind {
    #[serde(rename = "vapi")]
    Vapi,
}

fn schema_phone_connector_create_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["vapi"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneConnectorCreateRegion {
    #[serde(rename = "us")]
    Us,
    #[serde(rename = "eu")]
    Eu,
}

fn schema_phone_connector_create_region_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["us", "eu"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneConnectorCreate {
    #[schema(schema_with = schema_phone_connector_create_kind_property)]
    pub kind: PhoneConnectorCreateKind,
    #[schema(schema_with = schema_phone_connector_create_region_property)]
    pub region: PhoneConnectorCreateRegion,
    pub api_key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneConnectorRotate {
    pub api_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneConnectorStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "deleted")]
    Deleted,
}

fn schema_phone_connector_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["active", "failed", "deleted"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneConnector {
    pub id: String,
    pub kind: String,
    pub region: String,
    #[schema(schema_with = schema_phone_connector_status_property)]
    pub status: PhoneConnectorStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneConnectorList {
    pub data: Vec<PhoneConnector>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhonePortOutDecision2 {
    #[serde(rename = "authorized")]
    Authorized,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "provider_authorized")]
    ProviderAuthorized,
}

fn schema_phone_port_out_decision_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["authorized", "rejected", "provider_authorized", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhonePortOut {
    pub id: String,
    pub number_id: PhoneNumberId,
    pub status: String,
    pub respond_by: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_phone_port_out_decision_property)]
    pub decision: Option<Option<PhonePortOutDecision2>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhonePortOutList {
    pub data: Vec<PhonePortOut>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhonePortOutDecision {
    pub reason_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub reason: Option<String>,
    /// The person in the caller's own application who decided; required from a platform or partner key under `phone.key_approval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhonePortOutAuthorization {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub note: Option<String>,
    /// The person in the caller's own application who decided; required from a platform or partner key under `phone.key_approval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub on_behalf_of: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneChargesChargesItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub pence: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub number_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub created_at: Option<String>,
}

fn schema_phone_charges_charges_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"items": {"properties": {"created_at": {"format": "date-time", "type": "string"}, "id": {"type": "string"}, "kind": {"type": "string"}, "number_id": {"type": ["string", "null"]}, "pence": {"type": "integer"}}, "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneChargesCaps {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub tenant_monthly_cap_pence: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub identity_monthly_cap_pence: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub registration_cap_pence: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub used_pence: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub registration_used_pence: Option<i64>,
}

fn schema_phone_charges_caps_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"identity_monthly_cap_pence": {"type": ["integer", "null"]}, "registration_cap_pence": {"type": ["integer", "null"]}, "registration_used_pence": {"type": "integer"}, "tenant_monthly_cap_pence": {"type": ["integer", "null"]}, "used_pence": {"type": "integer"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneCharges {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub month: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_charges_charges_property)]
    pub charges: Option<Vec<PhoneChargesChargesItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_charges_caps_property)]
    pub caps: Option<PhoneChargesCaps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub holds_pence: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneStatementTenantsItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = BTreeMap<String, i64>)]
    pub categories: Option<BTreeMap<String, i64>>,
}

fn schema_phone_statement_tenants_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"categories": {"additionalProperties": {"type": "integer"}, "type": "object"}, "tenant_id": {"type": "string"}}, "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneStatement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub partner_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub month: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_phone_statement_tenants_property)]
    pub tenants: Option<Vec<PhoneStatementTenantsItem>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneAccountProbeLevel {
    #[serde(rename = "paid")]
    Paid,
    #[serde(rename = "verified")]
    Verified,
    #[serde(rename = "unknown")]
    Unknown,
}

fn schema_phone_account_probe_level_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["paid", "verified", "unknown"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PhoneAccountProbe {
    #[schema(schema_with = schema_phone_account_probe_level_property)]
    pub level: PhoneAccountProbeLevel,
    pub probed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub us_enabled: Option<bool>,
}
