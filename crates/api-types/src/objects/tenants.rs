//! Objects first used by the `tenants` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// An RFC 5321 address. Stored lower-cased with the domain as an IDNA A-label; dots in the local part are significant ([A1]). SMTPUTF8 (non-ASCII) local parts are refused with `address_unsupported`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EmailAddress(pub String);

/// A label. At most 64 per message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Label(pub String);

/// `erasing` and `erased` are set by a tenant-scope erasure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "erasing")]
    Erasing,
    #[serde(rename = "erased")]
    Erased,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantSlug(pub String);

/// Appended to usernames on the platform domain (`bookings` + `.acme` → `bookings.acme@agents.example`). Defaults to `"." + slug`. Only the default tenant made by `idvy setup` can have an empty suffix. A suffix that folds to a reserved name gets `400 address_reserved`, and one that folds to another tenant's suffix gets `409 suffix_taken` ([D12]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AddressSuffix(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantSuspendedBy {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "partner")]
    Partner,
}

fn schema_tenant_suspended_by_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "Who suspended the tenant while `status` is `suspended`: `platform` (a platform key; a partner key cannot lift it) or `partner` (the tenant's partner key). `null` otherwise.", "enum": ["platform", "partner", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Tenant {
    pub id: TenantId,
    pub slug: TenantSlug,
    pub name: String,
    pub mode: Mode,
    pub status: TenantStatus,
    /// Who suspended the tenant while `status` is `suspended`: `platform` (a platform key; a partner key cannot lift it) or `partner` (the tenant's partner key). `null` otherwise.
    #[schema(required = true, schema_with = schema_tenant_suspended_by_property)]
    pub suspended_by: Option<TenantSuspendedBy>,
    /// The partner whose key created the tenant, or `null` (created by a platform key, by `idvy setup` or by self-serve sign-up). Set at creation and never changed, also after the tenant is erased and the partner deleted.
    #[schema(required = true)]
    pub partner_id: Option<PartnerId>,
    pub address_suffix: AddressSuffix,
    /// IANA time zone. Daily caps and date filters resolve in it.
    pub timezone: String,
    pub policy: TenantPolicy,
    /// When the tenant's sending was paused automatically because its complaint or bounce rate reached the provider's review level over 7 days (FR-DLV-6, [G12]) or one of its domains had a burst far above it ([J33]); `null` while it may send. Every send then gets `409 sending_paused`, whose `details.reason` (`abuse_threshold` or `abuse_burst`) says which. Only a platform key can resume it (`sending_paused: false`).
    #[schema(required = true)]
    pub sending_paused_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyLargeAttachments {
    #[serde(rename = "refuse")]
    Refuse,
    #[serde(rename = "link")]
    Link,
}

fn schema_tenant_policy_large_attachments_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "refuse", "description": "`refuse` returns `413 message_too_large`; `link` turns oversized attachments into expiring signed links.", "enum": ["refuse", "link"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyAiDisclosureMode {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "footer")]
    Footer,
    #[serde(rename = "header")]
    Header,
}

fn schema_tenant_policy_ai_disclosure_mode_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "none", "description": "`footer` appends `text` to text and HTML bodies; `header` adds `X-AI-Generated: true`.", "enum": ["none", "footer", "header"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyAiDisclosure {
    /// `footer` appends `text` to text and HTML bodies; `header` adds `X-AI-Generated: true`.
    #[schema(schema_with = schema_tenant_policy_ai_disclosure_mode_property)]
    pub mode: TenantPolicyAiDisclosureMode,
    pub text: String,
}

fn schema_tenant_policy_ai_disclosure_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": {"mode": "none", "text": "This message was written with the help of an AI assistant."}, "properties": {"mode": {"default": "none", "description": "`footer` appends `text` to text and HTML bodies; `header` adds `X-AI-Generated: true`.", "enum": ["none", "footer", "header"], "type": "string"}, "text": {"default": "This message was written with the help of an AI assistant.", "type": "string"}}, "required": ["mode", "text"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyAutoReply {
    pub allowed: bool,
    /// Automatic replies allowed per thread before a human must act ([D6]).
    pub max_automatic_exchanges: i64,
}

fn schema_tenant_policy_auto_reply_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": {"allowed": true, "max_automatic_exchanges": 2}, "properties": {"allowed": {"default": true, "type": "boolean"}, "max_automatic_exchanges": {"default": 2, "description": "Automatic replies allowed per thread before a human must act ([D6]).", "minimum": 0, "type": "integer"}}, "required": ["allowed", "max_automatic_exchanges"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyQuarantine {
    /// Quarantine mail that fails authentication.
    pub on_auth_fail: bool,
    /// Quarantine mail whose spam score is at or above this value.
    pub spam_threshold: f64,
    /// Quarantine password-reset and OTP mail that no `wait` asked for ([E5]).
    pub unsolicited_otp: bool,
    /// Lets keys with `quarantine:review` that reach this tenant (its partner key included) release quarantined mail even when `PM_QUARANTINE_KEY_RELEASE` is `off`. Only a platform key, or the partner key of the tenant's own partner, can set it, at creation or later ([J14], [J16]).
    pub key_release: bool,
}

fn schema_tenant_policy_quarantine_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": {"key_release": false, "on_auth_fail": true, "spam_threshold": 0.8, "unsolicited_otp": true}, "properties": {"key_release": {"default": false, "description": "Lets keys with `quarantine:review` that reach this tenant (its partner key included) release quarantined mail even when `PM_QUARANTINE_KEY_RELEASE` is `off`. Only a platform key, or the partner key of the tenant's own partner, can set it, at creation or later ([J14], [J16]).", "type": "boolean"}, "on_auth_fail": {"default": true, "description": "Quarantine mail that fails authentication.", "type": "boolean"}, "spam_threshold": {"default": 0.8, "description": "Quarantine mail whose spam score is at or above this value.", "maximum": 1, "minimum": 0, "type": "number"}, "unsolicited_otp": {"default": true, "description": "Quarantine password-reset and OTP mail that no `wait` asked for ([E5]).", "type": "boolean"}}, "required": ["on_auth_fail", "spam_threshold", "unsolicited_otp", "key_release"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyInboundExtractAttachmentTextItem {
    #[serde(rename = "pdf")]
    Pdf,
    #[serde(rename = "office")]
    Office,
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "html")]
    Html,
}

fn schema_tenant_policy_inbound_extract_attachment_text_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": ["pdf", "office", "text", "html"], "description": "Attachment families whose text is extracted.", "items": {"enum": ["pdf", "office", "text", "html"], "type": "string"}, "type": "array", "uniqueItems": true}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyInbound {
    /// Inbound messages per sender per identity per hour; the excess is stored `throttled` ([D5]). A sender whose mail does not pass authentication is counted under `unauth:` + its address, so a forged `From` never uses a real correspondent's allowance.
    pub per_sender_per_hour: i64,
    /// Inbound messages per identity per hour, across all senders whose mail does not pass authentication and who are not known correspondents; the excess is stored `throttled` ([D13]).
    pub unauthenticated_per_hour: i64,
    /// Inbound messages per tenant per hour, across its identities, from senders who are not known correspondents; the excess is stored `throttled` ([D13]).
    pub per_tenant_per_hour: i64,
    /// Attachment families whose text is extracted.
    #[schema(schema_with = schema_tenant_policy_inbound_extract_attachment_text_property)]
    pub extract_attachment_text: Vec<TenantPolicyInboundExtractAttachmentTextItem>,
    /// Extract text from images.
    pub extract_image_text: bool,
    /// On SES-receiving domains, `true` bounces mail to retired addresses with `550 5.1.6` through SES receipt rules; `false` drops it without a bounce.
    pub ses_bounce_retired: bool,
    /// ARC sealers, by the `d=` of their `ARC-Seal` (A-label registrable domains), whose verified seal can rescue mail that fails DMARC (verdict `arc_rescued`). A public suffix is refused. `[]` turns the rescue off. Platform-only.
    pub trusted_arc_sealers: Vec<String>,
}

fn schema_tenant_policy_inbound_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"extract_attachment_text": ["pdf", "office", "text", "html"], "extract_image_text": false, "per_sender_per_hour": 60, "per_tenant_per_hour": 2000, "ses_bounce_retired": true, "trusted_arc_sealers": ["google.com", "microsoft.com"], "unauthenticated_per_hour": 120}, "properties": {"extract_attachment_text": {"default": ["pdf", "office", "text", "html"], "description": "Attachment families whose text is extracted.", "items": {"enum": ["pdf", "office", "text", "html"], "type": "string"}, "type": "array", "uniqueItems": true}, "extract_image_text": {"default": false, "description": "Extract text from images.", "type": "boolean"}, "per_sender_per_hour": {"default": 60, "description": "Inbound messages per sender per identity per hour; the excess is stored `throttled` ([D5]). A sender whose mail does not pass authentication is counted under `unauth:` + its address, so a forged `From` never uses a real correspondent's allowance.", "minimum": 1, "type": "integer"}, "per_tenant_per_hour": {"default": 2000, "description": "Inbound messages per tenant per hour, across its identities, from senders who are not known correspondents; the excess is stored `throttled` ([D13]).", "minimum": 1, "type": "integer"}, "ses_bounce_retired": {"default": true, "description": "On SES-receiving domains, `true` bounces mail to retired addresses with `550 5.1.6` through SES receipt rules; `false` drops it without a bounce.", "type": "boolean"}, "trusted_arc_sealers": {"default": ["google.com", "microsoft.com"], "description": "ARC sealers, by the `d=` of their `ARC-Seal` (A-label registrable domains), whose verified seal can rescue mail that fails DMARC (verdict `arc_rescued`). A public suffix is refused. `[]` turns the rescue off. Platform-only.", "items": {"pattern": "^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$", "type": "string"}, "maxItems": 20, "type": "array", "uniqueItems": true}, "unauthenticated_per_hour": {"default": 120, "description": "Inbound messages per identity per hour, across all senders whose mail does not pass authentication and who are not known correspondents; the excess is stored `throttled` ([D13]).", "minimum": 1, "type": "integer"}}, "required": ["per_sender_per_hour", "unauthenticated_per_hour", "per_tenant_per_hour", "extract_attachment_text", "extract_image_text", "ses_bounce_retired", "trusted_arc_sealers"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyRetention {
    /// Days raw MIME is kept. Then `410 raw_expired`.
    pub raw_days: i64,
    /// `null` keeps parsed messages indefinitely. A number deletes messages, attachments, index rows and vectors after that age, except held threads.
    #[schema(required = true)]
    pub message_days: Option<i64>,
    /// Days events, delivery logs and replayable event payloads are kept. Webhook replay reaches back 30 days from an event's `occurred_at`, or this many days if fewer.
    pub events_days: i64,
}

fn schema_tenant_policy_retention_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": {"events_days": 30, "message_days": null, "raw_days": 90}, "properties": {"events_days": {"default": 30, "description": "Days events, delivery logs and replayable event payloads are kept. Webhook replay reaches back 30 days from an event's `occurred_at`, or this many days if fewer.", "maximum": 365, "minimum": 1, "type": "integer"}, "message_days": {"default": null, "description": "`null` keeps parsed messages indefinitely. A number deletes messages, attachments, index rows and vectors after that age, except held threads.", "minimum": 1, "type": ["integer", "null"]}, "raw_days": {"default": 90, "description": "Days raw MIME is kept. Then `410 raw_expired`.", "minimum": 1, "type": "integer"}}, "required": ["raw_days", "message_days", "events_days"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyTriage {
    pub enabled: bool,
    /// Model-backed triage runs per day in the tenant's time zone. Past it, triage is rules-only (`model: "rules"`) until the next local midnight. It applies whatever the billing mode, so an `exempt` workspace still has a ceiling on model spend ([D13]). Screening counts its own calls against the same number on a separate counter, at most a tenth of it per authenticated sender address and four fifths for senders that are not known correspondents; a screening verdict past it is `unavailable` with reason `model_cap` and always held.
    pub daily_model_cap: i64,
    /// `null` uses the built-in categories. Otherwise up to 20 custom categories that replace them.
    #[schema(required = true)]
    pub categories: Option<Vec<TriageCategoryDefinition>>,
    /// Deterministic rules that run before the model (FR-TRI-2), at most 50.
    pub rules: Vec<TriageRule>,
}

