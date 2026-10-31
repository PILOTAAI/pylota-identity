//! The `erasure` job ([Privacy and erasure § 6]): message, thread, counterparty, identity and tenant
//! scope. Every step re-runs from its cursor with the same result; counts come from what each store
//! reported deleted. The substeps later milestones complete are named stubs at the end.
//!
//! [Privacy and erasure § 6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::address::address_hash;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::{
    BlobStore as _, ControlDb as _, PResult, Platform, VectorIndex as _,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Ctx, Slice, StepErr, StepResult};
use crate::db::identities as idb;
use crate::errors::ApiError;
use crate::mailbox::MailboxRequest;
use crate::mailbox::privacy::{EraseMode, PrivacyOp, Target};

/// One plan page.
const PAGE: u32 = 100;
/// The semantic probe waits this long between reads, at most this many times (Privacy § 6.8).
const PROBE_WAIT_MS: i64 = 10_000;
const PROBE_TRIES: i64 = 12;

/// The steps of each scope (Privacy § 6.3–6.6); a re-applied request runs only the mailbox side.
pub fn steps(params: &Value) -> Vec<&'static str> {
    let reapply = params["reapply"] == true;
    match params["scope"].as_str().unwrap_or("") {
        "message" | "thread" => vec![
            "init",
            "erase_mailbox",
            "scrub_control_plane",
            "probe",
            "receipt",
        ],
        "counterparty" => vec![
            "init",
            "erase_mailboxes",
            "scrub_control_plane",
            "probe",
            "receipt",
        ],
        "identity" if reapply => vec!["init", "erase_mailbox", "probe", "receipt"],
        "identity" => vec![
            "init",
            "tombstone_addresses",
            "erase_mailbox",
            "scrub_control_plane",
            "probe",
            "receipt",
        ],
        "tenant" if reapply => vec![
            "erase_identities",
            "sweep_vectors",
            "sweep_r2",
            "probe",
            "receipt",
        ],
        "tenant" => vec![
            "stop_routing",
            "cancel_billing",
            "remove_domains",
            "erase_identities",
            "delete_d1_rows",
            "sweep_vectors",
            "sweep_r2",
            "probe",
            "receipt",
        ],
        _ => Vec::new(),
    }
}

fn migrate<S: Sql>(s: &S) -> PResult<()> {
    for q in [
        "CREATE TABLE IF NOT EXISTS targets (idx INTEGER PRIMARY KEY, identity_id TEXT NOT NULL UNIQUE, mailbox_do_id TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS erased (id TEXT PRIMARY KEY, identity_id TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS events (id TEXT PRIMARY KEY)",
    ] {
        s.exec(q, &[])?;
    }
    Ok(())
}

#[derive(Deserialize, Clone)]
struct TargetRow {
    idx: i64,
    identity_id: String,
    mailbox_do_id: String,
}

fn targets<S: Sql>(s: &S) -> PResult<Vec<TargetRow>> {
    s.query(
        "SELECT idx, identity_id, mailbox_do_id FROM targets ORDER BY idx",
        &[],
    )
}

fn add_target<S: Sql>(s: &S, identity_id: &str, mailbox_do_id: &str) -> PResult<()> {
    s.exec(
        "INSERT OR IGNORE INTO targets (identity_id, mailbox_do_id) VALUES (?1, ?2)",
        &[identity_id.into(), mailbox_do_id.into()],
    )?;
    Ok(())
}

fn scope<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> String {
    c.params["scope"].as_str().unwrap_or("").to_owned()
}

