//! Objects first used by the `addresses` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AddressPage {
    pub data: Vec<Address>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