fn schema_tenant_policy_triage_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"categories": null, "daily_model_cap": 2000, "enabled": true, "rules": []}, "properties": {"categories": {"default": null, "description": "`null` uses the built-in categories. Otherwise up to 20 custom categories that replace them.", "items": {"$ref": "#/components/schemas/TriageCategoryDefinition"}, "maxItems": 20, "type": ["array", "null"]}, "daily_model_cap": {"default": 2000, "description": "Model-backed triage runs per day in the tenant's time zone. Past it, triage is rules-only (`model: \"rules\"`) until the next local midnight. It applies whatever the billing mode, so an `exempt` workspace still has a ceiling on model spend ([D13]). Screening counts its own calls against the same number on a separate counter, at most a tenth of it per authenticated sender address and four fifths for senders that are not known correspondents; a screening verdict past it is `unavailable` with reason `model_cap` and always held.", "minimum": 0, "type": "integer"}, "enabled": {"default": true, "type": "boolean"}, "rules": {"default": [], "description": "Deterministic rules that run before the model (FR-TRI-2), at most 50.", "items": {"$ref": "#/components/schemas/TriageRule"}, "maxItems": 50, "type": "array"}}, "required": ["enabled", "daily_model_cap", "categories", "rules"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicySearchRefsPacksItem {
    #[serde(rename = "core")]
    Core,
    #[serde(rename = "uk_vehicle")]
    UkVehicle,
}

fn schema_tenant_policy_search_refs_packs_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": ["core"], "description": "`core`: amounts, phones, emails, domains, dates, invoice and order numbers. `uk_vehicle`: plates and PCNs.", "items": {"enum": ["core", "uk_vehicle"], "type": "string"}, "type": "array", "uniqueItems": true}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicySearch {
    pub agentic_enabled: bool,
    /// Agentic searches per tenant per day. Then `429 agentic_budget_exhausted`.
    pub agentic_daily_cap: i64,
    /// The default, and the maximum, `budget.max_steps`.
    pub agentic_max_steps: i64,
    /// The default, and the maximum, `budget.max_seconds`.
    pub agentic_max_seconds: i64,
    /// `core`: amounts, phones, emails, domains, dates, invoice and order numbers. `uk_vehicle`: plates and PCNs.
    #[schema(schema_with = schema_tenant_policy_search_refs_packs_property)]
    pub refs_packs: Vec<TenantPolicySearchRefsPacksItem>,
    pub custom_refs: Vec<CustomRefPattern>,
}

fn schema_tenant_policy_search_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"agentic_daily_cap": 500, "agentic_enabled": true, "agentic_max_seconds": 8, "agentic_max_steps": 6, "custom_refs": [], "refs_packs": ["core"]}, "properties": {"agentic_daily_cap": {"default": 500, "description": "Agentic searches per tenant per day. Then `429 agentic_budget_exhausted`.", "minimum": 0, "type": "integer"}, "agentic_enabled": {"default": true, "type": "boolean"}, "agentic_max_seconds": {"default": 8, "description": "The default, and the maximum, `budget.max_seconds`.", "maximum": 30, "minimum": 3, "type": "integer"}, "agentic_max_steps": {"default": 6, "description": "The default, and the maximum, `budget.max_steps`.", "maximum": 10, "minimum": 2, "type": "integer"}, "custom_refs": {"default": [], "items": {"$ref": "#/components/schemas/CustomRefPattern"}, "maxItems": 20, "type": "array"}, "refs_packs": {"default": ["core"], "description": "`core`: amounts, phones, emails, domains, dates, invoice and order numbers. `uk_vehicle`: plates and PCNs.", "items": {"enum": ["core", "uk_vehicle"], "type": "string"}, "type": "array", "uniqueItems": true}}, "required": ["agentic_enabled", "agentic_daily_cap", "agentic_max_steps", "agentic_max_seconds", "refs_packs", "custom_refs"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyAbuse {
    /// Pause an identity whose complaint rate over its last 1,000 sends exceeds this (FR-DLV-3).
    pub complaint_rate_pause: f64,
    /// Pause an identity whose bounce rate over its last 200 sends exceeds this (FR-DLV-3).
    pub bounce_rate_pause: f64,
    /// Pause the tenant's sending, or one of its sending domains, when its complaint rate over the last 7 UTC days, with at least 500 outcomes, reaches this (FR-DLV-6, [G12]). The default is Amazon SES's review rate.
    pub tenant_complaint_rate_pause: f64,
    /// As `tenant_complaint_rate_pause`, for the bounce rate (FR-DLV-6, [G12]).
    pub tenant_bounce_rate_pause: f64,
}

fn schema_tenant_policy_abuse_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"bounce_rate_pause": 0.05, "complaint_rate_pause": 0.003, "tenant_bounce_rate_pause": 0.05, "tenant_complaint_rate_pause": 0.001}, "properties": {"bounce_rate_pause": {"default": 0.05, "description": "Pause an identity whose bounce rate over its last 200 sends exceeds this (FR-DLV-3).", "maximum": 1, "minimum": 0, "type": "number"}, "complaint_rate_pause": {"default": 0.003, "description": "Pause an identity whose complaint rate over its last 1,000 sends exceeds this (FR-DLV-3).", "maximum": 1, "minimum": 0, "type": "number"}, "tenant_bounce_rate_pause": {"default": 0.05, "description": "As `tenant_complaint_rate_pause`, for the bounce rate (FR-DLV-6, [G12]).", "maximum": 1, "minimum": 0, "type": "number"}, "tenant_complaint_rate_pause": {"default": 0.001, "description": "Pause the tenant's sending, or one of its sending domains, when its complaint rate over the last 7 UTC days, with at least 500 outcomes, reaches this (FR-DLV-6, [G12]). The default is Amazon SES's review rate.", "maximum": 1, "minimum": 0, "type": "number"}}, "required": ["complaint_rate_pause", "bounce_rate_pause", "tenant_complaint_rate_pause", "tenant_bounce_rate_pause"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyDomains {
    /// Lets the tenant's own keys, and its partner key, use the `nameservers` method, which creates a Cloudflare zone. Without it, they get `422 transport_unavailable` (`zone_creation_not_allowed`). Platform keys may always use it, when the Worker's Cloudflare token can create zones. Pylota Identity Cloud keeps it `false` (its Worker token, in a Cloudflare account shared with Pylota, cannot create zones). Platform-only.
    pub allow_create_zone: bool,
    /// Zones of the deployment's Cloudflare account, by A-label name, that the tenant's own keys and its partner key may use with `cloudflare_zone` (names strictly under a listed zone, never its apex), and exact host names written `=name` (for example `=notify.pylota.io`), which grant that one name only; list a zone you also use yourself only by exact names. These are besides the zones this deployment created for the tenant (`nameservers`, `delegated_subdomain`), and `replace_mx` is platform-only except on those. A zone created for another tenant, or under the zones of the deployment's platform domain, API host or console host, is refused even when listed (`403 scope_denied`, `details.reason = "zone_not_allowed"`). Platform keys may use any zone. The zone must also be among the zones of the Worker's Cloudflare token. Dropping a zone while the tenant still has domains in it is refused (`409 domain_in_use`, `zone_has_domains`). Platform-only.
    pub cloudflare_zones: Vec<String>,
}

fn schema_tenant_policy_domains_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"allow_create_zone": false, "cloudflare_zones": []}, "properties": {"allow_create_zone": {"default": false, "description": "Lets the tenant's own keys, and its partner key, use the `nameservers` method, which creates a Cloudflare zone. Without it, they get `422 transport_unavailable` (`zone_creation_not_allowed`). Platform keys may always use it, when the Worker's Cloudflare token can create zones. Pylota Identity Cloud keeps it `false` (its Worker token, in a Cloudflare account shared with Pylota, cannot create zones). Platform-only.", "type": "boolean"}, "cloudflare_zones": {"default": [], "description": "Zones of the deployment's Cloudflare account, by A-label name, that the tenant's own keys and its partner key may use with `cloudflare_zone` (names strictly under a listed zone, never its apex), and exact host names written `=name` (for example `=notify.pylota.io`), which grant that one name only; list a zone you also use yourself only by exact names. These are besides the zones this deployment created for the tenant (`nameservers`, `delegated_subdomain`), and `replace_mx` is platform-only except on those. A zone created for another tenant, or under the zones of the deployment's platform domain, API host or console host, is refused even when listed (`403 scope_denied`, `details.reason = \"zone_not_allowed\"`). Platform keys may use any zone. The zone must also be among the zones of the Worker's Cloudflare token. Dropping a zone while the tenant still has domains in it is refused (`409 domain_in_use`, `zone_has_domains`). Platform-only.", "items": {"pattern": "^=?[a-z0-9.-]{1,253}$", "type": "string"}, "maxItems": 50, "type": "array", "uniqueItems": true}}, "required": ["allow_create_zone", "cloudflare_zones"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyEvents {
    /// `true` emits the opt-in event `message.labels_changed`, delivered only to endpoints that name it.
    pub labels_changed: bool,
}

fn schema_tenant_policy_events_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"labels_changed": false}, "properties": {"labels_changed": {"default": false, "description": "`true` emits the opt-in event `message.labels_changed`, delivered only to endpoints that name it.", "type": "boolean"}}, "required": ["labels_changed"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyWebBotAuth {
    /// Lets the tenant's identities sign HTTP requests (Web Bot Auth). While it is `false`, `POST …/http-signatures` returns `403 policy_denied` ([O13]). It has no effect while the deployment has `PM_WEB_BOT_AUTH=off`. Agent assertions do not depend on it. Platform-only.
    pub allowed: bool,
}

fn schema_tenant_policy_web_bot_auth_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": {"allowed": false}, "properties": {"allowed": {"default": false, "description": "Lets the tenant's identities sign HTTP requests (Web Bot Auth). While it is `false`, `POST …/http-signatures` returns `403 policy_denied` ([O13]). It has no effect while the deployment has `PM_WEB_BOT_AUTH=off`. Agent assertions do not depend on it. Platform-only.", "type": "boolean"}}, "required": ["allowed"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyAccounts {
    /// `true` turns on the service sign-up ledger gate: a verification code or link from a service reaches the tenant's agents only when an approved ledger entry matches the mail; other such mail is quarantined `account_unapproved`, and `wait` with `kind=verification` needs an approved entry (`403 policy_denied`). Pylota Identity Cloud sets `true` in `PM_DEFAULT_POLICY`. Lower-only, with `false` as the looser value.
    pub require_approval: bool,
}