fn era_id<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> String {
    c.params["erasure_request_id"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

/// The mailbox target of the job.
fn target<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> PResult<Target> {
    let id = c.params["target_id"].as_str().unwrap_or("").to_owned();
    Ok(match scope(c).as_str() {
        "message" => Target::Message { id },
        "thread" => Target::Thread { id },
        "counterparty" => match c.get("target_address")? {
            Some(address) => Target::Counterparty { address },
            None => Target::CounterpartyHash {
                hash: c.params["counterparty_hash"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
            },
        },
        _ => Target::All,
    })
}

async fn identity_row<P: Platform>(p: &P, id: &str) -> Result<Option<idb::Row>, StepErr> {
    Ok(p.db().first::<idb::Row>(idb::get(id)).await?)
}

/// Deletes every object under `prefix` in `BLOBS` and, when bound, `BACKUP`; answers the count.
pub async fn delete_prefix<P: Platform>(p: &P, prefix: &str) -> Result<i64, StepErr> {
    let mut n = 0;
    for store in [Some(p.blobs()), p.backup()].into_iter().flatten() {
        loop {
            let page = store.list(prefix, None, 1000).await?;
            if page.keys.is_empty() {
                break;
            }
            let keys: Vec<String> = page.keys.into_iter().map(|(k, _)| k).collect();
            store.delete(&keys).await?;
            if std::ptr::eq(store, p.blobs()) {
                n += keys.len() as i64;
            }
        }
    }
    Ok(n)
}

/// Deletes `ids` in batches of 500 from the index (and the next index during a re-embed), and
/// records them for the semantic probe.
async fn delete_vectors<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    ids: &[String],
) -> Result<i64, StepErr> {
    for batch in ids.chunks(500) {
        c.p.vectors().delete_by_ids(batch).await?;
        if let Some(next) = c.p.vectors_next() {
            next.delete_by_ids(batch).await?;
        }
    }
    c.note_vectors(ids)?;
    Ok(ids.len() as i64)
}

/// One page of a mailbox's erasure: vectors, objects, rows (Privacy § 6.3, step 2). Answers the
/// slice with the next rowid as cursor.
async fn erase_page<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    t: &TargetRow,
    target: &Target,
    after: i64,
) -> Result<(Option<i64>, Value), StepErr> {
    let plan = match c
        .mailbox(
            &t.identity_id,
            &t.mailbox_do_id,
            MailboxRequest::Privacy(PrivacyOp::ErasurePlan {
                target: target.clone(),
                after_rowid: after,
                limit: PAGE,
            }),
        )
        .await?
    {
        Ok(v) => v,
        // A wiped mailbox (a resumption after WipeAll) has nothing left.
        Err(e) if e.code == ErrorCode::IdentityNotFound => return Ok((None, json!({}))),
        Err(e) => return Err(e.into()),
    };
    for h in plan["held"].as_array().cloned().unwrap_or_default() {
        c.hold(
            h["thread_id"].as_str().unwrap_or(""),
            &t.identity_id,
            h["reason"].as_str().unwrap_or(""),
        )?;
    }
    let msgs = plan["messages"].as_array().cloned().unwrap_or_default();
    let mut counts = Map::new();
    if !msgs.is_empty() {
        let vectors: Vec<String> = msgs
            .iter()
            .flat_map(|m| m["vector_ids"].as_array().cloned().unwrap_or_default())
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        let mut r2 = 0;
        for m in &msgs {
            for pfx in m["r2_prefixes"].as_array().cloned().unwrap_or_default() {
                if let Some(pfx) = pfx.as_str() {
                    r2 += delete_prefix(c.p, pfx).await?;
                }
            }
        }
        let vn = delete_vectors(c, &vectors).await?;
        let rowids: Vec<i64> = msgs.iter().filter_map(|m| m["rowid"].as_i64()).collect();
        let counterparty = matches!(
            target,
            Target::Counterparty { .. } | Target::CounterpartyHash { .. }
        )
        .then(|| target.clone());
        let out = c
            .mailbox(
                &t.identity_id,
                &t.mailbox_do_id,
                MailboxRequest::Privacy(PrivacyOp::EraseRows {
                    rowids,
                    counterparty,
                    mode: EraseMode::Erasure,
                }),
            )
            .await?
            .map_err(StepErr::from)?;
        for id in out["message_ids"].as_array().cloned().unwrap_or_default() {
            c.sql.exec(
                "INSERT OR IGNORE INTO erased (id, identity_id) VALUES (?1, ?2)",
                &[
                    id.as_str().unwrap_or("").into(),
                    t.identity_id.as_str().into(),
                ],
            )?;
        }
        for id in out["event_ids"].as_array().cloned().unwrap_or_default() {
            c.sql.exec(
                "INSERT OR IGNORE INTO events (id) VALUES (?1)",
                &[id.as_str().unwrap_or("").into()],
            )?;
        }
        pylota_identity_core::jobs::add_counts(&mut counts, &out["counts"]);
        pylota_identity_core::jobs::add_counts(
            &mut counts,
            &json!({ "r2_objects_deleted": r2, "vectors_deleted": vn }),
        );
    }
    Ok((plan["next_after_rowid"].as_i64(), Value::Object(counts)))
}

/// Whether an identity still has an active hold.
async fn holds_of<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, t: &TargetRow) -> Result<i64, StepErr> {
    match c
        .mailbox(
            &t.identity_id,
            &t.mailbox_do_id,
            MailboxRequest::Privacy(PrivacyOp::ActiveHolds { now_ms: c.now }),
        )
        .await?
    {
        Ok(v) => Ok(v["held"].as_i64().unwrap_or(0)),
        Err(e) if e.code == ErrorCode::IdentityNotFound => Ok(0),
        Err(e) => Err(e.into()),
    }
}

/// A whole identity's mailbox (identity step 3, tenant step 4). Cursor: the rowid page, then
/// `wipe`. Without holds the R2 prefix is swept and the mailbox wiped.
async fn erase_identity_mailbox<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    t: &TargetRow,
    cursor: Option<&str>,
) -> Result<(Option<String>, Value), StepErr> {
    if cursor != Some("wipe") {
        let after = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
        let (next, counts) = erase_page(c, t, &Target::All, after).await?;
        return Ok((
            Some(next.map_or_else(|| "wipe".to_owned(), |n| n.to_string())),
            counts,
        ));
    }
    if holds_of(c, t).await? > 0 {
        return Ok((None, json!({})));
    }
    let r2 = delete_prefix(c.p, &format!("t/{}/i/{}/", c.tenant(), t.identity_id)).await?;
    match c
        .mailbox(
            &t.identity_id,
            &t.mailbox_do_id,
            MailboxRequest::Privacy(PrivacyOp::WipeAll),
        )
        .await?
    {
        Ok(_) => {}
        Err(e) if e.code == ErrorCode::IdentityNotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok((None, json!({ "r2_objects_deleted": r2 })))
}

pub async fn run<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    step: &str,
    cursor: Option<&str>,
) -> StepResult {
    migrate(c.sql)?;
    match step {
        "init" => init(c).await,
        "erase_mailbox" | "erase_mailboxes" => erase_mailboxes(c, cursor).await,
        "tombstone_addresses" => tombstone_addresses(c).await,
        "scrub_control_plane" => scrub_control_plane(c).await,
        "probe" => probe(c).await,
        "receipt" => receipt(c).await,
        "stop_routing" => stop_routing(c).await,
        "cancel_billing" => cancel_billing(c).await,
        "remove_domains" => remove_domains(c, cursor).await,
        "erase_identities" => erase_identities(c, cursor).await,
        "delete_d1_rows" => delete_d1_rows(c, cursor).await,
        "sweep_vectors" => sweep_vectors(c).await,
        "sweep_r2" => sweep_r2(c).await,
        _ => Err(StepErr::new("unknown_step")),
    }
}

