//! Reads, label changes, release and `wait` ([Inbound › Read path and release], [Inbound › The `wait`
//! handler]). The handler has checked the key's scope and passes `review` (the key holds
//! `quarantine:review`); the mailbox never sees the key otherwise.
//!
//! Lists answer `limit + 1` items with their keyset (`sort`, `key`); the handler cuts the page and
//! encodes the cursor.
//!
//! [Inbound › Read path and release]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
//! [Inbound › The `wait` handler]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::internal::{EmbedReason, IndexJob, TriageReason};
use pylota_identity_core::visibility::{not_visible_sql, visible};
use pylota_identity_platform::d1::SqlValue;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{PResult, Platform, QueueProducer as _};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::messages::{self, AttachmentRow, Row};
use super::outbox::{self, Owner};
use super::threads;
use crate::errors::ApiError;
use crate::webhooks::payloads;

/// At most this many labels on one message.
pub const MAX_LABELS: usize = 64;
/// A released code can be released again for this long after its first release.
const RERELEASE_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

/// The last row of a page: its sort time and its `seq` or `rowid`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListBoundary {
    pub sort_ms: i64,
    pub key: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Include {
    #[serde(default)]
    pub quoted: bool,
    #[serde(default)]
    pub html: bool,
    #[serde(default)]
    pub headers: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThreadFilters {
    pub label: Option<String>,
    pub category: Option<String>,
    pub needs_reply_gte: Option<f64>,
    pub is_unread: Option<bool>,
    pub direction: Option<String>,
    pub after_ms: Option<i64>,
    pub before_ms: Option<i64>,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MessageFilters {
    pub thread_id: Option<String>,
    pub direction: Option<String>,
    pub status: Option<String>,
    pub label: Option<String>,
    pub after_ms: Option<i64>,
    pub before_ms: Option<i64>,
}

/// Who releases: a key, or a person in the console.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Actor {
    Key { key_id: String },
    User { user_id: String },
}

/// Decided by the handler (J14, J16): a key releases only with `Allowed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyRelease {
    NotAKey,
    Allowed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    #[default]
    Any,
    Reply,
    Verification,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
pub enum ReadRequest {
    ListThreads {
        filters: ThreadFilters,
        limit: u32,
        cursor: Option<ListBoundary>,
    },
    GetThread {
        thread_id: String,
        messages_limit: u32,
        cursor: Option<ListBoundary>,
        include: Include,
        review: bool,
    },
    ListMessages {
        filters: MessageFilters,
        limit: u32,
        cursor: Option<ListBoundary>,
        review: bool,
    },
    GetMessage {
        message_id: String,
        include: Include,
        review: bool,
    },
    UpdateMessage {
        message_id: String,
        labels_add: Vec<String>,
        labels_remove: Vec<String>,
        read: Option<bool>,
        review: bool,
    },
    UpdateThread {
        thread_id: String,
        labels_add: Vec<String>,
        labels_remove: Vec<String>,
        read: Option<bool>,
        archived: Option<bool>,
    },
    ListQuarantine {
        limit: u32,
        cursor: Option<ListBoundary>,
    },
    Release {
        message_id: String,
        actor: Actor,
        reason: String,
        key_release: KeyRelease,
        triage_enabled: bool,
        webhook_text_bytes: usize,
        /// The identity's `active` and `retiring` addresses (participants and `reply_from_address`).
        identity_addresses: Vec<String>,
    },
    /// The R2 key of a message's raw MIME (`null` once past `raw_days`).
    Raw {
        message_id: String,
        review: bool,
    },
    /// One attachment row of a message the key may see.
    Attachment {
        message_id: String,
        attachment_id: String,
        review: bool,
    },
    RegisterWait {
        from_domain: String,
        ttl_ms: i64,
    },
    WaitPoll {
        since_ms: i64,
        from: Option<String>,
        subject_contains: Option<String>,
        thread_id: Option<String>,
        kind: WaitKind,
        /// The registrable domain a code may be released for (`from`'s domain).
        release_domain: Option<String>,
        review: bool,
    },
}

fn not_found() -> ApiError {
    ApiError::not_found(ErrorCode::MessageNotFound)
}

fn thread_not_found() -> ApiError {
    ApiError::not_found(ErrorCode::ThreadNotFound)
}

#[derive(Deserialize)]
struct LabelRow {
    label: String,
}

#[derive(Deserialize)]
struct RefRow {
    kind: String,
    value: String,
}

fn labels_of<S: Sql>(s: &S, rowid: i64) -> PResult<Vec<String>> {
    let rows: Vec<LabelRow> = s.query(
        "SELECT label FROM labels WHERE message_rowid = ?1 ORDER BY label",
        &[rowid.into()],
    )?;
    Ok(rows.into_iter().map(|r| r.label).collect())
}

fn attachments_of<S: Sql>(s: &S, rowid: i64) -> PResult<Vec<AttachmentRow>> {
    s.query(messages::ATTACHMENTS_OF, &[rowid.into()])
}

/// The Message object of a row; `_raw_r2_key` is added when `include.headers` (the handler reads the
/// headers from R2 and removes it).
pub fn message_json<S: Sql>(
    s: &S,
    row: &Row,
    identity_id: &str,
    include: Include,
) -> PResult<Value> {
    let atts: Vec<Value> = attachments_of(s, row.rowid)?
        .iter()
        .map(AttachmentRow::json)
        .collect();
    let labels = labels_of(s, row.rowid)?;
    let refs: Vec<RefRow> = s.query(
        "SELECT DISTINCT kind, value FROM refs WHERE message_rowid = ?1 ORDER BY kind, value",
        &[row.rowid.into()],
    )?;
    let refs: Vec<Value> = refs
        .into_iter()
        .map(|r| json!({ "kind": r.kind, "value": r.value }))
        .collect();
    let mut m = row.json(
        identity_id,
        &atts,
        &labels,
        &refs,
        include.quoted,
        include.html,
    );
    if row.direction == "inbound" {
        m["deliveries"] = Value::Null;
    } else {
        #[derive(serde::Deserialize)]
        struct D {
            address: String,
            field: String,
            status: String,
            smtp_code: Option<String>,
            enhanced_code: Option<String>,
            bounce_type: Option<String>,
            updated_at: i64,
        }
        let ds: Vec<D> = s.query(
            "SELECT address, field, status, smtp_code, enhanced_code, bounce_type, updated_at FROM deliveries WHERE message_rowid = ?1 ORDER BY rowid",
            &[row.rowid.into()],
        )?;
        m["deliveries"] = Value::Array(
            ds.into_iter()
                .map(|d| json!({ "address": d.address, "field": d.field, "status": d.status, "smtp_code": d.smtp_code,
                    "enhanced_code": d.enhanced_code, "bounce_type": d.bounce_type,
                    "updated_at": pylota_identity_core::days::rfc3339(d.updated_at) }))
                .collect(),
        );
    }
    if include.headers {
        m["headers"] = Value::Null;
        m["_raw_r2_key"] = json!(row.raw_r2_key);
    }
    Ok(m)
}

fn by_id<S: Sql>(s: &S, message_id: &str) -> PResult<Option<Row>> {
    s.first(
        &format!("{} WHERE m.id = ?1", messages::SELECT),
        &[message_id.into()],
    )
}

/// A message the key may see: visible, or held with `review` (`hidden` and `throttled` never).
fn may_see(row: &Row, review: bool) -> bool {
    visible(&row.status) || (review && matches!(row.status.as_str(), "quarantined" | "screening"))
}

fn page(items: Vec<(i64, i64, Value)>) -> Value {
    json!({
        "items": items
            .into_iter()
            .map(|(sort, key, value)| json!({ "sort": sort, "key": key, "value": value }))
            .collect::<Vec<_>>()
    })
}

#[derive(Deserialize)]
struct ThreadRow {
    seq: i64,
    id: String,
    subject: String,
    participants_json: String,
    message_count: i64,
    unread_count: i64,
    first_at: i64,
    last_at: i64,
    last_inbound_at: Option<i64>,
    category: Option<String>,
    needs_reply: Option<f64>,
    urgency: Option<i64>,
    archived: i64,
    hold_json: Option<String>,
    last_direction: Option<String>,
    snippet: Option<String>,
}

const THREAD_COLUMNS: &str = "t.seq, t.id, t.subject, t.participants_json, t.message_count, t.unread_count, t.first_at, t.last_at, t.last_inbound_at, t.category, t.needs_reply, t.urgency, t.archived, t.hold_json, lm.direction AS last_direction, lm.snippet";

fn latest_visible_join() -> String {
    format!(
        "LEFT JOIN messages lm ON lm.rowid = (SELECT m.rowid FROM messages m WHERE m.thread_seq = t.seq AND m.status NOT IN {} ORDER BY m.received_at DESC, m.rowid DESC LIMIT 1)",
        not_visible_sql()
    )
}

fn thread_labels<S: Sql>(s: &S, seqs: &[i64]) -> PResult<Vec<(i64, String)>> {
    #[derive(Deserialize)]
    struct L {
        thread_seq: i64,
        label: String,
    }
    let rows: Vec<L> = s.query(
        &format!(
            "SELECT m.thread_seq, l.label FROM labels l JOIN messages m ON m.rowid = l.message_rowid WHERE m.thread_seq IN (SELECT value FROM json_each(?1)) AND m.status NOT IN {} GROUP BY m.thread_seq, l.label ORDER BY l.label",
            not_visible_sql()
        ),
        &[serde_json::to_string(seqs).unwrap_or_else(|_| "[]".into()).into()],
    )?;
    Ok(rows.into_iter().map(|r| (r.thread_seq, r.label)).collect())
}

fn thread_summary(t: &ThreadRow, labels: &[(i64, String)]) -> Value {
    let participants: Vec<Value> = serde_json::from_str::<Vec<Value>>(&t.participants_json)
        .unwrap_or_default()
        .into_iter()
        .take(50)
        .map(|p| {
            let name = p["name"]
                .as_str()
                .filter(|n| !n.is_empty())
                .map(str::to_owned);
            json!({ "address": p["address"], "name": name })
        })
        .collect();
    json!({
        "id": t.id,
        "subject": t.subject,
        "participants": participants,
        "message_count": t.message_count,
        "unread_count": t.unread_count,
        "first_at": pylota_identity_core::days::rfc3339(t.first_at),
        "last_at": pylota_identity_core::days::rfc3339(t.last_at),
        "last_inbound_at": t.last_inbound_at.map(pylota_identity_core::days::rfc3339),
        "last_direction": t.last_direction.as_deref().unwrap_or("inbound"),
        "snippet": t.snippet.as_deref().unwrap_or(""),
        "labels": labels.iter().filter(|(s, _)| *s == t.seq).map(|(_, l)| l.clone()).collect::<Vec<_>>(),
        "category": t.category,
        "needs_reply": t.needs_reply,
        "urgency": t.urgency,
        "archived": t.archived == 1,
        "hold": t.hold_json.as_deref().and_then(|h| serde_json::from_str::<Value>(h).ok()),
    })
}

fn thread_by_id<S: Sql>(s: &S, thread_id: &str) -> PResult<Option<ThreadRow>> {
    s.first(
        &format!(
            "SELECT {THREAD_COLUMNS} FROM threads t {} WHERE t.id = ?1",
            latest_visible_join()
        ),
        &[thread_id.into()],
    )
}

fn list_threads<S: Sql>(
    s: &S,
    f: &ThreadFilters,
    limit: u32,
    cursor: Option<ListBoundary>,
) -> PResult<Value> {
    let mut sql = format!(
        "SELECT {THREAD_COLUMNS} FROM threads t {} WHERE t.message_count > 0 AND t.archived = ?1",
        latest_visible_join()
    );
    let mut params: Vec<SqlValue> = vec![i64::from(f.archived).into()];
    let mut add = |frag: &str, v: SqlValue, sql: &mut String| {
        params.push(v);
        sql.push_str(&frag.replace('?', &format!("?{}", params.len())));
    };
    if let Some(c) = cursor {
        add(" AND (t.last_at < ?", c.sort_ms.into(), &mut sql);
        add(" OR (t.last_at = ?", c.sort_ms.into(), &mut sql);
        add(" AND t.seq < ?))", c.key.into(), &mut sql);
    }
    if let Some(v) = &f.category {
        add(" AND t.category = ?", v.clone().into(), &mut sql);
    }
    if let Some(v) = f.needs_reply_gte {
        add(" AND t.needs_reply >= ?", v.into(), &mut sql);
    }
    match f.is_unread {
        Some(true) => sql.push_str(" AND t.unread_count > 0"),
        Some(false) => sql.push_str(" AND t.unread_count = 0"),
        None => {}
    }
    if let Some(v) = &f.direction {
        add(" AND lm.direction = ?", v.clone().into(), &mut sql);
    }
    if let Some(v) = f.after_ms {
        add(" AND t.last_at >= ?", v.into(), &mut sql);
    }
    if let Some(v) = f.before_ms {
        add(" AND t.last_at < ?", v.into(), &mut sql);
    }
    if let Some(v) = &f.label {
        let frag = format!(
            " AND EXISTS (SELECT 1 FROM labels l JOIN messages m ON m.rowid = l.message_rowid WHERE m.thread_seq = t.seq AND l.label = ? AND m.status NOT IN {})",
            not_visible_sql()
        );
        add(&frag, v.clone().into(), &mut sql);
    }
    add(
        " ORDER BY t.last_at DESC, t.seq DESC LIMIT ?",
        (i64::from(limit) + 1).into(),
        &mut sql,
    );
    let rows: Vec<ThreadRow> = s.query(&sql, &params)?;
    let seqs: Vec<i64> = rows.iter().map(|r| r.seq).collect();
    let labels = thread_labels(s, &seqs)?;
    Ok(page(
        rows.iter()
            .map(|t| (t.last_at, t.seq, thread_summary(t, &labels)))
            .collect(),
    ))
}

/// The visibility predicate of a message list: visible only, or, with an explicit held `status`
/// filter, those messages with `review` (and none without).
fn status_clause(status: Option<&str>, review: bool) -> Result<String, ()> {
    match status {
        Some("hidden" | "throttled") => Err(()),
        Some("quarantined" | "screening") if !review => Err(()),
        Some("quarantined" | "screening") => Ok(String::new()),
        _ => Ok(format!(" AND m.status NOT IN {}", not_visible_sql())),
    }
}

fn list_messages<S: Sql>(
    s: &S,
    identity_id: &str,
    f: &MessageFilters,
    limit: u32,
    cursor: Option<ListBoundary>,
    review: bool,
) -> PResult<Value> {
    let Ok(vis) = status_clause(f.status.as_deref(), review) else {
        return Ok(page(Vec::new()));
    };
    let mut sql = format!("{} WHERE 1 = 1{vis}", messages::SELECT);
    let mut params: Vec<SqlValue> = Vec::new();
    let mut add = |frag: &str, v: SqlValue, sql: &mut String| {
        params.push(v);
        sql.push_str(&frag.replace('?', &format!("?{}", params.len())));
    };
    if let Some(c) = cursor {
        add(" AND (m.received_at < ?", c.sort_ms.into(), &mut sql);
        add(" OR (m.received_at = ?", c.sort_ms.into(), &mut sql);
        add(" AND m.rowid < ?))", c.key.into(), &mut sql);
    }
    if let Some(v) = &f.thread_id {
        add(" AND t.id = ?", v.clone().into(), &mut sql);
    }
    if let Some(v) = &f.direction {
        add(" AND m.direction = ?", v.clone().into(), &mut sql);
    }
    if let Some(v) = &f.status {
        add(" AND m.status = ?", v.clone().into(), &mut sql);
    }
    if let Some(v) = &f.label {
        add(
            " AND EXISTS (SELECT 1 FROM labels l WHERE l.message_rowid = m.rowid AND l.label = ?)",
            v.clone().into(),
            &mut sql,
        );
    }
    if let Some(v) = f.after_ms {
        add(" AND m.received_at >= ?", v.into(), &mut sql);
    }
    if let Some(v) = f.before_ms {
        add(" AND m.received_at < ?", v.into(), &mut sql);
    }
    add(
        " ORDER BY m.received_at DESC, m.rowid DESC LIMIT ?",
        (i64::from(limit) + 1).into(),
        &mut sql,
    );
    let rows: Vec<Row> = s.query(&sql, &params)?;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        items.push((
            r.received_at,
            r.rowid,
            message_json(s, r, identity_id, Include::default())?,
        ));
    }
    Ok(page(items))
}

