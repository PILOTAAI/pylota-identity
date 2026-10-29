//! Delivery events ([Outbound › Delivery events]): Cloudflare Email Sending events from
//! `pm-delivery-events`, and the simulator's and loopback's `TransportEvent`s from `pm-outbound`, are
//! normalised to one [`DeliveryEvent`], routed by sender to the mailbox (`ApplyDeliveryEvent`), then
//! the suppressions and the abuse windows are written.
//!
//! [Outbound › Delivery events]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_api_types::ids::{self, IdPrefix};
use pylota_identity_core::address::address_hash;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::queues::Incoming;
use pylota_identity_platform::{Clock as _, ControlDb as _, Ids as _, ObjectClient as _, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::mailbox::MailboxRequest;
use crate::quota::{self, Outcome, OutcomeDecision, PauseCause, QuotaRequest};
use crate::rpc::{RpcResult, envelope_for};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Cloudflare,
    Ses,
    Simulator,
    Loopback,
    Dsn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeliveryKind {
    Delivered,
    Deferred,
    Bounced { hard: bool },
    Failed,
    Rejected,
    Complained,
}

/// Every source's event, normalised.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryEvent {
    pub source: EventSource,
    pub event_id: String,
    pub provider_message_id: Option<String>,
    pub sender: String,
    pub recipient: String,
    pub subject: Option<String>,
    pub kind: DeliveryKind,
    pub smtp_code: Option<String>,
    pub enhanced_code: Option<String>,
    pub smtp_response: Option<String>,
    pub occurred_at: i64,
}

/// `ApplyDeliveryEvent`'s answer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    /// `applied`, `duplicate`, `reconciled` or `not_found`.
    pub kind: String,
    pub message_id: Option<String>,
    /// For the abuse windows: `delivered`, `bounced`, `complained` or `other`.
    pub outcome: Option<String>,
    /// `hard_bounce` or `complaint`.
    pub suppress: Option<String>,
    pub first_reply: bool,
}

/// Age below which an event for an unknown message is retried (G8).
const ORPHAN_AFTER_MS: i64 = 900_000;
const RETRY_S: u32 = if cfg!(feature = "itest-hooks") { 1 } else { 60 };

/// A Cloudflare Email Sending event, normalised; `None` for an unknown type.
pub fn from_cloudflare(v: &Value) -> Option<DeliveryEvent> {
    let p = &v["payload"];
    let kind = match v["type"].as_str()? {
        "cf.email.sending.message.delivered" => DeliveryKind::Delivered,
        "cf.email.sending.message.deferred" => DeliveryKind::Deferred,
        "cf.email.sending.message.bounced" => DeliveryKind::Bounced {
            hard: p["bounce"]["type"].as_str() != Some("soft"),
        },
        "cf.email.sending.message.failed" => DeliveryKind::Failed,
        "cf.email.sending.message.rejected" => DeliveryKind::Rejected,
        "cf.email.sending.message.complained" => DeliveryKind::Complained,
        _ => return None,
    };
    let d = &p["delivery"];
    let s = |x: &Value| {
        x.as_str()
            .map(str::to_owned)
            .or_else(|| x.as_i64().map(|n| n.to_string()))
    };
    let response = s(&d["smtpResponse"])
        .or_else(|| s(&p["bounce"]["reason"]))
        .or_else(|| s(&p["failure"]["reason"]))
        .or_else(|| s(&p["rejection"]["reason"]))
        .map(|r| r.chars().take(512).collect());
    let occurred = v["metadata"]["eventTimestamp"]
        .as_str()
        .and_then(pylota_identity_core::days::parse_rfc3339)
        .or_else(|| v["metadata"]["eventTimestamp"].as_i64())
        .unwrap_or(0);
    Some(DeliveryEvent {
        source: EventSource::Cloudflare,
        event_id: p["eventId"].as_str()?.to_owned(),
        provider_message_id: s(&p["messageId"]),
        sender: p["sender"].as_str()?.to_ascii_lowercase(),
        recipient: p["recipient"].as_str()?.to_ascii_lowercase(),
        subject: s(&p["subject"]),
        kind,
        smtp_code: s(&d["smtpStatusCode"]),
        enhanced_code: s(&d["smtpEnhancedStatusCode"]),
        smtp_response: response,
        occurred_at: occurred,
    })
}

