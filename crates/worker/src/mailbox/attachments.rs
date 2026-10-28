//! Attachment text in the mailbox ([Inbound › Attachment text extraction]): the pending attachments of a
//! message for the `AttachmentText` job, and `AttachmentTextReady`, which records the result and rebuilds
//! the message's keyword-index row in one transaction.
//!
//! [Inbound › Attachment text extraction]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_core::search::fts_doc::{self, ATTACHMENT_TEXT_BYTES, MessageForIndex};
use pylota_identity_platform::PResult;
use pylota_identity_platform::durable::Sql;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::ingest::RefRow;
use super::messages::{self, Row};

/// An attachment whose text the job extracts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub attachment_id: String,
    pub r2_key: String,
    pub filename: Option<String>,
    pub content_type: String,
    pub sniffed_type: Option<String>,
    pub size: i64,
}

/// One attachment's result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextItem {
    pub attachment_id: String,
    /// `ready` or `unavailable`.
    pub status: String,
    pub pages: Option<u32>,
    pub text_r2_key: Option<String>,
    #[serde(default)]
    pub refs: Vec<RefRow>,
    /// The text for the keyword index (at most 256 KiB).
    #[serde(default)]
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttachmentText {
    pub message_id: String,
    pub items: Vec<TextItem>,
}

#[derive(Deserialize)]
struct Id {
    #[allow(dead_code)]
    id: String,
}

fn erased<S: Sql>(s: &S, message_id: &str) -> PResult<bool> {
    let e: Option<Id> = s.first(
        "SELECT message_id AS id FROM erased_ids WHERE message_id = ?1",
        &[message_id.into()],
    )?;
    Ok(e.is_some())
}

fn message<S: Sql>(s: &S, message_id: &str) -> PResult<Option<Row>> {
    s.first(
        &format!("{} WHERE m.id = ?1", messages::SELECT),
        &[message_id.into()],
    )
}

/// The attachments of a message with `text_status = 'pending'`; `{"erased": true}` when it is gone.
pub fn pending<S: Sql>(s: &S, message_id: &str) -> PResult<Value> {
    let Some(m) = message(s, message_id)?.filter(|_| !erased(s, message_id).unwrap_or(true)) else {
        return Ok(json!({ "erased": true }));
    };
    let items: Vec<Pending> = s.query(
        "SELECT id AS attachment_id, r2_key, filename, content_type, sniffed_type, size FROM attachments WHERE message_rowid = ?1 AND text_status = 'pending' ORDER BY rowid",
        &[m.rowid.into()],
    )?;
    Ok(json!({ "erased": false, "items": items }))
}

#[derive(Deserialize)]
struct Named {
    id: String,
    filename: Option<String>,
}

#[derive(Deserialize)]
struct RefValue {
    value: String,
}

fn people(json: &str) -> Vec<(String, String)> {
    serde_json::from_str::<Vec<Value>>(json)
        .unwrap_or_default()
        .iter()
        .map(|a| {
            (
                a["name"].as_str().unwrap_or("").to_owned(),
                a["address"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

/// Records the results; `{"erased": true}` and no write when the message is gone (I10).
pub fn ready<S: Sql>(s: &S, r: &AttachmentText, is_system: bool) -> PResult<Value> {
    let Some(m) = message(s, &r.message_id)?.filter(|_| !erased(s, &r.message_id).unwrap_or(true))
    else {
        return Ok(json!({ "erased": true }));
    };
    for it in &r.items {
        s.exec(
            "UPDATE attachments SET text_status = ?2, text_r2_key = ?3, text_pages = ?4 WHERE id = ?1 AND message_rowid = ?5 AND text_status = 'pending'",
            &[
                it.attachment_id.clone().into(),
                it.status.clone().into(),
                it.text_r2_key.clone().into(),
                it.pages.map(i64::from).into(),
                m.rowid.into(),
            ],
        )?;
        for rf in &it.refs {
            s.exec(
                "INSERT OR IGNORE INTO refs (message_rowid, kind, value, source) VALUES (?1, ?2, ?3, ?4)",
                &[m.rowid.into(), rf.kind.clone().into(), rf.value.clone().into(), rf.source.clone().into()],
            )?;
        }
    }
    if !is_system {
        reindex(s, &m, &r.items)?;
    }
    Ok(json!({ "erased": false }))
}

/// Rebuilds the message's `fts` and `fts_tri` rows (contentless tables: delete, then insert).
fn reindex<S: Sql>(s: &S, m: &Row, items: &[TextItem]) -> PResult<()> {
    let names: Vec<Named> = s.query(
        "SELECT id, filename FROM attachments WHERE message_rowid = ?1 ORDER BY rowid",
        &[m.rowid.into()],
    )?;
    let attachments: Vec<(String, String)> = names
        .into_iter()
        .map(|n| {
            let text = items
                .iter()
                .find(|i| i.attachment_id == n.id && i.status == "ready")
                .map(|i| i.text.clone())
                .unwrap_or_default();
            (n.filename.unwrap_or_default(), text)
        })
        .collect();
    let refs: Vec<RefValue> = s.query(
        "SELECT value FROM refs WHERE message_rowid = ?1 ORDER BY rowid",
        &[m.rowid.into()],
    )?;
    let refs: Vec<String> = refs.into_iter().map(|r| r.value).collect();
    let mut recipients = people(&m.to_json);
    recipients.extend(people(&m.cc_json));
    let doc = fts_doc::fts_doc(&MessageForIndex {
        subject: m.subject.as_deref().unwrap_or(""),
        from_name: m.from_name.as_deref().unwrap_or(""),
        from_address: m.from_address.as_deref().unwrap_or(""),
        recipients: &recipients,
        delivered_to: m.delivered_to.as_deref().unwrap_or(""),
        extracted_text: m.extracted_text.as_deref().unwrap_or(""),
        text: m.text.as_deref().unwrap_or(""),
        attachments: &attachments,
        refs: &refs,
    });
    s.exec("DELETE FROM fts WHERE rowid = ?1", &[m.rowid.into()])?;
    s.exec("DELETE FROM fts_tri WHERE rowid = ?1", &[m.rowid.into()])?;
    s.exec(
        "INSERT INTO fts (rowid, subject, participants, body_new, body_full, attachments, refs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        &[
            m.rowid.into(),
            doc.subject.clone().into(),
            doc.participants.clone().into(),
            doc.body_new.into(),
            doc.body_full.into(),
            doc.attachments.into(),
            doc.refs.clone().into(),
        ],
    )?;
    s.exec(
        "INSERT INTO fts_tri (rowid, subject, participants, refs) VALUES (?1, ?2, ?3, ?4)",
        &[
            m.rowid.into(),
            doc.subject.into(),
            doc.participants.into(),
            doc.refs.into(),
        ],
    )?;
    Ok(())
}

/// The text an item carries for the index: at most [`ATTACHMENT_TEXT_BYTES`].
pub fn index_text(md: &str) -> String {
    let mut end = md.len().min(ATTACHMENT_TEXT_BYTES);
    while !md.is_char_boundary(end) {
        end -= 1;
    }
    md[..end].to_owned()
}
