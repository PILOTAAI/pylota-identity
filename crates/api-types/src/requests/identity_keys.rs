//! Request bodies of the `identity_keys` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A request for an agent assertion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AssertionRequest {
    /// The verifier's expected audience, a URL or an identifier: 1–256 characters of printable ASCII. Becomes the `aud` claim ([O4]).
    pub audience: String,
    /// Seconds until the token expires ([O5]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub expires_in: Option<i64>,
    /// Copied into the `nonce` claim, for the verifier's own challenge. 1–128 characters of printable ASCII.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub nonce: Option<String>,
    /// Extra claims placed under the `ext` claim: at most 2 KB as JSON, and no member may use a registered or Pylota claim name, so a claim the service sets is never overwritten ([O6]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub ext: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum HttpSignatureRequestComponentsItem {
    #[serde(rename = "@authority")]
    Authority,
    #[serde(rename = "signature-agent")]
    SignatureAgent,
    #[serde(rename = "from")]
    From,
    #[serde(rename = "@method")]
    Method,
    #[serde(rename = "@path")]
    Path,
    #[serde(rename = "@query")]
    Query,
}

fn schema_http_signature_request_components_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"default": ["@authority", "signature-agent", "from"], "description": "The components to sign. `@authority`, `signature-agent` and `from` are always signed; `@method`, `@path` and `@query` may be added. Any other component, or one whose value is not ASCII, is refused with `400 invalid_request` ([O10]).", "items": {"enum": ["@authority", "signature-agent", "from", "@method", "@path", "@query"], "type": "string"}, "type": "array", "uniqueItems": true}"###).expect("the contract's schema")
}

/// A request for Web Bot Auth signature headers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HttpSignatureRequest {
    /// The URL the agent will request: `https` only, at most 2,048 characters. An internationalised host is converted to its A-label for `@authority` ([O10]).
    pub url: String,
    /// The request method, an upper-case token. Signed only if `@method` is in `components`, and then required (`400 invalid_request` without it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub method: Option<String>,
    /// Seconds until the signature expires. Too short an expiry fails in transit ([O11]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub expires_in: Option<i64>,
    /// The components to sign. `@authority`, `signature-agent` and `from` are always signed; `@method`, `@path` and `@query` may be added. Any other component, or one whose value is not ASCII, is refused with `400 invalid_request` ([O10]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_http_signature_request_components_property)]
    pub components: Option<Vec<HttpSignatureRequestComponentsItem>>,
}
