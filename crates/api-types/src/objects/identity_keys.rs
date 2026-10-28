//! Objects first used by the `identity_keys` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum JwkKty {
    #[serde(rename = "OKP")]
    OKP,
}

fn schema_jwk_kty_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["OKP"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum JwkCrv {
    #[serde(rename = "Ed25519")]
    Ed25519,
}

fn schema_jwk_crv_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["Ed25519"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum JwkAlg {
    #[serde(rename = "EdDSA")]
    EdDSA,
}

fn schema_jwk_alg_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["EdDSA"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum JwkUse {
    #[serde(rename = "sig")]
    Sig,
}

fn schema_jwk_use_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["sig"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// An Ed25519 public key (RFC 8037).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Jwk {
    #[schema(schema_with = schema_jwk_kty_property)]
    pub kty: JwkKty,
    #[schema(schema_with = schema_jwk_crv_property)]
    pub crv: JwkCrv,
    /// The public key, base64url-encoded.
    pub x: String,
    pub kid: String,
    #[schema(schema_with = schema_jwk_alg_property)]
    pub alg: JwkAlg,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_jwk_use_property)]
    pub r#use: Option<JwkUse>,
}

/// An identity key's ID: the base64url RFC 7638 thumbprint of its public JWK (RFC 8037, appendix A.3). A deleted identity's key IDs are tombstoned and never published again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityKeyKid(pub String);

/// `active` (signs and is published; at most one per identity), `retiring` (published, does not sign, until `verify_until`) or `retired` (not published; kept until the identity is deleted, so its thumbprint is never reused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum IdentityKeyStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "retiring")]
    Retiring,
    #[serde(rename = "retired")]
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum IdentityKeyAlg {
    #[serde(rename = "EdDSA")]
    EdDSA,
}

fn schema_identity_key_alg_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["EdDSA"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// One Ed25519 signing key of an identity. The private key is sealed under `PM_MASTER_KEY`, never leaves the Worker and is never returned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityKey {
    pub kid: IdentityKeyKid,
    pub identity_id: IdentityId,
    pub status: IdentityKeyStatus,
    #[schema(schema_with = schema_identity_key_alg_property)]
    pub alg: IdentityKeyAlg,
    /// The public key as published in the identity's JWK Set.
    pub public_jwk: Jwk,
    pub created_at: String,
    /// Set when the key becomes `retiring`: the rotation time plus `PM_IDENTITY_KEY_OVERLAP_DAYS` (default 7 days). The key stays in the JWK Set until then, unless it is revoked. `null` while the key is `active`.
    #[schema(required = true)]
    pub verify_until: Option<String>,
    /// When the key became `retired`; `null` before that.
    #[schema(required = true)]
    pub retired_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityKeyPage {
    pub data: Vec<IdentityKey>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

/// The result of rotating an identity key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentityKeyRotation {
    /// The new key, `active`.
    pub key: IdentityKey,
    /// The key that was active, now `retiring` with `verify_until` set; `null` when the identity had no active key and the rotation created the first one.
    #[schema(required = true)]
    pub previous: Option<IdentityKey>,
}

/// A minted agent assertion. Neither the token nor its claims are stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AssertionResponse {
    /// The token, a compact JWS (RFC 7519) with header `{"alg":"EdDSA","typ":"agent-assertion+jwt","kid":…}` and the claims `iss`, `sub`, `aud`, `iat`, `nbf`, `exp`, `jti`, `email`, `email_verified`, `unverified` (`name`, `org`, `accountable_human`: set by the workspace, not checked), `ai_agent`, and `nonce` and `ext` when given.
    pub assertion: String,
    pub kid: IdentityKeyKid,
    /// The `exp` claim.
    pub expires_at: String,
    /// The identity's JWK Set, `https://{PM_API_HOST}/.well-known/jwks/{identity_id}.json`.
    pub jwks_uri: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HttpSignatureResponseHeaders {
    /// The deployment's origin as a structured-field string, in double quotes.
    #[serde(rename = "Signature-Agent")]
    pub signature_agent: String,
    /// The identity's vouched address, as for the assertion's `email` claim.
    #[serde(rename = "From")]
    pub from: EmailAddress,
    /// RFC 9421 signature parameters: the components, `created`, `expires`, `keyid` (the deployment key's JWK thumbprint), `alg="ed25519"`, a base64 `nonce` of 64 random bytes and `tag="web-bot-auth"`.
    #[serde(rename = "Signature-Input")]
    pub signature_input: String,
    /// The RFC 9421 signature, made with the deployment's active `web_bot_auth` key.
    #[serde(rename = "Signature")]
    pub signature: String,
}

fn schema_http_signature_response_headers_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"From": {"$ref": "#/components/schemas/EmailAddress", "description": "The identity's vouched address, as for the assertion's `email` claim."}, "Signature": {"description": "The RFC 9421 signature, made with the deployment's active `web_bot_auth` key.", "type": "string"}, "Signature-Agent": {"description": "The deployment's origin as a structured-field string, in double quotes.", "examples": ["\"https://mail.example.com\""], "type": "string"}, "Signature-Input": {"description": "RFC 9421 signature parameters: the components, `created`, `expires`, `keyid` (the deployment key's JWK thumbprint), `alg=\"ed25519\"`, a base64 `nonce` of 64 random bytes and `tag=\"web-bot-auth\"`.", "type": "string"}}, "required": ["Signature-Agent", "From", "Signature-Input", "Signature"], "type": "object"}"###).expect("the contract's schema")
}

/// The headers to attach to the request. Nothing is stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HttpSignatureResponse {
    #[schema(schema_with = schema_http_signature_response_headers_property)]
    pub headers: HttpSignatureResponseHeaders,
    /// When the signature expires (`expires` in `Signature-Input`).
    pub expires_at: String,
}
