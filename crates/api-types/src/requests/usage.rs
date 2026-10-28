//! Request bodies of the `usage` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BillingAccountUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = BillingMode)]
    pub mode: Option<BillingMode>,
    /// A complimentary plan from the catalog. Only for workspaces without a Stripe subscription (`409 plan_managed_by_stripe` otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub plan_id: Option<String>,
}