fn schema_tenant_policy_accounts_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": {"require_approval": false}, "properties": {"require_approval": {"default": false, "description": "`true` turns on the service sign-up ledger gate: a verification code or link from a service reaches the tenant's agents only when an approved ledger entry matches the mail; other such mail is quarantined `account_unapproved`, and `wait` with `kind=verification` needs an approved entry (`403 policy_denied`). Pylota Identity Cloud sets `true` in `PM_DEFAULT_POLICY`. Lower-only, with `false` as the looser value.", "type": "boolean"}}, "required": ["require_approval"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyDrafts {
    /// When a send needs approval: `never`, `new_recipients` (a recipient is neither known nor internal to the workspace), `external` (a recipient is not internal) or `always`. It applies to draft sends and to direct sends, which get `403 approval_required` when approval is needed; an identity's `send_policy.require_approval` can only make it stricter. A guard field: tightening means a level at or above the current one in the order `never` < `new_recipients` < `external` < `always`.
    pub require_approval: RequireApproval,
    /// Lets API keys with `drafts:approve` that reach this tenant, its partner key included, approve drafts even when `PM_QUARANTINE_KEY_RELEASE` is `off`; with it, only a platform key or the tenant's own partner key approves, never a tenant key. Only a platform key or the tenant's own partner key can set it; `quarantine.key_release` does not imply it.
    pub key_approval: bool,
}

fn schema_tenant_policy_drafts_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"key_approval": false, "require_approval": "never"}, "properties": {"key_approval": {"default": false, "description": "Lets API keys with `drafts:approve` that reach this tenant, its partner key included, approve drafts even when `PM_QUARANTINE_KEY_RELEASE` is `off`; with it, only a platform key or the tenant's own partner key approves, never a tenant key. Only a platform key or the tenant's own partner key can set it; `quarantine.key_release` does not imply it.", "type": "boolean"}, "require_approval": {"$ref": "#/components/schemas/RequireApproval", "default": "never", "description": "When a send needs approval: `never`, `new_recipients` (a recipient is neither known nor internal to the workspace), `external` (a recipient is not internal) or `always`. It applies to draft sends and to direct sends, which get `403 approval_required` when approval is needed; an identity's `send_policy.require_approval` can only make it stricter. A guard field: tightening means a level at or above the current one in the order `never` < `new_recipients` < `external` < `always`."}}, "required": ["require_approval", "key_approval"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyScreeningMode {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "monitor")]
    Monitor,
    #[serde(rename = "hold")]
    Hold,
}

fn schema_tenant_policy_screening_mode_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "monitor", "description": "`off`; `monitor` (mail is delivered at once and screened alongside, with a `message.screened` event); or `hold` (mail waits, hidden from agents, in status `screening` until its verdict). Guard field: tightening is a value at least as strict in the order `off` < `monitor` < `hold`.", "enum": ["off", "monitor", "hold"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyScreeningOnUnavailable {
    #[serde(rename = "quarantine")]
    Quarantine,
    #[serde(rename = "deliver")]
    Deliver,
}

fn schema_tenant_policy_screening_on_unavailable_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "quarantine", "description": "`quarantine` (no verdict by the deadline holds the message as `screening_unavailable`) or `deliver`. `deliver` applies only to `model_unavailable`, `ai_rate_limited` and `deadline_exceeded`; `model_cap` and `invalid_output`, which the mail itself can cause, are always held. Guard field; `quarantine` is the stricter value.", "enum": ["quarantine", "deliver"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyScreening {
    /// `off`; `monitor` (mail is delivered at once and screened alongside, with a `message.screened` event); or `hold` (mail waits, hidden from agents, in status `screening` until its verdict). Guard field: tightening is a value at least as strict in the order `off` < `monitor` < `hold`.
    #[schema(schema_with = schema_tenant_policy_screening_mode_property)]
    pub mode: TenantPolicyScreeningMode,
    /// `quarantine` (no verdict by the deadline holds the message as `screening_unavailable`) or `deliver`. `deliver` applies only to `model_unavailable`, `ai_rate_limited` and `deadline_exceeded`; `model_cap` and `invalid_output`, which the mail itself can cause, are always held. Guard field; `quarantine` is the stricter value.
    #[schema(schema_with = schema_tenant_policy_screening_on_unavailable_property)]
    pub on_unavailable: TenantPolicyScreeningOnUnavailable,
    /// The longest a held message waits for its verdict. Guard field: a tenant key may lower it only while the effective `on_unavailable` after the write is `quarantine`, otherwise only raise it.
    pub deadline_seconds: i64,
}

fn schema_tenant_policy_screening_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": {"deadline_seconds": 120, "mode": "monitor", "on_unavailable": "quarantine"}, "properties": {"deadline_seconds": {"default": 120, "description": "The longest a held message waits for its verdict. Guard field: a tenant key may lower it only while the effective `on_unavailable` after the write is `quarantine`, otherwise only raise it.", "maximum": 900, "minimum": 30, "type": "integer"}, "mode": {"default": "monitor", "description": "`off`; `monitor` (mail is delivered at once and screened alongside, with a `message.screened` event); or `hold` (mail waits, hidden from agents, in status `screening` until its verdict). Guard field: tightening is a value at least as strict in the order `off` < `monitor` < `hold`.", "enum": ["off", "monitor", "hold"], "type": "string"}, "on_unavailable": {"default": "quarantine", "description": "`quarantine` (no verdict by the deadline holds the message as `screening_unavailable`) or `deliver`. `deliver` applies only to `model_unavailable`, `ai_rate_limited` and `deadline_exceeded`; `model_cap` and `invalid_output`, which the mail itself can cause, are always held. Guard field; `quarantine` is the stricter value.", "enum": ["quarantine", "deliver"], "type": "string"}}, "required": ["mode", "on_unavailable", "deadline_seconds"], "type": "object"}"###).expect("the contract's schema")
}

/// The standing purchase budget (docs › Phone numbers › The standing purchase budget). A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage`. `version`, `set_by` and `set_at` are written by the service and ignored on write.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPhoneAutoApprove {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub countries: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<PhoneNumberType>)]
    pub number_types: Option<Vec<PhoneNumberType>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_numbers_per_identity: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_new_numbers_per_month: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_setup_pence_per_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_monthly_pence_per_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub expiring_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub version: Option<i64>,
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
}

fn schema_tenant_policy_phone_auto_approve_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": {"enabled": false}, "description": "The standing purchase budget (docs › Phone numbers › The standing purchase budget). A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage`. `version`, `set_by` and `set_at` are written by the service and ignored on write.", "properties": {"countries": {"items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": "array"}, "enabled": {"default": false, "type": "boolean"}, "expiring_only": {"default": false, "type": "boolean"}, "max_monthly_pence_per_number": {"minimum": 0, "type": "integer"}, "max_new_numbers_per_month": {"maximum": 200, "minimum": 1, "type": "integer"}, "max_numbers_per_identity": {"maximum": 20, "minimum": 1, "type": "integer"}, "max_setup_pence_per_number": {"minimum": 0, "type": "integer"}, "number_types": {"items": {"$ref": "#/components/schemas/PhoneNumberType"}, "type": "array"}, "set_at": {"format": "date-time", "readOnly": true, "type": ["string", "null"]}, "set_by": {"readOnly": true, "type": "object"}, "version": {"readOnly": true, "type": "integer"}}, "required": ["enabled"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPhoneSpend {
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
    /// A monthly cap for one-off registration fees (US 10DLC, toll-free verification), which count here instead of the monthly caps.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub registration_cap_pence: Option<Option<i64>>,
}

fn schema_tenant_policy_phone_spend_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"identity_monthly_cap_pence": {"default": null, "minimum": 0, "type": ["integer", "null"]}, "registration_cap_pence": {"default": null, "description": "A monthly cap for one-off registration fees (US 10DLC, toll-free verification), which count here instead of the monthly caps.", "minimum": 0, "type": ["integer", "null"]}, "tenant_monthly_cap_pence": {"default": null, "minimum": 0, "type": ["integer", "null"]}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPhoneSms {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub identity_daily_cap: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub tenant_daily_cap: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub tenant_monthly_cap: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<String>>)]
    pub allowed_destination_countries: Option<Option<Vec<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub brand_name: Option<Option<String>>,
}

fn schema_tenant_policy_phone_sms_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allowed_destination_countries": {"default": null, "items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "brand_name": {"default": null, "maxLength": 100, "type": ["string", "null"]}, "identity_daily_cap": {"default": 200, "minimum": 0, "type": "integer"}, "tenant_daily_cap": {"default": 2000, "minimum": 0, "type": "integer"}, "tenant_monthly_cap": {"default": null, "minimum": 0, "type": ["integer", "null"]}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPhoneInbound {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub per_sender_per_hour: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub per_identity_per_hour: Option<i64>,
}

fn schema_tenant_policy_phone_inbound_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"per_identity_per_hour": {"default": 600, "minimum": 1, "type": "integer"}, "per_sender_per_hour": {"default": 30, "minimum": 1, "type": "integer"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPhoneCallsOutbound {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "approval")]
    Approval,
    #[serde(rename = "allowed")]
    Allowed,
}

fn schema_tenant_policy_phone_calls_outbound_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "approval", "description": "Guard field: tightening is a value further left in `off` < `approval` < `allowed`.", "enum": ["off", "approval", "allowed"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPhoneCalls {
    /// Guard field: tightening is a value further left in `off` < `approval` < `allowed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_tenant_policy_phone_calls_outbound_property)]
    pub outbound: Option<TenantPolicyPhoneCallsOutbound>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<String>>)]
    pub allowed_destination_countries: Option<Option<Vec<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub concurrent_limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub max_minutes: Option<i64>,
}