fn get_thread<S: Sql>(
    s: &S,
    identity_id: &str,
    thread_id: &str,
    limit: u32,
    cursor: Option<ListBoundary>,
    include: Include,
    review: bool,
) -> Result<Value, ApiError> {
    let t = thread_by_id(s, thread_id)?
        .filter(|t| t.message_count > 0 || review)
        .ok_or_else(thread_not_found)?;
    let held = if review {
        "('hidden','throttled')"
    } else {
        not_visible_sql()
    };
    let mut sql = format!(
        "{} WHERE m.thread_seq = ?1 AND m.status NOT IN {held}",
        messages::SELECT
    );
    let mut params: Vec<SqlValue> = vec![t.seq.into()];
    if let Some(c) = cursor {
        sql.push_str(" AND (m.received_at > ?2 OR (m.received_at = ?2 AND m.rowid > ?3))");
        params.push(c.sort_ms.into());
        params.push(c.key.into());
    }
    sql.push_str(&format!(
        " ORDER BY m.received_at ASC, m.rowid ASC LIMIT ?{}",
        params.len() + 1
    ));
    params.push((i64::from(limit) + 1).into());
    let rows: Vec<Row> = s.query(&sql, &params)?;
    if rows.is_empty() && t.message_count == 0 {
        return Err(thread_not_found());
    }
    let labels = thread_labels(s, &[t.seq])?;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        items.push((
            r.received_at,
            r.rowid,
            message_json(s, r, identity_id, include)?,
        ));
    }
    let mut out = page(items);
    out["thread"] = thread_summary(&t, &labels);
    Ok(out)
}