/// One `pm-delivery-events` batch.
pub async fn batch<P: Platform>(p: &P, batch: Vec<Incoming<Value>>) {
    for m in batch {
        match from_cloudflare(&m.body) {
            Some(e) => match handle(p, &e).await {
                Handled::Ack => m.ack(),
                Handled::Retry(d) => m.retry(Some(d)),
            },
            None => {
                crate::metrics::count(p, "delivery_unknown_type_total", None, 1);
                m.ack();
            }
        }
    }
}

pub enum Handled {
    Ack,
    Retry(u32),
}

#[derive(Deserialize)]
struct Route {
    identity_id: String,
    tenant_id: String,
    mailbox_do_id: String,
    domain_id: String,
    domain_kind: String,
    is_system: i64,
    quota_do_id: String,
}

/// Steps 2–7 for one event.
pub async fn handle<P: Platform>(p: &P, e: &DeliveryEvent) -> Handled {
    let now = p.clock().now_ms();
    let route: Option<Route> = match p
        .db()
        .first(Stmt::new(
            "SELECT a.identity_id, a.tenant_id, i.mailbox_do_id, a.domain_id, d.kind AS domain_kind, i.is_system, t.quota_do_id FROM addresses a JOIN identities i ON i.id = a.identity_id JOIN domains d ON d.id = a.domain_id JOIN tenants t ON t.id = a.tenant_id WHERE a.address = ?1",
            vec![e.sender.clone().into()],
        ))
        .await
    {
        Ok(r) => r,
        Err(_) => return Handled::Retry(RETRY_S),
    };
    let Some(r) = route else {
        crate::metrics::count(p, "delivery_unroutable_total", None, 1);
        return Handled::Ack;
    };
    let request_id = crate::request_id::new(p);
    let env = envelope_for(
        p,
        Some(&r.tenant_id),
        Some(&r.identity_id),
        &request_id,
        None,
        MailboxRequest::ApplyDeliveryEvent(Box::new(e.clone())),
    );
    let applied: Applied = match p
        .objects()
        .call::<_, Applied>(DoClass::Mailbox, &r.mailbox_do_id, &env)
        .await
    {
        Ok(RpcResult::Ok(a)) => a,
        _ => return Handled::Retry(RETRY_S),
    };
    if applied.kind == "not_found" {
        if now - e.occurred_at < ORPHAN_AFTER_MS {
            return Handled::Retry(RETRY_S);
        }
        crate::metrics::count(p, "delivery_orphaned_total", None, 1);
        return Handled::Ack;
    }
    let Some(message_id) = applied.message_id.clone() else {
        return Handled::Ack;
    };
    // 5. Suppressions.
    if let Some(reason) = &applied.suppress
        && e.source != EventSource::Dsn
        && suppress(
            p,
            &r.tenant_id,
            &r.identity_id,
            &r.mailbox_do_id,
            &e.recipient,
            reason,
            &message_id,
            None,
        )
        .await
        .is_err()
    {
        return Handled::Retry(RETRY_S);
    }
    // 6. The abuse windows.
    if let Some(outcome) = applied
        .outcome
        .as_deref()
        .filter(|_| e.source != EventSource::Dsn)
    {
        let outcome = match outcome {
            "delivered" => Outcome::Delivered,
            "bounced" => Outcome::Bounced,
            "complained" => Outcome::Complained,
            _ => Outcome::Other,
        };
        let req = QuotaRequest::RecordOutcome {
            identity_id: r.identity_id.clone(),
            domain_id: Some(r.domain_id.clone()),
            outcome,
            at: e.occurred_at,
            event_id: e.event_id.clone(),
            counted: !applied.first_reply,
        };
        let Ok(v) = quota::call(p, &r.quota_do_id, &r.tenant_id, &request_id, req).await else {
            return Handled::Retry(RETRY_S);
        };
        let d: OutcomeDecision = serde_json::from_value(v).unwrap_or_default();
        if r.is_system == 0 {
            pause(p, &r, &d, &request_id, now).await;
        }
    }
    Handled::Ack
}

