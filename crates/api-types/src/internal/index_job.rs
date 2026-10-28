//! The `pm-index` job ([Search § 6]). M7 defines the whole enum; each later milestone fills in the arm of
//! its own kind (M10 `Embed`, `DeleteVectors` and `Reconcile`, M12 `Triage`).
//!
//! [Search § 6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndexJob {
    AttachmentText {
        tenant_id: String,
        identity_id: String,
        message_id: String,
        #[serde(default)]
        attempt: u32,
    },
    Embed {
        tenant_id: String,
        identity_id: String,
        message_id: String,
        #[serde(default)]
        reason: EmbedReason,
        #[serde(default)]
        attempt: u32,
        #[serde(default)]
        deferrals: u32,
    },
    Triage {
        tenant_id: String,
        identity_id: String,
        message_id: String,
        #[serde(default)]
        reason: TriageReason,
        #[serde(default)]
        attempt: u32,
        #[serde(default)]
        deferrals: u32,
    },
    DeleteVectors {
        tenant_id: String,
        /// At most 500.
        vector_ids: Vec<String>,
        #[serde(default)]
        also_next: bool,
    },
    Reconcile {
        tenant_id: String,
        identity_id: String,
        /// `YYYY-MM-DD`.
        run_date: String,
    },
}

impl IndexJob {
    /// The `kind` label of the job's metrics.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::AttachmentText { .. } => "attachment_text",
            Self::Embed { .. } => "embed",
            Self::Triage { .. } => "triage",
            Self::DeleteVectors { .. } => "delete_vectors",
            Self::Reconcile { .. } => "reconcile",
        }
    }

    /// The new job of `kind` for one message, `attempt` 0.
    pub fn attachment_text(tenant_id: &str, identity_id: &str, message_id: &str) -> Self {
        Self::AttachmentText {
            tenant_id: tenant_id.to_owned(),
            identity_id: identity_id.to_owned(),
            message_id: message_id.to_owned(),
            attempt: 0,
        }
    }

    pub fn embed(
        tenant_id: &str,
        identity_id: &str,
        message_id: &str,
        reason: EmbedReason,
    ) -> Self {
        Self::Embed {
            tenant_id: tenant_id.to_owned(),
            identity_id: identity_id.to_owned(),
            message_id: message_id.to_owned(),
            reason,
            attempt: 0,
            deferrals: 0,
        }
    }

    pub fn triage(
        tenant_id: &str,
        identity_id: &str,
        message_id: &str,
        reason: TriageReason,
    ) -> Self {
        Self::Triage {
            tenant_id: tenant_id.to_owned(),
            identity_id: identity_id.to_owned(),
            message_id: message_id.to_owned(),
            reason,
            attempt: 0,
            deferrals: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbedReason {
    #[default]
    New,
    AttachmentText,
    Release,
    Reembed,
    Reconcile,
    Rechunk,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageReason {
    #[default]
    Ingest,
    Release,
    Rerun,
    Reprocess,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_form_and_defaults() {
        let j = IndexJob::attachment_text("ten_1", "idn_1", "msg_1");
        assert_eq!(
            serde_json::to_value(&j).unwrap(),
            json!({ "kind": "attachment_text", "tenant_id": "ten_1", "identity_id": "idn_1", "message_id": "msg_1", "attempt": 0 })
        );
        let e: IndexJob = serde_json::from_value(
            json!({ "kind": "embed", "tenant_id": "t", "identity_id": "i", "message_id": "m" }),
        )
        .unwrap();
        assert_eq!(e, IndexJob::embed("t", "i", "m", EmbedReason::New));
        assert_eq!(e.kind(), "embed");
        let t: IndexJob = serde_json::from_value(
            json!({ "kind": "triage", "tenant_id": "t", "identity_id": "i", "message_id": "m", "reason": "rerun", "attempt": 2 }),
        )
        .unwrap();
        assert!(matches!(
            t,
            IndexJob::Triage {
                reason: TriageReason::Rerun,
                attempt: 2,
                deferrals: 0,
                ..
            }
        ));
    }
}
