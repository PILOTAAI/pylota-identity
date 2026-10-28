//! Message rows and the objects built from them: the API's Message and its `trust`, and the summary
//! the webhook events carry ([Inbound › Read path], [Webhooks › Event envelope and payloads]).
//!
//! [Inbound › Read path]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
//! [Webhooks › Event envelope and payloads]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use pylota_identity_core::days::rfc3339;
use serde::Deserialize;
use serde_json::{Value, json};

/// Every column a reader needs, with the thread's ID.
pub const SELECT: &str = "SELECT m.rowid AS rowid, m.id, t.id AS thread_id, m.thread_seq, m.direction, m.status, m.rfc_message_id, m.in_reply_to, m.references_json, m.raw_r2_key, m.raw_sha256, m.from_address, m.from_name, m.sender_domain, m.reply_to_json, m.to_json, m.cc_json, m.bcc_json, m.delivered_to, m.is_bcc, m.is_primary_recipient, m.subject, m.text, m.html_sanitized, m.extracted_text, m.snippet, m.sent_at, m.received_at, m.received_evented_at, m.kind, m.automated_json, m.auth_json, m.verdict, m.spam_score, m.known_sender, m.quarantine_reason, m.flags_json, m.read, m.triage_status, m.triage_json, m.provider, m.provider_message_id, m.draft_id, m.metadata_json FROM messages m JOIN threads t ON t.seq = m.thread_seq";

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub rowid: i64,
    pub id: String,
    pub thread_id: String,
    pub thread_seq: i64,
    pub direction: String,
    pub status: String,
    pub rfc_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references_json: String,
    pub raw_r2_key: Option<String>,
    pub raw_sha256: Option<String>,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub sender_domain: Option<String>,
    pub reply_to_json: String,
    pub to_json: String,
    pub cc_json: String,
    pub bcc_json: String,
    pub delivered_to: Option<String>,
    pub is_bcc: i64,
    pub is_primary_recipient: i64,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html_sanitized: Option<String>,
    pub extracted_text: Option<String>,
    pub snippet: Option<String>,
    pub sent_at: Option<i64>,
    pub received_at: i64,
    pub received_evented_at: Option<i64>,
    pub kind: String,
    pub automated_json: Option<String>,
    pub auth_json: Option<String>,
    pub verdict: Option<String>,
    pub spam_score: Option<f64>,
    pub known_sender: Option<i64>,
    pub quarantine_reason: Option<String>,
    pub flags_json: String,
    pub read: i64,
    pub triage_status: Option<String>,
    pub triage_json: Option<String>,
    pub provider: Option<String>,
    pub provider_message_id: Option<String>,
    pub draft_id: Option<String>,
    pub metadata_json: String,
}

/// The flags the API returns in `trust.flags`; every other flag is a message flag.
pub const TRUST_FLAGS: &[&str] = &[
    "hidden_text",
    "display_name_spoof",
    "lookalike_domain",
    "reply_to_mismatch",
    "thread_join_unverified",
    "shared_domain_sender",
    "steering_suspected",
    "attachments_unscreened",
];

/// The message flags of the API (`MessageFlag`).
const MESSAGE_FLAGS: &[&str] = &[
    "sent_via_fallback",
    "parse_degraded",
    "encrypted",
    "message_id_conflict",
    "reprocessed",
    "reconciled",
    "bcc",
    "loopback",
    "body_truncated",
    "sender_suppressed",
    "body_redacted",
    "dsn_untrusted",
];

fn arr(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!([]))
}

/// An `[{address, name}]` column as API addresses (`name` null when empty).
fn addresses(s: &str) -> Vec<Value> {
    arr(s)
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            let name = a["name"]
                .as_str()
                .filter(|n| !n.is_empty())
                .map(str::to_owned);
            json!({ "address": a["address"], "name": name })
        })
        .collect()
}

impl Row {
    pub fn flags(&self) -> Vec<String> {
        serde_json::from_str(&self.flags_json).unwrap_or_default()
    }

    fn auth(&self) -> Value {
        self.auth_json
            .as_deref()
            .and_then(|a| serde_json::from_str(a).ok())
            .unwrap_or(Value::Null)
    }

    pub fn from(&self) -> Value {
        match &self.from_address {
            Some(a) => {
                json!({ "address": a, "name": self.from_name.as_deref().filter(|n| !n.is_empty()) })
            }
            None => Value::Null,
        }
    }

