//! The mailbox's side of triage ([Triage § 1.1, § 9, § 10]): `load` (what the consumer reads),
//! `rerun` (the endpoint's eligibility check and debounce) and `commit` (the record, the labels a
//! tenant rule adds, the thread roll-up and `message.triaged`, in one transaction).
//!
//! [Triage § 1.1, § 9, § 10]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/triage.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::internal::TriageReason;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::{PResult, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::messages::{self, Row};
use super::outbox::{self, Owner};
use crate::errors::ApiError;

/// A re-run within this many milliseconds of the last one enqueues nothing (§ 10 step 4).
pub const RERUN_DEBOUNCE_MS: i64 = 60_000;

/// The mailbox's triage operations, tagged `tq` inside `{"op": "triage", …}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tq", rename_all = "snake_case")]
pub enum TriageOp {
    Load {
        message_id: String,
        reason: TriageReason,
        version: u32,
    },
    Rerun {
        message_id: String,
        review: bool,
    },
    Commit(Box<CommitIn>),
}

/// One attachment as the consumer sees it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LoadedAttachment {
    pub filename: String,
    pub effective_type: String,
    pub size: i64,
    pub inline: bool,
    pub risk: Option<String>,
    /// Set when the text is `ready`.
    pub text_r2_key: Option<String>,
}

/// The message before this one in its thread.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Previous {
    pub direction: String,
    pub at: i64,
    /// At most 500 characters.
    pub excerpt: String,
}

/// What `load` answers for a message the consumer should triage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Loaded {
    pub thread_id: String,
    pub kind: String,
    /// `automated_json.evidence`.
    pub evidence: Vec<String>,
    pub from_address: String,
    pub from_name: String,
    pub reply_to: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub delivered_to: Option<String>,
    pub subject: Option<String>,
    pub extracted_text: String,
    pub verdict: String,
    pub spf: String,
    pub dkim: String,
    pub dmarc: String,
    pub known_sender: bool,
    pub spam_score: f64,
    pub flags: Vec<String>,
    pub attachments: Vec<LoadedAttachment>,
    pub refs: Vec<(String, String)>,
    pub labels: Vec<String>,
    pub has_verification: bool,
    /// Earlier visible messages in the thread.
    pub earlier: i64,
    pub last_outbound_at: Option<i64>,
    pub previous: Option<Previous>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "load", rename_all = "snake_case")]
pub enum LoadOut {
    Skip,
    Triage(Box<Loaded>),
}

/// A record to store (§ 1.1 step 8).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommitIn {
    pub message_id: String,
    /// The record without `run`, which the commit sets.
    pub record: Value,
    pub labels_add: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommitOut {
    /// False when the message was erased, quarantined or hidden meanwhile: nothing was written.
    pub committed: bool,
}

pub fn handle<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &Owner<'_>,
    op: TriageOp,
    now: i64,
    is_system: bool,
) -> Result<Value, ApiError> {
    match op {
        TriageOp::Load {
            message_id,
            reason,
            version,
        } => {
            let out = if is_system {
                LoadOut::Skip
            } else {
                load(s, &message_id, reason, version)?
            };
            Ok(serde_json::to_value(out).unwrap_or(Value::Null))
        }
        TriageOp::Rerun { message_id, review } => {
            let enqueue = rerun(s, &message_id, review, now, is_system)?;
            Ok(json!({ "enqueue": enqueue }))
        }
        TriageOp::Commit(c) => {
            let committed = !is_system && s.transaction_sync(|s| commit(p, s, owner, &c, now))?;
            Ok(json!({ "committed": committed }))
        }
    }
}

fn by_id<S: Sql>(s: &S, message_id: &str) -> PResult<Option<Row>> {
    s.first(
        &format!("{} WHERE m.id = ?1", messages::SELECT),
        &[message_id.into()],
    )
}

/// The stored record's `version` and `run`.
fn stored(row: &Row) -> (Option<u64>, u64) {
    let v: Value = row
        .triage_json
        .as_deref()
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or(Value::Null);
    (v["version"].as_u64(), v["run"].as_u64().unwrap_or(0))
}

