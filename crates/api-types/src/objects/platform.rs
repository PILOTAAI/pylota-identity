//! Objects first used by the `platform` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Dead-letter item ID (`dlq_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DlqItemId(pub String);

/// Job ID (`job_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct JobId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupListItemHumanStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "delivered")]
    Delivered,
    #[serde(rename = "bounced")]
    Bounced,
}

fn schema_agent_signup_list_item_human_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["pending", "delivered", "bounced"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AgentSignupListItemVerifiedVia {
    #[serde(rename = "code")]
    Code,
    #[serde(rename = "link")]
    Link,
    #[serde(rename = "claim")]
    Claim,
}

fn schema_agent_signup_list_item_verified_via_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["code", "link", "claim", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignupListItem {
    pub signup_id: AgentSignupId,
    pub tenant_id: TenantId,
    pub state: AgentSignupState,
    pub address: EmailAddress,
    #[schema(required = true)]
    pub human_email_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_agent_signup_list_item_human_status_property)]
    pub human_status: Option<AgentSignupListItemHumanStatus>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub source: Option<Option<String>>,
    /// A keyed hash of the client network.
    pub network: String,
    pub created_at: String,
    pub expires_at: String,
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
    #[schema(schema_with = schema_agent_signup_list_item_verified_via_property)]
    pub verified_via: Option<Option<AgentSignupListItemVerifiedVia>>,
}

/// What a deployment signing key signs: `thread` (thread tokens), `link` (download links, console sign-in, invitation and session tokens, OAuth state hashes), `cursor` (search cursors) or `web_bot_auth` (Web Bot Auth HTTP signatures and the key directory).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SigningKeyPurpose {
    #[serde(rename = "thread")]
    Thread,
    #[serde(rename = "link")]
    Link,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "web_bot_auth")]
    WebBotAuth,
}

/// The key it replaced, and until when it keeps verifying (90 days for `thread`, 7 days for `link` and `web_bot_auth`, 24 hours for `cursor`; the rotation time with `revoke_previous=true`). A previous `web_bot_auth` key stays in the key directory until then. `null` when the purpose had no key yet: the rotation then creates the first one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SigningKeyRotationPrevious {
    pub kid: String,
    pub verify_until: String,
    /// `true` when the request set `revoke_previous=true` and the previous key was deleted.
    pub revoked: bool,
}

fn schema_signing_key_rotation_previous_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "The key it replaced, and until when it keeps verifying (90 days for `thread`, 7 days for `link` and `web_bot_auth`, 24 hours for `cursor`; the rotation time with `revoke_previous=true`). A previous `web_bot_auth` key stays in the key directory until then. `null` when the purpose had no key yet: the rotation then creates the first one.", "properties": {"kid": {"examples": ["3"], "type": "string"}, "revoked": {"description": "`true` when the request set `revoke_previous=true` and the previous key was deleted.", "type": "boolean"}, "verify_until": {"format": "date-time", "type": "string"}}, "required": ["kid", "verify_until", "revoked"], "type": ["object", "null"]}"###).expect("the contract's schema")
}

/// The result of a signing-key rotation. Key material is never returned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SigningKeyRotation {
    pub purpose: SigningKeyPurpose,
    /// The new current key's ID: one character for `thread`, `link` and `cursor`; the 43-character JWK thumbprint (base64url) for `web_bot_auth`.
    pub kid: String,
    pub created_at: String,
    /// The key it replaced, and until when it keeps verifying (90 days for `thread`, 7 days for `link` and `web_bot_auth`, 24 hours for `cursor`; the rotation time with `revoke_previous=true`). A previous `web_bot_auth` key stays in the key directory until then. `null` when the purpose had no key yet: the rotation then creates the first one.
    #[schema(required = true, schema_with = schema_signing_key_rotation_previous_property)]
    pub previous: Option<SigningKeyRotationPrevious>,
}

