//! Objects first used by the `partners` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// `suspended` refuses every key of the partner, and every API key of its tenants, with `403 partner_suspended`, and holds deliveries to its and its tenants' endpoints; the tenants' status does not change. `deleted` is set by `DELETE /v1/partners/{partner_id}` and is final.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PartnerStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "deleted")]
    Deleted,
}

/// The billing mode given to every tenant a partner key creates (`billing_accounts.mode`). Only a platform key changes a tenant's mode afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum PartnerBillingMode {
    #[serde(rename = "exempt")]
    Exempt,
    #[serde(rename = "metered")]
    Metered,
}

/// An integrator whose partner keys create tenants and act only on those tenants (FR-KEY-4). A partner holds nothing but its name and these settings. A deleted partner stays, with `status: "deleted"` and an empty `name`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Partner {
    pub id: PartnerId,
    /// `""` once the partner is deleted.
    pub name: String,
    pub status: PartnerStatus,
    pub default_billing_mode: PartnerBillingMode,
    pub max_tenants: PartnerMaxTenants,
    pub ramp_exempt: PartnerRampExempt,
    pub created_at: String,
    pub updated_at: String,
}

/// The most tenants that are not `erased` the partner may have; the next `POST /v1/tenants` by a partner key gets `403 partner_tenant_limit`. Default 25: with the default `tenant_daily_send_cap` of 5,000, one partner can send at most 125,000 messages a day (1,250 while its new tenants are on the send ramp). Lowering it below the current count refuses new tenants and changes no existing one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PartnerMaxTenants(pub i64);

/// `true` lets the partner's new tenants skip the new-workspace send ramp (a tenant daily cap of 50 for at least 7 days). `false`, the default: they follow it whatever their billing mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PartnerRampExempt(pub bool);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PartnerPage {
    pub data: Vec<Partner>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