/// Records the identities to visit; identity scope also marks the identity `deleting` and revokes
/// its keys.
async fn init<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    match scope(c).as_str() {
        "message" | "thread" | "identity" => {
            let id = c.params["identity_id"].as_str().unwrap_or("");
            if let Some(row) = identity_row(c.p, id).await? {
                add_target(c.sql, &row.id, &row.mailbox_do_id)?;
            }
            if scope(c) == "identity" && c.params["reapply"] != true {
                c.p.db()
                    .batch(vec![
                        Stmt::new(
                            "UPDATE identities SET status = 'deleting', updated_at = ?2 WHERE id = ?1 AND status IN ('active','paused')",
                            vec![id.into(), c.now.into()],
                        ),
                        Stmt::new(
                            "UPDATE api_keys SET revoked_at = ?2, prev_hash = NULL WHERE identity_id = ?1 AND level = 'identity' AND revoked_at IS NULL",
                            vec![id.into(), c.now.into()],
                        ),
                    ])
                    .await?;
            }
        }
        "counterparty" => {
            #[derive(Deserialize)]
            struct I {
                id: String,
                mailbox_do_id: String,
            }
            let only: Option<String> = c.params["identity_ids"]
                .as_array()
                .map(|a| serde_json::to_string(a).unwrap_or_default());
            let rows: Vec<I> = c
                .p
                .db()
                .query(Stmt::new(
                    "SELECT id, mailbox_do_id FROM identities WHERE tenant_id = ?1 AND status <> 'deleted' AND mailbox_do_id <> '' AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2))) ORDER BY id",
                    vec![c.tenant().into(), only.into()],
                ))
                .await?;
            for r in rows {
                add_target(c.sql, &r.id, &r.mailbox_do_id)?;
            }
        }
        _ => {}
    }
    Ok(Slice::done())
}

/// Message, thread and counterparty scope: each target mailbox page by page (cursor
/// `{idx}:{rowid}`); identity scope: the whole mailbox.
async fn erase_mailboxes<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    cursor: Option<&str>,
) -> StepResult {
    let all = targets(c.sql)?;
    let (idx, rest) = cursor
        .and_then(|c| c.split_once(':'))
        .map_or((0, ""), |(i, r)| (i.parse().unwrap_or(0), r));
    let Some(t) = all.iter().find(|t| t.idx >= idx).cloned() else {
        return Ok(Slice::done());
    };
    let rest = if t.idx == idx { rest } else { "" };
    let (next, counts) = if scope(c) == "identity" {
        erase_identity_mailbox(c, &t, (!rest.is_empty()).then_some(rest)).await?
    } else {
        let target = target(c)?;
        let after = rest.parse().unwrap_or(0);
        let (n, counts) = erase_page(c, &t, &target, after).await?;
        if n.is_none()
            && matches!(
                target,
                Target::Counterparty { .. } | Target::CounterpartyHash { .. }
            )
        {
            let _ = c
                .mailbox(
                    &t.identity_id,
                    &t.mailbox_do_id,
                    MailboxRequest::Privacy(PrivacyOp::ForgetSender { target }),
                )
                .await?;
        }
        (n.map(|n| n.to_string()), counts)
    };
    let cursor = match &next {
        Some(n) => format!("{}:{n}", t.idx),
        None => format!("{}:", t.idx + 1),
    };
    let more = next.is_some() || all.iter().any(|x| x.idx > t.idx);
    Ok(if more {
        Slice::More { cursor, counts }
    } else {
        Slice::Done { counts }
    })
}

#[derive(Deserialize)]
struct AddrRow {
    address: String,
    domain_id: String,
    routing_rule_id: Option<String>,
}

/// Deletes literal routing rules: `(domain_id, routing_rule_id)`.
async fn delete_rules<P: Platform>(p: &P, rules: &[(String, String)]) -> Result<(), StepErr> {
    if rules.is_empty() {
        return Ok(());
    }
    let Some(cf) = crate::domains::cloudflare_api::Cf::new(p) else {
        return Ok(());
    };
    #[derive(Deserialize)]
    struct Z {
        zone_id: Option<String>,
    }
    for (domain_id, rule) in rules {
        let z: Option<Z> = p
            .db()
            .first(Stmt::new(
                "SELECT zone_id FROM domains WHERE id = ?1",
                vec![domain_id.as_str().into()],
            ))
            .await?;
        if let Some(zone) = z.and_then(|z| z.zone_id) {
            match cf.delete_rule(&zone, rule).await {
                Ok(()) => {}
                Err(e) if e.status == 404 => {}
                Err(_) => return Err(StepErr::new("routing_rule_delete_failed")),
            }
        }
    }
    Ok(())
}

/// Identity step 2: tombstone and delete every address row and its literal rule (Privacy § 6.5).
async fn tombstone_addresses<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    let id = c.params["identity_id"].as_str().unwrap_or("");
    let mut rules: Vec<(String, String)> = c.params["rules"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|r| {
            Some((
                r["domain_id"].as_str()?.to_owned(),
                r["routing_rule_id"].as_str()?.to_owned(),
            ))
        })
        .collect();
    let rows: Vec<AddrRow> =
        c.p.db()
            .query(Stmt::new(
                "SELECT address, domain_id, routing_rule_id FROM addresses WHERE identity_id = ?1",
                vec![id.into()],
            ))
            .await?;
    let hash_key = &c.p.secrets().hash_key;
    let mut batch = Vec::new();
    for a in &rows {
        batch.push(crate::db::addresses::insert_tombstone(
            &address_hash(hash_key, &a.address),
            id,
            "erased",
        ));
        if let Some(r) = &a.routing_rule_id {
            rules.push((a.domain_id.clone(), r.clone()));
        }
    }
    batch.push(crate::db::addresses::delete_of_identity(id));
    c.p.db().batch(batch).await?;
    delete_rules(c.p, &rules).await?;
    Ok(Slice::done())
}

/// Runs `sql` (a delete of at most 1,000 rows on `?1`) until it deletes nothing (Privacy § 6.12);
/// answers the rows deleted and whether the table is done before the slice's statement budget ran out.
pub async fn batched_delete<P: Platform>(
    p: &P,
    sql: &'static str,
    args: Vec<pylota_identity_platform::d1::SqlValue>,
    budget: &mut i64,
) -> Result<(i64, bool), StepErr> {
    let mut n = 0;
    loop {
        if *budget <= 0 {
            return Ok((n, false));
        }
        *budget -= 1;
        let m = p.db().batch(vec![Stmt::new(sql, args.clone())]).await?;
        let changed = m.first().map_or(0, |m| m.changes as i64);
        n += changed;
        if changed == 0 {
            return Ok((n, true));
        }
    }
}

