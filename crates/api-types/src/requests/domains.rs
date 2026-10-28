//! Request bodies of the `domains` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainCreateRequestMethod {
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

fn schema_domain_create_request_method_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["cloudflare_zone", "nameservers", "dns_records", "send_only", "smtp_relay", "delegated_subdomain"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainCreateRequestKind {
    #[serde(rename = "zone")]
    Zone,
    #[serde(rename = "external")]
    External,
}

fn schema_domain_create_request_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"deprecated": true, "description": "Old spelling, used only when `method` is absent: `zone` → `cloudflare_zone`, `external` → `send_only`.", "enum": ["zone", "external"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainCreateRequestInbound {
    #[serde(rename = "forward")]
    Forward,
    #[serde(rename = "ses")]
    Ses,
}

fn schema_domain_create_request_inbound_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`smtp_relay` only, and required there: `forward` (the customer's mailbox forwards) or `ses` (they also publish the SES MX and DKIM records).", "enum": ["forward", "ses"], "type": "string"}"###).expect("the contract's schema")
}

/// `method` is required for new clients. When it is absent, the old `kind` is mapped: `zone` → `cloudflare_zone`, `external` → `send_only`; `kind: zone` with `create_zone: true` is the old spelling of `nameservers`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainCreateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_domain_create_request_method_property)]
    pub method: Option<DomainCreateRequestMethod>,
    /// Old spelling, used only when `method` is absent: `zone` → `cloudflare_zone`, `external` → `send_only`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_domain_create_request_kind_property)]
    pub kind: Option<DomainCreateRequestKind>,
    /// Old spelling: `kind: zone` with `create_zone: true` means `method: nameservers`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub create_zone: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub receiving: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub sending: Option<bool>,
    /// `cloudflare_zone` apex and `dns_records`. A name that already has MX records, none of them the expected host, is refused with `409 existing_mx` unless this is `true` ([H5]). On a zone apex, enabling routing replaces the existing mail provider; on `dns_records` it means "I will replace these", and health reports `mx_unexpected` until the old records are gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub replace_mx: Option<bool>,
    /// `nameservers` only. Confirms that a website or mail on the name may stop. Without it, a name with A, AAAA or MX records, or a `www` CNAME, A or AAAA record, returns `409 domain_not_dedicated` ([N21]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub confirm_dedicated: Option<bool>,
    /// Claims a name that another tenant holds but never verified ([H14]). Publish the TXT record from `details.claim` of the earlier `409 domain_exists` answer, then repeat the request with this set to `true`. When both resolvers show it and the holder never verified the domain, the holder is evicted and the answer is `409 domain_claim_pending` until its removal finishes; then the same request succeeds. Otherwise the answer is the same `409 domain_exists`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub claim: Option<bool>,
    /// `smtp_relay` only, and required there: `forward` (the customer's mailbox forwards) or `ses` (they also publish the SES MX and DKIM records).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_domain_create_request_inbound_property)]
    pub inbound: Option<DomainCreateRequestInbound>,
    /// `smtp_relay` only, and required there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SmtpSettings)]
    pub smtp: Option<SmtpSettings>,
}

fn schema_domain_update_request_sending_paused_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`false` lifts an automatic sending pause of this domain ([G12]), audit-logged. Platform keys only: any other key gets `403 scope_denied` with `details.field = \"sending_paused\"`.", "enum": [false], "type": "boolean"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DomainUpdateRequestTransport {
    #[serde(rename = "cloudflare")]
    Cloudflare,
    #[serde(rename = "ses")]
    Ses,
}

fn schema_domain_update_request_transport_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Platform keys only. The transport that sends as a domain on Cloudflare (the failover of [J5]). `ses` needs the SES transport configured and a verified SES identity for the domain. `dns_records` and `send_only` domains send only through `ses`, `smtp_relay` domains only through `smtp`, and the platform domain only through `cloudflare`; another value returns `422 transport_unavailable` with `details.reason: \"method_not_supported\"`.", "enum": ["cloudflare", "ses"], "type": "string"}"###).expect("the contract's schema")
}

/// `transport` (platform keys only), `smtp` (`smtp_relay` domains), `sending_paused` (platform keys only), or several.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DomainUpdateRequest {
    /// `false` lifts an automatic sending pause of this domain ([G12]), audit-logged. Platform keys only: any other key gets `403 scope_denied` with `details.field = "sending_paused"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_domain_update_request_sending_paused_property)]
    pub sending_paused: Option<bool>,
    /// Platform keys only. The transport that sends as a domain on Cloudflare (the failover of [J5]). `ses` needs the SES transport configured and a verified SES identity for the domain. `dns_records` and `send_only` domains send only through `ses`, `smtp_relay` domains only through `smtp`, and the platform domain only through `cloudflare`; another value returns `422 transport_unavailable` with `details.reason: "method_not_supported"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_domain_update_request_transport_property)]
    pub transport: Option<DomainUpdateRequestTransport>,
    /// `smtp_relay` domains, tenant or platform keys. New relay settings, tested with one connection before they are stored and kept pending until an alignment probe with them passes; until then sends keep using the current values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = SmtpSettings)]
    pub smtp: Option<SmtpSettings>,
}