#[derive(Deserialize)]
struct Text {
    t: String,
}

#[derive(Deserialize)]
struct N {
    n: i64,
}

#[derive(Deserialize)]
struct PrevRow {
    direction: String,
    at: i64,
    excerpt: Option<String>,
}

#[derive(Deserialize)]
struct ThreadFacts {
    last_outbound_at: Option<i64>,
}

#[derive(Deserialize)]
struct RefRow {
    kind: String,
    value: String,
}

fn strings(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<Value>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| match a {
            Value::String(s) => Some(s),
            Value::Object(o) => o.get("address").and_then(Value::as_str).map(str::to_owned),
            _ => None,
        })
        .collect()
}

/// Step 1 of § 1.1: `Skip` for a message that is gone, not inbound, not `received`, or already
/// triaged with this version (unless re-run).
fn load<S: Sql>(
    s: &S,
    message_id: &str,
    reason: TriageReason,
    version: u32,
) -> Result<LoadOut, ApiError> {
    let Some(row) = by_id(s, message_id)? else {
        return Ok(LoadOut::Skip);
    };
    if row.direction != "inbound" || row.status != "received" {
        return Ok(LoadOut::Skip);
    }
    let (v, _) = stored(&row);
    if row.triage_status.as_deref() == Some("done")
        && v == Some(u64::from(version))
        && reason != TriageReason::Rerun
    {
        return Ok(LoadOut::Skip);
    }
    let auto: Value = row
        .automated_json
        .as_deref()
        .and_then(|a| serde_json::from_str(a).ok())
        .unwrap_or(Value::Null);
    let auth: Value = row
        .auth_json
        .as_deref()
        .and_then(|a| serde_json::from_str(a).ok())
        .unwrap_or(Value::Null);
    let dkim = auth["dkim"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|d| d["result"] == "pass")
                .or_else(|| a.first())
                .and_then(|d| d["result"].as_str())
        })
        .or_else(|| auth["dkim"]["result"].as_str())
        .unwrap_or("none");
    let atts: Vec<messages::AttachmentRow> =
        s.query(messages::ATTACHMENTS_OF, &[row.rowid.into()])?;
    let refs: Vec<RefRow> = s.query(
        "SELECT DISTINCT kind, value FROM refs WHERE message_rowid = ?1 ORDER BY kind, value",
        &[row.rowid.into()],
    )?;
    let labels: Vec<Text> = s.query(
        "SELECT label AS t FROM labels WHERE message_rowid = ?1 ORDER BY label",
        &[row.rowid.into()],
    )?;
    let has_verification = s
        .first::<N>(
            "SELECT COUNT(*) AS n FROM verifications WHERE message_rowid = ?1",
            &[row.rowid.into()],
        )?
        .is_some_and(|r| r.n > 0);
    let earlier = s
        .first::<N>(
            "SELECT COUNT(*) AS n FROM messages WHERE thread_seq = ?1 AND rowid <> ?2 AND (received_at < ?3 OR (received_at = ?3 AND rowid < ?2)) AND status NOT IN ('quarantined','screening','hidden','throttled')",
            &[row.thread_seq.into(), row.rowid.into(), row.received_at.into()],
        )?
        .map_or(0, |r| r.n);
    let previous: Option<PrevRow> = s.first(
        "SELECT direction, COALESCE(sent_at, received_at) AS at, substr(COALESCE(extracted_text, snippet, ''), 1, 500) AS excerpt FROM messages WHERE thread_seq = ?1 AND rowid <> ?2 AND (received_at < ?3 OR (received_at = ?3 AND rowid < ?2)) AND status NOT IN ('quarantined','screening','hidden','throttled') ORDER BY received_at DESC, rowid DESC LIMIT 1",
        &[row.thread_seq.into(), row.rowid.into(), row.received_at.into()],
    )?;
    let facts: Option<ThreadFacts> = s.first(
        "SELECT last_outbound_at FROM threads WHERE seq = ?1",
        &[row.thread_seq.into()],
    )?;
    let flags = row.flags();
    Ok(LoadOut::Triage(Box::new(Loaded {
        thread_id: row.thread_id.clone(),
        kind: row.kind.clone(),
        evidence: auto["evidence"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        from_address: row.from_address.clone().unwrap_or_default(),
        from_name: row.from_name.clone().unwrap_or_default(),
        reply_to: strings(&row.reply_to_json),
        to: strings(&row.to_json),
        cc: strings(&row.cc_json),
        delivered_to: row.delivered_to.clone(),
        subject: row.subject.clone(),
        extracted_text: row
            .extracted_text
            .clone()
            .or_else(|| row.text.clone())
            .unwrap_or_default(),
        verdict: row.verdict.clone().unwrap_or_else(|| "none".into()),
        spf: auth["spf"]["result"].as_str().unwrap_or("none").to_owned(),
        dkim: dkim.to_owned(),
        dmarc: auth["dmarc"]["result"]
            .as_str()
            .unwrap_or("none")
            .to_owned(),
        known_sender: row.known_sender == Some(1),
        spam_score: row.spam_score.unwrap_or(0.0),
        flags,
        attachments: atts
            .into_iter()
            .map(|a| LoadedAttachment {
                filename: a.filename.unwrap_or_default(),
                effective_type: a.sniffed_type.unwrap_or(a.content_type),
                size: a.size,
                inline: a.disposition.as_deref() == Some("inline"),
                text_r2_key: (a.text_status == "ready")
                    .then_some(a.text_r2_key)
                    .flatten(),
                risk: a.risk,
            })
            .collect(),
        refs: refs.into_iter().map(|r| (r.kind, r.value)).collect(),
        labels: labels.into_iter().map(|l| l.t).collect(),
        has_verification,
        earlier,
        last_outbound_at: facts.and_then(|f| f.last_outbound_at),
        previous: previous.map(|p| Previous {
            direction: p.direction,
            at: p.at,
            excerpt: p.excerpt.unwrap_or_default(),
        }),
    })))
}

