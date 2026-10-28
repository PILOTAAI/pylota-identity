//! Objects first used by the `well_known` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// A JSON Web Key Set (RFC 7517).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

/// The Web Bot Auth key directory: a JWK Set (RFC 7517) of the deployment's `active` and `retiring` Ed25519 keys, at most three. A verifier identifies a key by its RFC 8037 JWK thumbprint, which is the `keyid` of the signatures made with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HttpMessageSignaturesDirectory {
    pub keys: Vec<DirectoryJwk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DirectoryJwkKty {
    #[serde(rename = "OKP")]
    OKP,
}

fn schema_directory_jwk_kty_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["OKP"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DirectoryJwkCrv {
    #[serde(rename = "Ed25519")]
    Ed25519,
}

fn schema_directory_jwk_crv_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["Ed25519"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// An Ed25519 public key in the Web Bot Auth key directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DirectoryJwk {
    #[schema(schema_with = schema_directory_jwk_kty_property)]
    pub kty: DirectoryJwkKty,
    #[schema(schema_with = schema_directory_jwk_crv_property)]
    pub crv: DirectoryJwkCrv,
    /// The public key, base64url-encoded without padding.
    pub x: String,
}