/// Expires every completed export of the tenant (Privacy § 6.11): the ZIP goes from both buckets.
pub async fn expire_exports<P: Platform>(p: &P, tenant_id: &str) -> Result<(), StepErr> {
    #[derive(Deserialize)]
    struct E {
        id: String,
        r2_key: Option<String>,
    }
    let rows: Vec<E> = p
        .db()
        .query(Stmt::new(
            "SELECT id, r2_key FROM exports WHERE tenant_id = ?1 AND status = 'completed'",
            vec![tenant_id.into()],
        ))
        .await?;
    for e in rows {
        if let Some(k) = &e.r2_key {
            p.blobs().delete(std::slice::from_ref(k)).await?;
            if let Some(b) = p.backup() {
                b.delete(std::slice::from_ref(k)).await?;
            }
        }
        p.db()
            .execute(Stmt::new(
                "UPDATE exports SET status = 'expired' WHERE id = ?1 AND status = 'completed'",
                vec![e.id.into()],
            ))
            .await?;
    }
    Ok(())
}

/// The deleted events' index rows.
async fn scrub_events<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> Result<(), StepErr> {
    #[derive(Deserialize)]
    struct E {
        id: String,
    }
    let ids: Vec<String> = c
        .sql
        .query::<E>("SELECT id FROM events", &[])?
        .into_iter()
        .map(|e| e.id)
        .collect();
    for chunk in ids.chunks(200) {
        let list = serde_json::to_string(chunk).unwrap_or_default();
        c.p.db()
            .batch(vec![
                Stmt::new(
                    "DELETE FROM webhook_deliveries WHERE event_id IN (SELECT value FROM json_each(?1))",
                    vec![list.clone().into()],
                ),
                Stmt::new(
                    "DELETE FROM event_index WHERE id IN (SELECT value FROM json_each(?1))",
                    vec![list.into()],
                ),
            ])
            .await?;
    }
    Ok(())
}

async fn scrub_control_plane<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    scrub_events(c).await?;
    let tenant = c.tenant().to_owned();
    let mut counts = json!({});
    match scope(c).as_str() {
        "counterparty" => {
            let hash = c.params["counterparty_hash"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            c.p.db()
                .execute(Stmt::new(
                    "UPDATE suppressions SET note = NULL, source_message_id = NULL WHERE tenant_id = ?1 AND address_hash = ?2",
                    vec![tenant.clone().into(), hash.into()],
                ))
                .await?;
            if let Some(address) = c.get("target_address")? {
                c.p.db()
                    .batch(vec![
                        Stmt::new(
                            "DELETE FROM idempotency_records WHERE tenant_id = ?1 AND instr(lower(response_body), ?2) > 0",
                            vec![tenant.clone().into(), address.to_ascii_lowercase().into()],
                        ),
                        Stmt::new(
                            "DELETE FROM dlq_items WHERE tenant_id = ?1 AND instr(lower(body_json), ?2) > 0",
                            vec![tenant.clone().into(), address.to_ascii_lowercase().into()],
                        ),
                    ])
                    .await?;
                counts = provider_suppressions(c, &address).await?;
            }
        }
        "identity" => {
            let id = c.params["identity_id"].as_str().unwrap_or("").to_owned();
            let held = !c.held()?.is_empty();
            counts = scrub_identity(c.p, &tenant, &id, held, c.now).await?;
        }
        _ => {}
    }
    expire_exports(c.p, &tenant).await?;
    Ok(Slice::Done { counts })
}

/// Identity step 4 (Privacy § 6.5): the D1 rows of one identity. Answers `list_entries_deleted`.
pub async fn scrub_identity<P: Platform>(
    p: &P,
    tenant: &str,
    id: &str,
    held: bool,
    now: i64,
) -> Result<Value, StepErr> {
    let status = if held { "deleting" } else { "deleted" };
    let lists = p
        .db()
        .batch(vec![
            Stmt::new(
                "DELETE FROM sender_lists WHERE identity_id = ?1",
                vec![id.into()],
            ),
            Stmt::new(
                "UPDATE identities SET owner_name = NULL, owner_email = NULL, signature_text = NULL, signature_html = NULL, client_id = NULL, client_fingerprint = NULL, display_name = '', metadata_json = '{}', send_policy_json = '{}', status = ?2, updated_at = ?3 WHERE id = ?1",
                vec![id.into(), status.into(), now.into()],
            ),
            Stmt::new(
                "INSERT OR IGNORE INTO key_tombstones (kid, deleted_at) SELECT id, ?2 FROM identity_keys WHERE identity_id = ?1",
                vec![id.into(), now.into()],
            ),
            Stmt::new(
                "DELETE FROM identity_keys WHERE identity_id = ?1",
                vec![id.into()],
            ),
            Stmt::new(
                "DELETE FROM webhook_deliveries WHERE event_id IN (SELECT id FROM event_index WHERE identity_id = ?1)",
                vec![id.into()],
            ),
            Stmt::new(
                "DELETE FROM event_index WHERE identity_id = ?1",
                vec![id.into()],
            ),
            Stmt::new(
                "DELETE FROM idempotency_records WHERE identity_id = ?1 OR (tenant_id = ?2 AND instr(response_body, ?1) > 0)",
                vec![id.into(), tenant.into()],
            ),
            Stmt::new(
                "DELETE FROM dlq_items WHERE tenant_id = ?2 AND instr(body_json, ?1) > 0",
                vec![id.into(), tenant.into()],
            ),
            Stmt::new(
                "DELETE FROM service_accounts WHERE identity_id = ?1",
                vec![id.into()],
            ),
            Stmt::new(
                "DELETE FROM draft_index WHERE identity_id = ?1",
                vec![id.into()],
            ),
        ])
        .await?
        .first()
        .map_or(0, |m| m.changes as i64);
    #[derive(Deserialize)]
    struct Q {
        quota_do_id: String,
    }
    if let Ok(Some(q)) = p
        .db()
        .first::<Q>(Stmt::new(
            "SELECT quota_do_id FROM tenants WHERE id = ?1",
            vec![tenant.into()],
        ))
        .await
    {
        let _ = crate::quota::call(
            p,
            &q.quota_do_id,
            tenant,
            "req_erasure",
            crate::quota::QuotaRequest::ForgetIdentity {
                identity_id: id.to_owned(),
            },
        )
        .await;
    }
    Ok(json!({ "list_entries_deleted": lists }))
}