fn schema_tenant_policy_phone_calls_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allowed_destination_countries": {"default": null, "items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "concurrent_limit": {"default": 2, "minimum": 1, "type": "integer"}, "max_minutes": {"default": 15, "minimum": 1, "type": "integer"}, "outbound": {"default": "approval", "description": "Guard field: tightening is a value further left in `off` < `approval` < `allowed`.", "enum": ["off", "approval", "allowed"], "type": "string"}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPhoneRecording {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "with_announcement")]
    WithAnnouncement,
}

fn schema_tenant_policy_phone_recording_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": "off", "description": "A guard field; `off` is the stricter value.", "enum": ["off", "with_announcement"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPhoneDisclosureCalls {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "off")]
    Off,
}

fn schema_tenant_policy_phone_disclosure_calls_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"default": "required", "enum": ["required", "off"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPhoneDisclosureFirstSms {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "off")]
    Off,
}

fn schema_tenant_policy_phone_disclosure_first_sms_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"default": "required", "enum": ["required", "off"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPhoneDisclosure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_tenant_policy_phone_disclosure_calls_property)]
    pub calls: Option<TenantPolicyPhoneDisclosureCalls>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_tenant_policy_phone_disclosure_first_sms_property)]
    pub first_sms: Option<TenantPolicyPhoneDisclosureFirstSms>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub call_text: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub sms_text: Option<Option<String>>,
}

fn schema_tenant_policy_phone_disclosure_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"call_text": {"default": null, "maxLength": 300, "minLength": 1, "type": ["string", "null"]}, "calls": {"default": "required", "enum": ["required", "off"], "type": "string"}, "first_sms": {"default": "required", "enum": ["required", "off"], "type": "string"}, "sms_text": {"default": null, "maxLength": 300, "minLength": 1, "type": ["string", "null"]}}, "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPhoneVerification {
    /// Codes shown as `[code]` in reads and events. A guard field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub redact: Option<bool>,
}

fn schema_tenant_policy_phone_verification_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"redact": {"default": true, "description": "Codes shown as `[code]` in reads and events. A guard field.", "type": "boolean"}}, "type": "object"}"###).expect("the contract's schema")
}

/// Phone numbers (docs › Phone numbers § 19). `allowed` and `key_approval` are platform or own-partner fields; `countries`, the number limits, `spend.*`, the SMS caps and destinations and the call limits are lower-only; `auto_approve`, `calls.outbound`, `recording`, `disclosure.calls`, `disclosure.first_sms` and `verification.redact` are guard fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPhone {
    /// Phone numbers may be used. On Pylota Identity Cloud only a platform key or the tenant's partner key turns it on.
    pub allowed: bool,
    /// A platform key or the tenant's partner key may approve phone requests, decide port-outs and set or loosen the standing budget, with `on_behalf_of`.
    pub key_approval: bool,
    /// Countries the tenant may buy in; `null` is every catalogue country.
    #[schema(required = true)]
    pub countries: Option<Vec<String>>,
    pub max_numbers_per_identity: i64,
    pub max_numbers_per_tenant: i64,
    /// The standing purchase budget (docs › Phone numbers › The standing purchase budget). A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage`. `version`, `set_by` and `set_at` are written by the service and ignored on write.
    #[schema(schema_with = schema_tenant_policy_phone_auto_approve_property)]
    pub auto_approve: TenantPolicyPhoneAutoApprove,
    #[schema(schema_with = schema_tenant_policy_phone_spend_property)]
    pub spend: TenantPolicyPhoneSpend,
    #[schema(schema_with = schema_tenant_policy_phone_sms_property)]
    pub sms: TenantPolicyPhoneSms,
    #[schema(schema_with = schema_tenant_policy_phone_inbound_property)]
    pub inbound: TenantPolicyPhoneInbound,
    #[schema(schema_with = schema_tenant_policy_phone_calls_property)]
    pub calls: TenantPolicyPhoneCalls,
    /// A guard field; `off` is the stricter value.
    #[schema(schema_with = schema_tenant_policy_phone_recording_property)]
    pub recording: TenantPolicyPhoneRecording,
    #[schema(schema_with = schema_tenant_policy_phone_disclosure_property)]
    pub disclosure: TenantPolicyPhoneDisclosure,
    #[schema(schema_with = schema_tenant_policy_phone_verification_property)]
    pub verification: TenantPolicyPhoneVerification,
}

