//! The `export` job ([Privacy and erasure § 9]): a subject-access ZIP of one identity's mailbox, or
//! of every message to or from a counterparty across the tenant. The platform has no multipart
//! upload, so `collect` writes each batch's ZIP entries as a part object and `finish_zip` joins the
//! parts with `messages.json`, the central directory and the end record. An export that overlapped
//! an erasure restarts once and then fails (§ 6.11).
//!
//! [Privacy and erasure § 9]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::zip;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::r2::{BlobBody, BlobMeta};
use pylota_identity_platform::{BlobStore as _, ControlDb as _, Ids as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Ctx, Slice, StepErr, StepResult};
use crate::db::audit;
use crate::errors::ApiError;
use crate::mailbox::MailboxRequest;
use crate::mailbox::privacy::{PrivacyOp, Target};

pub const STEPS: &[&str] = &["init", "collect", "finish_zip", "complete"];
/// The ZIP lives this long after the export is created.
pub const EXPORT_TTL_MS: i64 = 7 * 86_400_000;
/// Statuses read through the API only with `quarantine:review` (I14).
const WITHHELD: &[&str] = &["quarantined", "screening", "hidden", "throttled"];

fn migrate<S: Sql>(s: &S) -> pylota_identity_platform::PResult<()> {
    for q in [
        "CREATE TABLE IF NOT EXISTS xids (idx INTEGER PRIMARY KEY, identity_id TEXT NOT NULL UNIQUE, mailbox_do_id TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS records (seq INTEGER PRIMARY KEY, json TEXT NOT NULL)",
    ] {
        s.exec(q, &[])?;
    }
    Ok(())
}

#[derive(Deserialize, Clone)]
struct X {
    idx: i64,
    identity_id: String,
    mailbox_do_id: String,
}

fn export_id<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> String {
    c.params["export_id"].as_str().unwrap_or("").to_owned()
}

fn zip_key<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> String {
    format!("t/{}/exports/{}.zip", c.tenant(), export_id(c))
}

fn part_key<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, n: i64) -> String {
    format!("t/{}/exports/{}.part{n:05}", c.tenant(), export_id(c))
}

fn num<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, k: &str) -> i64 {
    c.get(k)
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

pub async fn run<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    step: &str,
    cursor: Option<&str>,
) -> StepResult {
    migrate(c.sql)?;
    match step {
        "init" => init(c).await,
        "collect" => collect(c, cursor).await,
        "finish_zip" => finish_zip(c).await,
        "complete" => complete(c).await,
        _ => Err(StepErr::new("unknown_step")),
    }
}

/// Step 1: waits for any erasure of the tenant, then the targets and a fresh set of parts.
async fn init<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    #[derive(Deserialize)]
    struct N {
        n: i64,
    }
    let running = c
        .p
        .db()
        .first::<N>(Stmt::new(
            "SELECT COUNT(*) AS n FROM erasure_requests WHERE tenant_id = ?1 AND status IN ('queued','running')",
            vec![c.tenant().into()],
        ))
        .await?
        .map_or(0, |n| n.n);
    if running > 0 {
        return Ok(Slice::Wait { after_ms: 60_000 });
    }
    delete_parts(c).await?;
    c.sql.exec("DELETE FROM xids", &[])?;
    c.sql.exec("DELETE FROM records", &[])?;
    c.sql
        .exec("DELETE FROM meta WHERE k LIKE 'zip_cd:%'", &[])?;
    for k in ["parts", "offset", "count"] {
        c.set(k, "0")?;
    }
    c.set("upload_id", &format!("parts:{}", export_id(c)))?;
    c.set("collect_started_at", &c.now.to_string())?;
    #[derive(Deserialize)]
    struct I {
        id: String,
        mailbox_do_id: String,
    }
    // The system identity's mailbox is never read by an export (A15).
    let rows: Vec<I> = c
        .p
        .db()
        .query(Stmt::new(
            "SELECT id, mailbox_do_id FROM identities WHERE tenant_id = ?1 AND is_system = 0 AND mailbox_do_id <> '' AND status <> 'deleted' AND (?2 IS NULL OR id = ?2) ORDER BY id",
            vec![c.tenant().into(), c.params["identity_id"].as_str().map(str::to_owned).into()],
        ))
        .await?;
    for r in rows {
        c.sql.exec(
            "INSERT OR IGNORE INTO xids (identity_id, mailbox_do_id) VALUES (?1, ?2)",
            &[r.id.into(), r.mailbox_do_id.into()],
        )?;
    }
    Ok(Slice::done())
}

