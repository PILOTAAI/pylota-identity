//! Request bodies of the `addresses` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct AddressCreateRequest {
    /// The local part on the tenant domain. The schema does not constrain it: the service validates it with the username rules for a tenant domain, with a maximum of 40 characters (room for a thread token), and returns their own codes, never a generic `invalid_request`: `400 address_reserved` for a reserved or confusable name, `400 address_unsupported` for any other non-ASCII character, and `400 address_invalid` when the lower-cased form does not match the stored form `^[a-z0-9][a-z0-9._-]{0,39}$` (no `..`, not ending in `.`).
    pub local_part: String,
    pub domain_id: DomainId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct PromoteAddressRequest {
    /// Days the previous primary stays `retiring` before it becomes `retired`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub retire_previous_after_days: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct RetireAddressRequest {
    /// Days the alias stays `retiring`. `0` retires it immediately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub after_days: Option<i64>,
}