fn schema_tenant_policy_phone_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"allowed": false, "auto_approve": {"enabled": false}, "calls": {"allowed_destination_countries": null, "concurrent_limit": 2, "max_minutes": 15, "outbound": "approval"}, "countries": null, "disclosure": {"call_text": null, "calls": "required", "first_sms": "required", "sms_text": null}, "inbound": {"per_identity_per_hour": 600, "per_sender_per_hour": 30}, "key_approval": false, "max_numbers_per_identity": 5, "max_numbers_per_tenant": 20, "recording": "off", "sms": {"allowed_destination_countries": null, "brand_name": null, "identity_daily_cap": 200, "tenant_daily_cap": 2000, "tenant_monthly_cap": null}, "spend": {"identity_monthly_cap_pence": null, "registration_cap_pence": null, "tenant_monthly_cap_pence": null}, "verification": {"redact": true}}, "description": "Phone numbers (docs › Phone numbers § 19). `allowed` and `key_approval` are platform or own-partner fields; `countries`, the number limits, `spend.*`, the SMS caps and destinations and the call limits are lower-only; `auto_approve`, `calls.outbound`, `recording`, `disclosure.calls`, `disclosure.first_sms` and `verification.redact` are guard fields.", "properties": {"allowed": {"default": false, "description": "Phone numbers may be used. On Pylota Identity Cloud only a platform key or the tenant's partner key turns it on.", "type": "boolean"}, "auto_approve": {"default": {"enabled": false}, "description": "The standing purchase budget (docs › Phone numbers › The standing purchase budget). A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage`. `version`, `set_by` and `set_at` are written by the service and ignored on write.", "properties": {"countries": {"items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": "array"}, "enabled": {"default": false, "type": "boolean"}, "expiring_only": {"default": false, "type": "boolean"}, "max_monthly_pence_per_number": {"minimum": 0, "type": "integer"}, "max_new_numbers_per_month": {"maximum": 200, "minimum": 1, "type": "integer"}, "max_numbers_per_identity": {"maximum": 20, "minimum": 1, "type": "integer"}, "max_setup_pence_per_number": {"minimum": 0, "type": "integer"}, "number_types": {"items": {"$ref": "#/components/schemas/PhoneNumberType"}, "type": "array"}, "set_at": {"format": "date-time", "readOnly": true, "type": ["string", "null"]}, "set_by": {"readOnly": true, "type": "object"}, "version": {"readOnly": true, "type": "integer"}}, "required": ["enabled"], "type": "object"}, "calls": {"properties": {"allowed_destination_countries": {"default": null, "items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "concurrent_limit": {"default": 2, "minimum": 1, "type": "integer"}, "max_minutes": {"default": 15, "minimum": 1, "type": "integer"}, "outbound": {"default": "approval", "description": "Guard field: tightening is a value further left in `off` < `approval` < `allowed`.", "enum": ["off", "approval", "allowed"], "type": "string"}}, "type": "object"}, "countries": {"default": null, "description": "Countries the tenant may buy in; `null` is every catalogue country.", "items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "disclosure": {"properties": {"call_text": {"default": null, "maxLength": 300, "minLength": 1, "type": ["string", "null"]}, "calls": {"default": "required", "enum": ["required", "off"], "type": "string"}, "first_sms": {"default": "required", "enum": ["required", "off"], "type": "string"}, "sms_text": {"default": null, "maxLength": 300, "minLength": 1, "type": ["string", "null"]}}, "type": "object"}, "inbound": {"properties": {"per_identity_per_hour": {"default": 600, "minimum": 1, "type": "integer"}, "per_sender_per_hour": {"default": 30, "minimum": 1, "type": "integer"}}, "type": "object"}, "key_approval": {"default": false, "description": "A platform key or the tenant's partner key may approve phone requests, decide port-outs and set or loosen the standing budget, with `on_behalf_of`.", "type": "boolean"}, "max_numbers_per_identity": {"default": 5, "maximum": 20, "minimum": 1, "type": "integer"}, "max_numbers_per_tenant": {"default": 20, "maximum": 200, "minimum": 1, "type": "integer"}, "recording": {"default": "off", "description": "A guard field; `off` is the stricter value.", "enum": ["off", "with_announcement"], "type": "string"}, "sms": {"properties": {"allowed_destination_countries": {"default": null, "items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "brand_name": {"default": null, "maxLength": 100, "type": ["string", "null"]}, "identity_daily_cap": {"default": 200, "minimum": 0, "type": "integer"}, "tenant_daily_cap": {"default": 2000, "minimum": 0, "type": "integer"}, "tenant_monthly_cap": {"default": null, "minimum": 0, "type": ["integer", "null"]}}, "type": "object"}, "spend": {"properties": {"identity_monthly_cap_pence": {"default": null, "minimum": 0, "type": ["integer", "null"]}, "registration_cap_pence": {"default": null, "description": "A monthly cap for one-off registration fees (US 10DLC, toll-free verification), which count here instead of the monthly caps.", "minimum": 0, "type": ["integer", "null"]}, "tenant_monthly_cap_pence": {"default": null, "minimum": 0, "type": ["integer", "null"]}}, "type": "object"}, "verification": {"properties": {"redact": {"default": true, "description": "Codes shown as `[code]` in reads and events. A guard field.", "type": "boolean"}}, "type": "object"}}, "required": ["allowed", "key_approval", "countries", "max_numbers_per_identity", "max_numbers_per_tenant", "auto_approve", "spend", "sms", "inbound", "calls", "recording", "disclosure", "verification"], "type": "object"}"###).expect("the contract's schema")
}

/// The vault (docs › Agent credential vault). `allowed` is platform-only; `key_approval` is a platform or own-partner field; `min_approval` and `codes_into_fills_only` are guard fields; `max_items`, `session_max_ttl_days` and `approval_deadline_seconds` are lower-only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyVault {
    /// Lets the tenant use the vault. Platform-only; turning it off closes every open use.
    pub allowed: bool,
    /// A platform key or the partner key of the tenant's own partner, holding `vault:approve`, may approve this tenant's grants and uses, naming the person with `on_behalf_of`. Never a tenant or identity key.
    pub key_approval: bool,
    /// The floor of every item's approval level. Guard field: tightening is a level at or above the current one in `grant` < `per_run` < `per_use`.
    pub min_approval: VaultApprovalLevel,
    /// Withholds every verification message (mail and SMS) from agents and delivers codes only into vault phases. Guard field; `true` is the stricter value.
    pub codes_into_fills_only: bool,
    pub max_items: i64,
    pub session_max_ttl_days: i64,
    pub approval_deadline_seconds: i64,
}

fn schema_tenant_policy_vault_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"default": {"allowed": false, "approval_deadline_seconds": 600, "codes_into_fills_only": false, "key_approval": false, "max_items": 1000, "min_approval": "grant", "session_max_ttl_days": 30}, "description": "The vault (docs › Agent credential vault). `allowed` is platform-only; `key_approval` is a platform or own-partner field; `min_approval` and `codes_into_fills_only` are guard fields; `max_items`, `session_max_ttl_days` and `approval_deadline_seconds` are lower-only.", "properties": {"allowed": {"default": false, "description": "Lets the tenant use the vault. Platform-only; turning it off closes every open use.", "type": "boolean"}, "approval_deadline_seconds": {"default": 600, "maximum": 900, "minimum": 60, "type": "integer"}, "codes_into_fills_only": {"default": false, "description": "Withholds every verification message (mail and SMS) from agents and delivers codes only into vault phases. Guard field; `true` is the stricter value.", "type": "boolean"}, "key_approval": {"default": false, "description": "A platform key or the partner key of the tenant's own partner, holding `vault:approve`, may approve this tenant's grants and uses, naming the person with `on_behalf_of`. Never a tenant or identity key.", "type": "boolean"}, "max_items": {"default": 1000, "maximum": 10000, "minimum": 1, "type": "integer"}, "min_approval": {"$ref": "#/components/schemas/VaultApprovalLevel", "default": "grant", "description": "The floor of every item's approval level. Guard field: tightening is a level at or above the current one in `grant` < `per_run` < `per_use`."}, "session_max_ttl_days": {"default": 30, "maximum": 30, "minimum": 1, "type": "integer"}}, "required": ["allowed", "key_approval", "min_approval", "codes_into_fills_only", "max_items", "session_max_ttl_days", "approval_deadline_seconds"], "type": "object"}"###).expect("the contract's schema")
}

/// The full effective policy of a tenant: the built-in defaults shown here, then `PM_DEFAULT_POLICY`, then the tenant's own values, deep-merged. Who may write each field (docs › Configuration › Who may change a field): platform keys write every field. For a partner key, `web_bot_auth.allowed`, `domains.allow_create_zone`, `domains.cloudflare_zones`, `inbound.trusted_arc_sealers` and `vault.allowed` are platform-only; `quarantine.key_release`, `drafts.key_approval`, `phone.allowed`, `phone.key_approval` and `vault.key_approval` are allowed on its own tenants; the lower-only fields (`identity_daily_send_cap`, `tenant_daily_send_cap`, `max_recipients`, `auto_reply.allowed`, `auto_reply.max_automatic_exchanges`, `inbound.per_sender_per_hour`, `inbound.unauthenticated_per_hour`, `inbound.per_tenant_per_hour`, `inbound.extract_image_text`, `retention.raw_days`, `retention.events_days`, `triage.enabled`, `triage.daily_model_cap`, `search.agentic_enabled`, `search.agentic_daily_cap`, `search.agentic_max_steps`, `search.agentic_max_seconds`, the `abuse` thresholds, `phone.countries`, the phone number limits, `phone.spend.*`, the `phone.sms` caps and destinations, the `phone.calls` destinations and limits, `vault.max_items`, `vault.session_max_ttl_days` and `vault.approval_deadline_seconds`) may be set at most to min(deployment default, platform ceiling), a switch to `true` only when both allow it (for `accounts.require_approval`, `false` is the looser value); every other field is free. A tenant key with `policy:write` (`updateTenantPolicy`) may set free fields, lower a lower-only field to at most its workspace ceiling (the strictest of the deployment default, the platform ceiling and the partner ceiling), and only tighten the guard fields `send_allowlist_only`, `quarantine.on_auth_fail`, `quarantine.spam_threshold`, `quarantine.unsolicited_otp`, `drafts.require_approval`, `screening.mode`, `screening.on_unavailable`, `screening.deadline_seconds`, `phone.auto_approve` (setting it or loosening any field), `phone.calls.outbound`, `phone.recording`, `phone.disclosure.calls`, `phone.disclosure.first_sms`, `phone.verification.redact`, `vault.min_approval` and `vault.codes_into_fills_only` unless API keys may take decisions reserved for people on the tenant; platform-only fields, `quarantine.key_release`, `drafts.key_approval`, `phone.allowed`, `phone.key_approval` and `vault.key_approval` are not writable for it. A refused field gets `403 scope_denied` with `details.field` (or `403 permission_denied` with `details.reason = "person_required"` for a guard field) and nothing is stored. Identity keys cannot write the policy. While a tenant is a provisional agent sign-up, its effective policy also takes a fixed overlay that only tightens (docs › Agent self-sign-up § 4.2); the overlay is not stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicy {
    /// Sends per identity per day (tenant time zone). Then `429 daily_cap_reached`.
    pub identity_daily_send_cap: i64,
    /// Sends per tenant per day (tenant time zone). Then `429 daily_cap_reached`. With billing on, a new workspace on the Free plan has an effective cap of the lower of this value and 50 until its new-workspace send ramp is lifted (at the earliest on day 7, or at once on a paid plan).
    pub tenant_daily_send_cap: i64,
    /// Recipients per message across `to`, `cc` and `bcc`, 1–49. Cloudflare allows 50 recipients per message, and one is kept for the hidden journal copy. Then `400 too_many_recipients`.
    pub max_recipients: i64,
    /// Only recipients on the send-allow list are allowed.
    pub send_allowlist_only: bool,
    /// `refuse` returns `413 message_too_large`; `link` turns oversized attachments into expiring signed links.
    #[schema(schema_with = schema_tenant_policy_large_attachments_property)]
    pub large_attachments: TenantPolicyLargeAttachments,
    /// Lifetime of signed attachment links.
    pub link_ttl_hours: i64,
    #[schema(schema_with = schema_tenant_policy_ai_disclosure_property)]
    pub ai_disclosure: TenantPolicyAiDisclosure,
    #[schema(schema_with = schema_tenant_policy_auto_reply_property)]
    pub auto_reply: TenantPolicyAutoReply,
    #[schema(schema_with = schema_tenant_policy_quarantine_property)]
    pub quarantine: TenantPolicyQuarantine,
    #[schema(schema_with = schema_tenant_policy_inbound_property)]
    pub inbound: TenantPolicyInbound,
    #[schema(schema_with = schema_tenant_policy_retention_property)]
    pub retention: TenantPolicyRetention,
    #[schema(schema_with = schema_tenant_policy_triage_property)]
    pub triage: TenantPolicyTriage,
    #[schema(schema_with = schema_tenant_policy_search_property)]
    pub search: TenantPolicySearch,
    /// Bytes of `extracted_text` included in `message.received` payloads.
    pub webhook_text_bytes: i64,
    #[schema(schema_with = schema_tenant_policy_abuse_property)]
    pub abuse: TenantPolicyAbuse,
    #[schema(schema_with = schema_tenant_policy_domains_property)]
    pub domains: TenantPolicyDomains,
    #[schema(schema_with = schema_tenant_policy_events_property)]
    pub events: TenantPolicyEvents,
    /// `false` fails sends on a failing domain (`domain_failing_no_fallback`) instead of using the platform address.
    pub domain_fallback: bool,
    #[schema(schema_with = schema_tenant_policy_web_bot_auth_property)]
    pub web_bot_auth: TenantPolicyWebBotAuth,
    #[schema(schema_with = schema_tenant_policy_accounts_property)]
    pub accounts: TenantPolicyAccounts,
    #[schema(schema_with = schema_tenant_policy_drafts_property)]
    pub drafts: TenantPolicyDrafts,
    #[schema(schema_with = schema_tenant_policy_screening_property)]
    pub screening: TenantPolicyScreening,
    /// Phone numbers (docs › Phone numbers § 19). `allowed` and `key_approval` are platform or own-partner fields; `countries`, the number limits, `spend.*`, the SMS caps and destinations and the call limits are lower-only; `auto_approve`, `calls.outbound`, `recording`, `disclosure.calls`, `disclosure.first_sms` and `verification.redact` are guard fields.
    #[schema(schema_with = schema_tenant_policy_phone_property)]
    pub phone: TenantPolicyPhone,
    /// The vault (docs › Agent credential vault). `allowed` is platform-only; `key_approval` is a platform or own-partner field; `min_approval` and `codes_into_fills_only` are guard fields; `max_items`, `session_max_ttl_days` and `approval_deadline_seconds` are lower-only.
    #[schema(schema_with = schema_tenant_policy_vault_property)]
    pub vault: TenantPolicyVault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchLargeAttachments {
    #[serde(rename = "refuse")]
    Refuse,
    #[serde(rename = "link")]
    Link,
}

fn schema_tenant_policy_patch_large_attachments_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["refuse", "link", null], "type": ["string", "null"]}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchAiDisclosureMode {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "footer")]
    Footer,
    #[serde(rename = "header")]
    Header,
}