fn not_eligible(reason: &str) -> ApiError {
    ApiError::new(
        ErrorCode::TriageNotEligible,
        "Triage does not run on this message.",
    )
    .details(json!({ "reason": reason }))
}

/// § 10 steps 1–4: the error, or whether a job is due.
fn rerun<S: Sql>(
    s: &S,
    message_id: &str,
    review: bool,
    now: i64,
    is_system: bool,
) -> Result<bool, ApiError> {
    s.transaction_sync(|s| -> Result<bool, ApiError> {
        let row = by_id(s, message_id)?
            .filter(|r| {
                !matches!(
                    r.status.as_str(),
                    "quarantined" | "screening" | "hidden" | "throttled"
                ) || review
            })
            .ok_or_else(|| ApiError::not_found(ErrorCode::MessageNotFound))?;
        if row.direction != "inbound" {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "Triage applies to inbound messages only.",
            ));
        }
        match row.status.as_str() {
            "quarantined" => return Err(not_eligible("quarantined")),
            "screening" => return Err(not_eligible("screening")),
            "hidden" | "throttled" => return Err(not_eligible("not_eligible")),
            _ => {}
        }
        if is_system || matches!(row.kind.as_str(), "dsn" | "mdn") {
            return Err(not_eligible("not_eligible"));
        }
        let key = format!("triage_rerun:{}", row.id);
        let last = s
            .first::<Text>(
                "SELECT v AS t FROM meta WHERE k = ?1",
                &[key.clone().into()],
            )?
            .and_then(|r| r.t.parse::<i64>().ok());
        if row.triage_status.as_deref() == Some("pending")
            && last.is_some_and(|at| now - at < RERUN_DEBOUNCE_MS)
        {
            return Ok(false);
        }
        s.exec(
            "UPDATE messages SET triage_status = 'pending' WHERE rowid = ?1",
            &[row.rowid.into()],
        )?;
        s.exec(
            "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            &[key.into(), now.to_string().into()],
        )?;
        Ok(true)
    })
}