/// Validates and applies label and read changes to one message row (in the caller's transaction).
fn apply_labels<S: Sql>(
    s: &S,
    rowid: i64,
    add: &[String],
    remove: &[String],
) -> Result<(), ApiError> {
    if !remove.is_empty() {
        s.exec(
            "DELETE FROM labels WHERE message_rowid = ?1 AND label IN (SELECT value FROM json_each(?2))",
            &[rowid.into(), serde_json::to_string(remove).unwrap_or_default().into()],
        )?;
    }
    for l in add {
        s.exec(
            "INSERT OR IGNORE INTO labels (message_rowid, label) VALUES (?1, ?2)",
            &[rowid.into(), l.clone().into()],
        )?;
    }
    let n = labels_of(s, rowid)?.len();
    if n > MAX_LABELS {
        return Err(ApiError::invalid(
            "labels_add",
            "A message has at most 64 labels.",
        ));
    }
    Ok(())
}

fn update_message<S: Sql>(
    s: &S,
    identity_id: &str,
    message_id: &str,
    add: &[String],
    remove: &[String],
    read: Option<bool>,
    review: bool,
) -> Result<Value, ApiError> {
    s.transaction_sync(|s| -> Result<Value, ApiError> {
        let row = by_id(s, message_id)?
            .filter(|r| may_see(r, review))
            .ok_or_else(not_found)?;
        apply_labels(s, row.rowid, add, remove)?;
        if let Some(r) = read {
            s.exec(
                "UPDATE messages SET read = ?2 WHERE rowid = ?1",
                &[row.rowid.into(), i64::from(r).into()],
            )?;
            threads::recount_unread(s, row.thread_seq)?;
        }
        let row = by_id(s, message_id)?.ok_or_else(not_found)?;
        Ok(message_json(s, &row, identity_id, Include::default())?)
    })
}