fn schema_tenant_policy_patch_ai_disclosure_mode_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["none", "footer", "header", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchAiDisclosure {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_ai_disclosure_mode_property)]
    pub mode: Option<Option<TenantPolicyPatchAiDisclosureMode>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub text: Option<Option<String>>,
}

fn schema_tenant_policy_patch_ai_disclosure_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"mode": {"enum": ["none", "footer", "header", null], "type": ["string", "null"]}, "text": {"type": ["string", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchAutoReply {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub allowed: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_automatic_exchanges: Option<Option<i64>>,
}

fn schema_tenant_policy_patch_auto_reply_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allowed": {"type": ["boolean", "null"]}, "max_automatic_exchanges": {"minimum": 0, "type": ["integer", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchQuarantine {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub on_auth_fail: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<f64>)]
    pub spam_threshold: Option<Option<f64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub unsolicited_otp: Option<Option<bool>>,
    /// Platform keys, and the partner key of the tenant's own partner, only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub key_release: Option<Option<bool>>,
}

fn schema_tenant_policy_patch_quarantine_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"key_release": {"description": "Platform keys, and the partner key of the tenant's own partner, only.", "type": ["boolean", "null"]}, "on_auth_fail": {"type": ["boolean", "null"]}, "spam_threshold": {"maximum": 1, "minimum": 0, "type": ["number", "null"]}, "unsolicited_otp": {"type": ["boolean", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchInboundExtractAttachmentTextItem {
    #[serde(rename = "pdf")]
    Pdf,
    #[serde(rename = "office")]
    Office,
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "html")]
    Html,
}

fn schema_tenant_policy_patch_inbound_extract_attachment_text_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"enum": ["pdf", "office", "text", "html"], "type": "string"}, "type": ["array", "null"], "uniqueItems": true}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchInbound {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub per_sender_per_hour: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub unauthenticated_per_hour: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub per_tenant_per_hour: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_inbound_extract_attachment_text_property)]
    pub extract_attachment_text:
        Option<Option<Vec<TenantPolicyPatchInboundExtractAttachmentTextItem>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub extract_image_text: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub ses_bounce_retired: Option<Option<bool>>,
    /// Platform-only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<String>>)]
    pub trusted_arc_sealers: Option<Option<Vec<String>>>,
}

fn schema_tenant_policy_patch_inbound_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"extract_attachment_text": {"items": {"enum": ["pdf", "office", "text", "html"], "type": "string"}, "type": ["array", "null"], "uniqueItems": true}, "extract_image_text": {"type": ["boolean", "null"]}, "per_sender_per_hour": {"minimum": 1, "type": ["integer", "null"]}, "per_tenant_per_hour": {"minimum": 1, "type": ["integer", "null"]}, "ses_bounce_retired": {"type": ["boolean", "null"]}, "trusted_arc_sealers": {"description": "Platform-only.", "items": {"pattern": "^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$", "type": "string"}, "maxItems": 20, "type": ["array", "null"], "uniqueItems": true}, "unauthenticated_per_hour": {"minimum": 1, "type": ["integer", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchRetention {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub raw_days: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub message_days: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub events_days: Option<Option<i64>>,
}

fn schema_tenant_policy_patch_retention_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"events_days": {"maximum": 365, "minimum": 1, "type": ["integer", "null"]}, "message_days": {"minimum": 1, "type": ["integer", "null"]}, "raw_days": {"minimum": 1, "type": ["integer", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchTriage {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub enabled: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub daily_model_cap: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<TriageCategoryDefinition>>)]
    pub categories: Option<Option<Vec<TriageCategoryDefinition>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<TriageRule>>)]
    pub rules: Option<Option<Vec<TriageRule>>>,
}

fn schema_tenant_policy_patch_triage_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"categories": {"items": {"$ref": "#/components/schemas/TriageCategoryDefinition"}, "maxItems": 20, "type": ["array", "null"]}, "daily_model_cap": {"minimum": 0, "type": ["integer", "null"]}, "enabled": {"type": ["boolean", "null"]}, "rules": {"items": {"$ref": "#/components/schemas/TriageRule"}, "maxItems": 50, "type": ["array", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchSearchRefsPacksItem {
    #[serde(rename = "core")]
    Core,
    #[serde(rename = "uk_vehicle")]
    UkVehicle,
}

fn schema_tenant_policy_patch_search_refs_packs_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"enum": ["core", "uk_vehicle"], "type": "string"}, "type": ["array", "null"], "uniqueItems": true}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchSearch {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub agentic_enabled: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub agentic_daily_cap: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub agentic_max_steps: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub agentic_max_seconds: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_search_refs_packs_property)]
    pub refs_packs: Option<Option<Vec<TenantPolicyPatchSearchRefsPacksItem>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<CustomRefPattern>>)]
    pub custom_refs: Option<Option<Vec<CustomRefPattern>>>,
}

fn schema_tenant_policy_patch_search_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"agentic_daily_cap": {"minimum": 0, "type": ["integer", "null"]}, "agentic_enabled": {"type": ["boolean", "null"]}, "agentic_max_seconds": {"maximum": 30, "minimum": 3, "type": ["integer", "null"]}, "agentic_max_steps": {"maximum": 10, "minimum": 2, "type": ["integer", "null"]}, "custom_refs": {"items": {"$ref": "#/components/schemas/CustomRefPattern"}, "maxItems": 20, "type": ["array", "null"]}, "refs_packs": {"items": {"enum": ["core", "uk_vehicle"], "type": "string"}, "type": ["array", "null"], "uniqueItems": true}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchAbuse {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<f64>)]
    pub complaint_rate_pause: Option<Option<f64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<f64>)]
    pub bounce_rate_pause: Option<Option<f64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<f64>)]
    pub tenant_complaint_rate_pause: Option<Option<f64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<f64>)]
    pub tenant_bounce_rate_pause: Option<Option<f64>>,
}

fn schema_tenant_policy_patch_abuse_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"bounce_rate_pause": {"maximum": 1, "minimum": 0, "type": ["number", "null"]}, "complaint_rate_pause": {"maximum": 1, "minimum": 0, "type": ["number", "null"]}, "tenant_bounce_rate_pause": {"maximum": 1, "minimum": 0, "type": ["number", "null"]}, "tenant_complaint_rate_pause": {"maximum": 1, "minimum": 0, "type": ["number", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchDomains {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub allow_create_zone: Option<Option<bool>>,
    /// Platform-only. Zones a tenant or partner key may use with `cloudflare_zone`, beyond the zones created for the tenant. A listed zone grants names strictly under it: its apex, and `replace_mx` there, stay platform-only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<String>>)]
    pub cloudflare_zones: Option<Option<Vec<String>>>,
}

fn schema_tenant_policy_patch_domains_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allow_create_zone": {"type": ["boolean", "null"]}, "cloudflare_zones": {"description": "Platform-only. Zones a tenant or partner key may use with `cloudflare_zone`, beyond the zones created for the tenant. A listed zone grants names strictly under it: its apex, and `replace_mx` there, stay platform-only.", "items": {"pattern": "^[a-z0-9.-]{1,253}$", "type": "string"}, "maxItems": 50, "type": ["array", "null"], "uniqueItems": true}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchEvents {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub labels_changed: Option<Option<bool>>,
}

fn schema_tenant_policy_patch_events_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"labels_changed": {"type": ["boolean", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchWebBotAuth {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub allowed: Option<Option<bool>>,
}

fn schema_tenant_policy_patch_web_bot_auth_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allowed": {"type": ["boolean", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchAccounts {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub require_approval: Option<Option<bool>>,
}

fn schema_tenant_policy_patch_accounts_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"require_approval": {"type": ["boolean", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchDraftsRequireApproval {
    #[serde(rename = "never")]
    Never,
    #[serde(rename = "new_recipients")]
    NewRecipients,
    #[serde(rename = "external")]
    External,
    #[serde(rename = "always")]
    Always,
}

fn schema_tenant_policy_patch_drafts_require_approval_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["never", "new_recipients", "external", "always", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchDrafts {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_drafts_require_approval_property)]
    pub require_approval: Option<Option<TenantPolicyPatchDraftsRequireApproval>>,
    /// Platform keys, and the partner key of the tenant's own partner, only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub key_approval: Option<Option<bool>>,
}

fn schema_tenant_policy_patch_drafts_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"key_approval": {"description": "Platform keys, and the partner key of the tenant's own partner, only.", "type": ["boolean", "null"]}, "require_approval": {"enum": ["never", "new_recipients", "external", "always", null], "type": ["string", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchScreeningMode {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "monitor")]
    Monitor,
    #[serde(rename = "hold")]
    Hold,
}

fn schema_tenant_policy_patch_screening_mode_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["off", "monitor", "hold", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchScreeningOnUnavailable {
    #[serde(rename = "quarantine")]
    Quarantine,
    #[serde(rename = "deliver")]
    Deliver,
}

fn schema_tenant_policy_patch_screening_on_unavailable_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["quarantine", "deliver", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchScreening {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_screening_mode_property)]
    pub mode: Option<Option<TenantPolicyPatchScreeningMode>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_screening_on_unavailable_property)]
    pub on_unavailable: Option<Option<TenantPolicyPatchScreeningOnUnavailable>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub deadline_seconds: Option<Option<i64>>,
}

fn schema_tenant_policy_patch_screening_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"deadline_seconds": {"maximum": 900, "minimum": 30, "type": ["integer", "null"]}, "mode": {"enum": ["off", "monitor", "hold", null], "type": ["string", "null"]}, "on_unavailable": {"enum": ["quarantine", "deliver", null], "type": ["string", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

/// The standing purchase budget (docs › Phone numbers › The standing purchase budget). A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage`. `version`, `set_by` and `set_at` are written by the service and ignored on write.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchPhoneAutoApprove {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub enabled: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<String>>)]
    pub countries: Option<Option<Vec<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<PhoneNumberType>>)]
    pub number_types: Option<Option<Vec<PhoneNumberType>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_numbers_per_identity: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_new_numbers_per_month: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_setup_pence_per_number: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_monthly_pence_per_number: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub expiring_only: Option<Option<bool>>,
}