fn target<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> Result<Target, StepErr> {
    if c.params["scope"] == "identity" {
        return Ok(Target::All);
    }
    Ok(match c.get("target_address")? {
        Some(address) => Target::Counterparty { address },
        None => Target::CounterpartyHash {
            hash: c.params["counterparty_hash"]
                .as_str()
                .unwrap_or("")
                .to_owned(),
        },
    })
}

/// `Bcc:` header lines (with their folded continuations) removed from a raw message.
pub fn strip_bcc(raw: &[u8]) -> (Vec<u8>, bool) {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = match text.find("\r\n\r\n") {
        Some(i) => (&text[..i + 2], &text[i + 2..]),
        None => (&text[..], ""),
    };
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    let mut removed = false;
    for line in head.split_inclusive("\r\n") {
        if line.starts_with([' ', '\t']) {
            if !skipping {
                out.push_str(line);
            }
            continue;
        }
        skipping = line.to_ascii_lowercase().starts_with("bcc:");
        removed |= skipping;
        if !skipping {
            out.push_str(line);
        }
    }
    out.push_str(body);
    (out.into_bytes(), removed)
}

/// A message rebuilt from its stored fields when the raw MIME is past `raw_days`.
fn reconstruct(m: &Value) -> Vec<u8> {
    let list = |v: &Value| {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x["address"].as_str().or(x.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    format!(
        "From: {}\r\nTo: {}\r\nCc: {}\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: <{}@export.invalid>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
        m["from"]["address"].as_str().unwrap_or(""),
        list(&m["to"]),
        list(&m["cc"]),
        m["subject"].as_str().unwrap_or("").replace(['\r', '\n'], " "),
        pylota_identity_core::days::rfc3339(m["received_at"].as_i64().unwrap_or(0)),
        m["id"].as_str().unwrap_or(""),
        m["text"].as_str().unwrap_or(""),
    )
    .into_bytes()
}

fn stamp(ms: i64) -> String {
    pylota_identity_core::days::rfc3339(ms)
        .replace(['-', ':'], "")
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches('Z')
        .to_owned()
        + "Z"
}

/// Step 2: one page of one identity's messages into one part (cursor `{idx}:{rowid}`).
async fn collect<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, cursor: Option<&str>) -> StepResult {
    let all: Vec<X> = c.sql.query(
        "SELECT idx, identity_id, mailbox_do_id FROM xids ORDER BY idx",
        &[],
    )?;
    let (idx, rest) = cursor
        .and_then(|c| c.split_once(':'))
        .map_or((0, ""), |(i, r)| (i.parse().unwrap_or(0), r));
    let Some(x) = all.iter().find(|x| x.idx >= idx).cloned() else {
        return Ok(Slice::done());
    };
    let after: i64 = if x.idx == idx {
        rest.parse().unwrap_or(0)
    } else {
        0
    };
    let page = c
        .mailbox(
            &x.identity_id,
            &x.mailbox_do_id,
            MailboxRequest::Privacy(PrivacyOp::ExportBatch {
                target: target(c)?,
                after_rowid: after,
                limit: 50,
            }),
        )
        .await?
        .map_err(StepErr::from)?;
    let include_held = c.params["include_held"] == true;
    let mut offset = num(c, "offset");
    let mut count = num(c, "count");
    let mut part: Vec<u8> = Vec::new();
    let mut cd: Vec<u8> = Vec::new();
    for m in page["messages"].as_array().cloned().unwrap_or_default() {
        let id = m["id"].as_str().unwrap_or("").to_owned();
        let status = m["status"].as_str().unwrap_or("");
        let mut record = json!({
            "id": id, "identity_id": x.identity_id, "thread_id": m["thread_id"], "direction": m["direction"],
            "status": status, "kind": m["kind"], "from": m["from"], "to": m["to"], "cc": m["cc"],
            "bcc": m["bcc"], "reply_to": m["reply_to"], "subject": m["subject"],
            "sent_at": m["sent_at"].as_i64().map(pylota_identity_core::days::rfc3339),
            "received_at": m["received_at"].as_i64().map(pylota_identity_core::days::rfc3339),
            "labels": m["labels"], "flags": m["flags"], "trust": m["trust"], "triage": m["triage"],
            "attachments": m["attachments"], "deliveries": m["deliveries"], "calendar": null,
            "screening": null, "screening_feedback": [],
        });
        if WITHHELD.contains(&status) && !include_held {
            record["content_withheld"] = "quarantine_review_required".into();
            record["eml_path"] = Value::Null;
        } else {
            let raw = match m["raw_key"].as_str() {
                Some(k) => c.p.blobs().get(k).await?.map(|b| b.body.to_vec()),
                None => None,
            };
            let (source, bytes) = match raw {
                Some(r) => ("original", r),
                None => ("reconstructed", reconstruct(&m)),
            };
            let (bytes, redacted) = strip_bcc(&bytes);
            let path = format!(
                "eml/{}/{}_{id}.eml",
                x.identity_id,
                stamp(m["received_at"].as_i64().unwrap_or(0))
            );
            let (local, dir) = zip::entry(&path, &bytes, offset as u64);
            offset += local.len() as i64;
            count += 1;
            part.extend_from_slice(&local);
            cd.extend_from_slice(&dir);
            record["eml_path"] = path.into();
            record["eml_source"] = source.into();
            record["bcc_redacted"] = redacted.into();
        }
        c.sql.exec(
            "INSERT INTO records (json) VALUES (?1)",
            &[record.to_string().into()],
        )?;
    }
    if !part.is_empty() {
        let n = num(c, "parts");
        c.p.blobs()
            .put(&part_key(c, n), BlobBody::Bytes(part), &BlobMeta::default())
            .await?;
        c.set(
            &format!("zip_cd:{n:05}"),
            &pylota_identity_core::links::encode_base64(&cd),
        )?;
        c.set("parts", &(n + 1).to_string())?;
        c.set("offset", &offset.to_string())?;
        c.set("count", &count.to_string())?;
    }
    let next = page["next_after_rowid"].as_i64();
    let cursor = match next {
        Some(n) => format!("{}:{n}", x.idx),
        None => format!("{}:", x.idx + 1),
    };
    Ok(if next.is_some() || all.iter().any(|y| y.idx > x.idx) {
        Slice::More {
            cursor,
            counts: json!({}),
        }
    } else {
        Slice::done()
    })
}

/// Step 3: `messages.json`, the central directory and the end record; the parts joined into the ZIP.
async fn finish_zip<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    #[derive(Deserialize)]
    struct R {
        json: String,
    }
    let records: Vec<Value> = c
        .sql
        .query::<R>("SELECT json FROM records ORDER BY seq", &[])?
        .into_iter()
        .filter_map(|r| serde_json::from_str(&r.json).ok())
        .collect();
    let identities: Vec<String> = c
        .sql
        .query::<X>(
            "SELECT idx, identity_id, mailbox_do_id FROM xids ORDER BY idx",
            &[],
        )?
        .into_iter()
        .map(|x| x.identity_id)
        .collect();
    let doc = json!({
        "export_id": export_id(c), "tenant_id": c.tenant(), "scope": c.params["scope"],
        "counterparty_hash": c.params["counterparty_hash"],
        "generated_at": pylota_identity_core::days::rfc3339(c.now),
        "generator": concat!("pylota-identity/", env!("CARGO_PKG_VERSION")),
        "identities": identities, "messages": records,
    });
    let mut offset = num(c, "offset");
    let mut zip_bytes: Vec<u8> = Vec::new();
    let parts = num(c, "parts");
    let mut cd: Vec<u8> = Vec::new();
    for n in 0..parts {
        let b =
            c.p.blobs()
                .get(&part_key(c, n))
                .await?
                .ok_or_else(|| StepErr::new("export_part_missing"))?;
        zip_bytes.extend_from_slice(&b.body.to_vec());
        let d = c
            .get(&format!("zip_cd:{n:05}"))?
            .and_then(|v| pylota_identity_core::links::decode_base64(&v))
            .unwrap_or_default();
        cd.extend_from_slice(&d);
    }
    let body = serde_json::to_vec_pretty(&doc).unwrap_or_default();
    let (local, dir) = zip::entry("messages.json", &body, offset as u64);
    offset += local.len() as i64;
    zip_bytes.extend_from_slice(&local);
    cd.extend_from_slice(&dir);
    let count = num(c, "count") + 1;
    zip_bytes.extend_from_slice(&cd);
    zip_bytes.extend_from_slice(&zip::end(count as u64, cd.len() as u64, offset as u64));
    let size = zip_bytes.len() as i64;
    c.p.blobs()
        .put(
            &zip_key(c),
            BlobBody::Bytes(zip_bytes),
            &BlobMeta {
                content_type: Some("application/zip".into()),
                custom: Vec::new(),
            },
        )
        .await?;
    delete_parts(c).await?;
    c.del("upload_id")?;
    c.set("size", &size.to_string())?;
    Ok(Slice::done())
}