    /// `trust` (inbound only): the summary results and the detail read from `auth_json`.
    pub fn trust(&self) -> Value {
        if self.direction != "inbound" {
            return Value::Null;
        }
        let a = self.auth();
        fn result(v: &Value) -> &str {
            match v.as_str() {
                Some(
                    r @ ("pass" | "fail" | "softfail" | "neutral" | "none" | "temperror"
                    | "permerror" | "policy"),
                ) => r,
                _ => "none",
            }
        }
        let dkim = a["dkim"].as_array().cloned().unwrap_or_default();
        let dkim_summary = dkim
            .iter()
            .find(|d| d["result"] == "pass" && d["aligned"] == true)
            .map(|_| "pass")
            .or_else(|| dkim.first().map(|d| result(&d["result"])))
            .unwrap_or("none");
        let flags = self.flags();
        json!({
            "verdict": self.verdict.as_deref().unwrap_or("none"),
            "spf": result(&a["spf"]["result"]),
            "dkim": dkim_summary,
            "dmarc": result(&a["dmarc"]["result"]),
            "arc": result(&a["arc"]["result"]),
            "spf_domain": a["spf"]["domain"],
            "spf_aligned": a["spf"]["aligned"].as_bool().unwrap_or(false),
            "dkim_domains": dkim.iter().map(|d| json!({ "domain": d["d"], "selector": d["s"], "result": result(&d["result"]), "aligned": d["aligned"].as_bool().unwrap_or(false) })).collect::<Vec<_>>(),
            "dmarc_domain": a["dmarc"]["record_domain"],
            "dmarc_policy": a["dmarc"]["policy"],
            "dmarc_aligned_by": a["dmarc"]["aligned_by"],
            "arc_sealer": a["arc"]["newest"]["seal_d"],
            "known_sender": self.known_sender == Some(1),
            "quarantined": self.status == "quarantined",
            "spam_score": self.spam_score,
            "automated": matches!(self.kind.as_str(), "dsn" | "mdn" | "list" | "automated"),
            "flags": flags.iter().filter(|f| TRUST_FLAGS.contains(&f.as_str())).collect::<Vec<_>>(),
        })
    }

    pub fn message_flags(&self) -> Vec<String> {
        self.flags()
            .into_iter()
            .filter(|f| MESSAGE_FLAGS.contains(&f.as_str()))
            .collect()
    }

    /// The thin summary of the webhook events (`MessageSummary`).
    pub fn summary(&self, labels: &[String]) -> Value {
        let inbound = self.direction == "inbound";
        json!({
            "id": self.id, "thread_id": self.thread_id, "direction": self.direction, "status": self.status,
            "from": self.from(), "to": addresses(&self.to_json), "cc": addresses(&self.cc_json),
            "delivered_to": self.delivered_to,
            "is_primary_recipient": inbound.then_some(self.is_primary_recipient == 1),
            "subject": self.subject, "sent_at": self.sent_at.map(rfc3339),
            "received_at": rfc3339(self.received_at), "kind": self.kind, "labels": labels,
            "in_reply_to": self.in_reply_to, "draft_id": self.draft_id, "flags": self.message_flags(),
        })
    }

    /// The Message object. `text` and `html` only when asked for; `headers` is filled by the caller.
    pub fn json(
        &self,
        identity_id: &str,
        attachments: &[Value],
        labels: &[String],
        refs: &[Value],
        with_text: bool,
        with_html: bool,
    ) -> Value {
        let inbound = self.direction == "inbound";
        let triage = match (&self.triage_status, &self.triage_json) {
            (Some(_), Some(t)) => serde_json::from_str::<Value>(t).unwrap_or(Value::Null),
            (Some(s), None) => json!({ "status": s }),
            _ => Value::Null,
        };
        let mut m = json!({
            "id": self.id, "thread_id": self.thread_id, "identity_id": identity_id,
            "direction": self.direction, "status": self.status, "from": self.from(),
            "to": addresses(&self.to_json), "cc": addresses(&self.cc_json),
            "bcc": addresses(&self.bcc_json), "reply_to": addresses(&self.reply_to_json),
            "delivered_to": self.delivered_to,
            "is_primary_recipient": inbound.then_some(self.is_primary_recipient == 1),
            "subject": self.subject, "sent_at": self.sent_at.map(rfc3339),
            "received_at": rfc3339(self.received_at), "extracted_text": self.extracted_text,
            "text": if with_text { self.text.clone() } else { None },
            "html": if with_html { self.html_sanitized.clone() } else { None },
            "attachments": attachments, "labels": labels, "kind": self.kind,
            "trust": self.trust(), "triage": triage, "screening": null, "calendar": null,
            "refs": refs, "rfc_message_id": self.rfc_message_id, "in_reply_to": self.in_reply_to,
            "draft_id": self.draft_id, "flags": self.message_flags(),
            "metadata": serde_json::from_str::<Value>(&self.metadata_json).unwrap_or_else(|_| json!({})),
        });
        if inbound {
            m["quarantine_reason"] = json!(self.quarantine_reason);
        } else {
            m["provider_message_id"] = json!(self.provider_message_id);
        }
        m
    }
}

/// An attachment row as the API returns it (and the event's `attachments[]` from the same row).
#[derive(Clone, Debug, Deserialize)]
pub struct AttachmentRow {
    pub id: String,
    pub message_rowid: i64,
    pub filename: Option<String>,
    pub content_type: String,
    pub sniffed_type: Option<String>,
    pub size: i64,
    pub sha256: String,
    pub disposition: Option<String>,
    pub r2_key: String,
    pub text_status: String,
    pub text_r2_key: Option<String>,
    pub text_pages: Option<i64>,
    pub risk: Option<String>,
    pub scan_status: String,
    pub upload_id: Option<String>,
}

pub const ATTACHMENTS_OF: &str = "SELECT id, message_rowid, filename, content_type, sniffed_type, size, sha256, disposition, r2_key, text_status, text_r2_key, text_pages, risk, scan_status, upload_id FROM attachments WHERE message_rowid = ?1 ORDER BY rowid";

impl AttachmentRow {
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "filename": self.filename, "content_type": self.content_type,
            "size": self.size, "disposition": self.disposition, "text_status": self.text_status,
            "pages": self.text_pages, "risk": self.risk, "upload_id": self.upload_id,
        })
    }

    pub fn event_json(&self) -> Value {
        json!({ "id": self.id, "filename": self.filename, "content_type": self.content_type, "size": self.size })
    }
}
