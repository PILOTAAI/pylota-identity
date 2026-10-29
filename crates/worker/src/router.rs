//! The deny-by-default router table ([Security §5.1]). Every route is registered with its method, path
//! pattern, permissions, scope rule, idempotency rule and key levels; the router is built from this table
//! only and matches a request before authenticating it. A path no entry matches is `404
//! route_not_found`; a path matched only by other methods is `405 method_not_allowed` with `Allow`.
//! Neither reads a key or D1.
//!
//! [Security §5.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use pylota_identity_api_types::ErrorCode;
pub use pylota_identity_api_types::openapi::{Idempotency, Method};

use crate::auth::Level;
use crate::errors::ApiError;

/// Who may call a route and how its target is resolved (Security §5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Section 4.7 routes only: no key.
    Public,
    /// `GET /v1/me`.
    AnyKey,
    /// `/v1/platform/*`, `/v1/partners/*`.
    PlatformOnly,
    /// `POST /v1/tenants`, `GET /v1/tenants`.
    PlatformOrPartner,
    /// `/v1/tenants/{tenant_id}/…`.
    TenantPath { param: &'static str },
    /// `/v1/keys/{key_id}` and the other resources: resolved by the handler.
    Resource { param: &'static str },
    /// A collection with an optional `tenant_id` filter (`GET /v1/audit-events`, `GET /v1/keys`).
    TenantFilter,
}

/// The handler a route dispatches to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handler {
    Health,
    OpenApi,
    SecurityTxt,
    Me,
    TenantCreate,
    TenantList,
    TenantGet,
    TenantUpdate,
    PolicyGet,
    PolicyUpdate,
    PartnerCreate,
    PartnerList,
    PartnerGet,
    PartnerUpdate,
    PartnerDelete,
    KeyCreate,
    KeyList,
    KeyGet,
    KeyRevoke,
    KeyRotate,
    AuditList,
    SigningKeyRotate,
    IdentityCreate,
    TenantIdentityList,
    IdentityList,
    IdentityLookup,
    IdentityGet,
    IdentityUpdate,
    IdentityDelete,
    AddressList,
    AddressCreate,
    AddressPromote,
    AddressRetire,
    AddressDelete,
    DomainGet,
    DomainCreate,
    DomainUpdate,
    DomainDelete,
    DomainRecords,
    DomainHealth,
    DomainVerify,
    DomainProbe,
    DomainReprove,
    TenantDomainList,
    ThreadList,
    ThreadGet,
    ThreadUpdate,
    MessageList,
    MessageGet,
    MessageUpdate,
    MessageRaw,
    AttachmentGet,
    AttachmentText,
    QuarantineList,
    QuarantineRelease,
    RerunTriage,
    Wait,
    WebhookCreate,
    WebhookList,
    TenantWebhookList,
    WebhookGet,
    WebhookUpdate,
    WebhookDelete,
    WebhookRotate,
    WebhookTest,
    WebhookDeliveries,
    WebhookReplay,
    Send,
    Reply,
    ReplyAll,
    Forward,
    Cancel,
    Resolve,
    SuppressionList,
    SuppressionCreate,
    SuppressionDelete,
    ListEntries,
    ListEntryGet,
    ListEntryPut,
    ListEntryDelete,
    SignedLink,
    SearchIdentity,
    SearchTenant,
    Related,
    Contacts,
    ErasureCreate,
    ErasureList,
    ErasureGet,
    ErasureReapply,
    HoldSet,
    HoldRemove,
    MessageDelete,
    ExportCreate,
    ExportGet,
    ExportDelete,
    JobCreate,
    JobGet,
    PlatformStatus,
    DlqList,
    DlqRedrive,
}