fn update_thread<S: Sql>(
    s: &S,
    thread_id: &str,
    add: &[String],
    remove: &[String],
    read: Option<bool>,
    archived: Option<bool>,
) -> Result<Value, ApiError> {
    s.transaction_sync(|s| -> Result<Value, ApiError> {
        let t = thread_by_id(s, thread_id)?
            .filter(|t| t.message_count > 0)
            .ok_or_else(thread_not_found)?;
        #[derive(Deserialize)]
        struct R {
            rowid: i64,
        }
        let rows: Vec<R> = s.query(
            &format!(
                "SELECT rowid FROM messages WHERE thread_seq = ?1 AND status NOT IN {}",
                not_visible_sql()
            ),
            &[t.seq.into()],
        )?;
        for r in &rows {
            apply_labels(s, r.rowid, add, remove)?;
        }
        if let Some(r) = read {
            s.exec(
                &format!(
                    "UPDATE messages SET read = ?2 WHERE thread_seq = ?1 AND direction = 'inbound' AND status NOT IN {}",
                    not_visible_sql()
                ),
                &[t.seq.into(), i64::from(r).into()],
            )?;
            threads::recount_unread(s, t.seq)?;
        }
        if let Some(a) = archived {
            s.exec(
                "UPDATE threads SET archived = ?2 WHERE seq = ?1",
                &[t.seq.into(), i64::from(a).into()],
            )?;
        }
        let t = thread_by_id(s, thread_id)?.ok_or_else(thread_not_found)?;
        let labels = thread_labels(s, &[t.seq])?;
        Ok(thread_summary(&t, &labels))
    })
}

