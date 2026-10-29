//! Objects first used by the `meta` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Tenant ID (`ten_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct TenantId(pub String);

/// Partner ID (`ptn_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct PartnerId(pub String);

/// Identity ID (`idn_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct IdentityId(pub String);

/// Message ID (`msg_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct MessageId(pub String);

/// API key ID (`key_` + ULID). Not secret.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct KeyId(pub String);

/// Request ID (`req_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct RequestId(pub String);

/// `details.reason` of `422 transport_unavailable`.
///
/// | Reason | When |
/// |---|---|
/// | `ses_not_configured` | The SES transport (`PM_SES_*`) is not configured: `dns_records`, `send_only`, or `PATCH` with `transport: ses` |
/// | `ses_receiving_not_configured` | `dns_records`, or `smtp_relay` with `inbound: ses`, without `PM_SES_INBOUND_TOPIC_ARN` (and the bucket and queue) |
/// | `ses_identity_limit` | The SES region already has 10,000 identities, and the method needs one (`dns_records`, `send_only`, `smtp_relay` with `inbound: ses`) |
/// | `subdomain_setup_disabled` | `delegated_subdomain` while `PM_CF_SUBDOMAIN_SETUP` is not `on` |
/// | `zone_creation_not_allowed` | `nameservers` by a tenant or partner key whose tenant's policy lacks `domains.allow_create_zone: true`, or a zone create that Cloudflare refused for the Worker token's scope (a token limited to named zones, as on Pylota Identity Cloud) |
/// | `ses_identity_not_verified` | `PATCH` with `transport: ses` while SES does not report the domain's identity as verified for sending with DKIM `SUCCESS` (`details.dkim_status`) |
/// | `method_not_supported` | The domain's method does not support the operation: `PATCH` with a transport it cannot use, `probe` on a domain whose transport is not `smtp`, `test-forwarding` on an address whose domain does not use `inbound: forward` |
/// | `marketing_needs_ses` | A `kind: marketing` send or reply from a domain whose transport is `cloudflare`, the platform domain included: Cloudflare Email Service is for transactional mail only. Use a domain with the `ses` or `smtp` transport |
/// | `calendar_needs_raw_mime` | A calendar message on the `cloudflare` transport before the raw send is available (spike S1). A calendar message never falls back to the platform domain |
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TransportUnavailableReason {
    #[serde(rename = "ses_not_configured")]
    SesNotConfigured,
    #[serde(rename = "ses_receiving_not_configured")]
    SesReceivingNotConfigured,
    #[serde(rename = "ses_identity_limit")]
    SesIdentityLimit,
    #[serde(rename = "subdomain_setup_disabled")]
    SubdomainSetupDisabled,
    #[serde(rename = "zone_creation_not_allowed")]
    ZoneCreationNotAllowed,
    #[serde(rename = "ses_identity_not_verified")]
    SesIdentityNotVerified,
    #[serde(rename = "method_not_supported")]
    MethodNotSupported,
    #[serde(rename = "marketing_needs_ses")]
    MarketingNeedsSes,
    #[serde(rename = "calendar_needs_raw_mime")]
    CalendarNeedsRawMime,
}