fn schema_tenant_policy_patch_phone_auto_approve_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "The standing purchase budget (docs › Phone numbers › The standing purchase budget). A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage`. `version`, `set_by` and `set_at` are written by the service and ignored on write.", "properties": {"countries": {"items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "enabled": {"type": ["boolean", "null"]}, "expiring_only": {"type": ["boolean", "null"]}, "max_monthly_pence_per_number": {"minimum": 0, "type": ["integer", "null"]}, "max_new_numbers_per_month": {"maximum": 200, "minimum": 1, "type": ["integer", "null"]}, "max_numbers_per_identity": {"maximum": 20, "minimum": 1, "type": ["integer", "null"]}, "max_setup_pence_per_number": {"minimum": 0, "type": ["integer", "null"]}, "number_types": {"items": {"$ref": "#/components/schemas/PhoneNumberType"}, "type": ["array", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchPhoneSpend {
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
    /// A monthly cap for one-off registration fees (US 10DLC, toll-free verification), which count here instead of the monthly caps.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub registration_cap_pence: Option<Option<i64>>,
}

fn schema_tenant_policy_patch_phone_spend_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"identity_monthly_cap_pence": {"minimum": 0, "type": ["integer", "null"]}, "registration_cap_pence": {"description": "A monthly cap for one-off registration fees (US 10DLC, toll-free verification), which count here instead of the monthly caps.", "minimum": 0, "type": ["integer", "null"]}, "tenant_monthly_cap_pence": {"minimum": 0, "type": ["integer", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchPhoneSms {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub identity_daily_cap: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub tenant_daily_cap: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub tenant_monthly_cap: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<String>>)]
    pub allowed_destination_countries: Option<Option<Vec<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub brand_name: Option<Option<String>>,
}

fn schema_tenant_policy_patch_phone_sms_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allowed_destination_countries": {"items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "brand_name": {"maxLength": 100, "type": ["string", "null"]}, "identity_daily_cap": {"minimum": 0, "type": ["integer", "null"]}, "tenant_daily_cap": {"minimum": 0, "type": ["integer", "null"]}, "tenant_monthly_cap": {"minimum": 0, "type": ["integer", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchPhoneInbound {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub per_sender_per_hour: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub per_identity_per_hour: Option<Option<i64>>,
}

fn schema_tenant_policy_patch_phone_inbound_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"per_identity_per_hour": {"minimum": 1, "type": ["integer", "null"]}, "per_sender_per_hour": {"minimum": 1, "type": ["integer", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchPhoneCallsOutbound {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "approval")]
    Approval,
    #[serde(rename = "allowed")]
    Allowed,
}

fn schema_tenant_policy_patch_phone_calls_outbound_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Guard field: tightening is a value further left in `off` < `approval` < `allowed`.", "enum": ["off", "approval", "allowed", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchPhoneCalls {
    /// Guard field: tightening is a value further left in `off` < `approval` < `allowed`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_calls_outbound_property)]
    pub outbound: Option<Option<TenantPolicyPatchPhoneCallsOutbound>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<String>>)]
    pub allowed_destination_countries: Option<Option<Vec<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub concurrent_limit: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_minutes: Option<Option<i64>>,
}

fn schema_tenant_policy_patch_phone_calls_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allowed_destination_countries": {"items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "concurrent_limit": {"minimum": 1, "type": ["integer", "null"]}, "max_minutes": {"minimum": 1, "type": ["integer", "null"]}, "outbound": {"description": "Guard field: tightening is a value further left in `off` < `approval` < `allowed`.", "enum": ["off", "approval", "allowed", null], "type": ["string", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchPhoneRecording {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "with_announcement")]
    WithAnnouncement,
}

fn schema_tenant_policy_patch_phone_recording_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "A guard field; `off` is the stricter value.", "enum": ["off", "with_announcement", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchPhoneDisclosureCalls {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "off")]
    Off,
}

fn schema_tenant_policy_patch_phone_disclosure_calls_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["required", "off", null], "type": ["string", "null"]}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchPhoneDisclosureFirstSms {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "off")]
    Off,
}

fn schema_tenant_policy_patch_phone_disclosure_first_sms_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["required", "off", null], "type": ["string", "null"]}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchPhoneDisclosure {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_disclosure_calls_property)]
    pub calls: Option<Option<TenantPolicyPatchPhoneDisclosureCalls>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_disclosure_first_sms_property)]
    pub first_sms: Option<Option<TenantPolicyPatchPhoneDisclosureFirstSms>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub call_text: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub sms_text: Option<Option<String>>,
}

fn schema_tenant_policy_patch_phone_disclosure_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"call_text": {"maxLength": 300, "minLength": 1, "type": ["string", "null"]}, "calls": {"enum": ["required", "off", null], "type": ["string", "null"]}, "first_sms": {"enum": ["required", "off", null], "type": ["string", "null"]}, "sms_text": {"maxLength": 300, "minLength": 1, "type": ["string", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchPhoneVerification {
    /// Codes shown as `[code]` in reads and events. A guard field.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub redact: Option<Option<bool>>,
}

fn schema_tenant_policy_patch_phone_verification_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"redact": {"description": "Codes shown as `[code]` in reads and events. A guard field.", "type": ["boolean", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchPhone {
    /// Phone numbers may be used. On Pylota Identity Cloud only a platform key or the tenant's partner key turns it on.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub allowed: Option<Option<bool>>,
    /// A platform key or the tenant's partner key may approve phone requests, decide port-outs and set or loosen the standing budget, with `on_behalf_of`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub key_approval: Option<Option<bool>>,
    /// Countries the tenant may buy in; `null` is every catalogue country.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<String>>)]
    pub countries: Option<Option<Vec<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_numbers_per_identity: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_numbers_per_tenant: Option<Option<i64>>,
    /// The standing purchase budget (docs › Phone numbers › The standing purchase budget). A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage`. `version`, `set_by` and `set_at` are written by the service and ignored on write.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_auto_approve_property)]
    pub auto_approve: Option<Option<TenantPolicyPatchPhoneAutoApprove>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_spend_property)]
    pub spend: Option<Option<TenantPolicyPatchPhoneSpend>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_sms_property)]
    pub sms: Option<Option<TenantPolicyPatchPhoneSms>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_inbound_property)]
    pub inbound: Option<Option<TenantPolicyPatchPhoneInbound>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_calls_property)]
    pub calls: Option<Option<TenantPolicyPatchPhoneCalls>>,
    /// A guard field; `off` is the stricter value.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_recording_property)]
    pub recording: Option<Option<TenantPolicyPatchPhoneRecording>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_disclosure_property)]
    pub disclosure: Option<Option<TenantPolicyPatchPhoneDisclosure>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_verification_property)]
    pub verification: Option<Option<TenantPolicyPatchPhoneVerification>>,
}

fn schema_tenant_policy_patch_phone_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allowed": {"description": "Phone numbers may be used. On Pylota Identity Cloud only a platform key or the tenant's partner key turns it on.", "type": ["boolean", "null"]}, "auto_approve": {"description": "The standing purchase budget (docs › Phone numbers › The standing purchase budget). A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage`. `version`, `set_by` and `set_at` are written by the service and ignored on write.", "properties": {"countries": {"items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "enabled": {"type": ["boolean", "null"]}, "expiring_only": {"type": ["boolean", "null"]}, "max_monthly_pence_per_number": {"minimum": 0, "type": ["integer", "null"]}, "max_new_numbers_per_month": {"maximum": 200, "minimum": 1, "type": ["integer", "null"]}, "max_numbers_per_identity": {"maximum": 20, "minimum": 1, "type": ["integer", "null"]}, "max_setup_pence_per_number": {"minimum": 0, "type": ["integer", "null"]}, "number_types": {"items": {"$ref": "#/components/schemas/PhoneNumberType"}, "type": ["array", "null"]}}, "type": ["object", "null"]}, "calls": {"properties": {"allowed_destination_countries": {"items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "concurrent_limit": {"minimum": 1, "type": ["integer", "null"]}, "max_minutes": {"minimum": 1, "type": ["integer", "null"]}, "outbound": {"description": "Guard field: tightening is a value further left in `off` < `approval` < `allowed`.", "enum": ["off", "approval", "allowed", null], "type": ["string", "null"]}}, "type": ["object", "null"]}, "countries": {"description": "Countries the tenant may buy in; `null` is every catalogue country.", "items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "disclosure": {"properties": {"call_text": {"maxLength": 300, "minLength": 1, "type": ["string", "null"]}, "calls": {"enum": ["required", "off", null], "type": ["string", "null"]}, "first_sms": {"enum": ["required", "off", null], "type": ["string", "null"]}, "sms_text": {"maxLength": 300, "minLength": 1, "type": ["string", "null"]}}, "type": ["object", "null"]}, "inbound": {"properties": {"per_identity_per_hour": {"minimum": 1, "type": ["integer", "null"]}, "per_sender_per_hour": {"minimum": 1, "type": ["integer", "null"]}}, "type": ["object", "null"]}, "key_approval": {"description": "A platform key or the tenant's partner key may approve phone requests, decide port-outs and set or loosen the standing budget, with `on_behalf_of`.", "type": ["boolean", "null"]}, "max_numbers_per_identity": {"maximum": 20, "minimum": 1, "type": ["integer", "null"]}, "max_numbers_per_tenant": {"maximum": 200, "minimum": 1, "type": ["integer", "null"]}, "recording": {"description": "A guard field; `off` is the stricter value.", "enum": ["off", "with_announcement", null], "type": ["string", "null"]}, "sms": {"properties": {"allowed_destination_countries": {"items": {"pattern": "^[A-Z]{2}$", "type": "string"}, "type": ["array", "null"]}, "brand_name": {"maxLength": 100, "type": ["string", "null"]}, "identity_daily_cap": {"minimum": 0, "type": ["integer", "null"]}, "tenant_daily_cap": {"minimum": 0, "type": ["integer", "null"]}, "tenant_monthly_cap": {"minimum": 0, "type": ["integer", "null"]}}, "type": ["object", "null"]}, "spend": {"properties": {"identity_monthly_cap_pence": {"minimum": 0, "type": ["integer", "null"]}, "registration_cap_pence": {"description": "A monthly cap for one-off registration fees (US 10DLC, toll-free verification), which count here instead of the monthly caps.", "minimum": 0, "type": ["integer", "null"]}, "tenant_monthly_cap_pence": {"minimum": 0, "type": ["integer", "null"]}}, "type": ["object", "null"]}, "verification": {"properties": {"redact": {"description": "Codes shown as `[code]` in reads and events. A guard field.", "type": ["boolean", "null"]}}, "type": ["object", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TenantPolicyPatchVaultMinApproval {
    #[serde(rename = "grant")]
    Grant,
    #[serde(rename = "per_run")]
    PerRun,
    #[serde(rename = "per_use")]
    PerUse,
}