/// I12: the provider's `sending_domain` suppressions of the address on the tenant's own domains.
async fn provider_suppressions<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    address: &str,
) -> Result<Value, StepErr> {
    let (Some(account), Some(_)) = (
        c.p.config().cf_account_id.clone(),
        c.p.secrets().cf_api_token.clone(),
    ) else {
        return Ok(json!({ "provider_suppressions_skipped": 1 }));
    };
    let Some(cf) = crate::domains::cloudflare_api::Cf::new(c.p) else {
        return Ok(json!({ "provider_suppressions_skipped": 1 }));
    };
    #[derive(Deserialize)]
    struct D {
        name: String,
    }
    let own: Vec<String> =
        c.p.db()
            .query::<D>(Stmt::new(
                "SELECT name FROM domains WHERE tenant_id = ?1 AND kind <> 'platform'",
                vec![c.tenant().into()],
            ))
            .await?
            .into_iter()
            .map(|d| d.name.to_ascii_lowercase())
            .collect();
    let path = format!(
        "/accounts/{account}/email/sending/suppressions?email={}",
        address.replace('+', "%2B").replace('@', "%40")
    );
    let list = cf
        .call("GET", &path, None)
        .await
        .map_err(|_| StepErr::new("provider_suppressions_failed"))?;
    let mut deleted = 0;
    for item in list.as_array().cloned().unwrap_or_default() {
        let domain = item["domain"]
            .as_str()
            .or(item["sending_domain"].as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if item["scope"] == "account" || item["read_only"] == true || !own.contains(&domain) {
            continue;
        }
        if let Some(id) = item["id"].as_str() {
            match cf
                .call(
                    "DELETE",
                    &format!("/accounts/{account}/email/sending/suppressions/{id}"),
                    None,
                )
                .await
            {
                Ok(_) => deleted += 1,
                Err(e) if e.status == 404 => {}
                Err(_) => return Err(StepErr::new("provider_suppressions_failed")),
            }
        }
    }
    Ok(json!({ "provider_suppressions_deleted": deleted }))
}

/// Privacy § 6.7: keyword, semantic and (until the event log) a zero log probe. Semantic hits make
/// the step wait and re-send the deletes, up to two minutes, then fail this attempt.
async fn probe<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    #[derive(Deserialize)]
    struct E {
        id: String,
    }
    let erased: Vec<String> = c
        .sql
        .query::<E>("SELECT id FROM erased", &[])?
        .into_iter()
        .map(|e| e.id)
        .collect();
    let target = target(c)?;
    let mut keyword = 0;
    for t in targets(c.sql)? {
        match c
            .mailbox(
                &t.identity_id,
                &t.mailbox_do_id,
                MailboxRequest::Privacy(PrivacyOp::ProbeKeyword {
                    target: target.clone(),
                    message_ids: erased.clone(),
                }),
            )
            .await?
        {
            Ok(v) => keyword += v["hits"].as_i64().unwrap_or(0),
            Err(e) if e.code == ErrorCode::IdentityNotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    if keyword > 0 {
        return Err(StepErr::new("probe_keyword_hits"));
    }
    let mut ids = c.deleted_vectors()?;
    for m in &erased {
        ids.extend((0..64).map(|n| format!("{m}:{n}")));
    }
    let mut left: Vec<String> = Vec::new();
    for batch in ids.chunks(100) {
        left.extend(c.p.vectors().get_by_ids(batch).await?);
    }
    if scope(c) == "tenant" && c.held()?.is_empty() {
        let probe = vec![0.5f32; crate::search::semantic::DIMENSIONS];
        left.extend(
            c.p.vectors()
                .query(c.tenant(), &probe, 100, &json!({}))
                .await?
                .into_iter()
                .map(|m| m.id),
        );
    }
    if !left.is_empty() {
        let tries = c
            .get("probe_tries")?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        if tries < PROBE_TRIES {
            c.set("probe_tries", &(tries + 1).to_string())?;
            for batch in left.chunks(500) {
                c.p.vectors().delete_by_ids(batch).await?;
            }
            return Ok(Slice::Wait {
                after_ms: PROBE_WAIT_MS,
            });
        }
        c.del("probe_tries")?;
        return Err(StepErr::new("probe_semantic_hits"));
    }
    c.del("probe_tries")?;
    c.set("probe", &json!([keyword, 0, 0]).to_string())?;
    Ok(Slice::done())
}

fn identities_affected<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> PResult<Vec<String>> {
    #[derive(Deserialize)]
    struct I {
        identity_id: String,
    }
    let mut ids: Vec<String> = c
        .sql
        .query::<I>(
            "SELECT identity_id FROM erased UNION SELECT identity_id FROM held",
            &[],
        )?
        .into_iter()
        .map(|i| i.identity_id)
        .collect();
    if matches!(scope(c).as_str(), "identity") {
        ids.extend(c.params["identity_id"].as_str().map(str::to_owned));
    }
    if scope(c) == "tenant" {
        ids.extend(targets(c.sql)?.into_iter().map(|t| t.identity_id));
    }
    Ok(ids)
}

fn build_receipt<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    counts: &Map<String, Value>,
    probed: bool,
) -> PResult<(Value, &'static str)> {
    let held: Vec<(String, String)> = c
        .held()?
        .into_iter()
        .map(|h| (h.thread_id, h.reason))
        .collect();
    let probe = probed
        .then(|| c.get("probe").ok().flatten())
        .flatten()
        .and_then(|p| serde_json::from_str::<Vec<i64>>(&p).ok())
        .map(|p| (p[0], p[1], p[2]));
    Ok(pylota_identity_core::jobs::receipt(
        counts,
        &identities_affected(c)?,
        &held,
        probe.or(probed.then_some((0, 0, 0))),
    ))
}

/// Privacy § 10: the receipt, the request's status and the events.
async fn receipt<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    let (r, status) = build_receipt(c, &c.counts()?, true)?;
    let era = era_id(c);
    c.p.db()
        .execute(Stmt::new(
            "UPDATE erasure_requests SET status = ?2, receipt_json = ?3, completed_at = ?4 WHERE id = ?1",
            vec![era.clone().into(), status.into(), r.to_string().into(), c.now.into()],
        ))
        .await?;
    c.set("receipt", &r.to_string())?;
    let tenant = c.tenant_id.clone();
    match scope(c).as_str() {
        "identity" if status == "completed" && c.params["reapply"] != true => {
            let id = c.params["identity_id"].as_str().unwrap_or("");
            super::emit(
                c.p,
                tenant.as_deref(),
                Some(id),
                &format!("{}:identity", c.job_id),
                "identity.deleted",
                &json!({ "identity_id": id, "erasure_request_id": era }),
                c.now,
            )
            .await?;
        }
        "tenant" if status == "completed" && c.params["reapply"] != true => {
            c.p.db()
                .execute(Stmt::new(
                    "UPDATE tenants SET status = 'erased', name = '', policy_json = '{}', updated_at = ?2 WHERE id = ?1",
                    vec![c.tenant().into(), c.now.into()],
                ))
                .await?;
        }
        _ => {}
    }
    super::emit(
        c.p,
        tenant.as_deref(),
        None,
        &c.job_id,
        "erasure.completed",
        &json!({ "erasure_request": request_json(c.p, &era).await?.unwrap_or(Value::Null) }),
        c.now,
    )
    .await?;
    Ok(Slice::done())
}