/// A permission a key can hold.
///
/// | Permission | Allows |
/// |---|---|
/// | `tenants:manage` | Create, update and suspend tenants, and their billing accounts. Platform keys, for every tenant; partner keys, for the tenants their partner's keys created, without changing billing |
/// | `identities:read`, `identities:write` | Read, and create, update, pause or delete identities and addresses, and test forwarding; read, and create, rotate or revoke identity signing keys. Deleting an identity also needs `erasure:manage`, because it starts an identity-scope erasure |
/// | `identities:sign` | Mint agent assertions and Web Bot Auth HTTP signatures as an identity. Tenant and identity keys (an identity key only for its own identity); platform and partner keys cannot hold it, but may grant it to the tenant and identity keys they mint |
/// | `domains:read`, `domains:write` | Read, and add, update, verify, probe or remove domains |
/// | `messages:read` | Threads, messages (one at a time or up to 100 with `batch-get`), raw MIME, deliveries, screening feedback |
/// | `messages:send` | Send, reply, reply-all, forward, cancel, calendar replies, and send drafts (with `drafts:read`); create, read and delete uploads (or with `drafts:write`) |
/// | `messages:write` | Labels, read state and archiving, of a message, a thread or up to 100 messages (`batch-update`, with its atomic claim); re-run triage; resolve uncertain sends; screening feedback |
/// | `drafts:read` | List and read drafts and their attachments, and the tenant's queue of drafts awaiting approval, scheduled or failed. Every key level; the tenant queue needs a platform, partner or tenant key |
/// | `drafts:write` | Create, update, request approval for and delete (cancel) drafts; create, read and delete uploads (or with `messages:send`). Includes `drafts:read`. Every key level |
/// | `drafts:approve` | Approve and reject drafts. Includes `drafts:read`. Platform, partner and tenant keys; never identity keys, so an agent cannot approve its own mail. Approving by an API key is also a decision reserved for people: a key approves only while `PM_QUARANTINE_KEY_RELEASE` is `on` or `PM_CONSOLE` is `off`, or where the tenant's `drafts.key_approval` is `true`, which admits only a platform key or the tenant's own partner key; approving with `send: true` also needs `messages:send` |
/// | `attachments:read` | Attachment bytes and extracted text |
/// | `search:read` | Keyword, semantic, hybrid search, contacts, related, wait |
/// | `search:agentic` | Agentic search |
/// | `quarantine:review` | See and release quarantined mail |
/// | `webhooks:read` | Read webhook endpoints and their deliveries |
/// | `webhooks:manage` | Create, change, test, rotate and delete webhook endpoints, and replay. Includes `webhooks:read` |
/// | `events:read` | Read the tenant's event log: the feed, durable event consumers, stream tokens and the WebSocket stream |
/// | `metrics:read` | Pause-window rates and event counts, and the screening summary. Every key level; never implicit |
/// | `keys:manage` | API keys within the caller's scope. A partner key manages only tenant and identity keys of its own tenants |
/// | `erasure:manage` | Erasure requests, legal holds, exports. A `tenant`-scope erasure request also needs `tenants:erase` |
/// | `tenants:erase` | Delete the workspace (a `tenant`-scope erasure request, with `erasure:manage`). Platform keys, partner keys for their own tenants, and tenant keys minted by the workspace owner's console session or by a key descended from one |
/// | `suppressions:manage` | Suppressions, and allow or block lists of the tenant or of one identity |
/// | `usage:read` | Plan, allowances and usage figures. Every tenant and identity key holds it implicitly for its own workspace; platform and partner keys must hold it explicitly and pass `tenant_id` |
/// | `audit:read` | Audit log |
/// | `policy:write` | Read the per-field limits of, and change, the tenant's policy (`getTenantPolicy`, `updateTenantPolicy`) within the key level's rules |
/// | `accounts:request` | Request, list, read and close service-ledger entries |
/// | `accounts:approve` | Approve, reject and delete service-ledger entries; includes `accounts:request`. Approval by an API key is a decision reserved for people |
/// | `members:read` | List console members and pending invitations (tenant, partner and platform keys; every console role holds it) |
/// | `members:manage` | Invite, revoke, change roles and remove console members (tenant, partner and platform keys). Includes `members:read` |
/// | `partners:manage` | Create, list, read, update and delete partners (platform keys only) |
/// | `phone:read` | The catalogue, setup status, budget, numbers, requests, SMS, calls, usage, registrations, consent, charges. An identity key only for its own identity |
/// | `phone:request` | Buy within the budget; search; create and cancel own purchase, port-in and release requests; onboarding drafts; shorten `expires_at`. An identity key only for its own identity |
/// | `phone:send` | Send SMS; resolve uncertain SMS; record an opt-out. An identity key only for its own identity |
/// | `phone:call` | Place calls or create `outbound_call` requests. An identity key only for its own identity |
/// | `phone:approve` | Approve and reject phone requests; decide port-outs (decisions reserved for people) |
/// | `phone:manage` | Number roles and moves; connectors; routes and emergency requests; opt-ins; tighten the budget; extend `expires_at`; tenant reconcile |
/// | `phone:compliance` | The compliance profile and documents; compliance, 10DLC and toll-free requests; the sole-proprietor code |
/// | `vault:read` | Vault item metadata (never values), grants, uses, the tenant's vault status (tenant level and above). An identity key only for its own identity |
/// | `vault:write` | Create, update, archive, restore and rotate vault items; values are write-only; import a session. An identity key only for its own identity's items |
/// | `vault:request` | Ask for vault grants; give up its own identity's grants; P1 `sign` uses |
/// | `vault:use` | Create, cancel and mint tickets for vault uses: the integrator's orchestrator |
/// | `vault:approve` | Approve and deny vault grants and uses, and loosen an item's approval level; a decision reserved for people. Includes `vault:read` |
/// | `vault:admin` | Delete items, manage consumers, rotate the tenant's vault key, change custody, `revoke-all`. Includes `vault:read` |
/// | `platform:ops` | Platform operations: signing-key rotation, the dead-letter queue, maintenance jobs, waitlist invitations, the list of agent sign-ups (platform keys only) |
///
/// The upload routes take either `messages:send` or `drafts:write` (`x-required-permission-any`): a key
/// needs one of them, not both. While a workspace is a provisional agent sign-up, every key of it acts
/// with at most `identities:read`, `messages:read`, `messages:send`, `messages:write`, `attachments:read`,
/// `search:read`, `events:read` and the implicit `usage:read`, whatever it lists; a route outside that
/// set answers `403 permission_denied` with `details.reason = "signup_unverified"`.
///
/// Which key levels can hold a permission. `POST /v1/keys` refuses any other combination with
/// `400 invalid_request` and `details.reason = "permission_not_allowed_for_level"`:
///
/// | Permissions | Key levels |
/// |---|---|
/// | `platform:ops`, `partners:manage` | platform |
/// | `tenants:manage` | platform, partner |
/// | `members:read`, `members:manage`, `suppressions:manage`, `audit:read`, `usage:read`, `policy:write`, `accounts:approve`, `drafts:approve`, `phone:approve`, `phone:manage`, `phone:compliance`, `vault:use`, `vault:approve`, `vault:admin` | platform, partner, tenant |
/// | `tenants:erase` | platform, partner, and tenant keys minted by the workspace owner or its keys (otherwise `details.reason = "permission_owner_only"`) |
/// | `identities:sign` | tenant, identity (platform and partner keys may grant it without holding it) |
/// | Every other permission | platform, partner, tenant, identity |
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum Permission {
    #[serde(rename = "tenants:manage")]
    TenantsManage,
    #[serde(rename = "tenants:erase")]
    TenantsErase,
    #[serde(rename = "partners:manage")]
    PartnersManage,
    #[serde(rename = "identities:read")]
    IdentitiesRead,
    #[serde(rename = "identities:write")]
    IdentitiesWrite,
    #[serde(rename = "identities:sign")]
    IdentitiesSign,
    #[serde(rename = "domains:read")]
    DomainsRead,
    #[serde(rename = "domains:write")]
    DomainsWrite,
    #[serde(rename = "messages:read")]
    MessagesRead,
    #[serde(rename = "messages:send")]
    MessagesSend,
    #[serde(rename = "messages:write")]
    MessagesWrite,
    #[serde(rename = "drafts:read")]
    DraftsRead,
    #[serde(rename = "drafts:write")]
    DraftsWrite,
    #[serde(rename = "drafts:approve")]
    DraftsApprove,
    #[serde(rename = "attachments:read")]
    AttachmentsRead,
    #[serde(rename = "search:read")]
    SearchRead,
    #[serde(rename = "search:agentic")]
    SearchAgentic,
    #[serde(rename = "quarantine:review")]
    QuarantineReview,
    #[serde(rename = "webhooks:read")]
    WebhooksRead,
    #[serde(rename = "webhooks:manage")]
    WebhooksManage,
    #[serde(rename = "events:read")]
    EventsRead,
    #[serde(rename = "metrics:read")]
    MetricsRead,
    #[serde(rename = "keys:manage")]
    KeysManage,
    #[serde(rename = "erasure:manage")]
    ErasureManage,
    #[serde(rename = "suppressions:manage")]
    SuppressionsManage,
    #[serde(rename = "usage:read")]
    UsageRead,
    #[serde(rename = "audit:read")]
    AuditRead,
    #[serde(rename = "members:read")]
    MembersRead,
    #[serde(rename = "members:manage")]
    MembersManage,
    #[serde(rename = "policy:write")]
    PolicyWrite,
    #[serde(rename = "accounts:request")]
    AccountsRequest,
    #[serde(rename = "accounts:approve")]
    AccountsApprove,
    #[serde(rename = "phone:read")]
    PhoneRead,
    #[serde(rename = "phone:request")]
    PhoneRequest,
    #[serde(rename = "phone:send")]
    PhoneSend,
    #[serde(rename = "phone:call")]
    PhoneCall,
    #[serde(rename = "phone:approve")]
    PhoneApprove,
    #[serde(rename = "phone:manage")]
    PhoneManage,
    #[serde(rename = "phone:compliance")]
    PhoneCompliance,
    #[serde(rename = "vault:read")]
    VaultRead,
    #[serde(rename = "vault:write")]
    VaultWrite,
    #[serde(rename = "vault:request")]
    VaultRequest,
    #[serde(rename = "vault:use")]
    VaultUse,
    #[serde(rename = "vault:approve")]
    VaultApprove,
    #[serde(rename = "vault:admin")]
    VaultAdmin,
    #[serde(rename = "platform:ops")]
    PlatformOps,
}