/// § 1.1 step 8 and § 9.
fn commit<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &Owner<'_>,
    c: &CommitIn,
    now: i64,
) -> Result<bool, ApiError> {
    let Some(row) = by_id(s, &c.message_id)? else {
        return Ok(false);
    };
    if row.direction != "inbound" || row.status != "received" {
        return Ok(false);
    }
    let status = c.record["status"].as_str().unwrap_or("failed").to_owned();
    let (_, run) = stored(&row);
    let mut record = c.record.clone();
    // `run` counts completed runs: a skipped record (allowance, AI unavailable) keeps the count.
    record["run"] = json!(if matches!(status.as_str(), "done" | "failed") {
        run + 1
    } else {
        run
    });
    if record.get("completed_at").is_none() {
        record["completed_at"] = json!(now);
    }
    s.exec(
        "UPDATE messages SET triage_status = ?2, triage_json = ?3 WHERE rowid = ?1",
        &[
            row.rowid.into(),
            status.clone().into(),
            record.to_string().into(),
        ],
    )?;
    s.exec(
        "DELETE FROM meta WHERE k = ?1",
        &[format!("triage_rerun:{}", row.id).into()],
    )?;
    if status == "done" {
        for l in &c.labels_add {
            s.exec(
                "INSERT OR IGNORE INTO labels (message_rowid, label) SELECT ?1, ?2 WHERE (SELECT COUNT(*) FROM labels WHERE message_rowid = ?1) < ?3",
                &[
                    row.rowid.into(),
                    l.clone().into(),
                    (super::read::MAX_LABELS as i64).into(),
                ],
            )?;
        }
        s.exec(
            "UPDATE threads SET category = ?2, urgency = ?4, needs_reply = CASE WHEN COALESCE(last_outbound_at, 0) > ?5 THEN 0 ELSE ?3 END \
             WHERE seq = ?1 AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.thread_seq = ?1 AND m.direction = 'inbound' AND m.triage_status = 'done' \
             AND m.rowid <> ?6 AND (m.received_at > ?5 OR (m.received_at = ?5 AND m.rowid > ?6)))",
            &[
                row.thread_seq.into(),
                record["category"].as_str().map(str::to_owned).into(),
                record["needs_reply"].as_f64().into(),
                record["urgency"].as_i64().into(),
                row.received_at.into(),
                row.rowid.into(),
            ],
        )?;
    }
    if matches!(status.as_str(), "done" | "failed") {
        outbox::append(
            s,
            owner,
            &outbox::new_event_id(p),
            "message.triaged",
            now,
            &json!({
                "message_id": row.id,
                "thread_id": row.thread_id,
                "triage": api(Some(&status), Some(&record.to_string())),
            }),
            now,
        )?;
        super::set_wake(s, "outbox", now)?;
    }
    Ok(true)
}

/// The API's triage object (§ 2) from the two columns: the ten fields, and `reason` only for a skipped
/// or failed record. A pending message shows `pending` with no fields, whatever an earlier run left.
pub fn api(status: Option<&str>, json: Option<&str>) -> Value {
    let Some(status) = status else {
        return Value::Null;
    };
    let r: Value = json
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or(Value::Null);
    let pending = status == "pending";
    let field = |k: &str| {
        if pending {
            Value::Null
        } else {
            r.get(k).cloned().unwrap_or(Value::Null)
        }
    };
    let mut t = json!({
        "status": status,
        "category": field("category"),
        "needs_reply": field("needs_reply"),
        "urgency": field("urgency"),
        "summary": field("summary"),
        "language": field("language"),
        "risk_flags": if pending { json!([]) } else { r.get("risk_flags").cloned().unwrap_or_else(|| json!([])) },
        "model": field("model"),
        "version": r["version"].as_u64().unwrap_or(u64::from(crate::triage::TRIAGE_VERSION)),
        "run": r["run"].as_u64().unwrap_or(0),
    });
    if matches!(status, "skipped" | "failed")
        && let Some(reason) = r.get("reason").filter(|v| !v.is_null())
    {
        t["reason"] = reason.clone();
    }
    t
}