async fn request_json<P: Platform>(p: &P, era: &str) -> Result<Option<Value>, ApiError> {
    let e: Option<crate::db::jobs::Erasure> =
        p.db().first(crate::db::jobs::get_erasure(era)).await?;
    Ok(e.map(|e| e.json()))
}

/// The end of the job: a failed one writes the partial receipt and `erasure.failed`; a canceled one
/// marks its request `canceled`.
pub async fn on_end<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    outcome: &str,
    counts: &Map<String, Value>,
    failed: Option<(&str, &str)>,
) -> Result<Value, ApiError> {
    let era = era_id(c);
    match outcome {
        "completed" => Ok(c
            .get("receipt")?
            .and_then(|r| serde_json::from_str(&r).ok())
            .unwrap_or(Value::Null)),
        "failed" => {
            let (r, _) = build_receipt(c, counts, false)?;
            c.p.db()
                .execute(Stmt::new(
                    "UPDATE erasure_requests SET status = 'failed', receipt_json = ?2 WHERE id = ?1",
                    vec![era.clone().into(), r.to_string().into()],
                ))
                .await?;
            let (step, code) = failed.unwrap_or(("", ""));
            super::emit(
                c.p,
                c.tenant_id.as_deref(),
                None,
                &c.job_id,
                "erasure.failed",
                &json!({ "erasure_request_id": era, "step": step, "error": code }),
                c.now,
            )
            .await?;
            let mut out = r;
            out["step"] = step.into();
            out["error"] = code.into();
            Ok(out)
        }
        _ => {
            c.p.db()
                .execute(Stmt::new(
                    "UPDATE erasure_requests SET status = 'canceled' WHERE id = ?1 AND status IN ('queued','running')",
                    vec![era.into()],
                ))
                .await?;
            Ok(json!({ "canceled": true }))
        }
    }
}

// ---- Tenant scope (Privacy § 6.6) ----------------------------------------------------------------

/// Step 1: the tenant becomes `erasing`; every key revoked, endpoints disabled, other jobs canceled.
async fn stop_routing<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    let t = c.tenant().to_owned();
    c.p.db()
        .batch(vec![
            Stmt::new(
                "UPDATE tenants SET status = 'erasing', updated_at = ?2 WHERE id = ?1 AND status <> 'erased'",
                vec![t.clone().into(), c.now.into()],
            ),
            Stmt::new(
                "UPDATE api_keys SET revoked_at = ?2, prev_hash = NULL WHERE tenant_id = ?1 AND level IN ('tenant','identity') AND revoked_at IS NULL",
                vec![t.clone().into(), c.now.into()],
            ),
            Stmt::new(
                "UPDATE webhook_endpoints SET enabled = 0, disabled_reason = 'manual', updated_at = ?2 WHERE tenant_id = ?1",
                vec![t.clone().into(), c.now.into()],
            ),
        ])
        .await?;
    #[derive(Deserialize)]
    struct J {
        runner_do_id: String,
    }
    let others: Vec<J> = c
        .p
        .db()
        .query(Stmt::new(
            "SELECT runner_do_id FROM jobs WHERE tenant_id = ?1 AND id <> ?2 AND status IN ('queued','running') AND NOT (kind = 'erasure' AND id IN (SELECT job_id FROM erasure_requests WHERE scope = 'tenant'))",
            vec![t.clone().into(), c.job_id.clone().into()],
        ))
        .await?;
    for j in others {
        super::cancel(c.p, &j.runner_do_id, Some(&t)).await;
    }
    Ok(Slice::done())
}

