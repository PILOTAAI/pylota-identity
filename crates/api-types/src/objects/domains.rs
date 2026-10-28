//! Objects first used by the `domains` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// How the domain is connected (FR-DOM-7). The method fixes `kind`, `inbound` and `transport`; `platform`
/// is the deployment's shared domain only.
///
/// | `method` | The customer changes | `kind` | `inbound` | `transport` |
/// |---|---|---|---|---|
/// | `cloudflare_zone` | Nothing: the zone is in this Cloudflare account and the Worker writes the records | `zone` | `routing` | `cloudflare` |
/// | `nameservers` | Two NS records at the registrar, for a domain used only for mail | `zone` | `routing` | `cloudflare` |
/// | `dns_records` | One MX, three DKIM CNAMEs, a MAIL FROM MX and TXT, and the ownership TXT, at any DNS host | `external` | `ses` | `ses` |
/// | `send_only` | Three DKIM CNAMEs, a MAIL FROM MX and TXT, and the ownership TXT; their own mailbox forwards to the agent | `external` | `forward` | `ses` |
/// | `smtp_relay` | The ownership TXT, plus what their own mail provider already needs | `external` | `forward` or `ses` | `smtp` |
/// | `delegated_subdomain` | NS records for one subdomain | `delegated` | `routing` | `cloudflare` |
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainMethod {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "cloudflare_zone")]
    CloudflareZone,
    #[serde(rename = "nameservers")]
    Nameservers,
    #[serde(rename = "dns_records")]
    DnsRecords,
    #[serde(rename = "send_only")]
    SendOnly,
    #[serde(rename = "smtp_relay")]
    SmtpRelay,
    #[serde(rename = "delegated_subdomain")]
    DelegatedSubdomain,
}

/// Where the domain's DNS lives relative to this deployment. `platform` is the deployment's shared domain; `zone` a zone in this Cloudflare account; `delegated` a child zone for a subdomain whose parent stays at another DNS host; `external` DNS at any other host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainKind {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "zone")]
    Zone,
    #[serde(rename = "delegated")]
    Delegated,
    #[serde(rename = "external")]
    External,
}

/// How mail to the domain's addresses reaches identities: `routing` (Cloudflare Email Routing), `ses` (Amazon SES receiving, through `/hooks/ses/inbound`), `forward` (the customer's own mailbox forwards to each identity's platform address) or `none`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainInbound {
    #[serde(rename = "routing")]
    Routing,
    #[serde(rename = "ses")]
    Ses,
    #[serde(rename = "forward")]
    Forward,
    #[serde(rename = "none")]
    None,
}

/// `pending` → `verifying` → `healthy` ⇄ `degraded` → `failing` → `suspended`; `removing` → `removed` after deletion. A return to `healthy` emits `domain.recovered`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainState {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "verifying")]
    Verifying,
    #[serde(rename = "healthy")]
    Healthy,
    #[serde(rename = "degraded")]
    Degraded,
    #[serde(rename = "failing")]
    Failing,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "removing")]
    Removing,
    #[serde(rename = "removed")]
    Removed,
}

/// `catch_all` (zone apex, or SES receiving), `literal` (one routing rule per address, zone subdomains) or `forward` (forwarding domains).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RoutingMode {
    #[serde(rename = "catch_all")]
    CatchAll,
    #[serde(rename = "literal")]
    Literal,
    #[serde(rename = "forward")]
    Forward,
}

/// How mail from the domain's addresses is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum Transport {
    #[serde(rename = "cloudflare")]
    Cloudflare,
    #[serde(rename = "ses")]
    Ses,
    #[serde(rename = "smtp")]
    Smtp,
}

/// `subaddress` puts the thread token in the `Reply-To` sub-address (FR-OUT-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ReplyTokenMode {
    #[serde(rename = "subaddress")]
    Subaddress,
    #[serde(rename = "none")]
    None,
}

/// SMTP relay settings (`smtp_relay` only). The Worker connects once (EHLO, STARTTLS, AUTH, QUIT) before it stores them. The credentials are sealed under `PM_MASTER_KEY` and never returned, logged or exported.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SmtpSettings {
    /// A DNS name, not an IP literal. It must resolve to public addresses.
    pub host: String,
    /// `465` (implicit TLS) or `587` (STARTTLS). Any other port, `25` included, returns `400 smtp_port_not_allowed`.
    pub port: i64,
    pub username: String,
    pub password: String,
    /// An address the relay accepts as sender, used by the alignment probe. Defaults to `postmaster@{name}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub probe_from: Option<String>,
}