/// From widest to narrowest: `platform` reaches every tenant; `partner` reaches the tenants its partner's keys created and its partner's webhook endpoints (FR-KEY-4); `tenant` reaches its own tenant; `identity` reaches its own identity, plus its tenant's domains read-only with `domains:read` and its tenant's webhook endpoints and deliveries read-only with `webhooks:read`. A route or field that needs a higher level returns `403 scope_denied`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum KeyLevel {
    #[serde(rename = "platform")]
    #[default]
    Platform,
    #[serde(rename = "partner")]
    Partner,
    #[serde(rename = "tenant")]
    Tenant,
    #[serde(rename = "identity")]
    Identity,
}

/// `live` or `test`. Mail a `test` tenant sends never leaves the deployment (FR-TEN-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum Mode {
    #[serde(rename = "live")]
    #[default]
    Live,
    #[serde(rename = "test")]
    Test,
}

/// The key that made the request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Me {
    pub key_id: KeyId,
    pub name: String,
    pub level: KeyLevel,
    pub mode: Mode,
    /// `null` unless the key is partner-level.
    #[schema(required = true)]
    pub partner_id: Option<PartnerId>,
    /// `null` for platform and partner keys.
    #[schema(required = true)]
    pub tenant_id: Option<TenantId>,
    /// `null` unless the key is identity-level.
    #[schema(required = true)]
    pub identity_id: Option<IdentityId>,
    pub permissions: Vec<Permission>,
    pub mcp_tools: McpToolList,
    #[schema(required = true)]
    pub expires_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum HealthStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "degraded")]
    Degraded,
}