fn schema_tenant_policy_patch_vault_min_approval_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["grant", "per_run", "per_use", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatchVault {
    /// Platform keys only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub allowed: Option<Option<bool>>,
    /// A platform key or the partner key of the tenant's own partner, holding `vault:approve`, may approve this tenant's grants and uses, naming the person with `on_behalf_of`. Never a tenant or identity key.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub key_approval: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_vault_min_approval_property)]
    pub min_approval: Option<Option<TenantPolicyPatchVaultMinApproval>>,
    /// Withholds every verification message (mail and SMS) from agents and delivers codes only into vault phases. Guard field; `true` is the stricter value.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub codes_into_fills_only: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_items: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub session_max_ttl_days: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub approval_deadline_seconds: Option<Option<i64>>,
}

fn schema_tenant_policy_patch_vault_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"allowed": {"description": "Platform keys only.", "type": ["boolean", "null"]}, "approval_deadline_seconds": {"maximum": 900, "minimum": 60, "type": ["integer", "null"]}, "codes_into_fills_only": {"description": "Withholds every verification message (mail and SMS) from agents and delivers codes only into vault phases. Guard field; `true` is the stricter value.", "type": ["boolean", "null"]}, "key_approval": {"description": "A platform key or the partner key of the tenant's own partner, holding `vault:approve`, may approve this tenant's grants and uses, naming the person with `on_behalf_of`. Never a tenant or identity key.", "type": ["boolean", "null"]}, "max_items": {"maximum": 10000, "minimum": 1, "type": ["integer", "null"]}, "min_approval": {"enum": ["grant", "per_run", "per_use", null], "type": ["string", "null"]}, "session_max_ttl_days": {"maximum": 30, "minimum": 1, "type": ["integer", "null"]}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

/// A partial `TenantPolicy`, deep-merged over the current policy. Any field set to `null` resets to its default (for `retention.message_days` and `triage.categories`, whose default is `null`, this is the same as setting `null`). Arrays replace, they do not merge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyPatch {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub identity_daily_send_cap: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub tenant_daily_send_cap: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub max_recipients: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub send_allowlist_only: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_large_attachments_property)]
    pub large_attachments: Option<Option<TenantPolicyPatchLargeAttachments>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub link_ttl_hours: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_ai_disclosure_property)]
    pub ai_disclosure: Option<Option<TenantPolicyPatchAiDisclosure>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_auto_reply_property)]
    pub auto_reply: Option<Option<TenantPolicyPatchAutoReply>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_quarantine_property)]
    pub quarantine: Option<Option<TenantPolicyPatchQuarantine>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_inbound_property)]
    pub inbound: Option<Option<TenantPolicyPatchInbound>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_retention_property)]
    pub retention: Option<Option<TenantPolicyPatchRetention>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_triage_property)]
    pub triage: Option<Option<TenantPolicyPatchTriage>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_search_property)]
    pub search: Option<Option<TenantPolicyPatchSearch>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub webhook_text_bytes: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_abuse_property)]
    pub abuse: Option<Option<TenantPolicyPatchAbuse>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_domains_property)]
    pub domains: Option<Option<TenantPolicyPatchDomains>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_events_property)]
    pub events: Option<Option<TenantPolicyPatchEvents>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<bool>)]
    pub domain_fallback: Option<Option<bool>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_web_bot_auth_property)]
    pub web_bot_auth: Option<Option<TenantPolicyPatchWebBotAuth>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_accounts_property)]
    pub accounts: Option<Option<TenantPolicyPatchAccounts>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_drafts_property)]
    pub drafts: Option<Option<TenantPolicyPatchDrafts>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_screening_property)]
    pub screening: Option<Option<TenantPolicyPatchScreening>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_phone_property)]
    pub phone: Option<Option<TenantPolicyPatchPhone>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_tenant_policy_patch_vault_property)]
    pub vault: Option<Option<TenantPolicyPatchVault>>,
}

/// A tenant's effective policy and, for each field, what the calling key may do with it (docs › Workspace policy). Clients build forms from `fields` so they never send a value the write would refuse.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPolicyView {
    pub tenant_id: TenantId,
    /// Rises by one with every policy write; the console uses it to detect a stale form.
    pub policy_version: i64,
    pub policy: TenantPolicy,
    pub fields: Vec<PolicyFieldAccess>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PolicyFieldAccessClass {
    #[serde(rename = "platform_only")]
    PlatformOnly,
    #[serde(rename = "partner_or_platform")]
    PartnerOrPlatform,
    #[serde(rename = "lower_only")]
    LowerOnly,
    #[serde(rename = "guard")]
    Guard,
    #[serde(rename = "free")]
    Free,
}

fn schema_policy_field_access_class_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`platform_only`, `partner_or_platform` (`quarantine.key_release`, `drafts.key_approval`), `lower_only`, `guard` or `free` (docs › Configuration › Who may change a field).", "enum": ["platform_only", "partner_or_platform", "lower_only", "guard", "free"], "type": "string"}"###).expect("the contract's schema")
}

fn schema_policy_field_access_ceiling_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "For a lower-only field, the loosest value the calling key may set (a number, or a boolean for a switch); `null` when no ceiling binds the key or the field is not lower-only.", "type": ["number", "boolean", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PolicyFieldAccessCeilingSource {
    #[serde(rename = "deployment")]
    Deployment,
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "partner")]
    Partner,
}

fn schema_policy_field_access_ceiling_source_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Which bound decided `ceiling`.", "enum": ["deployment", "platform", "partner", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PolicyFieldAccess {
    /// The dotted path of the field.
    pub field: String,
    /// `platform_only`, `partner_or_platform` (`quarantine.key_release`, `drafts.key_approval`), `lower_only`, `guard` or `free` (docs › Configuration › Who may change a field).
    #[schema(schema_with = schema_policy_field_access_class_property)]
    pub class: PolicyFieldAccessClass,
    /// Whether the calling key may set the field to any value at all.
    pub writable: bool,
    /// For a lower-only field, the loosest value the calling key may set (a number, or a boolean for a switch); `null` when no ceiling binds the key or the field is not lower-only.
    #[schema(required = true, schema_with = schema_policy_field_access_ceiling_property)]
    pub ceiling: Option<serde_json::Value>,
    /// Which bound decided `ceiling`.
    #[schema(required = true, schema_with = schema_policy_field_access_ceiling_source_property)]
    pub ceiling_source: Option<PolicyFieldAccessCeilingSource>,
    /// For a guard field: `true` when the calling key may only tighten it, because API keys may not take decisions reserved for people on this tenant.
    pub person_required_to_loosen: bool,
}

/// A custom triage category. The description is given to the triage model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TriageCategoryDefinition {
    pub name: String,
    pub description: String,
}

/// A deterministic tenant triage rule (`policy.triage.rules`, at most 50), as in the Triage design (`project/design/triage.md`, sections 4 and 5) and the Triage guide. Tenant rules run first, in array order, before the built-in rules and the model (FR-TRI-2). For every rule whose `match` holds: `category` and `needs_reply` are set by the first rule that sets them (first match wins), `urgency_min` takes the highest value, `labels_add` accumulates (at most 10 in all), and `skip_model` is on if any matching rule turns it on. A matching rule with `stop: true` ends tenant rules. Tenant rules cannot add or remove risk flags. With `skip_model`, triage ends with a rules-only record (`model: "rules"`); otherwise the values set here are fixed and the model fills the rest. Strings are NFKC-folded and lower-cased when the policy is written. Rules apply to mail that arrives afterwards; re-run triage to apply them to a message already triaged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TriageRule {
    /// Unique in the list. Recorded (stored only) in the IDs of the rules that matched.
    pub id: String,
    pub r#match: TriageRuleMatch,
    pub set: TriageRuleSet,
    /// `true` ends tenant rules when this rule matches. Built-in rules still run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub stop: Option<bool>,
}

/// The conditions of a tenant triage rule. At least one field. **All** listed fields must hold; within a field's list, **any** value may match. Each list has at most 20 entries and each string at most 200 characters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TriageRuleMatch {
    /// The From address is one of these exact addresses (case-insensitive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<EmailAddress>)]
    pub from: Option<Vec<EmailAddress>>,
    /// The From address is at one of these domains or any of their subdomains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub from_domain: Option<Vec<String>>,
    /// The message was delivered to one of these identities of the tenant (identity IDs or usernames).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub to_identity: Option<Vec<String>>,
    /// The folded subject contains one of these strings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub subject_contains: Option<Vec<String>>,
    /// The first 64 KB of `extracted_text` contains one of these strings (case-insensitive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub body_contains: Option<Vec<String>>,
    /// A non-inline attachment exists (`true`) or none does (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub has_attachment: Option<bool>,
    /// The message carries one of these labels at triage time, including labels added by earlier tenant rules in the same run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub label: Option<Vec<Label>>,
}

/// The triage fields a matching tenant rule sets. At least one field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TriageRuleSet {
    /// Must be in the effective category list (the built-in categories, or the tenant's custom ones).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<Label>)]
    pub labels_add: Option<Vec<Label>>,
    /// The lowest urgency the message can get; the model may raise it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub urgency_min: Option<i64>,
    /// Fixes `needs_reply`; the model's value is not used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = f64)]
    pub needs_reply: Option<f64>,
    /// `true` finishes triage with a rules-only record, without the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub skip_model: Option<bool>,
}

/// A tenant-defined reference pattern, matched exactly at ingest and searchable with `ref:` (stored with kind `custom:<name>`). Patterns use the Rust `regex` crate syntax: linear time, no back-references, compiled size capped at 64 KB.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CustomRefPattern {
    pub name: String,
    pub pattern: String,
    /// How matched values are normalised before storage and comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub normalise: Option<String>,
}

/// `metered` (the plan's allowances plus top-ups), `exempt` (no limits) or `disabled` (self-hosted without billing; only the daily caps in tenant policy apply).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum BillingMode {
    #[serde(rename = "metered")]
    Metered,
    #[serde(rename = "exempt")]
    Exempt,
    #[serde(rename = "disabled")]
    Disabled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TenantPage {
    pub data: Vec<Tenant>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PhoneNumberType {
    #[serde(rename = "mobile")]
    Mobile,
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "national")]
    National,
    #[serde(rename = "toll_free")]
    TollFree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum VaultApprovalLevel {
    #[serde(rename = "grant")]
    Grant,
    #[serde(rename = "per_run")]
    PerRun,
    #[serde(rename = "per_use")]
    PerUse,
}