/// Step 4 (§ 6.11): one conditional statement marks the export completed only if no erasure of the
/// tenant ran since collection started; otherwise the ZIP goes and the job restarts once.
async fn complete<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    #[derive(Deserialize)]
    struct Ca {
        created_at: i64,
    }
    let created =
        c.p.db()
            .first::<Ca>(Stmt::new(
                "SELECT created_at FROM exports WHERE id = ?1",
                vec![export_id(c).into()],
            ))
            .await?
            .map_or(c.now, |r| r.created_at);
    let m = c
        .p
        .db()
        .batch(vec![Stmt::new(
            "UPDATE exports SET status = 'completed', r2_key = ?2, size = ?3, expires_at = ?4 WHERE id = ?1 AND status IN ('queued','running') AND NOT EXISTS ( \
               SELECT 1 FROM erasure_requests WHERE tenant_id = ?5 AND (status IN ('queued','running') OR completed_at > ?6))",
            vec![
                export_id(c).into(),
                zip_key(c).into(),
                num(c, "size").into(),
                (created + EXPORT_TTL_MS).into(),
                c.tenant().into(),
                num(c, "collect_started_at").into(),
            ],
        )])
        .await?;
    if m.first().is_some_and(|m| m.changes == 1) {
        super::emit(
            c.p,
            c.tenant_id.as_deref(),
            None,
            &c.job_id,
            "export.completed",
            &json!({ "export_id": export_id(c), "scope": c.params["scope"] }),
            c.now,
        )
        .await?;
        c.p.db()
            .execute(audit::insert(
                &c.p.ids().new_id(IdPrefix::Audit),
                Some(c.tenant()),
                None,
                "export.completed",
                "export",
                &export_id(c),
                &json!({}),
                &format!("req_{}", &c.job_id[4..]),
                c.now,
            ))
            .await?;
        return Ok(Slice::done());
    }
    // An erasure overlapped collection: the ZIP may hold what it erased.
    delete_keys(c.p, &[zip_key(c)]).await?;
    if c.get("restarted")?.is_some() {
        return Err(StepErr::new("superseded_by_erasure"));
    }
    c.set("restarted", "1")?;
    c.sql.exec(
        "UPDATE steps SET status = 'pending', cursor = NULL, counts_json = '{}' WHERE name IN ('init','collect','finish_zip')",
        &[],
    )?;
    Ok(Slice::More {
        cursor: "restart".into(),
        counts: json!({}),
    })
}