fn schema_domain_smtp_port_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": [465, 587], "type": "integer"}"###)
        .expect("the contract's schema")
}

/// The relay settings as stored, never the password.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainSmtp {
    pub host: String,
    #[schema(schema_with = schema_domain_smtp_port_property)]
    pub port: i64,
    pub username: String,
    pub probe_from: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainProbeResult {
    #[serde(rename = "pass")]
    Pass,
    #[serde(rename = "smtp_unaligned")]
    SmtpUnaligned,
    #[serde(rename = "smtp_from_rewritten")]
    SmtpFromRewritten,
    #[serde(rename = "smtp_probe_timeout")]
    SmtpProbeTimeout,
    #[serde(rename = "smtp_auth_failed")]
    SmtpAuthFailed,
    #[serde(rename = "smtp_tls_required")]
    SmtpTlsRequired,
}

fn schema_domain_probe_result_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "`pass`, or the issue code of the failure. `null` before the first probe.", "enum": ["pass", "smtp_unaligned", "smtp_from_rewritten", "smtp_probe_timeout", "smtp_auth_failed", "smtp_tls_required", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

/// The last alignment probe of an `smtp` domain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainProbe {
    /// `null` before the first probe.
    #[schema(required = true)]
    pub last_at: Option<String>,
    /// `pass`, or the issue code of the failure. `null` before the first probe.
    #[schema(required = true, schema_with = schema_domain_probe_result_property)]
    pub result: Option<DomainProbeResult>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainDeliveryEvents {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "none")]
    None,
}

fn schema_domain_delivery_events_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`active`: provider delivery events reach the service. `manual`: a Cloudflare-transport domain was created without an Email Sending event subscription (spike S9 fallback); until `idvy domains subscribe <domain>` has run, its sends go out from each identity's platform address (`sent_via_fallback`), whose delivery events are seen ([H17]). `none`: `sending` is false.", "enum": ["active", "manual", "none"], "type": "string"}"###).expect("the contract's schema")
}

/// `null`, or the operator step that remains while `delivery_events` is `manual`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainDetails {
    pub action: String,
}

fn schema_domain_details_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`null`, or the operator step that remains while `delivery_events` is `manual`.", "oneOf": [{"properties": {"action": {"examples": ["run idvy domains subscribe agents.brightwell.example"], "type": "string"}}, "required": ["action"], "type": "object"}, {"type": "null"}]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Domain {
    pub id: DomainId,
    /// `null` for the platform domain.
    #[schema(required = true)]
    pub tenant_id: Option<TenantId>,
    /// The domain as an IDNA A-label, lower case.
    pub name: String,
    pub method: DomainMethod,
    pub kind: DomainKind,
    pub inbound: DomainInbound,
    pub transport: Transport,
    pub is_apex: bool,
    pub routing_mode: RoutingMode,
    pub reply_token: ReplyTokenMode,
    pub receiving: bool,
    pub sending: bool,
    /// The region of the domain's SES identity: set when `inbound` or `transport` is `ses`, and on a Cloudflare-method domain that has an SES identity for the [J5] failover; otherwise `null`.
    #[schema(required = true)]
    pub ses_region: Option<String>,
    /// The custom MAIL FROM domain, `pm-bounce.{name}`, of a `dns_records` or `send_only` domain, whose mail SES sends. The local part `pm-bounce` is reserved on such domains. Otherwise `null`, including a Cloudflare-method domain sending through its [J5] failover identity after a `PATCH` to `ses`: that identity has no custom MAIL FROM.
    #[schema(required = true)]
    pub mail_from_domain: Option<String>,
    /// `smtp_relay` only, otherwise `null`. Never the password.
    #[schema(required = true)]
    pub smtp: Option<DomainSmtp>,
    /// `smtp` transport only, otherwise `null`.
    #[schema(required = true)]
    pub probe: Option<DomainProbe>,
    pub state: DomainState,
    /// Machine code for the state, for example `dkim_missing`; `zone_expired` on a `nameservers` domain whose zone Cloudflare deleted; `evicted` or `unverified_expired` on a never-verified domain that is being or was removed.
    #[schema(required = true)]
    pub state_reason: Option<String>,
    pub state_changed_at: String,
    /// When sending from this tenant domain was paused automatically (FR-DLV-6, [G12]); `null` while it may send, and always `null` for the platform domain. Sends from it get `409 sending_paused` and never fall back to the platform domain. Only a platform key can resume it (`sending_paused: false`).
    #[schema(required = true)]
    pub sending_paused_at: Option<String>,
    /// `active`: provider delivery events reach the service. `manual`: a Cloudflare-transport domain was created without an Email Sending event subscription (spike S9 fallback); until `idvy domains subscribe <domain>` has run, its sends go out from each identity's platform address (`sent_via_fallback`), whose delivery events are seen ([H17]). `none`: `sending` is false.
    #[schema(schema_with = schema_domain_delivery_events_property)]
    pub delivery_events: DomainDeliveryEvents,
    /// `null`, or the operator step that remains while `delivery_events` is `manual`.
    #[schema(required = true, schema_with = schema_domain_details_property)]
    pub details: Option<DomainDetails>,
    /// The records last read from the provider APIs, with their last check result.
    pub records: Vec<DnsRecord>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProbeAccepted {
    /// The probe's ID. The result arrives as a domain health change.
    pub probe_id: String,
}