fn list_quarantine<S: Sql>(
    s: &S,
    identity_id: &str,
    limit: u32,
    cursor: Option<ListBoundary>,
) -> PResult<Value> {
    let mut sql = format!(
        "{} WHERE m.direction = 'inbound' AND m.status = 'quarantined'",
        messages::SELECT
    );
    let mut params: Vec<SqlValue> = Vec::new();
    if let Some(c) = cursor {
        sql.push_str(" AND (m.received_at < ?1 OR (m.received_at = ?1 AND m.rowid < ?2))");
        params.push(c.sort_ms.into());
        params.push(c.key.into());
    }
    sql.push_str(&format!(
        " ORDER BY m.received_at DESC, m.rowid DESC LIMIT ?{}",
        params.len() + 1
    ));
    params.push((i64::from(limit) + 1).into());
    let rows: Vec<Row> = s.query(&sql, &params)?;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        items.push((
            r.received_at,
            r.rowid,
            message_json(s, r, identity_id, Include::default())?,
        ));
    }
    Ok(page(items))
}

/// The release transaction (Read path and release, steps 1–5). Answers the Message and whether a
/// triage job is due.
#[allow(clippy::too_many_arguments)]
fn release<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &Owner<'_>,
    message_id: &str,
    actor: &Actor,
    reason: &str,
    key_release: KeyRelease,
    triage_enabled: bool,
    text_bytes: usize,
    own: &[String],
    now: i64,
) -> Result<(Value, bool), ApiError> {
    if matches!(actor, Actor::Key { .. }) && key_release != KeyRelease::Allowed {
        return Err(ApiError::new(
            ErrorCode::PermissionDenied,
            "This key may not release mail.",
        ));
    }
    s.transaction_sync(|s| -> Result<(Value, bool), ApiError> {
        // 1. Load.
        let row = by_id(s, message_id)?
            .filter(|r| r.direction == "inbound")
            .ok_or_else(not_found)?;
        if row.status != "quarantined" {
            return Err(ApiError::new(
                ErrorCode::NotQuarantined,
                "This message is not quarantined.",
            )
            .details(json!({ "status": row.status })));
        }
        // 2. Status and triage.
        let triage = triage_enabled && !matches!(row.kind.as_str(), "dsn" | "mdn");
        let triage_json = (!triage).then(|| {
            json!({ "status": "skipped", "reason": "policy_disabled", "risk_flags": [], "model": null, "version": 1, "run": 0 }).to_string()
        });
        s.exec(
            "UPDATE messages SET status = 'received', quarantine_reason = NULL, triage_status = ?2, triage_json = ?3 WHERE rowid = ?1",
            &[
                row.rowid.into(),
                if triage { "pending" } else { "skipped" }.into(),
                triage_json.into(),
            ],
        )?;
        // 3. The thread, as if the message had just arrived.
        let mut addrs: Vec<(String, String)> = Vec::new();
        if let Some(a) = &row.from_address {
            addrs.push((a.clone(), row.from_name.clone().unwrap_or_default()));
        }
        for list in [&row.to_json, &row.cc_json] {
            for a in serde_json::from_str::<Vec<Value>>(list).unwrap_or_default() {
                addrs.push((
                    a["address"].as_str().unwrap_or("").to_owned(),
                    a["name"].as_str().unwrap_or("").to_owned(),
                ));
            }
        }
        let addr_refs: Vec<(&str, &str)> = addrs.iter().map(|(a, n)| (a.as_str(), n.as_str())).collect();
        let own: Vec<&str> = own.iter().map(String::as_str).collect();
        let reply_from = row
            .delivered_to
            .clone()
            .filter(|d| row.is_bcc == 0 && own.contains(&d.as_str()));
        threads::recompute(s, row.thread_seq, &addr_refs, &own, reply_from.as_deref(), true)?;
        // 4. Verification: a match with a `pass` verdict, unless expired.
        if row.verdict.as_deref() == Some("pass") {
            let auto: Value = row
                .automated_json
                .as_deref()
                .and_then(|a| serde_json::from_str(a).ok())
                .unwrap_or(Value::Null);
            let v = &auto["verification"];
            let expires = row.received_at + DAY_MS;
            if let (Some(kind), Some(value)) = (v["kind"].as_str(), v["value"].as_str())
                && expires > now
            {
                s.exec(
                    "INSERT INTO verifications (message_rowid, kind, value, sender_domain, expires_at, consumed_at, account_id) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)",
                    &[
                        row.rowid.into(),
                        kind.into(),
                        value.into(),
                        row.sender_domain.clone().unwrap_or_default().into(),
                        expires.into(),
                    ],
                )?;
            }
        }
        // 5. The outbox: message.released, then message.received once.
        let row = by_id(s, message_id)?.ok_or_else(not_found)?;
        let (key_id, user_id) = match actor {
            Actor::Key { key_id } => (Some(key_id.as_str()), None),
            Actor::User { user_id } => (None, Some(user_id.as_str())),
        };
        outbox::append(
            s,
            owner,
            &outbox::new_event_id(p),
            "message.released",
            now,
            &payloads::message_released(&row, key_id, user_id, reason),
            now,
        )?;
        if row.received_evented_at.is_none() {
            let atts = attachments_of(s, row.rowid)?;
            let labels = labels_of(s, row.rowid)?;
            outbox::append(
                s,
                owner,
                &outbox::new_event_id(p),
                "message.received",
                now,
                &payloads::message_received(&row, &atts, &labels, text_bytes, true),
                now,
            )?;
            s.exec(
                "UPDATE messages SET received_evented_at = ?2 WHERE rowid = ?1",
                &[row.rowid.into(), now.into()],
            )?;
        }
        super::set_wake(s, "outbox", now)?;
        let row = by_id(s, message_id)?.ok_or_else(not_found)?;
        Ok((message_json(s, &row, owner.identity_id, Include::default())?, triage))
    })
}