fn schema_health_status_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`ok`, or `degraded` when a feature is off because of the configuration; the fields below name it.", "enum": ["ok", "degraded"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum HealthSes {
    #[serde(rename = "sns_topic_missing")]
    SnsTopicMissing,
}

fn schema_health_ses_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present when the SES transport is off because `PM_SES_SNS_TOPIC_ARN` is missing.", "enum": ["sns_topic_missing"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum HealthBilling {
    #[serde(rename = "stripe_secrets_missing")]
    StripeSecretsMissing,
}

fn schema_health_billing_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present when `PM_BILLING=stripe` lacks `PM_STRIPE_SECRET_KEY` or `PM_STRIPE_WEBHOOK_SECRET`, so billing has not started.", "enum": ["stripe_secrets_missing"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum HealthCfApiToken {
    #[serde(rename = "missing")]
    Missing,
}

fn schema_health_cf_api_token_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "Present when a Cloudflare REST fallback is in use (a spike S6 result) and `PM_CF_API_TOKEN` is not set.", "enum": ["missing"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum HealthEnv {
    #[serde(rename = "production")]
    Production,
    #[serde(rename = "staging")]
    Staging,
    #[serde(rename = "local")]
    Local,
}

fn schema_health_env_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`PM_ENV` of the deployment.", "enum": ["production", "staging", "local"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Health {
    /// `ok`, or `degraded` when a feature is off because of the configuration; the fields below name it.
    #[schema(schema_with = schema_health_status_property)]
    pub status: HealthStatus,
    /// Present when the SES transport is off because `PM_SES_SNS_TOPIC_ARN` is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_health_ses_property)]
    pub ses: Option<HealthSes>,
    /// Present when `PM_BILLING=stripe` lacks `PM_STRIPE_SECRET_KEY` or `PM_STRIPE_WEBHOOK_SECRET`, so billing has not started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_health_billing_property)]
    pub billing: Option<HealthBilling>,
    /// Present when a Cloudflare REST fallback is in use (a spike S6 result) and `PM_CF_API_TOKEN` is not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_health_cf_api_token_property)]
    pub cf_api_token: Option<HealthCfApiToken>,
    /// `PM_SES_REGION`, present when SES is configured, so anyone can see where AWS processes mail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub ses_region: Option<String>,
    /// The deployed release, for example `1.0.0`.
    pub version: String,
    /// The short commit hash of the build.
    pub commit: String,
    /// `PM_ENV` of the deployment.
    #[schema(schema_with = schema_health_env_property)]
    pub env: HealthEnv,
    /// Present and `true` while the deployment is frozen for a restore (`PM_FREEZE = "on"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub frozen: Option<bool>,
}