/// `unexpected` means an extra record that conflicts, for example a second SPF record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DnsRecordStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "missing")]
    Missing,
    #[serde(rename = "mismatch")]
    Mismatch,
    #[serde(rename = "unexpected")]
    Unexpected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DnsRecordType {
    #[serde(rename = "TXT")]
    TXT,
    #[serde(rename = "MX")]
    MX,
    #[serde(rename = "CNAME")]
    CNAME,
    #[serde(rename = "NS")]
    NS,
}

fn schema_dns_record_type_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["TXT", "MX", "CNAME", "NS"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DnsRecord {
    #[schema(schema_with = schema_dns_record_type_property)]
    pub r#type: DnsRecordType,
    /// The fully qualified name.
    pub name: String,
    /// The same name relative to the registrable domain (from the Public Suffix List), for DNS hosts that ask for only that part ([N17]).
    pub host: String,
    pub value: String,
    /// What the record is for, for example `ownership`, `mx`, `dkim`, `return_path`, `spf`, `dmarc` or `ns`.
    pub purpose: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = DnsRecordStatus)]
    pub status: Option<DnsRecordStatus>,
    /// The values DNS returned at the last check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub observed: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainRecords {
    pub data: Vec<DnsRecord>,
    pub checked_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainIssue {
    /// Machine code, for example `dkim_missing`.
    pub code: String,
    /// The record name the issue is about.
    pub record: String,
    /// The exact change to make.
    pub fix: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainCheckResolver {
    #[serde(rename = "cloudflare-doh")]
    CloudflareDoh,
    #[serde(rename = "google-doh")]
    GoogleDoh,
}

fn schema_domain_check_resolver_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["cloudflare-doh", "google-doh"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainCheckOutcome {
    #[serde(rename = "pass")]
    Pass,
    #[serde(rename = "degraded")]
    Degraded,
    #[serde(rename = "fail")]
    Fail,
    #[serde(rename = "ownership_changed")]
    OwnershipChanged,
    #[serde(rename = "error")]
    Error,
}

fn schema_domain_check_outcome_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["pass", "degraded", "fail", "ownership_changed", "error"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainCheck {
    pub at: String,
    #[schema(schema_with = schema_domain_check_resolver_property)]
    pub resolver: DomainCheckResolver,
    #[schema(schema_with = schema_domain_check_outcome_property)]
    pub outcome: DomainCheckOutcome,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainHealth {
    pub state: DomainState,
    /// Machine code for the state, for example `dkim_missing`.
    #[schema(required = true)]
    pub reason: Option<String>,
    pub since: String,
    pub issues: Vec<DomainIssue>,
    /// Recent checks, newest first.
    pub checks: Vec<DomainCheck>,
    /// Sends from this domain currently go out from platform addresses (`sent_via_fallback`).
    pub fallback_active: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainPage {
    pub data: Vec<Domain>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