#[derive(Deserialize)]
struct VerificationRow {
    kind: String,
    value: String,
    sender_domain: String,
    consumed_at: Option<i64>,
}

#[allow(clippy::too_many_arguments)]
fn wait_poll<S: Sql>(
    s: &S,
    identity_id: &str,
    since_ms: i64,
    from: Option<&str>,
    subject_contains: Option<&str>,
    thread_id: Option<&str>,
    kind: WaitKind,
    release_domain: Option<&str>,
    review: bool,
    now: i64,
) -> Result<Value, ApiError> {
    let statuses = if review {
        "('received','quarantined')"
    } else {
        "('received')"
    };
    let mut sql = format!(
        "{} WHERE m.direction = 'inbound' AND m.status IN {statuses} AND m.received_at > ?1",
        messages::SELECT
    );
    let mut params: Vec<SqlValue> = vec![since_ms.into()];
    let mut add = |frag: &str, v: SqlValue, sql: &mut String| {
        params.push(v);
        sql.push_str(&frag.replace('?', &format!("?{}", params.len())));
    };
    match from {
        Some(f) if f.starts_with('@') => {
            add(
                " AND substr(m.from_address, instr(m.from_address, '@') + 1) = ?",
                f[1..].to_ascii_lowercase().into(),
                &mut sql,
            );
        }
        Some(f) => add(
            " AND m.from_address = ?",
            f.to_ascii_lowercase().into(),
            &mut sql,
        ),
        None => {}
    }
    if let Some(sub) = subject_contains {
        add(
            " AND instr(lower(COALESCE(m.subject, '')), ?) > 0",
            sub.to_lowercase().into(),
            &mut sql,
        );
    }
    if let Some(t) = thread_id {
        add(" AND t.id = ?", t.to_owned().into(), &mut sql);
    }
    match kind {
        WaitKind::Reply => sql.push_str(
            " AND EXISTS (SELECT 1 FROM messages o WHERE o.thread_seq = m.thread_seq AND o.direction = 'outbound' AND o.rowid < m.rowid) \
              AND EXISTS (SELECT 1 FROM messages e WHERE e.thread_seq = m.thread_seq AND e.rowid < m.rowid) \
              AND m.flags_json NOT LIKE '%\"thread_join_unverified\"%'",
        ),
        WaitKind::Verification => {
            add(
                " AND EXISTS (SELECT 1 FROM verifications v WHERE v.message_rowid = m.rowid AND v.expires_at > ?)",
                now.into(),
                &mut sql,
            );
        }
        WaitKind::Any => {}
    }
    sql.push_str(" ORDER BY m.received_at ASC, m.rowid ASC LIMIT 1");
    let Some(row) = s.first::<Row>(&sql, &params)? else {
        return Ok(json!({ "message": null, "verification": null }));
    };
    // Release: a pass verdict and the expected domain; a value released once stays releasable for an
    // hour.
    let mut verification = Value::Null;
    if kind != WaitKind::Reply && row.verdict.as_deref() == Some("pass") {
        let v: Option<VerificationRow> = s.first(
            "SELECT kind, value, sender_domain, consumed_at FROM verifications WHERE message_rowid = ?1 AND expires_at > ?2 LIMIT 1",
            &[row.rowid.into(), now.into()],
        )?;
        if let Some(v) = v
            && Some(v.sender_domain.as_str()) == release_domain
            && v.consumed_at.is_none_or(|c| now - c <= RERELEASE_MS)
        {
            if v.consumed_at.is_none() {
                s.exec(
                    "UPDATE verifications SET consumed_at = ?2 WHERE message_rowid = ?1",
                    &[row.rowid.into(), now.into()],
                )?;
            }
            let (code, link) = if v.kind == "code" {
                (Some(v.value), None)
            } else {
                (None, Some(v.value))
            };
            verification = json!({ "code": code, "link": link, "sender_domain": v.sender_domain });
        }
    }
    Ok(json!({
        "message": message_json(s, &row, identity_id, Include::default())?,
        "verification": verification,
    }))
}