async fn delete_keys<P: Platform>(p: &P, keys: &[String]) -> Result<(), StepErr> {
    p.blobs().delete(keys).await?;
    if let Some(b) = p.backup() {
        b.delete(keys).await?;
    }
    Ok(())
}

/// The parts of an unfinished ZIP (the multipart upload's abort).
async fn delete_parts<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> Result<(), StepErr> {
    let prefix = format!("t/{}/exports/{}.part", c.tenant(), export_id(c));
    loop {
        let page = c.p.blobs().list(&prefix, None, 1000).await?;
        if page.keys.is_empty() {
            return Ok(());
        }
        let keys: Vec<String> = page.keys.into_iter().map(|(k, _)| k).collect();
        c.p.blobs().delete(&keys).await?;
    }
}

/// Every end: the open parts go, and a canceled export's ZIP too.
pub async fn finalise<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    outcome: &str,
) -> Result<(), StepErr> {
    migrate(c.sql)?;
    if c.get("upload_id")?.is_some() {
        delete_parts(c).await?;
        c.del("upload_id")?;
    }
    if outcome != "completed" {
        delete_keys(c.p, &[zip_key(c)]).await?;
    }
    c.sql
        .exec("DELETE FROM meta WHERE k LIKE 'zip_cd:%'", &[])?;
    c.sql.exec("DELETE FROM records", &[])?;
    Ok(())
}

/// The export row's end state.
pub async fn on_end<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    outcome: &str,
    failed: Option<(&str, &str)>,
) -> Result<Value, ApiError> {
    if outcome != "completed" {
        c.p.db()
            .execute(Stmt::new(
                "UPDATE exports SET status = ?2 WHERE id = ?1 AND status IN ('queued','running')",
                vec![export_id(c).into(), outcome.into()],
            ))
            .await?;
    }
    Ok(match failed {
        Some((step, code)) => json!({ "step": step, "error": code, "details": { "reason": code } }),
        None => json!({ "size": num(c, "size") }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcc_lines_removed() {
        let raw =
            b"From: a@x\r\nBcc: secret@y,\r\n  other@z\r\nSubject: s\r\n\r\nBcc: in body stays\r\n";
        let (out, removed) = strip_bcc(raw);
        assert!(removed);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "From: a@x\r\nSubject: s\r\n\r\nBcc: in body stays\r\n"
        );
    }
}
