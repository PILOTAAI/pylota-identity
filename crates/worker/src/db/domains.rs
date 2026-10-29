//! `domains` (Data model § 1): a row by ID, the platform domain and a tenant's rows. M13 writes tenant
//! domains (`domains::onboard`, the monitor and removal).

use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub tenant_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub method: String,
    pub zone_id: Option<String>,
    pub is_apex: i64,
    pub routing_mode: String,
    pub inbound: String,
    pub transport: String,
    pub reply_token: String,
    pub receiving: i64,
    pub sending: i64,
    pub state: String,
    pub state_reason: Option<String>,
    pub state_changed_at: i64,
    pub ownership_token: Option<String>,
    pub ownership_verified_at: Option<i64>,
    pub expected_ns_json: Option<String>,
    pub provider_objects_json: String,
    pub event_subscription_id: Option<String>,
    pub ses_identity: Option<String>,
    pub ses_region: Option<String>,
    pub mail_from_domain: Option<String>,
    pub records_json: String,
    pub monitor_do_id: String,
    pub sending_paused_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The columns every read of a row selects.
#[macro_export]
macro_rules! domain_columns {
    () => {
        "id, tenant_id, name, kind, method, zone_id, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_reason, state_changed_at, ownership_token, ownership_verified_at, expected_ns_json, provider_objects_json, event_subscription_id, ses_identity, ses_region, mail_from_domain, records_json, monitor_do_id, sending_paused_at, created_at, updated_at"
    };
}

pub fn get(id: &str) -> Stmt {
    Stmt::new(
        concat!("SELECT ", domain_columns!(), " FROM domains WHERE id = ?1"),
        vec![id.into()],
    )
}

/// The platform domain (there is one live row).
pub fn platform() -> Stmt {
    Stmt::new(
        concat!(
            "SELECT ",
            domain_columns!(),
            " FROM domains WHERE kind = 'platform' AND state <> 'removed' ORDER BY created_at LIMIT 1"
        ),
        vec![],
    )
}

/// A tenant's domains and the platform domain, newest first.
pub fn of_tenant(tenant_id: &str, keyset: Option<(i64, String)>, limit: u32) -> Stmt {
    let (kc, ki) = keyset.map_or((None, None), |(c, i)| (Some(c), Some(i)));
    Stmt::new(
        concat!(
            "SELECT ",
            domain_columns!(),
            " FROM domains \
         WHERE (tenant_id = ?1 OR kind = 'platform') AND state <> 'removed' \
         AND (?2 IS NULL OR created_at < ?2 OR (created_at = ?2 AND id < ?3)) ORDER BY created_at DESC, id DESC LIMIT ?4"
        ),
        vec![tenant_id.into(), kc.into(), ki.into(), (limit + 1).into()],
    )
}

/// The keys of a published record ([`Domain.records`]).
const RECORD_KEYS: &[&str] = &[
    "type", "name", "host", "value", "purpose", "required", "status",
];

impl Row {
    /// The Domain object.
    pub fn json(&self) -> Value {
        let records: Vec<Value> = serde_json::from_str::<Vec<Value>>(&self.records_json)
            .unwrap_or_default()
            .into_iter()
            .map(|mut r| {
                if let Some(name) = r["name"].as_str() {
                    r["host"] = pylota_identity_core::dns::host(name).into();
                }
                let mut out = serde_json::Map::new();
                for k in RECORD_KEYS {
                    if let Some(v) = r.get(*k) {
                        out.insert((*k).to_owned(), v.clone());
                    }
                }
                Value::Object(out)
            })
            .collect();
        let delivery_events = match (self.sending, self.transport.as_str()) {
            (0, _) => "none",
            (_, "cloudflare") if self.event_subscription_id.is_none() => "manual",
            _ => "active",
        };
        let details = (delivery_events == "manual")
            .then(|| json!({ "action": format!("run idvy domains subscribe {}", self.name) }));
        json!({
            "id": self.id, "tenant_id": self.tenant_id, "name": self.name, "method": self.method,
            "kind": self.kind, "inbound": self.inbound, "transport": self.transport,
            "is_apex": self.is_apex == 1, "routing_mode": self.routing_mode,
            "reply_token": self.reply_token, "receiving": self.receiving == 1,
            "sending": self.sending == 1, "ses_region": self.ses_region,
            "mail_from_domain": self.mail_from_domain, "smtp": null, "probe": null,
            "state": self.state, "state_reason": self.state_reason,
            "state_changed_at": rfc3339(self.state_changed_at),
            "sending_paused_at": self.sending_paused_at.map(rfc3339),
            "delivery_events": delivery_events, "details": details, "records": records,
            "created_at": rfc3339(self.created_at),
        })
    }
}
