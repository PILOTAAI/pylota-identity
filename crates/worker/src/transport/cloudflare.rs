//! `CloudflareTransport`: the `EMAIL` binding's structured send, and the classification of its codes
//! (Workers API page of Email Service, read 2026-10-09).

use pylota_identity_platform::Platform;
use pylota_identity_platform::email::{MailSender as _, SendError, StructuredEmail};

use super::{DEADLINE_MS, OutgoingMessage, RetryClass, TransportResult};

/// Codes that mean the request itself was refused (G10).
const VALIDATION: &[&str] = &[
    "E_VALIDATION_ERROR",
    "E_FIELD_MISSING",
    "E_TOO_MANY_RECIPIENTS",
    "E_TOO_MANY_ATTACHMENTS",
    "E_CONTENT_TOO_LARGE",
    "E_HEADER_NOT_ALLOWED",
    "E_HEADER_USE_API_FIELD",
    "E_HEADER_VALUE_INVALID",
    "E_HEADER_VALUE_TOO_LONG",
    "E_HEADER_NAME_INVALID",
    "E_HEADERS_TOO_LARGE",
    "E_HEADERS_TOO_MANY",
    "E_RECIPIENT_NOT_ALLOWED",
];

/// One error of the binding as a transport result; `recipients` is the number of queued recipients.
pub fn classify(e: &SendError, recipients: usize) -> TransportResult {
    let rejected = |reason: &str, code: &str, detail: &str| TransportResult::Rejected {
        reason: reason.to_owned(),
        code: code.to_owned(),
        detail: detail.to_owned(),
    };
    match e {
        SendError::Timeout => TransportResult::Unknown {
            reason: "transport_timeout".into(),
            detail: "no answer within the deadline".into(),
        },
        SendError::Exception { message } => TransportResult::Unknown {
            reason: "transport_connection_lost".into(),
            detail: message.clone(),
        },
        SendError::Coded { code, message } => {
            let detail = format!("{code}: {message}");
            match code.as_str() {
                c if VALIDATION.contains(&c) => rejected("provider_validation", c, &detail),
                "E_DELIVERY_FAILED" if recipients <= 1 => {
                    rejected("provider_validation", code, &detail)
                }
                "E_DELIVERY_FAILED" => TransportResult::Unknown {
                    reason: "provider_outcome_unknown".into(),
                    detail,
                },
                "E_SENDER_DOMAIN_NOT_AVAILABLE" | "E_SENDER_NOT_VERIFIED" => {
                    rejected("sender_domain_unavailable", code, &detail)
                }
                "E_RECIPIENT_SUPPRESSED" => {
                    TransportResult::RecipientSuppressed { code: code.clone() }
                }
                "E_RATE_LIMIT_EXCEEDED" => TransportResult::RetryLater {
                    class: RetryClass::RateLimit,
                    code: code.clone(),
                    refused: Vec::new(),
                },
                "E_DAILY_LIMIT_EXCEEDED" => TransportResult::RetryLater {
                    class: RetryClass::Quota,
                    code: code.clone(),
                    refused: Vec::new(),
                },
                _ => TransportResult::Unknown {
                    reason: "transport_connection_lost".into(),
                    detail,
                },
            }
        }
    }
}

/// One structured send through the binding.
pub async fn send<P: Platform>(p: &P, m: &OutgoingMessage) -> TransportResult {
    let msg = StructuredEmail {
        from: m.from.clone(),
        to: m.to.clone(),
        cc: m.cc.clone(),
        bcc: m.bcc.clone(),
        reply_to: m.reply_to.clone(),
        subject: m.subject.clone(),
        text: m.text.clone(),
        html: m.html.clone(),
        headers: m.headers.clone(),
        attachments: m.attachments.clone(),
    };
    match p.mail().send(&msg, DEADLINE_MS).await {
        Ok(id) => TransportResult::Accepted {
            rfc_message_id: pylota_identity_core::compose::derive_cf_message_id(
                &id,
                pylota_identity_core::address::domain_of(&m.from.0),
            ),
            provider_message_id: id,
            refused: Vec::new(),
            deferred: Vec::new(),
        },
        Err(e) => classify(&e, m.recipients().count()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coded(c: &str) -> SendError {
        SendError::Coded {
            code: c.into(),
            message: "m".into(),
        }
    }

    #[test]
    fn g10_and_g2_codes() {
        for c in VALIDATION {
            assert!(
                matches!(classify(&coded(c), 1), TransportResult::Rejected { reason, .. } if reason == "provider_validation")
            );
        }
        assert!(matches!(
            classify(&coded("E_DELIVERY_FAILED"), 1),
            TransportResult::Rejected { .. }
        ));
        assert!(
            matches!(classify(&coded("E_DELIVERY_FAILED"), 3), TransportResult::Unknown { reason, .. } if reason == "provider_outcome_unknown")
        );
        assert!(matches!(
            classify(&coded("E_INTERNAL_SERVER_ERROR"), 1),
            TransportResult::Unknown { .. }
        ));
        assert!(matches!(
            classify(&coded("E_DAILY_LIMIT_EXCEEDED"), 1),
            TransportResult::RetryLater {
                class: RetryClass::Quota,
                ..
            }
        ));
        assert!(
            matches!(classify(&SendError::Timeout, 1), TransportResult::Unknown { reason, .. } if reason == "transport_timeout")
        );
    }
}