/// The source queue of a dead-letter item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DlqQueue {
    #[serde(rename = "pm-inbound")]
    PmInbound,
    #[serde(rename = "pm-outbound")]
    PmOutbound,
    #[serde(rename = "pm-delivery-events")]
    PmDeliveryEvents,
    #[serde(rename = "pm-webhooks")]
    PmWebhooks,
    #[serde(rename = "pm-index")]
    PmIndex,
    #[serde(rename = "pm-screen")]
    PmScreen,
}

/// A dead-lettered queue message. The stored body is not returned: it is a pointer, and inbound pointers carry envelope addresses. Kept for 14 days.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DlqItem {
    pub id: DlqItemId,
    pub queue: DlqQueue,
    /// The body's `kind`, if any.
    #[schema(required = true)]
    pub kind: Option<String>,
    /// The tenant the body names, if any.
    #[schema(required = true)]
    pub tenant_id: Option<TenantId>,
    pub first_seen_at: String,
    #[schema(required = true)]
    pub redriven_at: Option<String>,
    pub redrive_count: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DlqItemPage {
    pub data: Vec<DlqItem>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

/// `reparse` (re-parse messages from raw MIME and re-emit their events with `reprocessed: true`), `reembed` (re-chunk and re-embed into Vectorize), `reindex` (rebuild the keyword index) or `restore_reconcile` (after a mailbox restore) or `event_log_rebuild` (rebuild a tenant's event log in a new epoch) can be started with `createJob`. `getJob` also returns the jobs the system runs itself: `retention`, `backup` and `domain_remove`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum JobKind {
    #[serde(rename = "reparse")]
    Reparse,
    #[serde(rename = "reembed")]
    Reembed,
    #[serde(rename = "reindex")]
    Reindex,
    #[serde(rename = "restore_reconcile")]
    RestoreReconcile,
    #[serde(rename = "event_log_rebuild")]
    EventLogRebuild,
    #[serde(rename = "retention")]
    Retention,
    #[serde(rename = "backup")]
    Backup,
    #[serde(rename = "domain_remove")]
    DomainRemove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum JobStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "canceled")]
    Canceled,
}

fn schema_job_result_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"additionalProperties": {"type": ["integer", "string"]}, "description": "Counts, once the job ends (for `reparse`, including the messages skipped past `raw_days`); for a `failed` job also `failed_step` and `error` (strings).", "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Job {
    pub id: JobId,
    pub kind: JobKind,
    /// The tenant; `null` for a deployment-wide job (`backup`, the global `retention` job).
    #[schema(required = true)]
    pub tenant_id: Option<TenantId>,
    pub status: JobStatus,
    pub created_at: String,
    #[schema(required = true)]
    pub completed_at: Option<String>,
    /// Counts, once the job ends (for `reparse`, including the messages skipped past `raw_days`); for a `failed` job also `failed_step` and `error` (strings).
    #[schema(required = true, schema_with = schema_job_result_property)]
    pub result: Option<BTreeMap<String, serde_json::Value>>,
    /// The API key that started the job, or `null` for a job the system started itself (for example the `reembed` job that the cron creates during an embedding-model change).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub created_by_key_id: Option<Option<String>>,
}

/// `agent_signup` (default `on`) stops new agent sign-ups and their mail when `off`; the containment rule `agent_signup_off` turns it off automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SwitchName {
    #[serde(rename = "read_only")]
    ReadOnly,
    #[serde(rename = "free_sending")]
    FreeSending,
    #[serde(rename = "emergency_prune")]
    EmergencyPrune,
    #[serde(rename = "agent_signup")]
    AgentSignup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SwitchUpdateState {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
}

