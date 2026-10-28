//! `SimulatorTransport` ([Outbound › Simulator], L2): the local part of a `@simulator.invalid` recipient
//! picks the result and the delivery events, which are queued on `pm-outbound` with a delay.
//!
//! [Outbound › Simulator]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use super::TransportResult;
use crate::consumers::delivery::{DeliveryEvent, DeliveryKind, EventSource};

pub const DOMAIN: &str = "simulator.invalid";

/// The script of one local part: `(delay in seconds, kind, smtp code, enhanced code)`.
pub fn script(
    local: &str,
) -> Vec<(
    u32,
    DeliveryKind,
    Option<&'static str>,
    Option<&'static str>,
)> {
    match local {
        "bounce" => vec![(
            1,
            DeliveryKind::Bounced { hard: true },
            Some("550"),
            Some("5.1.1"),
        )],
        "softbounce" => vec![
            (1, DeliveryKind::Deferred, Some("451"), Some("4.2.0")),
            (
                3,
                DeliveryKind::Bounced { hard: false },
                Some("451"),
                Some("4.2.0"),
            ),
        ],
        "complaint" => vec![
            (1, DeliveryKind::Delivered, Some("250"), None),
            (3, DeliveryKind::Complained, None, None),
        ],
        "deferred" => vec![(1, DeliveryKind::Deferred, Some("451"), Some("4.2.0"))],
        "reject" | "timeout" => Vec::new(),
        _ => vec![(1, DeliveryKind::Delivered, Some("250"), None)],
    }
}

/// The result for the simulator recipients of one message: any `reject` rejects the whole message,
/// else any `timeout` makes it uncertain, else it is accepted.
pub fn result(message_id: &str, recipients: &[&str]) -> TransportResult {
    let local = |a: &&str| a.split('@').next().unwrap_or("").to_owned();
    if recipients.iter().any(|a| local(a) == "reject") {
        return TransportResult::Rejected {
            reason: "provider_validation".into(),
            code: "E_SIMULATED_REJECT".into(),
            detail: "E_SIMULATED_REJECT: the simulator rejected this message".into(),
        };
    }
    if recipients.iter().any(|a| local(a) == "timeout") {
        return TransportResult::Unknown {
            reason: "transport_timeout".into(),
            detail: "simulated timeout".into(),
        };
    }
    TransportResult::Accepted {
        provider_message_id: provider_id(message_id),
        rfc_message_id: None,
        refused: Vec::new(),
        deferred: Vec::new(),
    }
}

/// `test-{message ulid, lower case}`.
pub fn provider_id(message_id: &str) -> String {
    format!(
        "test-{}",
        message_id
            .split_once('_')
            .map_or(message_id, |(_, u)| u)
            .to_ascii_lowercase()
    )
}

/// The scripted events of one recipient, with their delays.
pub fn events(
    message_id: &str,
    sender: &str,
    recipient: &str,
    subject: &str,
    now: i64,
) -> Vec<(u32, DeliveryEvent)> {
    let local = recipient.split('@').next().unwrap_or("");
    script(local)
        .into_iter()
        .enumerate()
        .map(|(n, (delay, kind, code, enhanced))| {
            (
                delay,
                DeliveryEvent {
                    source: EventSource::Simulator,
                    event_id: format!("sim:{message_id}:{recipient}:{n}"),
                    provider_message_id: Some(provider_id(message_id)),
                    sender: sender.to_owned(),
                    recipient: recipient.to_owned(),
                    subject: Some(subject.to_owned()),
                    kind,
                    smtp_code: code.map(str::to_owned),
                    enhanced_code: enhanced.map(str::to_owned),
                    smtp_response: code.map(|c| format!("{c} simulated")),
                    occurred_at: now + i64::from(delay) * 1000,
                },
            )
        })
        .collect()
}