pub async fn handle<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &Owner<'_>,
    r: ReadRequest,
    now: i64,
    _is_system: bool,
) -> Result<Value, ApiError> {
    let idn = owner.identity_id;
    match r {
        ReadRequest::ListThreads {
            filters,
            limit,
            cursor,
        } => Ok(list_threads(s, &filters, limit, cursor)?),
        ReadRequest::GetThread {
            thread_id,
            messages_limit,
            cursor,
            include,
            review,
        } => get_thread(s, idn, &thread_id, messages_limit, cursor, include, review),
        ReadRequest::ListMessages {
            filters,
            limit,
            cursor,
            review,
        } => Ok(list_messages(s, idn, &filters, limit, cursor, review)?),
        ReadRequest::GetMessage {
            message_id,
            include,
            review,
        } => {
            let row = by_id(s, &message_id)?
                .filter(|r| may_see(r, review))
                .ok_or_else(not_found)?;
            Ok(message_json(s, &row, idn, include)?)
        }
        ReadRequest::UpdateMessage {
            message_id,
            labels_add,
            labels_remove,
            read,
            review,
        } => update_message(
            s,
            idn,
            &message_id,
            &labels_add,
            &labels_remove,
            read,
            review,
        ),
        ReadRequest::UpdateThread {
            thread_id,
            labels_add,
            labels_remove,
            read,
            archived,
        } => update_thread(s, &thread_id, &labels_add, &labels_remove, read, archived),
        ReadRequest::ListQuarantine { limit, cursor } => {
            Ok(list_quarantine(s, idn, limit, cursor)?)
        }
        ReadRequest::Release {
            message_id,
            actor,
            reason,
            key_release,
            triage_enabled,
            webhook_text_bytes,
            identity_addresses,
        } => {
            let (m, triage) = release(
                p,
                s,
                owner,
                &message_id,
                &actor,
                &reason,
                key_release,
                triage_enabled,
                webhook_text_bytes,
                &identity_addresses,
                now,
            )?;
            // After the commit: Triage (when pending) and Embed. A failed send is left to the
            // nightly reconciliation (M10); the release itself has happened.
            let mut jobs = vec![IndexJob::embed(
                owner.tenant_id,
                idn,
                &message_id,
                EmbedReason::Release,
            )];
            if triage {
                jobs.push(IndexJob::triage(
                    owner.tenant_id,
                    idn,
                    &message_id,
                    TriageReason::Release,
                ));
            }
            let _ = p.queues().send_batch(QueueName::Index, &jobs, 0).await;
            Ok(m)
        }
        ReadRequest::Raw { message_id, review } => {
            let row = by_id(s, &message_id)?
                .filter(|r| may_see(r, review))
                .ok_or_else(not_found)?;
            Ok(json!({ "raw_r2_key": row.raw_r2_key }))
        }
        ReadRequest::Attachment {
            message_id,
            attachment_id,
            review,
        } => {
            let row = by_id(s, &message_id)?
                .filter(|r| may_see(r, review))
                .ok_or_else(not_found)?;
            let att = attachments_of(s, row.rowid)?
                .into_iter()
                .find(|a| a.id == attachment_id)
                .ok_or_else(|| ApiError::not_found(ErrorCode::AttachmentNotFound))?;
            Ok(json!({
                "r2_key": att.r2_key, "filename": att.filename, "content_type": att.content_type,
                "sniffed_type": att.sniffed_type, "size": att.size, "text_status": att.text_status,
                "text_r2_key": att.text_r2_key, "text_pages": att.text_pages,
                "received_at": row.received_at,
            }))
        }
        ReadRequest::RegisterWait {
            from_domain,
            ttl_ms,
        } => {
            s.exec(
                "INSERT INTO meta (k, v) VALUES ('wait:' || ?1, ?2) ON CONFLICT (k) DO UPDATE SET v = CAST(MAX(CAST(v AS INTEGER), CAST(?2 AS INTEGER)) AS TEXT)",
                &[from_domain.into(), (now + ttl_ms).to_string().into()],
            )?;
            Ok(Value::Null)
        }
        ReadRequest::WaitPoll {
            since_ms,
            from,
            subject_contains,
            thread_id,
            kind,
            release_domain,
            review,
        } => wait_poll(
            s,
            idn,
            since_ms,
            from.as_deref(),
            subject_contains.as_deref(),
            thread_id.as_deref(),
            kind,
            release_domain.as_deref(),
            review,
            now,
        ),
    }
}
