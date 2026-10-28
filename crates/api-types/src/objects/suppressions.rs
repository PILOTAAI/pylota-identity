//! Objects first used by the `suppressions` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// A suppressed recipient. Only a keyed hash of the address is stored, so it is shown masked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Suppression {
    pub address_hint: String,
    pub reason: SuppressionReason,
    pub created_at: String,
    /// `null` means permanent.
    #[schema(required = true)]
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SuppressionPage {
    pub data: Vec<Suppression>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