/// Service-ledger entry ID (`sac_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct ServiceAccountId(pub String);

/// `pending_approval` (expires after 7 days), `approved`, `rejected` or `closed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ServiceAccountStatus {
    #[serde(rename = "pending_approval")]
    PendingApproval,
    #[serde(rename = "approved")]
    Approved,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "closed")]
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PauseReason {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "abuse_threshold")]
    AbuseThreshold,
    #[serde(rename = "tenant_suspended")]
    TenantSuspended,
}

/// Inbound: `received`, `quarantined`, `screening` (held until its screening verdict, hidden from agents), `throttled` (over the per-sender limit, hidden from agents) and `hidden` (receive-blocked sender, or a delivery report, kept for audit). Outbound: see `OutboundStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageStatus {
    #[serde(rename = "received")]
    Received,
    #[serde(rename = "quarantined")]
    Quarantined,
    #[serde(rename = "screening")]
    Screening,
    #[serde(rename = "throttled")]
    Throttled,
    #[serde(rename = "hidden")]
    Hidden,
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "submitted")]
    Submitted,
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

/// `editing`, `pending_approval` (waits for an approver), `approved`, `scheduled` (waits for `send_at`), `sending`, `sent` (became a message), `failed` (a send it did not answer synchronously ended without a message; never retried automatically), `rejected` or `canceled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DraftStatus {
    #[serde(rename = "editing")]
    Editing,
    #[serde(rename = "pending_approval")]
    PendingApproval,
    #[serde(rename = "approved")]
    Approved,
    #[serde(rename = "scheduled")]
    Scheduled,
    #[serde(rename = "sending")]
    Sending,
    #[serde(rename = "sent")]
    Sent,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "canceled")]
    Canceled,
}

/// When a send needs approval: `never`; `new_recipients` (a recipient is neither known nor internal to the workspace); `external` (a recipient is not internal); `always`. Ordered from loosest to strictest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RequireApproval {
    #[serde(rename = "never")]
    Never,
    #[serde(rename = "new_recipients")]
    NewRecipients,
    #[serde(rename = "external")]
    External,
    #[serde(rename = "always")]
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum UploadStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "uploading")]
    Uploading,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "expired")]
    Expired,
}

/// An MCP tool name, as listed by `tools/list`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct McpToolName(pub String);

/// The MCP tools the key may see, narrowing what its permissions and level allow. `null` means every allowed tool; `[]` means none. MCP only: REST access follows the permissions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct McpToolList(pub serde_json::Value);

impl utoipa::PartialSchema for McpToolList {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"description": "The MCP tools the key may see, narrowing what its permissions and level allow. `null` means every allowed tool; `[]` means none. MCP only: REST access follows the permissions.", "oneOf": [{"items": {"$ref": "#/components/schemas/McpToolName"}, "maxItems": 64, "type": "array", "uniqueItems": true}, {"type": "null"}]}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for McpToolList {}

/// A metered allowance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum UsageFeatureName {
    #[serde(rename = "inboxes")]
    Inboxes,
    #[serde(rename = "sends")]
    Sends,
    #[serde(rename = "triage")]
    Triage,
    #[serde(rename = "custom_domains")]
    CustomDomains,
    #[serde(rename = "storage_gb")]
    StorageGb,
    #[serde(rename = "seats")]
    Seats,
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for PartnerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for IdentityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for KeyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for ServiceAccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for McpToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
