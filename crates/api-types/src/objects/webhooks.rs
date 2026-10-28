//! Objects first used by the `webhooks` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Webhook endpoint ID (`whk_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WebhookId(pub String);

/// Webhook delivery attempt ID (`dlv_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DeliveryId(pub String);

/// Event ID (`evt_` + ULID). Also sent as the `webhook-id` header.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventId(pub String);

/// An event type, or `*` for every event type that is not opt-in, including types added later. The opt-in type `message.labels_changed` is delivered only to endpoints that name it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WebhookEventFilter(pub String);

impl utoipa::PartialSchema for WebhookEventFilter {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"description": "An event type, or `*` for every event type that is not opt-in, including types added later. The opt-in type `message.labels_changed` is delivered only to endpoints that name it.", "oneOf": [{"$ref": "#/components/schemas/EventType"}, {"enum": ["*"], "type": "string"}]}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for WebhookEventFilter {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum WebhookScope {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "partner")]
    Partner,
    #[serde(rename = "tenant")]
    Tenant,
}

fn schema_webhook_scope_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Whose events the endpoint receives: `platform`, every tenant's; `partner`, only those of the tenants its partner's keys created; `tenant`, its tenant's. Set by the key that created it.", "enum": ["platform", "partner", "tenant"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum WebhookDisabledReason {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "failing")]
    Failing,
    #[serde(rename = "url_changed")]
    UrlChanged,
    #[serde(rename = "creator_removed")]
    CreatorRemoved,
}

fn schema_webhook_disabled_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`manual`, `failing` (a `410` or 100 failures over 24 hours), `url_changed` (a `PATCH` changed the URL; a passing test delivery re-enables it, [J29]), or `creator_removed` (the person who created it left the workspace or lost `webhooks:manage`; an owner or admin re-enables it, [W47]).", "enum": ["manual", "failing", "url_changed", "creator_removed", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Webhook {
    pub id: WebhookId,
    /// Whose events the endpoint receives: `platform`, every tenant's; `partner`, only those of the tenants its partner's keys created; `tenant`, its tenant's. Set by the key that created it.
    #[schema(schema_with = schema_webhook_scope_property)]
    pub scope: WebhookScope,
    /// The partner of a `partner` endpoint; `null` otherwise.
    #[schema(required = true)]
    pub partner_id: Option<PartnerId>,
    /// `null` for a platform or partner endpoint.
    #[schema(required = true)]
    pub tenant_id: Option<TenantId>,
    pub url: String,
    pub events: Vec<WebhookEventFilter>,
    /// Only events of these identities. `null` means every identity.
    #[schema(required = true)]
    pub identity_ids: Option<Vec<IdentityId>>,
    #[schema(required = true)]
    pub description: Option<String>,
    /// The names of the custom delivery headers, in their stored casing. Values are never returned.
    pub header_names: Vec<String>,
    pub enabled: bool,
    /// `manual`, `failing` (a `410` or 100 failures over 24 hours), `url_changed` (a `PATCH` changed the URL; a passing test delivery re-enables it, [J29]), or `creator_removed` (the person who created it left the workspace or lost `webhooks:manage`; an owner or admin re-enables it, [W47]).
    #[schema(required = true, schema_with = schema_webhook_disabled_reason_property)]
    pub disabled_reason: Option<WebhookDisabledReason>,
    pub created_at: String,
    pub updated_at: String,
}

fn schema_webhook_created_secret_replayed_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present only on an idempotent replay, which carries no secret. Rotate the secret if it was lost.", "enum": [false], "type": "boolean"}"###).expect("the contract's schema")
}

/// An endpoint with its signing secret. The secret is shown only in this response: an idempotent replay (`Idempotent-Replayed: true`) returns the same body without `secret` and with `"secret_replayed": false`, because the stored response never holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WebhookCreated {
    #[serde(flatten)]
    pub base: Webhook,
    /// `whsec_` followed by the base64-encoded signing key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub secret: Option<String>,
    /// Present only on an idempotent replay, which carries no secret. Rotate the secret if it was lost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_webhook_created_secret_replayed_property)]
    pub secret_replayed: Option<bool>,
}

/// Static request headers sent with every delivery: at most 10, values together at most 4,096 bytes, write-only. A name that would change how the request is framed, routed or attributed (the Fetch forbidden request headers, `Proxy-*`, `Sec-*`, `Content-Type`, `Content-Encoding`, `User-Agent`, `webhook-*`, `Forwarded`, `X-Real-IP`, `True-Client-IP`, `X-Forwarded-*`, `CF-*`, `X-HTTP-Method`, `X-HTTP-Method-Override`, `X-Method-Override`, `X-Pylota-*`) is `400 header_not_allowed`; the schema leaves names unconstrained so that this code, not a schema error, names it. `Authorization` is allowed, and so are `CF-Access-Client-Id` and `CF-Access-Client-Secret` (a Cloudflare Access service token). Too many or too long: `422 webhook_header_limit_reached`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebhookHeaderMap(pub BTreeMap<String, String>);

impl utoipa::PartialSchema for WebhookHeaderMap {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"additionalProperties": {"maxLength": 1024, "minLength": 1, "pattern": "^[\\x21-\\x7E]([\\x20-\\x7E]*[\\x21-\\x7E])?$", "type": "string"}, "description": "Static request headers sent with every delivery: at most 10, values together at most 4,096 bytes, write-only. A name that would change how the request is framed, routed or attributed (the Fetch forbidden request headers, `Proxy-*`, `Sec-*`, `Content-Type`, `Content-Encoding`, `User-Agent`, `webhook-*`, `Forwarded`, `X-Real-IP`, `True-Client-IP`, `X-Forwarded-*`, `CF-*`, `X-HTTP-Method`, `X-HTTP-Method-Override`, `X-Method-Override`, `X-Pylota-*`) is `400 header_not_allowed`; the schema leaves names unconstrained so that this code, not a schema error, names it. `Authorization` is allowed, and so are `CF-Access-Client-Id` and `CF-Access-Client-Secret` (a Cloudflare Access service token). Too many or too long: `422 webhook_header_limit_reached`.", "maxProperties": 10, "type": "object"}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for WebhookHeaderMap {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum WebhookDeliveryStatus {
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "dead")]
    Dead,
}

/// One delivery attempt of one event to one endpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WebhookDelivery {
    pub id: DeliveryId,
    pub webhook_id: WebhookId,
    pub event_id: EventId,
    pub event_type: EventType,
    pub attempt: i64,
    pub status: WebhookDeliveryStatus,
    /// The endpoint's HTTP status, when it answered.
    #[schema(required = true)]
    pub http_status: Option<i64>,
    /// Machine code when the attempt failed without a usable answer, for example `timeout`, `tls`, `dns` or `status_5xx`.
    #[schema(required = true)]
    pub error: Option<String>,
    #[schema(required = true)]
    pub duration_ms: Option<i64>,
    /// When the next retry is scheduled, if any.
    #[schema(required = true)]
    pub next_attempt_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReplayByIds {
    pub event_ids: Vec<EventId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReplayByRange {
    /// Events whose `occurred_at` is at or after this time. Only events at most 30 days old (or `retention.events_days`, if shorter) can be replayed.
    pub since: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub until: Option<String>,
    /// Only events whose latest delivery to this endpoint has this status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = WebhookDeliveryStatus)]
    pub status: Option<WebhookDeliveryStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReplayResponse {
    pub queued: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WebhookPage {
    pub data: Vec<Webhook>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WebhookDeliveryPage {
    pub data: Vec<WebhookDelivery>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