/// Step 2: Stripe billing (M22). Skipped until then.
async fn cancel_billing<P: Platform, S: Sql>(_c: &Ctx<'_, P, S>) -> StepResult {
    Ok(Slice::Done { counts: json!({}) })
}

/// Step 3: every tenant domain removed inline (cursor: the domains done).
async fn remove_domains<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    _cursor: Option<&str>,
) -> StepResult {
    let rows: Vec<crate::db::domains::Row> = c
        .p
        .db()
        .query(Stmt::new(
            concat!(
                "SELECT ",
                crate::domain_columns!(),
                " FROM domains WHERE tenant_id = ?1 AND kind <> 'platform' AND state <> 'removed' ORDER BY id LIMIT 1"
            ),
            vec![c.tenant().into()],
        ))
        .await?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(Slice::done());
    };
    super::domain_remove::remove(c.p, &row, c.now).await?;
    Ok(Slice::More {
        cursor: row.id,
        counts: json!({}),
    })
}

/// Step 4: identity steps 2 and 3 for each identity (cursor `{idx}:{inner}`).
async fn erase_identities<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    cursor: Option<&str>,
) -> StepResult {
    if cursor.is_none() {
        #[derive(Deserialize)]
        struct I {
            id: String,
            mailbox_do_id: String,
        }
        let only: Option<String> = c.params["source_identities"]
            .as_array()
            .map(|a| serde_json::to_string(a).unwrap_or_default());
        let rows: Vec<I> = c
            .p
            .db()
            .query(Stmt::new(
                "SELECT id, mailbox_do_id FROM identities WHERE tenant_id = ?1 AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2))) ORDER BY id",
                vec![c.tenant().into(), only.into()],
            ))
            .await?;
        for r in rows {
            add_target(c.sql, &r.id, &r.mailbox_do_id)?;
        }
    }
    let all = targets(c.sql)?;
    let (idx, rest) = cursor
        .and_then(|c| c.split_once(':'))
        .map_or((0, ""), |(i, r)| (i.parse().unwrap_or(0), r));
    let Some(t) = all.iter().find(|t| t.idx >= idx).cloned() else {
        return Ok(Slice::done());
    };
    let rest = if t.idx == idx { rest } else { "" };
    let mut counts = json!({});
    if rest.is_empty() && c.params["reapply"] != true {
        // The addresses go first (every status), so nothing new arrives.
        let rows: Vec<AddrRow> = c
            .p
            .db()
            .query(Stmt::new(
                "SELECT address, domain_id, routing_rule_id FROM addresses WHERE identity_id = ?1",
                vec![t.identity_id.as_str().into()],
            ))
            .await?;
        let hash_key = &c.p.secrets().hash_key;
        let mut batch: Vec<Stmt> = rows
            .iter()
            .map(|a| {
                crate::db::addresses::insert_tombstone(
                    &address_hash(hash_key, &a.address),
                    &t.identity_id,
                    "erased",
                )
            })
            .collect();
        batch.push(crate::db::addresses::delete_of_identity(&t.identity_id));
        batch.push(Stmt::new(
            "UPDATE identities SET status = 'deleting', updated_at = ?2 WHERE id = ?1 AND status NOT IN ('deleting','deleted')",
            vec![t.identity_id.as_str().into(), c.now.into()],
        ));
        c.p.db().batch(batch).await?;
    }
    let next = if t.mailbox_do_id.is_empty() {
        None
    } else {
        let (n, cts) = erase_identity_mailbox(c, &t, (!rest.is_empty()).then_some(rest)).await?;
        counts = cts;
        n
    };
    if next.is_none() && c.params["reapply"] != true {
        let held = c.held()?.iter().any(|h| h.identity_id == t.identity_id);
        let extra = scrub_identity(c.p, c.tenant(), &t.identity_id, held, c.now).await?;
        pylota_identity_core::jobs::add_counts(
            counts.as_object_mut().get_or_insert(&mut Map::new()),
            &extra,
        );
    }
    let cursor = match &next {
        Some(n) => format!("{}:{n}", t.idx),
        None => format!("{}:", t.idx + 1),
    };
    Ok(if next.is_some() || all.iter().any(|x| x.idx > t.idx) {
        Slice::More { cursor, counts }
    } else {
        Slice::Done { counts }
    })
}

/// A batched delete of at most 1,000 rows of `$t` matching `$f` (Privacy § 6.12).
macro_rules! del {
    ($t:literal, $f:literal) => {
        (
            $t,
            concat!(
                "DELETE FROM ",
                $t,
                " WHERE rowid IN (SELECT rowid FROM ",
                $t,
                " WHERE ",
                $f,
                " LIMIT 1000)"
            ),
        )
    };
}

/// The tables of step 5 in their order, each deleted by the tenant (`?1`).
const TENANT_TABLES: &[(&str, &str)] = &[
    del!("webhook_deliveries", "tenant_id = ?1"),
    del!("webhook_endpoints", "tenant_id = ?1"),
    del!("event_index", "tenant_id = ?1"),
    del!("suppressions", "tenant_id = ?1"),
    del!("sender_lists", "tenant_id = ?1"),
    del!("idempotency_records", "scope = ?1 OR tenant_id = ?1"),
    del!("rpc_intents", "tenant_id = ?1"),
    del!("usage_daily", "tenant_id = ?1"),
    del!("identity_keys", "tenant_id = ?1"),
    del!("service_accounts", "tenant_id = ?1"),
    del!("draft_index", "tenant_id = ?1"),
    del!("notification_prefs", "tenant_id = ?1"),
    del!("agent_signups", "tenant_id = ?1"),
    del!("api_keys", "tenant_id = ?1"),
    del!("identities", "tenant_id = ?1 AND status <> 'deleting'"),
    del!("zone_claims", "tenant_id = ?1"),
    del!("domain_onboarding", "tenant_id = ?1"),
    del!("domains", "tenant_id = ?1"),
    del!("exports", "tenant_id = ?1"),
    del!("jobs", "tenant_id = ?1 AND kind <> 'erasure'"),
    del!("invitations", "tenant_id = ?1"),
    del!("sessions", "tenant_id = ?1"),
    del!("members", "tenant_id = ?1"),
    del!("billing_events", "tenant_id = ?1"),
    del!("billing_accounts", "tenant_id = ?1"),
    del!("dlq_items", "tenant_id = ?1"),
    del!(
        "audit_log",
        "tenant_id = ?1 AND action NOT LIKE 'erasure.%' AND action NOT LIKE 'hold.%'"
    ),
];