fn schema_switch_update_state_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["on", "off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SwitchUpdate {
    pub name: SwitchName,
    #[schema(schema_with = schema_switch_update_state_property)]
    pub state: SwitchUpdateState,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SwitchState {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
}

fn schema_switch_state_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["on", "off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SwitchSetBy {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "platform")]
    Platform,
}

fn schema_switch_set_by_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`auto` when an automatic containment rule set it, `platform` when a platform key did.", "enum": ["auto", "platform"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Switch {
    pub name: SwitchName,
    #[schema(schema_with = schema_switch_state_property)]
    pub state: SwitchState,
    /// `auto` when an automatic containment rule set it, `platform` when a platform key did.
    #[schema(schema_with = schema_switch_set_by_property)]
    pub set_by: SwitchSetBy,
    pub reason: String,
    pub set_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusSwitchesReadOnly {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
}

fn schema_platform_status_switches_read_only_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["on", "off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusSwitchesFreeSending {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
}

fn schema_platform_status_switches_free_sending_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["on", "off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusSwitchesEmergencyPrune {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
}

fn schema_platform_status_switches_emergency_prune_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["on", "off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusSwitchesAgentSignup {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
}

fn schema_platform_status_switches_agent_signup_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["on", "off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlatformStatusSwitches {
    #[schema(schema_with = schema_platform_status_switches_read_only_property)]
    pub read_only: PlatformStatusSwitchesReadOnly,
    #[schema(schema_with = schema_platform_status_switches_free_sending_property)]
    pub free_sending: PlatformStatusSwitchesFreeSending,
    #[schema(schema_with = schema_platform_status_switches_emergency_prune_property)]
    pub emergency_prune: PlatformStatusSwitchesEmergencyPrune,
    #[schema(schema_with = schema_platform_status_switches_agent_signup_property)]
    pub agent_signup: PlatformStatusSwitchesAgentSignup,
}

fn schema_platform_status_switches_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"agent_signup": {"enum": ["on", "off"], "type": "string"}, "emergency_prune": {"enum": ["on", "off"], "type": "string"}, "free_sending": {"enum": ["on", "off"], "type": "string"}, "read_only": {"enum": ["on", "off"], "type": "string"}}, "required": ["read_only", "free_sending", "emergency_prune", "agent_signup"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusAlertEmail {
    #[serde(rename = "configured")]
    Configured,
    #[serde(rename = "missing")]
    Missing,
}

fn schema_platform_status_alert_email_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`missing` when neither `PM_ALERT_EMAIL` nor a `mailto:` `PM_SECURITY_CONTACT` is set.", "enum": ["configured", "missing"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusFiringItemSeverity {
    #[serde(rename = "page")]
    Page,
    #[serde(rename = "ticket")]
    Ticket,
}

fn schema_platform_status_firing_item_severity_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["page", "ticket"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlatformStatusFiringItem {
    /// The alert key, for example `dlq:pm-inbound`.
    pub alert: String,
    #[schema(schema_with = schema_platform_status_firing_item_severity_property)]
    pub severity: PlatformStatusFiringItemSeverity,
    pub fired_at: String,
}

fn schema_platform_status_firing_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"alert": {"description": "The alert key, for example `dlq:pm-inbound`.", "type": "string"}, "fired_at": {"format": "date-time", "type": "string"}, "severity": {"enum": ["page", "ticket"], "type": "string"}}, "required": ["alert", "severity", "fired_at"], "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusMasterKeyActiveSlot {
    #[serde(rename = "a")]
    A,
    #[serde(rename = "b")]
    B,
}

