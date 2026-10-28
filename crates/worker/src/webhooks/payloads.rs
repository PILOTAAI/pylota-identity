//! The message event payloads ([Webhooks › Event envelope and payloads]). M7 has the inbound ones;
//! the send and delivery builders arrive with M8 and M9. Payloads are thin: a summary, the verdicts and
//! at most `policy.webhook_text_bytes` of `extracted_text`.
//!
//! [Webhooks › Event envelope and payloads]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use serde_json::{Value, json};

use crate::mailbox::messages::{AttachmentRow, Row};

/// `text` cut to at most `max` bytes at a character boundary, and whether it was cut.
pub fn cut(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// `message.received`; `released` marks a release, `reprocessed` a re-parse.
pub fn message_received(
    row: &Row,
    attachments: &[AttachmentRow],
    labels: &[String],
    text_bytes: usize,
    released: bool,
) -> Value {
    let (text, truncated) = cut(row.extracted_text.as_deref().unwrap_or(""), text_bytes);
    let mut d = json!({
        "message": row.summary(labels), "thread_id": row.thread_id, "trust": row.trust(),
        "extracted_text": row.extracted_text.as_ref().map(|_| text), "extracted_text_truncated": truncated,
        "attachments": attachments.iter().map(AttachmentRow::event_json).collect::<Vec<_>>(),
    });
    if released {
        d["released"] = true.into();
    }
    d
}

/// `message.quarantined`: no text.
pub fn message_quarantined(row: &Row, attachments: &[AttachmentRow], labels: &[String]) -> Value {
    json!({
        "message": row.summary(labels), "thread_id": row.thread_id, "trust": row.trust(),
        "attachments": attachments.iter().map(AttachmentRow::event_json).collect::<Vec<_>>(),
        "quarantine_reason": row.quarantine_reason,
    })
}

/// `message.released`: one of the two actor IDs is set.
pub fn message_released(
    row: &Row,
    key_id: Option<&str>,
    user_id: Option<&str>,
    reason: &str,
) -> Value {
    json!({
        "message": row.summary(&[]), "released_by_key_id": key_id, "released_by_user_id": user_id,
        "reason": reason,
    })
}

/// `verification.received`: never the value, which only `wait` releases.
pub fn verification_received(message_id: &str, sender_domain: &str, kind: &str) -> Value {
    json!({ "message_id": message_id, "sender_domain": sender_domain, "kind": kind, "account_id": null })
}
