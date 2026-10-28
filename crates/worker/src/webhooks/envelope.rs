//! The event envelope, the `identity_*` and `address_*` payload builders and the `pm-webhooks` message
//! ([Webhooks and events]). They live here because M6's outbox emits identity events before the webhook
//! consumers exist; `webhooks/payloads.rs` (M8) imports them.
//!
//! [Webhooks and events]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use pylota_identity_core::days::rfc3339;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// `api_version` of every envelope.
pub const API_VERSION: &str = "2026-10-01";

/// The owner of an event's sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerKind {
    Mailbox,
    Domain,
    Job,
    Platform,
}

impl OwnerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mailbox => "mailbox",
            Self::Domain => "domain",
            Self::Job => "job",
            Self::Platform => "platform",
        }
    }
}

/// The envelope, serialised once: these exact bytes are signed and sent on every attempt.
pub fn envelope(
    event_id: &str,
    event_type: &str,
    occurred_at: i64,
    tenant_id: Option<&str>,
    identity_id: Option<&str>,
    sequence: Option<i64>,
    data: &Value,
) -> String {
    json!({
        "id": event_id, "type": event_type, "api_version": API_VERSION,
        "occurred_at": rfc3339_ms(occurred_at), "tenant_id": tenant_id, "identity_id": identity_id,
        "sequence": sequence, "data": data,
    })
    .to_string()
}

/// RFC 3339 UTC with milliseconds.
pub fn rfc3339_ms(ms: i64) -> String {
    let s = rfc3339(ms.div_euclid(1000) * 1000);
    format!("{}.{:03}Z", s.trim_end_matches('Z'), ms.rem_euclid(1000))
}

/// `identity.created`: the Identity object.
pub fn identity_created(identity: &Value) -> Value {
    json!({ "identity": identity })
}

/// `identity.updated`: the Identity object and the names of the fields that changed.
pub fn identity_updated(identity: &Value, changed: &[String]) -> Value {
    json!({ "identity": identity, "changed": changed })
}

/// `identity.paused`: `metrics` only for `abuse_threshold`.
pub fn identity_paused(identity_id: &str, reason: &str, metrics: Option<&Value>) -> Value {
    json!({ "identity_id": identity_id, "reason": reason, "metrics": metrics })
}

pub fn identity_resumed(identity_id: &str) -> Value {
    json!({ "identity_id": identity_id })
}

/// `identity.address_added`, `…_activated` and `…_retired`: the Address object.
pub fn address_event(address: &Value) -> Value {
    json!({ "address": address })
}

/// `identity.address_promoted`.
pub fn address_promoted(address: &Value, previous_primary: &Value) -> Value {
    json!({ "address": address, "previous_primary": previous_primary })
}

/// One `pm-webhooks` message: pointers only, never the payload (I5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WebhookJob {
    Fanout {
        v: u8,
        event_id: String,
        event_type: String,
        tenant_id: Option<String>,
        identity_id: Option<String>,
        owner_kind: OwnerKind,
        owner_id: String,
        occurred_at: i64,
    },
    Deliver {
        v: u8,
        event_id: String,
        event_type: String,
        endpoint_id: String,
        tenant_id: Option<String>,
        identity_id: Option<String>,
        owner_kind: OwnerKind,
        owner_id: String,
        attempt: u32,
        first_attempt: u32,
        replay: bool,
    },
}

#[cfg(test)]
mod tests {
    use pylota_identity_api_types::objects::{IdentityPausedData, IdentityResumedData};

    use super::*;

    #[test]
    fn envelope_and_identity_payloads_match_the_contract() {
        let data = identity_paused("idn_01ARYZ6S410000000000000000", "manual", None);
        serde_json::from_value::<IdentityPausedData>(data.clone()).unwrap();
        serde_json::from_value::<IdentityResumedData>(identity_resumed("idn_x")).unwrap();
        let env: Value = serde_json::from_str(&envelope(
            "evt_01ARYZ6S410000000000000000",
            "identity.paused",
            1_469_918_176_385,
            Some("ten_01ARYZ6S410000000000000000"),
            Some("idn_01ARYZ6S410000000000000000"),
            Some(3),
            &data,
        ))
        .unwrap();
        assert_eq!(env["occurred_at"], "2016-07-30T22:36:16.385Z");
        assert_eq!(env["api_version"], API_VERSION);
        serde_json::from_value::<pylota_identity_api_types::objects::EventBase>(env.clone())
            .unwrap();
        assert_eq!(env["sequence"], 3);
    }

    #[test]
    fn fanout_wire_form() {
        let j = WebhookJob::Fanout {
            v: 1,
            event_id: "evt_1".into(),
            event_type: "identity.created".into(),
            tenant_id: Some("ten_1".into()),
            identity_id: Some("idn_1".into()),
            owner_kind: OwnerKind::Mailbox,
            owner_id: "abc".into(),
            occurred_at: 5,
        };
        let v = serde_json::to_value(&j).unwrap();
        assert_eq!(
            (v["kind"].as_str(), v["owner_kind"].as_str()),
            (Some("fanout"), Some("mailbox"))
        );
    }
}