/// The identity, tenant and domain pauses an outcome decided (FR-DLV-3, G12); each change once.
async fn pause<P: Platform>(p: &P, r: &Route, d: &OutcomeDecision, request_id: &str, now: i64) {
    let db = p.db();
    let audit = |action: &str, target_type: &str, target: &str, details: Value| {
        crate::db::audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            Some(&r.tenant_id),
            None,
            action,
            target_type,
            target,
            &details,
            request_id,
            now,
        )
    };
    if d.pause_identity
        && let Ok(m) = db
            .execute(Stmt::new(
                "UPDATE identities SET status = 'paused', pause_reason = 'abuse_threshold', updated_at = ?2 WHERE id = ?1 AND status = 'active'",
                vec![r.identity_id.clone().into(), now.into()],
            ))
            .await
        && m.changes > 0
    {
        let _ = db
            .execute(audit("identity.auto_pause", "identity", &r.identity_id, json!({ "reason": "abuse_threshold" })))
            .await;
        let env = envelope_for(
            p,
            Some(&r.tenant_id),
            Some(&r.identity_id),
            request_id,
            None,
            MailboxRequest::EmitEvent(crate::mailbox::EmitEvent {
                event_id: p.ids().new_id(IdPrefix::Event),
                event_type: "identity.paused".into(),
                occurred_at: now,
                data: json!({ "identity_id": r.identity_id, "reason": "abuse_threshold",
                    "metrics": { "complaint_window": 1000, "bounce_window": 200 } }),
            }),
        );
        let _ = p
            .objects()
            .call::<_, Value>(DoClass::Mailbox, &r.mailbox_do_id, &env)
            .await;
    }
    if let Some(cause) = d.pause_tenant {
        let reason = match cause {
            PauseCause::AbuseThreshold => "abuse_threshold",
            PauseCause::AbuseBurst => "abuse_burst",
        };
        if let Ok(m) = db
            .execute(Stmt::new(
                "UPDATE tenants SET sending_paused_at = ?2, sending_pause_reason = ?3, updated_at = ?2 WHERE id = ?1 AND sending_paused_at IS NULL",
                vec![r.tenant_id.clone().into(), now.into(), reason.into()],
            ))
            .await
            && m.changes > 0
        {
            let _ = db
                .execute(audit("tenant.auto_pause", "tenant", &r.tenant_id, json!({ "reason": reason })))
                .await;
            crate::log::error(
                if reason == "abuse_burst" { "containment_tenant_paused" } else { "sending_pause" },
                &[("tenant", r.tenant_id.clone().into())],
            );
            crate::ops::alerts::report(
                p,
                &format!("sending_pause:{}", r.tenant_id),
                Some(&r.tenant_id),
                None,
                &json!({ "reason": reason }),
                "delivery",
            )
            .await;
        }
    }
    if d.pause_domain
        && r.domain_kind != "platform"
        && let Ok(m) = db
            .execute(Stmt::new(
                "UPDATE domains SET sending_paused_at = ?2, updated_at = ?2 WHERE id = ?1 AND sending_paused_at IS NULL",
                vec![r.domain_id.clone().into(), now.into()],
            ))
            .await
        && m.changes > 0
    {
        let _ = db
            .execute(audit("domain.auto_pause", "domain", &r.domain_id, json!({ "reason": "abuse_threshold" })))
            .await;
        crate::log::error("sending_pause", &[("domain", r.domain_id.clone().into())]);
    }
    if d.bounce_alert || d.complaint_alert {
        crate::log::warn(
            "deliverability_alert",
            &[("tenant", r.tenant_id.clone().into())],
        );
    }
}

/// A store did not answer; the event is retried.
#[derive(Debug)]
pub struct Unavailable;

/// The masked form of an address: `j***@example.com`.
pub fn hint(address: &str) -> String {
    match address.split_once('@') {
        Some((l, d)) => format!("{}***@{d}", l.chars().next().unwrap_or('*')),
        None => "***".into(),
    }
}