/// Step 5: the tenant's D1 rows, in batched deletes (cursor: the table index).
async fn delete_d1_rows<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    cursor: Option<&str>,
) -> StepResult {
    let held = !c.held()?.is_empty();
    let start: usize = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
    let mut budget = 20;
    let mut counts = Map::new();
    for (i, (table, sql)) in TENANT_TABLES.iter().enumerate().skip(start) {
        if *table == "identity_keys" {
            c.p.db()
                .execute(Stmt::new(
                    "INSERT OR IGNORE INTO key_tombstones (kid, deleted_at) SELECT id, ?2 FROM identity_keys WHERE tenant_id = ?1",
                    vec![c.tenant().into(), c.now.into()],
                ))
                .await?;
        }
        let (n, finished) = batched_delete(c.p, sql, vec![c.tenant().into()], &mut budget).await?;
        if *table == "sender_lists" {
            pylota_identity_core::jobs::add_counts(
                &mut counts,
                &json!({ "list_entries_deleted": n }),
            );
        }
        if !finished {
            return Ok(Slice::More {
                cursor: i.to_string(),
                counts: Value::Object(counts),
            });
        }
    }
    delete_console_rows(c).await?;
    delete_billing_rows(c).await?;
    delete_notifier(c).await?;
    if !held {
        c.p.db()
            .execute(Stmt::new(
                "UPDATE tenants SET name = '', policy_json = '{}', updated_at = ?2 WHERE id = ?1",
                vec![c.tenant().into(), c.now.into()],
            ))
            .await?;
    }
    #[derive(Deserialize)]
    struct Q {
        quota_do_id: String,
    }
    if let Some(q) =
        c.p.db()
            .first::<Q>(Stmt::new(
                "SELECT quota_do_id FROM tenants WHERE id = ?1",
                vec![c.tenant().into()],
            ))
            .await?
    {
        let _ = crate::quota::call(
            c.p,
            &q.quota_do_id,
            c.tenant(),
            "req_erasure",
            crate::quota::QuotaRequest::Wipe,
        )
        .await;
    }
    Ok(Slice::Done {
        counts: Value::Object(counts),
    })
}

/// Step 6: the tenant namespace swept by a fixed probe vector until two empty queries.
async fn sweep_vectors<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    if !c.held()?.is_empty() {
        return Ok(Slice::done());
    }
    let probe = vec![0.5f32; crate::search::semantic::DIMENSIONS];
    let mut deleted = 0;
    let mut empty = 0;
    for _ in 0..100 {
        let ids: Vec<String> =
            c.p.vectors()
                .query(c.tenant(), &probe, 100, &json!({}))
                .await?
                .into_iter()
                .map(|m| m.id)
                .collect();
        if ids.is_empty() {
            empty += 1;
            if empty >= 2 {
                return Ok(Slice::Done {
                    counts: json!({ "vectors_deleted": deleted }),
                });
            }
            continue;
        }
        empty = 0;
        deleted += delete_vectors(c, &ids).await?;
    }
    Ok(Slice::More {
        cursor: "sweep".into(),
        counts: json!({ "vectors_deleted": deleted }),
    })
}

/// Step 7: every object under `t/{tenant}/` (with holds: only the exports and the wiped identities).
async fn sweep_r2<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> StepResult {
    let held = c.held()?;
    let n = if held.is_empty() {
        delete_prefix(c.p, &format!("t/{}/", c.tenant())).await?
    } else {
        let mut n = delete_prefix(c.p, &format!("t/{}/exports/", c.tenant())).await?;
        for t in targets(c.sql)? {
            if !held.iter().any(|h| h.identity_id == t.identity_id) {
                n += delete_prefix(c.p, &format!("t/{}/i/{}/", c.tenant(), t.identity_id)).await?;
            }
        }
        n
    };
    Ok(Slice::Done {
        counts: json!({ "r2_objects_deleted": n }),
    })
}

// ---- Substeps later milestones complete (Build plan › M14, "Stubs completed by later milestones") -

/// Console rows of the tenant: `invitations`, `sessions` (active workspace this tenant) and `members`
/// are deleted by the table deletes above; a session of a person whose active workspace was this one
/// cannot act anywhere until they pick another. People left with no workspace are deleted by M24
/// (Privacy § 6.9).
async fn delete_console_rows<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> Result<(), ApiError> {
    c.p.db()
        .execute(pylota_identity_platform::d1::Stmt::new(
            "UPDATE sessions SET tenant_id = NULL WHERE tenant_id = ?1",
            vec![c.tenant().into()],
        ))
        .await?;
    Ok(())
}

/// `billing_events` and `billing_accounts` beyond the table deletes: M22.
async fn delete_billing_rows<P: Platform, S: Sql>(_c: &Ctx<'_, P, S>) -> Result<(), ApiError> {
    Ok(())
}

/// `Notifier` `delete_all`: M26.
async fn delete_notifier<P: Platform, S: Sql>(_c: &Ctx<'_, P, S>) -> Result<(), ApiError> {
    Ok(())
}

/// The domain's addresses in the `pm-retired-{n}` receipt rules: M23.
pub async fn prune_retired_rules<P: Platform>(_p: &P, _domain_id: &str) -> Result<(), StepErr> {
    Ok(())
}