fn schema_platform_status_master_key_active_slot_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["a", "b"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusMasterKeySlotsItemSlot {
    #[serde(rename = "a")]
    A,
    #[serde(rename = "b")]
    B,
}

fn schema_platform_status_master_key_slots_item_slot_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["a", "b"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlatformStatusMasterKeySlotsItem {
    #[schema(schema_with = schema_platform_status_master_key_slots_item_slot_property)]
    pub slot: PlatformStatusMasterKeySlotsItemSlot,
    pub present: bool,
    /// First 8 bytes of SHA-256 of the key, lower-case hex.
    #[schema(required = true)]
    pub kid: Option<String>,
}

fn schema_platform_status_master_key_slots_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"kid": {"description": "First 8 bytes of SHA-256 of the key, lower-case hex.", "type": ["string", "null"]}, "present": {"type": "boolean"}, "slot": {"enum": ["a", "b"], "type": "string"}}, "required": ["slot", "present", "kid"], "type": "object"}, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlatformStatusMasterKey {
    #[schema(schema_with = schema_platform_status_master_key_active_slot_property)]
    pub active_slot: PlatformStatusMasterKeyActiveSlot,
    #[schema(schema_with = schema_platform_status_master_key_slots_property)]
    pub slots: Vec<PlatformStatusMasterKeySlotsItem>,
    /// Sealed values whose key ID is not the active key's; 0 once a rotation's re-seal is done.
    pub remaining: i64,
    /// When the Worker first saw the active key.
    #[schema(required = true)]
    pub activated_at: Option<String>,
    /// When `remaining` first reached 0 under the active key; a next rotation is allowed without `--discard-previous` 30 days later.
    #[schema(required = true)]
    pub resealed_at: Option<String>,
}

fn schema_platform_status_master_key_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"activated_at": {"description": "When the Worker first saw the active key.", "format": "date-time", "type": ["string", "null"]}, "active_slot": {"enum": ["a", "b"], "type": "string"}, "remaining": {"description": "Sealed values whose key ID is not the active key's; 0 once a rotation's re-seal is done.", "minimum": 0, "type": "integer"}, "resealed_at": {"description": "When `remaining` first reached 0 under the active key; a next rotation is allowed without `--discard-previous` 30 days later.", "format": "date-time", "type": ["string", "null"]}, "slots": {"items": {"properties": {"kid": {"description": "First 8 bytes of SHA-256 of the key, lower-case hex.", "type": ["string", "null"]}, "present": {"type": "boolean"}, "slot": {"enum": ["a", "b"], "type": "string"}}, "required": ["slot", "present", "kid"], "type": "object"}, "type": "array"}}, "required": ["active_slot", "slots", "remaining", "activated_at", "resealed_at"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlatformStatusCapacity {
    #[schema(required = true)]
    pub d1_bytes: Option<i64>,
    #[schema(required = true)]
    pub d1_pct: Option<f64>,
    #[schema(required = true)]
    pub vectors: Option<i64>,
    #[schema(required = true)]
    pub vectors_pct: Option<f64>,
    /// Tenants that are not erased (one Vectorize namespace each).
    pub tenants: i64,
    pub namespaces_pct: f64,
    #[schema(required = true)]
    pub measured_at: Option<String>,
}

fn schema_platform_status_capacity_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"d1_bytes": {"minimum": 0, "type": ["integer", "null"]}, "d1_pct": {"type": ["number", "null"]}, "measured_at": {"format": "date-time", "type": ["string", "null"]}, "namespaces_pct": {"type": "number"}, "tenants": {"description": "Tenants that are not erased (one Vectorize namespace each).", "minimum": 0, "type": "integer"}, "vectors": {"minimum": 0, "type": ["integer", "null"]}, "vectors_pct": {"type": ["number", "null"]}}, "required": ["d1_bytes", "d1_pct", "vectors", "vectors_pct", "tenants", "namespaces_pct", "measured_at"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusAgentSignupReason {
    #[serde(rename = "signup_not_open")]
    SignupNotOpen,
    #[serde(rename = "billing_off")]
    BillingOff,
    #[serde(rename = "console_off")]
    ConsoleOff,
    #[serde(rename = "switch_off")]
    SwitchOff,
}

fn schema_platform_status_agent_signup_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["signup_not_open", "billing_off", "console_off", "switch_off", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PlatformStatusAgentSignupSwitch {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
}

fn schema_platform_status_agent_signup_switch_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["on", "off"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlatformStatusAgentSignupOutcomes24h {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub outcomes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub bounced: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub complained: Option<i64>,
}

fn schema_platform_status_agent_signup_outcomes24h_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"bounced": {"minimum": 0, "type": "integer"}, "complained": {"minimum": 0, "type": "integer"}, "outcomes": {"minimum": 0, "type": "integer"}}, "type": "object"}"###).expect("the contract's schema")
}

