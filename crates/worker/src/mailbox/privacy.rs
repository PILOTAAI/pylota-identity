//! The mailbox operations of jobs ([Privacy and erasure § 4.1]): plans and deletes for erasure and
//! retention, probes, holds, and the export's batches. Every write is one `transaction_sync`.
//!
//! [Privacy and erasure § 4.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

use pylota_identity_core::address::address_hash;
use pylota_identity_platform::PResult;
use pylota_identity_platform::durable::Sql;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::outbox::Owner;
use super::threads;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    Message { id: String },
    Thread { id: String },
    Counterparty { address: String },
    CounterpartyHash { hash: String },
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EraseMode {
    Erasure,
    Retention,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PrivacyOp {
    ErasurePlan {
        target: Target,
        after_rowid: i64,
        limit: u32,
    },
    EraseRows {
        rowids: Vec<i64>,
        counterparty: Option<Target>,
        mode: EraseMode,
    },
    ProbeKeyword {
        target: Target,
        message_ids: Vec<String>,
    },
    CountAll,
    WipeAll,
    ForgetSender {
        target: Target,
    },
    ActiveHolds {
        now_ms: i64,
    },
    SetHold {
        thread_id: String,
        reason: String,
        until: Option<i64>,
        set_by: String,
    },
    ClearHold {
        thread_id: String,
    },
    RetentionPlan {
        raw_cutoff_ms: Option<i64>,
        message_cutoff_ms: Option<i64>,
        after_rowid: i64,
        limit: u32,
    },
    PurgeRaw {
        message_ids: Vec<String>,
    },
    PurgeEvents {
        cutoff_ms: i64,
    },
    Hygiene {
        now_ms: i64,
    },
    ExportBatch {
        target: Target,
        after_rowid: i64,
        limit: u32,
    },
    /// The `reparse`, `reindex` and `reembed` jobs: inbound messages in a time range.
    MessagesInRange {
        after_rowid: i64,
        limit: u32,
        after: Option<i64>,
        before: Option<i64>,
    },
}

#[derive(Clone, Debug, Deserialize)]
struct MsgRow {
    rowid: i64,
    id: String,
    thread_id: String,
    hold_json: Option<String>,
    from_address: Option<String>,
    to_json: String,
    cc_json: String,
    bcc_json: String,
    reply_to_json: String,
}

macro_rules! msg_cols {
    () => {
        "m.rowid, m.id, m.thread_seq, t.id AS thread_id, t.hold_json, m.from_address, m.to_json, m.cc_json, m.bcc_json, m.reply_to_json"
    };
}

/// A hold is active while set and its `until` is unset or later than `now`.
pub fn hold_active(hold_json: Option<&str>, now: i64) -> Option<String> {
    let h: Value = serde_json::from_str(hold_json?).ok()?;
    let until = h["until"].as_i64();
    (until.is_none() || until > Some(now)).then(|| h["reason"].as_str().unwrap_or("").to_owned())
}

fn addrs_of(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<Value>>(json)
        .unwrap_or_default()
        .iter()
        .filter_map(|a| {
            a.as_str()
                .or_else(|| a["address"].as_str())
                .map(str::to_ascii_lowercase)
        })
        .collect()
}

impl MsgRow {
    fn addresses<S: Sql>(&self, s: &S) -> PResult<Vec<String>> {
        let mut out: Vec<String> = self
            .from_address
            .iter()
            .map(|a| a.to_ascii_lowercase())
            .collect();
        for j in [
            &self.to_json,
            &self.cc_json,
            &self.bcc_json,
            &self.reply_to_json,
        ] {
            out.extend(addrs_of(j));
        }
        #[derive(Deserialize)]
        struct A {
            address: String,
        }
        out.extend(
            s.query::<A>(
                "SELECT address FROM deliveries WHERE message_rowid = ?1",
                &[self.rowid.into()],
            )?
            .into_iter()
            .map(|a| a.address.to_ascii_lowercase()),
        );
        out.sort();
        out.dedup();
        Ok(out)
    }

    fn matches<S: Sql>(&self, s: &S, target: &Target, hash_key: &[u8; 32]) -> PResult<bool> {
        Ok(match target {
            Target::Message { id } => &self.id == id,
            Target::Thread { id } => &self.thread_id == id,
            Target::All => true,
            Target::Counterparty { address } => {
                self.addresses(s)?.contains(&address.to_ascii_lowercase())
            }
            Target::CounterpartyHash { hash } => self
                .addresses(s)?
                .iter()
                .any(|a| address_hash(hash_key, a) == *hash),
        })
    }
}

/// The R2 prefixes of a message: inbound (`m/{msg}/`: raw, attachments, their text) and outbound
/// (`out/{msg}.eml`, `out/{msg}/a/…`).
pub fn r2_prefixes(owner: &Owner<'_>, message_id: &str) -> Vec<String> {
    let base = format!("t/{}/i/{}", owner.tenant_id, owner.identity_id);
    vec![
        format!("{base}/m/{message_id}/"),
        format!("{base}/out/{message_id}"),
    ]
}

fn vector_ids<S: Sql>(s: &S, rowid: i64) -> PResult<Vec<String>> {
    #[derive(Deserialize)]
    struct V {
        vector_id: String,
    }
    Ok(s.query::<V>(
        "SELECT vector_id FROM chunks WHERE message_rowid = ?1",
        &[rowid.into()],
    )?
    .into_iter()
    .map(|v| v.vector_id)
    .collect())
}

#[derive(Deserialize)]
struct N {
    n: i64,
}

fn count<S: Sql>(s: &S, sql: &str, rowids: &str) -> PResult<i64> {
    Ok(s.first::<N>(sql, &[rowids.into()])?.map_or(0, |n| n.n))
}

/// One page of the messages a target selects, with held threads set aside.
fn plan<S: Sql>(
    s: &S,
    owner: &Owner<'_>,
    target: &Target,
    after_rowid: i64,
    limit: u32,
    now: i64,
    hash_key: &[u8; 32],
) -> PResult<Value> {
    let (sql, arg): (&str, Option<String>) = match target {
        Target::Message { id } => (
            concat!(
                "SELECT ",
                msg_cols!(),
                " FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE m.id = ?1 AND m.rowid > ?2 ORDER BY m.rowid LIMIT ?3"
            ),
            Some(id.clone()),
        ),
        Target::Thread { id } => (
            concat!(
                "SELECT ",
                msg_cols!(),
                " FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE t.id = ?1 AND m.rowid > ?2 ORDER BY m.rowid LIMIT ?3"
            ),
            Some(id.clone()),
        ),
        _ => (
            concat!(
                "SELECT ",
                msg_cols!(),
                " FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE ?1 IS NULL AND m.rowid > ?2 ORDER BY m.rowid LIMIT ?3"
            ),
            None,
        ),
    };
    let page: Vec<MsgRow> = s.query(
        sql,
        &[arg.into(), after_rowid.into(), i64::from(limit).into()],
    )?;
    let next = (page.len() as u32 == limit)
        .then(|| page.last().map(|r| r.rowid))
        .flatten();
    let mut messages = Vec::new();
    let mut held: Vec<Value> = Vec::new();
    for m in &page {
        if !m.matches(s, target, hash_key)? {
            continue;
        }
        if let Some(reason) = hold_active(m.hold_json.as_deref(), now) {
            if !held.iter().any(|h| h["thread_id"] == m.thread_id) {
                held.push(json!({ "thread_id": m.thread_id, "reason": reason }));
            }
            continue;
        }
        messages.push(json!({
            "rowid": m.rowid, "id": m.id, "thread_id": m.thread_id,
            "r2_prefixes": r2_prefixes(owner, &m.id), "vector_ids": vector_ids(s, m.rowid)?,
        }));
    }
    // A held thread with no message on this page is still reported for a thread target.
    if let Target::Thread { id } = target
        && messages.is_empty()
        && held.is_empty()
    {
        #[derive(Deserialize)]
        struct H {
            hold_json: Option<String>,
        }
        if let Some(h) = s.first::<H>(
            "SELECT hold_json FROM threads WHERE id = ?1",
            &[id.as_str().into()],
        )? && let Some(reason) = hold_active(h.hold_json.as_deref(), now)
        {
            held.push(json!({ "thread_id": id, "reason": reason }));
        }
    }
    Ok(json!({ "messages": messages, "held": held, "next_after_rowid": next }))
}

/// Deletes the rows of `rowids` (Privacy § 6.2) and answers the counts and deleted IDs.
fn erase_rows<S: Sql>(
    s: &S,
    rowids: &[i64],
    counterparty: Option<&Target>,
    now: i64,
    hash_key: &[u8; 32],
) -> PResult<Value> {
    let list = serde_json::to_string(rowids).unwrap_or_else(|_| "[]".into());
    #[derive(Deserialize)]
    struct R {
        rowid: i64,
        id: String,
        thread_seq: i64,
    }
    let rows: Vec<R> = s.query(
        "SELECT rowid, id, thread_seq FROM messages WHERE rowid IN (SELECT value FROM json_each(?1))",
        &[list.as_str().into()],
    )?;
    let attachments = count(
        s,
        "SELECT COUNT(*) AS n FROM attachments WHERE message_rowid IN (SELECT value FROM json_each(?1))",
        &list,
    )?;
    let refs = count(
        s,
        "SELECT COUNT(*) AS n FROM refs WHERE message_rowid IN (SELECT value FROM json_each(?1))",
        &list,
    )?;
    let screenings = count(
        s,
        "SELECT COUNT(*) AS n FROM screenings WHERE message_rowid IN (SELECT value FROM json_each(?1))",
        &list,
    )?;
    let feedback = count(
        s,
        "SELECT COUNT(*) AS n FROM screening_feedback WHERE message_rowid IN (SELECT value FROM json_each(?1))",
        &list,
    )?;
    let mut events = 0i64;
    let mut event_ids: Vec<String> = Vec::new();
    let mut fts = 0i64;
    let mut seqs: Vec<i64> = Vec::new();
    for r in &rows {
        fts += s
            .exec("DELETE FROM fts WHERE rowid = ?1", &[r.rowid.into()])?
            .changes as i64;
        s.exec("DELETE FROM fts_tri WHERE rowid = ?1", &[r.rowid.into()])?;
        #[derive(Deserialize)]
        struct Ev {
            event_id: String,
        }
        event_ids.extend(
            s.query::<Ev>(
                "SELECT event_id FROM outbox WHERE json_extract(payload_json, '$.data.message_id') = ?1 OR json_extract(payload_json, '$.data.message.id') = ?1",
                &[r.id.as_str().into()],
            )?
            .into_iter()
            .map(|e| e.event_id),
        );
        events += s
            .exec(
                "DELETE FROM outbox WHERE json_extract(payload_json, '$.data.message_id') = ?1 OR json_extract(payload_json, '$.data.message.id') = ?1",
                &[r.id.as_str().into()],
            )?
            .changes as i64;
        s.exec(
            "DELETE FROM idempotency WHERE message_id = ?1",
            &[r.id.as_str().into()],
        )?;
        for q in [
            "DELETE FROM deliveries WHERE message_rowid = ?1",
            "DELETE FROM attachments WHERE message_rowid = ?1",
            "DELETE FROM labels WHERE message_rowid = ?1",
            "DELETE FROM refs WHERE message_rowid = ?1",
            "DELETE FROM chunks WHERE message_rowid = ?1",
            "DELETE FROM verifications WHERE message_rowid = ?1",
            "DELETE FROM screenings WHERE message_rowid = ?1",
            "DELETE FROM screening_feedback WHERE message_rowid = ?1",
        ] {
            s.exec(q, &[r.rowid.into()])?;
        }
        s.exec("DELETE FROM messages WHERE rowid = ?1", &[r.rowid.into()])?;
        s.exec(
            "INSERT INTO erased_ids (message_id, erased_at) VALUES (?1, ?2) ON CONFLICT (message_id) DO NOTHING",
            &[r.id.as_str().into(), now.into()],
        )?;
        if !seqs.contains(&r.thread_seq) {
            seqs.push(r.thread_seq);
        }
    }
    let address = match counterparty {
        Some(Target::Counterparty { address }) => Some(address.to_ascii_lowercase()),
        _ => None,
    };
    let hash = match counterparty {
        Some(Target::Counterparty { address }) => {
            Some(address_hash(hash_key, &address.to_ascii_lowercase()))
        }
        Some(Target::CounterpartyHash { hash }) => Some(hash.clone()),
        _ => None,
    };
    for seq in &seqs {
        let left = s
            .first::<N>(
                "SELECT COUNT(*) AS n FROM messages WHERE thread_seq = ?1",
                &[(*seq).into()],
            )?
            .map_or(0, |n| n.n);
        if left == 0 {
            s.exec("DELETE FROM threads WHERE seq = ?1", &[(*seq).into()])?;
        } else {
            threads::recompute(s, *seq, &[], &[], None, false)?;
        }
    }
    if let Some(h) = &hash {
        // The counterparty leaves every surviving thread and the contacts.
        #[derive(Deserialize)]
        struct T {
            seq: i64,
            participants_json: String,
        }
        let all: Vec<T> = s.query("SELECT seq, participants_json FROM threads", &[])?;
        for t in all {
            let list: Vec<Value> = serde_json::from_str(&t.participants_json).unwrap_or_default();
            let kept: Vec<&Value> = list
                .iter()
                .filter(|p| {
                    let a = p["address"].as_str().unwrap_or("").to_ascii_lowercase();
                    Some(&a) != address.as_ref() && address_hash(hash_key, &a) != *h
                })
                .collect();
            if kept.len() != list.len() {
                s.exec(
                    "UPDATE threads SET participants_json = ?2 WHERE seq = ?1",
                    &[
                        t.seq.into(),
                        serde_json::to_string(&kept).unwrap_or_default().into(),
                    ],
                )?;
            }
        }
        #[derive(Deserialize)]
        struct C {
            address: String,
        }
        for c in s.query::<C>("SELECT address FROM contacts", &[])? {
            let a = c.address.to_ascii_lowercase();
            if Some(&a) == address.as_ref() || address_hash(hash_key, &a) == *h {
                s.exec(
                    "DELETE FROM contacts WHERE address = ?1",
                    &[c.address.as_str().into()],
                )?;
            }
        }
        s.exec(
            "DELETE FROM delivery_stubs WHERE address_hash = ?1",
            &[h.as_str().into()],
        )?;
    }
    Ok(json!({
        "counts": {
            "messages_deleted": rows.len(), "attachments_deleted": attachments, "fts_rows_deleted": fts,
            "refs_deleted": refs, "events_deleted": events, "screenings_deleted": screenings,
            "screening_feedback_deleted": feedback,
        },
        "message_ids": rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        "event_ids": event_ids,
    }))
}

/// Messages still matching the target outside held threads, plus any erased ID still present.
fn probe<S: Sql>(
    s: &S,
    owner: &Owner<'_>,
    target: &Target,
    ids: &[String],
    now: i64,
    hash_key: &[u8; 32],
) -> PResult<i64> {
    let mut hits = count(
        s,
        "SELECT COUNT(*) AS n FROM messages WHERE id IN (SELECT value FROM json_each(?1))",
        &serde_json::to_string(ids).unwrap_or_default(),
    )?;
    if !matches!(target, Target::All) {
        let mut after = 0;
        loop {
            let p = plan(s, owner, target, after, 500, now, hash_key)?;
            hits += p["messages"].as_array().map_or(0, |m| m.len() as i64);
            match p["next_after_rowid"].as_i64() {
                Some(n) => after = n,
                None => break,
            }
        }
    }
    Ok(hits)
}

/// Every table's row count, for the receipt of a wipe.
fn count_all<S: Sql>(s: &S) -> PResult<Value> {
    let c = |q: &str| -> PResult<i64> { Ok(s.first::<N>(q, &[])?.map_or(0, |n| n.n)) };
    let messages = c("SELECT COUNT(*) AS n FROM messages")?;
    Ok(json!({
        "messages_deleted": messages, "attachments_deleted": c("SELECT COUNT(*) AS n FROM attachments")?,
        "fts_rows_deleted": messages, "refs_deleted": c("SELECT COUNT(*) AS n FROM refs")?,
        "events_deleted": c("SELECT COUNT(*) AS n FROM outbox")?,
        "drafts_deleted": c("SELECT COUNT(*) AS n FROM drafts")?, "uploads_deleted": c("SELECT COUNT(*) AS n FROM uploads")?,
        "screenings_deleted": c("SELECT COUNT(*) AS n FROM screenings")?,
        "screening_feedback_deleted": c("SELECT COUNT(*) AS n FROM screening_feedback")?,
    }))
}

/// `ActiveHolds`: clears holds whose `until` passed and answers how many threads are still held.
fn active_holds<S: Sql>(s: &S, now: i64) -> PResult<Value> {
    #[derive(Deserialize)]
    struct H {
        id: String,
        hold_json: Option<String>,
    }
    let rows: Vec<H> = s.query(
        "SELECT id, hold_json FROM threads WHERE hold_json IS NOT NULL",
        &[],
    )?;
    let mut held = 0;
    let mut expired = Vec::new();
    for r in rows {
        if hold_active(r.hold_json.as_deref(), now).is_some() {
            held += 1;
        } else {
            s.exec(
                "UPDATE threads SET hold_json = NULL WHERE id = ?1",
                &[r.id.as_str().into()],
            )?;
            expired.push(r.id);
        }
    }
    Ok(json!({ "held": held, "expired": expired }))
}

#[derive(Deserialize, Serialize)]
struct RetRow {
    rowid: i64,
    id: String,
    raw_r2_key: Option<String>,
    received_at: i64,
    direction: String,
    hold_json: Option<String>,
}

fn retention_plan<S: Sql>(
    s: &S,
    owner: &Owner<'_>,
    raw_cutoff: Option<i64>,
    message_cutoff: Option<i64>,
    after_rowid: i64,
    limit: u32,
    now: i64,
) -> PResult<Value> {
    let cutoff = message_cutoff.or(raw_cutoff).unwrap_or(i64::MIN);
    let rows: Vec<RetRow> = s.query(
        "SELECT m.rowid, m.id, m.raw_r2_key, m.received_at, m.direction, t.hold_json FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE m.rowid > ?1 AND m.received_at < ?2 ORDER BY m.rowid LIMIT ?3",
        &[after_rowid.into(), cutoff.into(), i64::from(limit).into()],
    )?;
    let next = (rows.len() as u32 == limit)
        .then(|| rows.last().map(|r| r.rowid))
        .flatten();
    let base = format!("t/{}/i/{}", owner.tenant_id, owner.identity_id);
    let mut out = Vec::new();
    for r in rows {
        if hold_active(r.hold_json.as_deref(), now).is_some() {
            continue;
        }
        let raw_keys: Vec<String> = match (&r.raw_r2_key, r.direction.as_str()) {
            (Some(k), _) => vec![k.clone()],
            (None, _) => Vec::new(),
        };
        out.push(json!({
            "rowid": r.rowid, "id": r.id, "raw_keys": raw_keys,
            "out_key": (r.direction == "outbound").then(|| format!("{base}/out/{}.eml", r.id)),
            "r2_prefixes": r2_prefixes(owner, &r.id), "vector_ids": vector_ids(s, r.rowid)?,
        }));
    }
    Ok(json!({ "messages": out, "next_after_rowid": next }))
}

/// One page of the export (Privacy § 9.1): message records and their raw keys.
fn export_batch<S: Sql>(
    s: &S,
    owner: &Owner<'_>,
    target: &Target,
    after_rowid: i64,
    limit: u32,
    hash_key: &[u8; 32],
) -> PResult<Value> {
    #[derive(Deserialize)]
    struct E {
        rowid: i64,
        id: String,
        thread_id: String,
        direction: String,
        status: String,
        kind: String,
        from_address: Option<String>,
        from_name: Option<String>,
        to_json: String,
        cc_json: String,
        bcc_json: String,
        reply_to_json: String,
        subject: Option<String>,
        sent_at: Option<i64>,
        received_at: i64,
        flags_json: String,
        verdict: Option<String>,
        known_sender: Option<i64>,
        triage_json: Option<String>,
        raw_r2_key: Option<String>,
        text: Option<String>,
    }
    let page: Vec<MsgRow> = s.query(
        concat!("SELECT ", msg_cols!(), " FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE m.rowid > ?1 ORDER BY m.rowid LIMIT ?2"),
        &[after_rowid.into(), i64::from(limit).into()],
    )?;
    let next = (page.len() as u32 == limit)
        .then(|| page.last().map(|r| r.rowid))
        .flatten();
    let mut out = Vec::new();
    for m in page {
        if !m.matches(s, target, hash_key)? {
            continue;
        }
        let Some(e) = s.first::<E>(
            "SELECT m.rowid, m.id, t.id AS thread_id, m.direction, m.status, m.kind, m.from_address, m.from_name, m.to_json, m.cc_json, m.bcc_json, m.reply_to_json, m.subject, m.sent_at, m.received_at, m.flags_json, m.verdict, m.known_sender, m.triage_json, m.raw_r2_key, m.text FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE m.rowid = ?1",
            &[m.rowid.into()],
        )?
        else {
            continue;
        };
        #[derive(Deserialize, Serialize)]
        struct A {
            id: String,
            filename: Option<String>,
            content_type: String,
            size: i64,
            sha256: String,
        }
        let atts: Vec<A> = s.query(
            "SELECT id, filename, content_type, size, sha256 FROM attachments WHERE message_rowid = ?1",
            &[e.rowid.into()],
        )?;
        #[derive(Deserialize, Serialize)]
        struct D {
            address: String,
            field: String,
            status: String,
            smtp_code: Option<String>,
            updated_at: i64,
        }
        let deliveries: Option<Vec<D>> = if e.direction == "outbound" {
            Some(s.query(
                "SELECT address, field, status, smtp_code, updated_at FROM deliveries WHERE message_rowid = ?1",
                &[e.rowid.into()],
            )?)
        } else {
            None
        };
        let labels: Vec<Value> = s
            .query::<N2>(
                "SELECT label AS v FROM labels WHERE message_rowid = ?1",
                &[e.rowid.into()],
            )?
            .into_iter()
            .map(|l| Value::String(l.v))
            .collect();
        let bcc: Value = match target {
            // A counterparty export discloses only the counterparty in Bcc.
            Target::Counterparty { address } => {
                let a = address.to_ascii_lowercase();
                Value::Array(
                    addrs_of(&e.bcc_json)
                        .into_iter()
                        .filter(|b| *b == a)
                        .map(|b| json!({ "address": b, "name": "" }))
                        .collect(),
                )
            }
            _ => serde_json::from_str(&e.bcc_json).unwrap_or_else(|_| json!([])),
        };
        let raw_key = e.raw_r2_key.clone().or_else(|| {
            (e.direction == "outbound").then(|| {
                format!(
                    "t/{}/i/{}/out/{}.eml",
                    owner.tenant_id, owner.identity_id, e.id
                )
            })
        });
        out.push(json!({
            "id": e.id, "identity_id": owner.identity_id, "thread_id": e.thread_id,
            "direction": e.direction, "status": e.status, "kind": e.kind,
            "from": { "address": e.from_address, "name": e.from_name.unwrap_or_default() },
            "to": serde_json::from_str::<Value>(&e.to_json).unwrap_or_else(|_| json!([])),
            "cc": serde_json::from_str::<Value>(&e.cc_json).unwrap_or_else(|_| json!([])),
            "bcc": bcc,
            "reply_to": serde_json::from_str::<Value>(&e.reply_to_json).unwrap_or_else(|_| json!([])),
            "subject": e.subject, "sent_at": e.sent_at, "received_at": e.received_at,
            "labels": labels, "flags": serde_json::from_str::<Value>(&e.flags_json).unwrap_or_else(|_| json!([])),
            "trust": { "verdict": e.verdict, "known_sender": e.known_sender == Some(1),
                       "quarantined": e.status == "quarantined" },
            "triage": e.triage_json.as_deref().and_then(|t| serde_json::from_str::<Value>(t).ok()),
            "attachments": atts, "deliveries": deliveries, "calendar": null,
            "screening": null, "screening_feedback": [],
            "raw_key": raw_key, "text": e.text,
            "fields": { "to": e.to_json, "cc": e.cc_json, "reply_to": e.reply_to_json },
        }));
    }
    Ok(json!({ "messages": out, "next_after_rowid": next }))
}

#[derive(Deserialize)]
struct N2 {
    v: String,
}

/// The operations that run inside one transaction (all but `WipeAll`, which the caller runs).
pub fn handle<S: Sql>(
    s: &S,
    owner: &Owner<'_>,
    op: PrivacyOp,
    now: i64,
    hash_key: &[u8; 32],
) -> PResult<Value> {
    match op {
        PrivacyOp::ErasurePlan {
            target,
            after_rowid,
            limit,
        } => plan(
            s,
            owner,
            &target,
            after_rowid,
            limit.clamp(1, 500),
            now,
            hash_key,
        ),
        PrivacyOp::EraseRows {
            rowids,
            counterparty,
            mode: _,
        } => s.transaction_sync(|t| erase_rows(t, &rowids, counterparty.as_ref(), now, hash_key)),
        PrivacyOp::ProbeKeyword {
            target,
            message_ids,
        } => Ok(json!({ "hits": probe(s, owner, &target, &message_ids, now, hash_key)? })),
        PrivacyOp::CountAll => count_all(s),
        PrivacyOp::WipeAll => Ok(Value::Null),
        PrivacyOp::ForgetSender { target } => {
            let n = match target {
                Target::Counterparty { address } => {
                    s.exec(
                        "DELETE FROM rate_windows WHERE lower(sender) = lower(?1)",
                        &[address.into()],
                    )?
                    .changes
                }
                Target::CounterpartyHash { hash } => {
                    #[derive(Deserialize)]
                    struct W {
                        sender: String,
                    }
                    let mut n = 0;
                    for w in s.query::<W>("SELECT DISTINCT sender FROM rate_windows", &[])? {
                        if address_hash(hash_key, &w.sender.to_ascii_lowercase()) == hash {
                            n += s
                                .exec(
                                    "DELETE FROM rate_windows WHERE sender = ?1",
                                    &[w.sender.into()],
                                )?
                                .changes;
                        }
                    }
                    n
                }
                _ => 0,
            };
            Ok(json!({ "deleted": n }))
        }
        PrivacyOp::ActiveHolds { now_ms } => s.transaction_sync(|t| active_holds(t, now_ms)),
        PrivacyOp::SetHold {
            thread_id,
            reason,
            until,
            set_by,
        } => {
            let hold = json!({ "reason": reason, "until": until, "set_by": set_by, "set_at": now });
            let n = s
                .exec(
                    "UPDATE threads SET hold_json = ?2 WHERE id = ?1",
                    &[thread_id.into(), hold.to_string().into()],
                )?
                .changes;
            Ok(json!({ "found": n == 1, "hold": hold }))
        }
        PrivacyOp::ClearHold { thread_id } => {
            let n = s
                .exec(
                    "UPDATE threads SET hold_json = NULL WHERE id = ?1",
                    &[thread_id.into()],
                )?
                .changes;
            Ok(json!({ "found": n == 1 }))
        }
        PrivacyOp::RetentionPlan {
            raw_cutoff_ms,
            message_cutoff_ms,
            after_rowid,
            limit,
        } => retention_plan(
            s,
            owner,
            raw_cutoff_ms,
            message_cutoff_ms,
            after_rowid,
            limit.clamp(1, 500),
            now,
        ),
        PrivacyOp::PurgeRaw { message_ids } => {
            let n = s
                .exec(
                    "UPDATE messages SET raw_r2_key = NULL WHERE id IN (SELECT value FROM json_each(?1)) AND raw_r2_key IS NOT NULL",
                    &[serde_json::to_string(&message_ids).unwrap_or_default().into()],
                )?
                .changes;
            Ok(json!({ "purged": n }))
        }
        PrivacyOp::PurgeEvents { cutoff_ms } => {
            let n = s
                .exec(
                    "DELETE FROM outbox WHERE dispatched_at IS NOT NULL AND occurred_at < ?1",
                    &[cutoff_ms.into()],
                )?
                .changes;
            Ok(json!({ "events_deleted": n }))
        }
        PrivacyOp::Hygiene { now_ms } => {
            let i = s
                .exec(
                    "DELETE FROM idempotency WHERE expires_at < ?1",
                    &[now_ms.into()],
                )?
                .changes;
            let v = s
                .exec(
                    "DELETE FROM verifications WHERE expires_at < ?1",
                    &[now_ms.into()],
                )?
                .changes;
            Ok(json!({ "idempotency": i, "verifications": v }))
        }
        PrivacyOp::ExportBatch {
            target,
            after_rowid,
            limit,
        } => export_batch(
            s,
            owner,
            &target,
            after_rowid,
            limit.clamp(1, 200),
            hash_key,
        ),
        PrivacyOp::MessagesInRange {
            after_rowid,
            limit,
            after,
            before,
        } => {
            #[derive(Deserialize, Serialize)]
            struct M {
                rowid: i64,
                id: String,
                direction: String,
                raw_r2_key: Option<String>,
                received_at: i64,
            }
            let rows: Vec<M> = s.query(
                "SELECT rowid, id, direction, raw_r2_key, received_at FROM messages WHERE rowid > ?1 AND received_at >= ?2 AND received_at < ?3 ORDER BY rowid LIMIT ?4",
                &[
                    after_rowid.into(),
                    after.unwrap_or(i64::MIN).into(),
                    before.unwrap_or(i64::MAX).into(),
                    i64::from(limit.clamp(1, 500)).into(),
                ],
            )?;
            let next = (rows.len() as u32 == limit.clamp(1, 500))
                .then(|| rows.last().map(|r| r.rowid))
                .flatten();
            Ok(json!({ "messages": rows, "next_after_rowid": next }))
        }
    }
}
