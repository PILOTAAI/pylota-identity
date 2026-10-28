//! What a send reads from D1 ([Outbound › Handler], step 6): the identity, its tenant, its addresses
//! with their domains, and, once the mailbox knows the recipients, their suppressions, list entries and
//! (for a test tenant) directory rows. The handler loads the first part; the mailbox the second, because
//! a reply's recipients come from the message it answers.
//!
//! [Outbound › Handler]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

pub mod context;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityCtx {
    pub id: String,
    pub display_name: String,
    pub status: String,
    pub pause_reason: Option<String>,
    pub owner_name: Option<String>,
    pub owner_email: Option<String>,
    pub signature_text: Option<String>,
    pub signature_html: Option<String>,
    pub send_policy: Value,
    pub is_system: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TenantCtx {
    pub id: String,
    pub status: String,
    pub mode: String,
    pub timezone: String,
    pub sending_paused_at: Option<i64>,
    pub sending_pause_reason: Option<String>,
    pub partner_id: Option<String>,
    pub partner_status: Option<String>,
    pub quota_do_id: String,
}

/// One address of the identity with its domain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddressCtx {
    pub address: String,
    pub status: String,
    pub role: String,
    pub domain_id: String,
    pub domain: String,
    pub domain_kind: String,
    pub state: String,
    pub transport: String,
    pub reply_token: String,
    pub sending_paused_at: Option<i64>,
    pub delivery_events: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SendContext {
    pub identity: IdentityCtx,
    pub tenant: TenantCtx,
    /// The tenant's effective policy.
    pub policy: Value,
    pub addresses: Vec<AddressCtx>,
    pub platform_domain: String,
    pub ses_configured: bool,
}

impl SendContext {
    pub fn pol(&self, path: &str) -> Value {
        pylota_identity_core::policy::get_path(&self.policy, path)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// The identity's platform-domain address (any status but retired), for fallback.
    pub fn platform_address(&self) -> Option<&AddressCtx> {
        self.addresses
            .iter()
            .find(|a| a.domain_kind == "platform" && a.status != "retired")
    }

    pub fn primary(&self) -> Option<&AddressCtx> {
        self.addresses.iter().find(|a| a.role == "primary")
    }

    pub fn address(&self, a: &str) -> Option<&AddressCtx> {
        self.addresses.iter().find(|x| x.address == a)
    }
}

/// What D1 says about the recipients of one send.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecipientFacts {
    /// address → suppression reason.
    pub suppressed: Vec<(String, String)>,
    /// address → matching list rows `(identity, block, entry)`.
    pub send_lists: Vec<(String, bool, bool, String)>,
    /// address → (tenant, tenant mode, tenant partner, identity) of an `active`/`retiring` address.
    pub directory: Vec<(String, String, String, Option<String>, String)>,
}

impl RecipientFacts {
    pub fn suppression(&self, address: &str) -> Option<&str> {
        self.suppressed
            .iter()
            .find(|(a, _)| a == address)
            .map(|(_, r)| r.as_str())
    }

    /// The effective send decision for `address`.
    pub fn decision(&self, address: &str) -> Option<pylota_identity_core::lists::ListDecision> {
        let rows: Vec<pylota_identity_core::lists::ListRow> = self
            .send_lists
            .iter()
            .filter(|(a, ..)| a == address)
            .map(
                |(_, identity, block, entry)| pylota_identity_core::lists::ListRow {
                    identity: *identity,
                    block: *block,
                    entry: entry.clone(),
                },
            )
            .collect();
        pylota_identity_core::lists::decide(&rows)
    }
}