/// Agent self-sign-up: whether `PM_AGENT_SIGNUP` is `on` (`configured`), whether sign-ups are accepted now (`active`), why not, the switch, today's (UTC) sign-ups against `PM_AGENT_SIGNUP_DAILY_CAP`, and the last 24 hours' outcomes of sign-up mail.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlatformStatusAgentSignup {
    pub configured: bool,
    pub active: bool,
    #[schema(required = true, schema_with = schema_platform_status_agent_signup_reason_property)]
    pub reason: Option<PlatformStatusAgentSignupReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_platform_status_agent_signup_switch_property)]
    pub switch: Option<PlatformStatusAgentSignupSwitch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub signups_today: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub daily_cap: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_platform_status_agent_signup_outcomes24h_property)]
    pub outcomes_24h: Option<PlatformStatusAgentSignupOutcomes24h>,
}

fn schema_platform_status_agent_signup_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Agent self-sign-up: whether `PM_AGENT_SIGNUP` is `on` (`configured`), whether sign-ups are accepted now (`active`), why not, the switch, today's (UTC) sign-ups against `PM_AGENT_SIGNUP_DAILY_CAP`, and the last 24 hours' outcomes of sign-up mail.", "properties": {"active": {"type": "boolean"}, "configured": {"type": "boolean"}, "daily_cap": {"maximum": 10000, "minimum": 1, "type": "integer"}, "outcomes_24h": {"properties": {"bounced": {"minimum": 0, "type": "integer"}, "complained": {"minimum": 0, "type": "integer"}, "outcomes": {"minimum": 0, "type": "integer"}}, "type": "object"}, "reason": {"enum": ["signup_not_open", "billing_off", "console_off", "switch_off", null], "type": ["string", "null"]}, "signups_today": {"minimum": 0, "type": "integer"}, "switch": {"enum": ["on", "off"], "type": "string"}}, "required": ["configured", "active", "reason"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlatformStatus {
    /// `true` while `PM_FREEZE = "on"` (a restore is in progress).
    pub frozen: bool,
    #[schema(required = true)]
    pub frozen_since: Option<String>,
    #[schema(schema_with = schema_platform_status_switches_property)]
    pub switches: PlatformStatusSwitches,
    /// `missing` when neither `PM_ALERT_EMAIL` nor a `mailto:` `PM_SECURITY_CONTACT` is set.
    #[schema(schema_with = schema_platform_status_alert_email_property)]
    pub alert_email: PlatformStatusAlertEmail,
    #[schema(schema_with = schema_platform_status_firing_property)]
    pub firing: Vec<PlatformStatusFiringItem>,
    #[schema(schema_with = schema_platform_status_master_key_property)]
    pub master_key: PlatformStatusMasterKey,
    #[schema(schema_with = schema_platform_status_capacity_property)]
    pub capacity: PlatformStatusCapacity,
    /// Agent self-sign-up: whether `PM_AGENT_SIGNUP` is `on` (`configured`), whether sign-ups are accepted now (`active`), why not, the switch, today's (UTC) sign-ups against `PM_AGENT_SIGNUP_DAILY_CAP`, and the last 24 hours' outcomes of sign-up mail.
    #[schema(schema_with = schema_platform_status_agent_signup_property)]
    pub agent_signup: PlatformStatusAgentSignup,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MailboxRestoreResult {
    pub identity_id: IdentityId,
    /// The `at` asked for, or `null` when a bookmark was restored.
    #[schema(required = true)]
    pub restored_to: Option<String>,
    /// Restores the state this call replaced.
    pub undo_bookmark: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WaitlistInviteResult {
    /// Entries invited by this request.
    pub invited: i64,
    /// Confirmed entries still not invited.
    pub waiting: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentSignupPage {
    pub data: Vec<AgentSignupListItem>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