/// The deterministic event ID of `suppression.created`: the time is the row's `created_at`, the random
/// part the first 10 bytes of `HMAC-SHA256(PM_HASH_KEY, "suppression:" + tenant + hash + created_at)`.
pub fn suppression_event_id(
    hash_key: &[u8; 32],
    tenant_id: &str,
    hash: &str,
    created_at: i64,
) -> String {
    let mac = address_hash(
        hash_key,
        &format!("suppression:{tenant_id}{hash}{created_at}"),
    );
    let random = u128::from_str_radix(&mac[..20], 16).unwrap_or(0);
    ids::format(IdPrefix::Event, created_at.max(0) as u64, random)
}

/// Writes a suppression (`INSERT OR IGNORE`) and, when its row names this message, emits
/// `suppression.created` from the message's mailbox. `expires_at` `None` is permanent.
#[allow(clippy::too_many_arguments)]
pub async fn suppress<P: Platform>(
    p: &P,
    tenant_id: &str,
    identity_id: &str,
    mailbox_do_id: &str,
    address: &str,
    reason: &str,
    message_id: &str,
    expires_at: Option<i64>,
) -> Result<(), Unavailable> {
    #[derive(Deserialize)]
    struct S {
        source_message_id: Option<String>,
        created_at: i64,
    }
    let now = p.clock().now_ms();
    let hash = address_hash(&p.secrets().hash_key, address);
    let db = p.db();
    db.execute(Stmt::new(
        "INSERT OR IGNORE INTO suppressions (tenant_id, address_hash, address_hint, reason, source_message_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        vec![tenant_id.into(), hash.clone().into(), hint(address).into(), reason.into(), message_id.into(), now.into(), expires_at.into()],
    ))
    .await
    .map_err(|_| Unavailable)?;
    let row: Option<S> = db
        .first(Stmt::new(
            "SELECT source_message_id, created_at FROM suppressions WHERE tenant_id = ?1 AND address_hash = ?2",
            vec![tenant_id.into(), hash.clone().into()],
        ))
        .await
        .map_err(|_| Unavailable)?;
    let Some(row) = row.filter(|r| r.source_message_id.as_deref() == Some(message_id)) else {
        return Ok(());
    };
    let env = envelope_for(
        p,
        Some(tenant_id),
        Some(identity_id),
        &crate::request_id::new(p),
        None,
        MailboxRequest::EmitEvent(crate::mailbox::EmitEvent {
            event_id: suppression_event_id(&p.secrets().hash_key, tenant_id, &hash, row.created_at),
            event_type: "suppression.created".into(),
            occurred_at: row.created_at,
            data: json!({ "address_hint": hint(address), "reason": reason, "source_message_id": message_id }),
        }),
    );
    match p
        .objects()
        .call::<_, Value>(DoClass::Mailbox, mailbox_do_id, &env)
        .await
    {
        Ok(RpcResult::Ok(_)) => Ok(()),
        _ => Err(Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloudflare_event() {
        let v = json!({ "type": "cf.email.sending.message.bounced", "source": {},
            "payload": { "eventId": "e1", "messageId": "<a@x.example>", "sender": "A@x.example",
              "recipient": "jo@y.example", "terminal": true, "bounce": { "type": "hard", "reason": "550 5.1.1 unknown" },
              "delivery": { "smtpStatusCode": 550, "smtpEnhancedStatusCode": "5.1.1" } },
            "metadata": { "eventTimestamp": "2026-10-28T10:00:00Z" } });
        let e = from_cloudflare(&v).unwrap();
        assert_eq!(e.kind, DeliveryKind::Bounced { hard: true });
        assert_eq!(e.sender, "a@x.example");
        assert_eq!(e.smtp_code.as_deref(), Some("550"));
        assert!(from_cloudflare(&json!({ "type": "cf.email.other" })).is_none());
    }

    #[test]
    fn deterministic_suppression_id() {
        let a = suppression_event_id(&[1; 32], "ten_a", "abc", 1_790_000_000_000);
        assert_eq!(
            a,
            suppression_event_id(&[1; 32], "ten_a", "abc", 1_790_000_000_000)
        );
        assert_ne!(
            a,
            suppression_event_id(&[1; 32], "ten_a", "abd", 1_790_000_000_000)
        );
        assert!(a.starts_with("evt_"));
        assert_eq!(hint("jo@example.com"), "j***@example.com");
    }
}
