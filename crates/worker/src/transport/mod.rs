//! Mail transports ([Outbound › Transports]): one result type for every provider, and the outcome
//! classification of [Outbound › Transport outcome classification]. M9 builds Cloudflare, the simulator
//! and loopback; SES and SMTP arrive with M13.
//!
//! [Outbound › Transports]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md
//! [Outbound › Transport outcome classification]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

pub mod breaker;
pub mod cloudflare;
pub mod loopback;
pub mod simulator;

use pylota_identity_platform::email::OutAttachment;
use serde::{Deserialize, Serialize};

/// The transport call's deadline.
pub const DEADLINE_MS: u32 = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Cloudflare,
    Ses,
    Smtp,
    Simulator,
    Loopback,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Cloudflare => "cloudflare",
            Provider::Ses => "ses",
            Provider::Smtp => "smtp",
            Provider::Simulator => "simulator",
            Provider::Loopback => "loopback",
        }
    }
}

/// What a transport sends: the structured fields, and the composed MIME for raw transports.
#[derive(Clone, Debug, Default)]
pub struct OutgoingMessage {
    pub message_id: String,
    /// `<…>` form of the composed Message-ID.
    pub rfc_message_id: String,
    pub from: (String, String),
    pub reply_to: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text: Option<String>,
    pub html: Option<String>,
    pub headers: Vec<(String, String)>,
    pub attachments: Vec<OutAttachment>,
    pub mime: Vec<u8>,
}

impl OutgoingMessage {
    pub fn recipients(&self) -> impl Iterator<Item = &String> {
        self.to.iter().chain(&self.cc).chain(&self.bcc)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientRefusal {
    pub address: String,
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClass {
    RateLimit,
    Quota,
    Paused,
    Relay,
}

/// The result of one transport call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransportResult {
    Accepted {
        provider_message_id: String,
        rfc_message_id: Option<String>,
        #[serde(default)]
        refused: Vec<RecipientRefusal>,
        #[serde(default)]
        deferred: Vec<String>,
    },
    /// Definitely not sent. `reason`: `provider_validation`, `sender_domain_unavailable`,
    /// `recipient_suppressed_by_provider`.
    Rejected {
        reason: String,
        code: String,
        detail: String,
    },
    /// `E_RECIPIENT_SUPPRESSED`: definitely not sent (G4).
    RecipientSuppressed { code: String },
    RetryLater {
        class: RetryClass,
        code: String,
        #[serde(default)]
        refused: Vec<RecipientRefusal>,
    },
    /// May have been sent. `reason`: `transport_timeout`, `transport_connection_lost`,
    /// `provider_outcome_unknown`.
    Unknown { reason: String, detail: String },
}

impl TransportResult {
    pub fn definitive(&self) -> bool {
        !matches!(self, TransportResult::Unknown { .. })
    }
}

/// The back-off delay of attempt `n` (1-based) of a class, before jitter (seconds).
pub fn backoff_s(class: RetryClass, n: u32) -> i64 {
    match class {
        RetryClass::RateLimit => (10 * i64::from(n)).min(60),
        _ => (60i64 << n.saturating_sub(1).min(20)).min(3_600),
    }
}