#[derive(Clone, Copy, Debug)]
pub struct RouteSpec {
    pub method: Method,
    /// Under the host root: `/v1/…` for API routes.
    pub pattern: &'static str,
    /// `openapi.yaml` operationId.
    pub operation_id: &'static str,
    /// All required.
    pub permissions: &'static [&'static str],
    /// Also required when the target tenant is not the key's own (and always for platform and partner
    /// keys).
    pub foreign_permissions: &'static [&'static str],
    pub scope: Scope,
    pub idempotency: Idempotency,
    /// The key levels that may call the route (`x-key-levels`); a lower level gets `403 scope_denied`.
    pub levels: &'static [Level],
    pub handler: Handler,
}

const ALL: &[Level] = &[
    Level::Platform,
    Level::Partner,
    Level::Tenant,
    Level::Identity,
];
const PLATFORM: &[Level] = &[Level::Platform];
const PROVIDERS: &[Level] = &[Level::Platform, Level::Partner];
const TENANT_UP: &[Level] = &[Level::Platform, Level::Partner, Level::Tenant];

/// Registers a route. Every argument is required: a route cannot exist without permissions and a scope
/// rule, and only `Scope::Public`, `Scope::AnyKey` and a route with `foreign_permissions` may have an
/// empty permission list (`router::tests::empty_permissions_only_where_allowed`).
#[allow(clippy::too_many_arguments)]
const fn route(
    method: Method,
    pattern: &'static str,
    operation_id: &'static str,
    permissions: &'static [&'static str],
    foreign_permissions: &'static [&'static str],
    scope: Scope,
    idempotency: Idempotency,
    levels: &'static [Level],
    handler: Handler,
) -> RouteSpec {
    RouteSpec {
        method,
        pattern,
        operation_id,
        permissions,
        foreign_permissions,
        scope,
        idempotency,
        levels,
        handler,
    }
}

use Idempotency as I;
use Method as M;

pub static ROUTES: &[RouteSpec] = &[
    route(
        M::Get,
        "/health",
        "getHealth",
        &[],
        &[],
        Scope::Public,
        I::None,
        &[],
        Handler::Health,
    ),
    route(
        M::Get,
        "/openapi.json",
        "getOpenApiDocument",
        &[],
        &[],
        Scope::Public,
        I::None,
        &[],
        Handler::OpenApi,
    ),
    route(
        M::Get,
        "/.well-known/security.txt",
        "getSecurityTxt",
        &[],
        &[],
        Scope::Public,
        I::None,
        &[],
        Handler::SecurityTxt,
    ),
    route(
        M::Get,
        "/v1/me",
        "getMe",
        &[],
        &[],
        Scope::AnyKey,
        I::None,
        ALL,
        Handler::Me,
    ),
    route(
        M::Post,
        "/v1/tenants",
        "createTenant",
        &["tenants:manage"],
        &[],
        Scope::PlatformOrPartner,
        I::Optional,
        PROVIDERS,
        Handler::TenantCreate,
    ),
    route(
        M::Get,
        "/v1/tenants",
        "listTenants",
        &["tenants:manage"],
        &[],
        Scope::PlatformOrPartner,
        I::None,
        PROVIDERS,
        Handler::TenantList,
    ),
    route(
        M::Get,
        "/v1/tenants/{tenant_id}",
        "getTenant",
        &[],
        &["tenants:manage"],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::TenantGet,
    ),
    route(
        M::Patch,
        "/v1/tenants/{tenant_id}",
        "updateTenant",
        &["tenants:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        PROVIDERS,
        Handler::TenantUpdate,
    ),
    route(
        M::Get,
        "/v1/tenants/{tenant_id}/policy",
        "getTenantPolicy",
        &["policy:write"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::PolicyGet,
    ),
    route(
        M::Patch,
        "/v1/tenants/{tenant_id}/policy",
        "updateTenantPolicy",
        &["policy:write"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::PolicyUpdate,
    ),
    route(
        M::Post,
        "/v1/partners",
        "createPartner",
        &["partners:manage"],
        &[],
        Scope::PlatformOnly,
        I::Optional,
        PLATFORM,
        Handler::PartnerCreate,
    ),
    route(
        M::Get,
        "/v1/partners",
        "listPartners",
        &["partners:manage"],
        &[],
        Scope::PlatformOnly,
        I::None,
        PLATFORM,
        Handler::PartnerList,
    ),
    route(
        M::Get,
        "/v1/partners/{partner_id}",
        "getPartner",
        &["partners:manage"],
        &[],
        Scope::PlatformOnly,
        I::None,
        PLATFORM,
        Handler::PartnerGet,
    ),
    route(
        M::Patch,
        "/v1/partners/{partner_id}",
        "updatePartner",
        &["partners:manage"],
        &[],
        Scope::PlatformOnly,
        I::None,
        PLATFORM,
        Handler::PartnerUpdate,
    ),
    route(
        M::Delete,
        "/v1/partners/{partner_id}",
        "deletePartner",
        &["partners:manage"],
        &[],
        Scope::PlatformOnly,
        I::None,
        PLATFORM,
        Handler::PartnerDelete,
    ),
    route(
        M::Post,
        "/v1/keys",
        "createKey",
        &["keys:manage"],
        &[],
        Scope::TenantFilter,
        I::Optional,
        ALL,
        Handler::KeyCreate,
    ),
    route(
        M::Get,
        "/v1/keys",
        "listKeys",
        &["keys:manage"],
        &[],
        Scope::TenantFilter,
        I::None,
        ALL,
        Handler::KeyList,
    ),
    route(
        M::Get,
        "/v1/keys/{key_id}",
        "getKey",
        &["keys:manage"],
        &[],
        Scope::Resource { param: "key_id" },
        I::None,
        ALL,
        Handler::KeyGet,
    ),
    route(
        M::Delete,
        "/v1/keys/{key_id}",
        "revokeKey",
        &["keys:manage"],
        &[],
        Scope::Resource { param: "key_id" },
        I::None,
        ALL,
        Handler::KeyRevoke,
    ),
    route(
        M::Post,
        "/v1/keys/{key_id}/rotate",
        "rotateKey",
        &["keys:manage"],
        &[],
        Scope::Resource { param: "key_id" },
        I::Optional,
        ALL,
        Handler::KeyRotate,
    ),
    route(
        M::Get,
        "/v1/audit-events",
        "listAuditEvents",
        &["audit:read"],
        &[],
        Scope::TenantFilter,
        I::None,
        TENANT_UP,
        Handler::AuditList,
    ),
    route(
        M::Get,
        "/v1/platform/dlq",
        "listDlqItems",
        &["platform:ops"],
        &[],
        Scope::PlatformOnly,
        I::None,
        PLATFORM,
        Handler::DlqList,
    ),
    route(
        M::Post,
        "/v1/platform/dlq/{dlq_id}/redrive",
        "redriveDlqItem",
        &["platform:ops"],
        &[],
        Scope::PlatformOnly,
        I::Optional,
        PLATFORM,
        Handler::DlqRedrive,
    ),
    route(
        M::Get,
        "/v1/platform/status",
        "getPlatformStatus",
        &["audit:read"],
        &[],
        Scope::PlatformOnly,
        I::None,
        PLATFORM,
        Handler::PlatformStatus,
    ),
    route(
        M::Post,
        "/v1/platform/keys/{purpose}/rotate",
        "rotateSigningKey",
        &["platform:ops"],
        &[],
        Scope::PlatformOnly,
        I::Optional,
        PLATFORM,
        Handler::SigningKeyRotate,
    ),
    route(
        M::Post,
        "/v1/tenants/{tenant_id}/identities",
        "createIdentity",
        &["identities:write"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::Optional,
        TENANT_UP,
        Handler::IdentityCreate,
    ),
    route(
        M::Get,
        "/v1/tenants/{tenant_id}/identities",
        "listTenantIdentities",
        &["identities:read"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::TenantIdentityList,
    ),
    route(
        M::Get,
        "/v1/identities",
        "listIdentities",
        &["identities:read"],
        &[],
        Scope::TenantFilter,
        I::None,
        ALL,
        Handler::IdentityList,
    ),
    // Before `/v1/identities/{identity_id}`: the first matching entry wins.
    route(
        M::Get,
        "/v1/identities/lookup",
        "lookupIdentity",
        &["identities:read"],
        &[],
        Scope::Resource { param: "address" },
        I::None,
        ALL,
        Handler::IdentityLookup,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}",
        "getIdentity",
        &["identities:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::IdentityGet,
    ),
    route(
        M::Patch,
        "/v1/identities/{identity_id}",
        "updateIdentity",
        &["identities:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::IdentityUpdate,
    ),
    route(
        M::Delete,
        "/v1/identities/{identity_id}",
        "deleteIdentity",
        &["identities:write", "erasure:manage"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::IdentityDelete,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/addresses",
        "listAddresses",
        &["identities:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::AddressList,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/addresses",
        "createAddress",
        &["identities:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Optional,
        ALL,
        Handler::AddressCreate,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/addresses/{address_id}/promote",
        "promoteAddress",
        &["identities:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Optional,
        ALL,
        Handler::AddressPromote,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/addresses/{address_id}/retire",
        "retireAddress",
        &["identities:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Optional,
        ALL,
        Handler::AddressRetire,
    ),
    route(
        M::Delete,
        "/v1/identities/{identity_id}/addresses/{address_id}",
        "deleteAddress",
        &["identities:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::AddressDelete,
    ),
    route(
        M::Get,
        "/v1/tenants/{tenant_id}/domains",
        "listTenantDomains",
        &["domains:read"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        ALL,
        Handler::TenantDomainList,
    ),
    route(
        M::Get,
        "/v1/domains/{domain_id}",
        "getDomain",
        &["domains:read"],
        &[],
        Scope::Resource { param: "domain_id" },
        I::None,
        ALL,
        Handler::DomainGet,
    ),
    route(
        M::Post,
        "/v1/tenants/{tenant_id}/domains",
        "createDomain",
        &["domains:write"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::Optional,
        TENANT_UP,
        Handler::DomainCreate,
    ),
    route(
        M::Patch,
        "/v1/domains/{domain_id}",
        "updateDomain",
        &["domains:write"],
        &[],
        Scope::Resource { param: "domain_id" },
        I::None,
        TENANT_UP,
        Handler::DomainUpdate,
    ),
    route(
        M::Delete,
        "/v1/domains/{domain_id}",
        "deleteDomain",
        &["domains:write"],
        &[],
        Scope::Resource { param: "domain_id" },
        I::None,
        TENANT_UP,
        Handler::DomainDelete,
    ),
    route(
        M::Get,
        "/v1/domains/{domain_id}/records",
        "getDomainRecords",
        &["domains:read"],
        &[],
        Scope::Resource { param: "domain_id" },
        I::None,
        ALL,
        Handler::DomainRecords,
    ),
    route(
        M::Post,
        "/v1/domains/{domain_id}/probe",
        "probeDomain",
        &["domains:write"],
        &[],
        Scope::Resource { param: "domain_id" },
        I::Optional,
        TENANT_UP,
        Handler::DomainProbe,
    ),
    route(
        M::Post,
        "/v1/domains/{domain_id}/verify",
        "verifyDomain",
        &["domains:write"],
        &[],
        Scope::Resource { param: "domain_id" },
        I::Optional,
        TENANT_UP,
        Handler::DomainVerify,
    ),
    route(
        M::Get,
        "/v1/domains/{domain_id}/health",
        "getDomainHealth",
        &["domains:read"],
        &[],
        Scope::Resource { param: "domain_id" },
        I::None,
        ALL,
        Handler::DomainHealth,
    ),
    route(
        M::Post,
        "/v1/domains/{domain_id}/reprove",
        "reproveDomain",
        &["domains:write"],
        &[],
        Scope::Resource { param: "domain_id" },
        I::Optional,
        TENANT_UP,
        Handler::DomainReprove,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/threads",
        "listThreads",
        &["messages:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::ThreadList,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/threads/{thread_id}",
        "getThread",
        &["messages:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::ThreadGet,
    ),
    route(
        M::Patch,
        "/v1/identities/{identity_id}/threads/{thread_id}",
        "updateThread",
        &["messages:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::ThreadUpdate,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/messages",
        "listMessages",
        &["messages:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::MessageList,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/messages/{message_id}",
        "getMessage",
        &["messages:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::MessageGet,
    ),
    route(
        M::Patch,
        "/v1/identities/{identity_id}/messages/{message_id}",
        "updateMessage",
        &["messages:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::MessageUpdate,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/messages/{message_id}/raw",
        "getMessageRaw",
        &["messages:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::MessageRaw,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/messages/{message_id}/attachments/{attachment_id}",
        "getAttachment",
        &["attachments:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::AttachmentGet,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/messages/{message_id}/attachments/{attachment_id}/text",
        "getAttachmentText",
        &["attachments:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::AttachmentText,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/messages/{message_id}/triage",
        "rerunTriage",
        &["messages:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Optional,
        ALL,
        Handler::RerunTriage,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/messages/{message_id}/release",
        "releaseMessage",
        &["quarantine:review"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Optional,
        ALL,
        Handler::QuarantineRelease,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/wait",
        "waitForMessage",
        &["search:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::Wait,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/quarantine",
        "listQuarantine",
        &["quarantine:review"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::QuarantineList,
    ),
    route(
        M::Post,
        "/v1/webhooks",
        "createPlatformWebhook",
        &["webhooks:manage"],
        &[],
        Scope::PlatformOrPartner,
        I::Optional,
        PROVIDERS,
        Handler::WebhookCreate,
    ),
    route(
        M::Get,
        "/v1/webhooks",
        "listWebhooks",
        &["webhooks:read"],
        &[],
        Scope::AnyKey,
        I::None,
        ALL,
        Handler::WebhookList,
    ),
    route(
        M::Post,
        "/v1/tenants/{tenant_id}/webhooks",
        "createTenantWebhook",
        &["webhooks:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::Optional,
        TENANT_UP,
        Handler::WebhookCreate,
    ),
    route(
        M::Get,
        "/v1/tenants/{tenant_id}/webhooks",
        "listTenantWebhooks",
        &["webhooks:read"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        ALL,
        Handler::TenantWebhookList,
    ),
    route(
        M::Get,
        "/v1/webhooks/{webhook_id}",
        "getWebhook",
        &["webhooks:read"],
        &[],
        Scope::Resource {
            param: "webhook_id",
        },
        I::None,
        ALL,
        Handler::WebhookGet,
    ),
    route(
        M::Patch,
        "/v1/webhooks/{webhook_id}",
        "updateWebhook",
        &["webhooks:manage"],
        &[],
        Scope::Resource {
            param: "webhook_id",
        },
        I::None,
        TENANT_UP,
        Handler::WebhookUpdate,
    ),
    route(
        M::Delete,
        "/v1/webhooks/{webhook_id}",
        "deleteWebhook",
        &["webhooks:manage"],
        &[],
        Scope::Resource {
            param: "webhook_id",
        },
        I::None,
        TENANT_UP,
        Handler::WebhookDelete,
    ),
    route(
        M::Post,
        "/v1/webhooks/{webhook_id}/rotate-secret",
        "rotateWebhookSecret",
        &["webhooks:manage"],
        &[],
        Scope::Resource {
            param: "webhook_id",
        },
        I::Optional,
        TENANT_UP,
        Handler::WebhookRotate,
    ),
    route(
        M::Post,
        "/v1/webhooks/{webhook_id}/test",
        "testWebhook",
        &["webhooks:manage"],
        &[],
        Scope::Resource {
            param: "webhook_id",
        },
        I::Optional,
        TENANT_UP,
        Handler::WebhookTest,
    ),
    route(
        M::Get,
        "/v1/webhooks/{webhook_id}/deliveries",
        "listWebhookDeliveries",
        &["webhooks:read"],
        &[],
        Scope::Resource {
            param: "webhook_id",
        },
        I::None,
        ALL,
        Handler::WebhookDeliveries,
    ),
    route(
        M::Post,
        "/v1/webhooks/{webhook_id}/replay",
        "replayWebhookEvents",
        &["webhooks:manage"],
        &[],
        Scope::Resource {
            param: "webhook_id",
        },
        I::Optional,
        TENANT_UP,
        Handler::WebhookReplay,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/messages",
        "sendMessage",
        &["messages:send"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Required,
        ALL,
        Handler::Send,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/messages/{message_id}/reply",
        "replyToMessage",
        &["messages:send"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Required,
        ALL,
        Handler::Reply,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/messages/{message_id}/reply-all",
        "replyAllToMessage",
        &["messages:send"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Required,
        ALL,
        Handler::ReplyAll,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/messages/{message_id}/forward",
        "forwardMessage",
        &["messages:send"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Required,
        ALL,
        Handler::Forward,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/messages/{message_id}/cancel",
        "cancelMessage",
        &["messages:send"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Optional,
        ALL,
        Handler::Cancel,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/messages/{message_id}/resolve",
        "resolveMessage",
        &["messages:write"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Optional,
        ALL,
        Handler::Resolve,
    ),
    route(
        M::Get,
        "/v1/tenants/{tenant_id}/suppressions",
        "listSuppressions",
        &["suppressions:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::SuppressionList,
    ),
    route(
        M::Post,
        "/v1/tenants/{tenant_id}/suppressions",
        "createSuppression",
        &["suppressions:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::Optional,
        TENANT_UP,
        Handler::SuppressionCreate,
    ),
    route(
        M::Delete,
        "/v1/tenants/{tenant_id}/suppressions/{address}",
        "deleteSuppression",
        &["suppressions:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::SuppressionDelete,
    ),
    route(
        M::Get,
        "/v1/tenants/{tenant_id}/lists/{direction}/{kind}",
        "listListEntries",
        &["suppressions:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::ListEntries,
    ),
    route(
        M::Get,
        "/v1/tenants/{tenant_id}/lists/{direction}/{kind}/{entry}",
        "getListEntry",
        &["suppressions:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::ListEntryGet,
    ),
    route(
        M::Put,
        "/v1/tenants/{tenant_id}/lists/{direction}/{kind}/{entry}",
        "putListEntry",
        &["suppressions:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::ListEntryPut,
    ),
    route(
        M::Delete,
        "/v1/tenants/{tenant_id}/lists/{direction}/{kind}/{entry}",
        "deleteListEntry",
        &["suppressions:manage"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::ListEntryDelete,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/lists/{direction}/{kind}",
        "listIdentityListEntries",
        &["suppressions:manage"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        TENANT_UP,
        Handler::ListEntries,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/lists/{direction}/{kind}/{entry}",
        "getIdentityListEntry",
        &["suppressions:manage"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        TENANT_UP,
        Handler::ListEntryGet,
    ),
    route(
        M::Put,
        "/v1/identities/{identity_id}/lists/{direction}/{kind}/{entry}",
        "putIdentityListEntry",
        &["suppressions:manage"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        TENANT_UP,
        Handler::ListEntryPut,
    ),
    route(
        M::Delete,
        "/v1/identities/{identity_id}/lists/{direction}/{kind}/{entry}",
        "deleteIdentityListEntry",
        &["suppressions:manage"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        TENANT_UP,
        Handler::ListEntryDelete,
    ),
    route(
        M::Delete,
        "/v1/identities/{identity_id}/messages/{message_id}",
        "deleteMessage",
        &["erasure:manage"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::MessageDelete,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/threads/{thread_id}/hold",
        "setThreadHold",
        &["erasure:manage"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::Optional,
        ALL,
        Handler::HoldSet,
    ),
    route(
        M::Delete,
        "/v1/identities/{identity_id}/threads/{thread_id}/hold",
        "removeThreadHold",
        &["erasure:manage"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::HoldRemove,
    ),
    route(
        M::Post,
        "/v1/erasure-requests",
        "createErasureRequest",
        &["erasure:manage"],
        &[],
        Scope::TenantFilter,
        I::Optional,
        ALL,
        Handler::ErasureCreate,
    ),
    route(
        M::Get,
        "/v1/erasure-requests",
        "listErasureRequests",
        &["erasure:manage"],
        &[],
        Scope::TenantFilter,
        I::None,
        ALL,
        Handler::ErasureList,
    ),
    route(
        M::Get,
        "/v1/erasure-requests/{erasure_id}",
        "getErasureRequest",
        &["erasure:manage"],
        &[],
        Scope::Resource {
            param: "erasure_id",
        },
        I::None,
        ALL,
        Handler::ErasureGet,
    ),
    route(
        M::Post,
        "/v1/platform/erasure-requests/{erasure_id}/reapply",
        "reapplyErasureRequest",
        &["platform:ops", "erasure:manage"],
        &[],
        Scope::PlatformOnly,
        I::Optional,
        PLATFORM,
        Handler::ErasureReapply,
    ),
    route(
        M::Post,
        "/v1/exports",
        "createExport",
        &["erasure:manage", "messages:read", "attachments:read"],
        &[],
        Scope::TenantFilter,
        I::Optional,
        TENANT_UP,
        Handler::ExportCreate,
    ),
    route(
        M::Get,
        "/v1/exports/{export_id}",
        "getExport",
        &["erasure:manage"],
        &[],
        Scope::Resource { param: "export_id" },
        I::None,
        ALL,
        Handler::ExportGet,
    ),
    route(
        M::Delete,
        "/v1/exports/{export_id}",
        "deleteExport",
        &["erasure:manage"],
        &[],
        Scope::Resource { param: "export_id" },
        I::None,
        ALL,
        Handler::ExportDelete,
    ),
    route(
        M::Post,
        "/v1/platform/jobs",
        "createJob",
        &["platform:ops"],
        &[],
        Scope::PlatformOnly,
        I::Optional,
        PLATFORM,
        Handler::JobCreate,
    ),
    route(
        M::Get,
        "/v1/platform/jobs/{job_id}",
        "getJob",
        &["platform:ops"],
        &[],
        Scope::PlatformOnly,
        I::None,
        PLATFORM,
        Handler::JobGet,
    ),
    route(
        M::Get,
        "/v1/links/{token}",
        "getSignedLink",
        &[],
        &[],
        Scope::Public,
        I::None,
        &[],
        Handler::SignedLink,
    ),
    route(
        M::Post,
        "/v1/identities/{identity_id}/search",
        "searchIdentity",
        &["search:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::SearchIdentity,
    ),
    route(
        M::Post,
        "/v1/tenants/{tenant_id}/search",
        "searchTenant",
        &["search:read"],
        &[],
        Scope::TenantPath { param: "tenant_id" },
        I::None,
        TENANT_UP,
        Handler::SearchTenant,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/messages/{message_id}/related",
        "findRelatedMessages",
        &["search:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::Related,
    ),
    route(
        M::Get,
        "/v1/identities/{identity_id}/contacts",
        "searchContacts",
        &["search:read"],
        &[],
        Scope::Resource {
            param: "identity_id",
        },
        I::None,
        ALL,
        Handler::Contacts,
    ),
];

/// A matched route with its path parameters.
#[derive(Debug)]
pub struct Matched {
    pub spec: &'static RouteSpec,
    pub params: Vec<(&'static str, String)>,
}

impl Matched {
    pub fn param(&self, name: &str) -> &str {
        self.params
            .iter()
            .find(|(n, _)| *n == name)
            .map_or("", |(_, v)| v.as_str())
    }
}

fn method_of(name: &str) -> Option<Method> {
    Some(match name {
        "GET" | "HEAD" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "DELETE" => Method::Delete,
        _ => return None,
    })
}

/// A path segment from the URL, percent-decoded; `None` when it does not decode to UTF-8.
fn decode(seg: &str) -> Option<String> {
    let bytes = seg.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = seg.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn match_pattern(pattern: &'static str, path: &str) -> Option<Vec<(&'static str, String)>> {
    let ps: Vec<&str> = pattern.split('/').collect();
    let xs: Vec<&str> = path.split('/').collect();
    if ps.len() != xs.len() {
        return None;
    }
    let mut params = Vec::new();
    for (p, x) in ps.into_iter().zip(&xs) {
        if let Some(name) = p.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
            let v = decode(x)?;
            if v.is_empty() {
                return None;
            }
            params.push((name, v));
        } else if p != *x {
            return None;
        }
    }
    Some(params)
}

/// Matches a method and path (without the query).
pub fn find(method: &str, path: &str) -> Result<Matched, ApiError> {
    let path = if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    };
    let want = method_of(method);
    let mut allow: Vec<&'static str> = Vec::new();
    for spec in ROUTES {
        if let Some(params) = match_pattern(spec.pattern, path) {
            if Some(spec.method) == want {
                return Ok(Matched { spec, params });
            }
            allow.push(match spec.method {
                Method::Get => "GET",
                Method::Post => "POST",
                Method::Put => "PUT",
                Method::Patch => "PATCH",
                Method::Delete => "DELETE",
            });
        }
    }
    if allow.is_empty() {
        Err(ApiError::new(
            ErrorCode::RouteNotFound,
            "No route matches this path.",
        ))
    } else {
        Err(ApiError::new(
            ErrorCode::MethodNotAllowed,
            "This path does not take this method.",
        )
        .header("allow", allow.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use pylota_identity_api_types::openapi::OPERATIONS;

    use super::*;

    /// Every route is an operation of `openapi.yaml`, with its method, idempotency and key levels, and
    /// the permissions it lists (Security §5.1 names the one route that differs, `getTenant`).
    #[test]
    fn table_matches_the_contract() {
        for r in ROUTES {
            let op = OPERATIONS
                .iter()
                .find(|o| o.id == r.operation_id)
                .unwrap_or_else(|| panic!("{} is not an operation", r.operation_id));
            let full = if op.root {
                op.path.to_owned()
            } else {
                format!("/v1{}", op.path)
            };
            assert_eq!(full, r.pattern, "{}", r.operation_id);
            assert_eq!(op.method, r.method, "{}", r.operation_id);
            assert_eq!(op.idempotency, r.idempotency, "{}", r.operation_id);
            let levels: Vec<&str> = r.levels.iter().map(|l| l.as_str()).collect();
            if r.scope != Scope::Public {
                assert_eq!(op.key_levels, levels.as_slice(), "{}", r.operation_id);
            }
            if r.operation_id != "getTenant" {
                assert_eq!(
                    op.permissions.unwrap_or(&[]),
                    r.permissions,
                    "{}",
                    r.operation_id
                );
            }
        }
    }

    #[test]
    fn empty_permissions_only_where_allowed() {
        for r in ROUTES {
            if r.permissions.is_empty() {
                assert!(
                    matches!(r.scope, Scope::Public | Scope::AnyKey)
                        || !r.foreign_permissions.is_empty(),
                    "{} has no permission",
                    r.operation_id
                );
            }
        }
    }

    #[test]
    fn matching_404_and_405() {
        let m = find("GET", "/v1/tenants/ten_01ARYZ6S410000000000000000").unwrap();
        assert_eq!(m.spec.handler, Handler::TenantGet);
        assert_eq!(m.param("tenant_id"), "ten_01ARYZ6S410000000000000000");
        assert_eq!(
            find("GET", "/v1/nope").unwrap_err().code,
            ErrorCode::RouteNotFound
        );
        let e = find("PUT", "/v1/tenants").unwrap_err();
        assert_eq!(e.code, ErrorCode::MethodNotAllowed);
        assert_eq!(e.headers, [("allow".to_owned(), "POST, GET".to_owned())]);
        assert_eq!(
            find("GET", "/v1/tenants/").unwrap().spec.handler,
            Handler::TenantList
        );
        assert_eq!(
            find("GET", "/v1/keys/%ZZ").unwrap_err().code,
            ErrorCode::RouteNotFound
        );
    }
}
