//! `IdentityMailbox` (Data model § 2): one SQLite Durable Object per identity. M6 builds its schema
//! (every table of v1, applied on wake), its owner (`Init`), the identity events written after a D1
//! change (`EmitEvent`) and the outbox; the mail operations arrive with their milestones.
//!
//! The logic is generic over `platform::durable::{Sql, ObjectState}`, so native tests run it on
//! `platform::fakes`; `objects::stubs::MailboxObject` wires it to the runtime.

pub mod alarms;
pub mod attachments;
pub mod deliveries;
pub mod idempotency;
pub mod ingest;
pub mod locks;
pub mod messages;
pub mod outbox;
pub mod privacy;
pub mod read;
pub mod search;
#[cfg(feature = "itest-hooks")]
pub mod seed;
pub mod submit;
pub mod threads;
pub mod triage;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::durable::{ObjectState, RpcEnvelope, Sql};
use pylota_identity_platform::{Clock as _, PResult, Platform};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::ApiError;
use crate::rpc::{check_owner, error_body};

/// The schema this build applies.
pub const SCHEMA_VERSION: i64 = 1;
/// Mailbox schema v1 (Data model § 2).
pub const V1: &str = include_str!("schema/v1.sql");

/// An identity event written after a D1 change (Design conventions § 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmitEvent {
    /// Derived from the `rpc_intents` row, so a re-sent call is stored once.
    pub event_id: String,
    pub event_type: String,
    pub occurred_at: i64,
    pub data: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InitMailbox {
    pub created_at: i64,
    pub created_event: EmitEvent,
    pub message_hours: Option<u32>,
    /// The system identity's mailbox (`meta.is_system`): set by the cron that mints it.
    #[serde(default)]
    pub is_system: bool,
}

/// The requests M6 answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MailboxRequest {
    Init(InitMailbox),
    EmitEvent(EmitEvent),
    /// Stores an inbound message (Inbound › IdentityMailbox.ingest).
    Ingest(Box<ingest::IngestInput>),
    /// Counts a parse attempt (Inbound › consumer step 4); answers the new count.
    NoteParseAttempt {
        message_id: String,
    },
    /// Post-commit check (I10): whether the message still exists.
    ConfirmStored {
        message_id: String,
    },
    /// The stored envelopes of outbox events (Webhooks › Delivering an attempt, step 3); IDs no longer
    /// in the outbox are left out.
    GetEvents {
        event_ids: Vec<String>,
    },
    /// A re-parse's inputs (J3): the raw object and the ready attachment texts.
    ReparseSource {
        message_id: String,
    },
    /// Stores a re-parse and re-emits `message.received` with `reprocessed: true`.
    Reparse(Box<attachments::Reparsed>),
    /// The attachments an `AttachmentText` job extracts.
    PendingAttachmentText {
        message_id: String,
    },
    /// The job's results (Inbound › Attachment text extraction).
    AttachmentTextReady(attachments::AttachmentText),
    /// Reads, label changes and release (Inbound › Read path and release), and `wait`; the request
    /// is tagged `q` inside (`{"op": "read", "q": "list_threads", …}`).
    Read(read::ReadRequest),
    /// Search and index operations ([Search]); tagged `sq` inside.
    Search(search::SearchOp),
    /// Triage load, re-run and commit (Triage § 1.1, § 10); tagged `tq` inside.
    Triage(triage::TriageOp),
    /// Erasure, retention, holds and export (Privacy § 4.1).
    Privacy(privacy::PrivacyOp),
    /// A send, reply, reply-all or forward (Outbound › The submit path).
    Submit(Box<submit::SubmitInput>),
    /// What the outbound consumer reads before D1: status, From and the queued recipients.
    OutboundFacts {
        message_id: String,
    },
    BeginTransport(Box<deliveries::BeginInput>),
    ClaimTransport {
        message_id: String,
    },
    /// G4: deliveries the provider suppressed; answers the recipients left.
    MarkSuppressed {
        message_id: String,
        items: Vec<(String, String)>,
    },
    AllSuppressed {
        message_id: String,
    },
    RecordTransportOutcome {
        message_id: String,
        claim_token: String,
        provider: String,
        result: crate::transport::TransportResult,
    },
    ApplyDeliveryEvent(Box<crate::consumers::delivery::DeliveryEvent>),
    Cancel {
        message_id: String,
    },
    Resolve {
        message_id: String,
        outcome: String,
    },
    /// `POST /__test/mailbox-schema` (Testing § 6.2): `schema_version` back by one.
    #[cfg(feature = "itest-hooks")]
    TestSchemaBack,
    /// `POST /__test/alarm`: runs the alarm now.
    #[cfg(feature = "itest-hooks")]
    TestAlarm,
    /// `POST /__test/mailbox-query`: rows of one statement on this mailbox's SQLite.
    #[cfg(feature = "itest-hooks")]
    TestQuery {
        sql: String,
    },
    /// `POST /__test/bulk-seed`: synthetic messages for the benchmarks.
    #[cfg(feature = "itest-hooks")]
    TestBulkSeed {
        start: u32,
        count: u32,
        seed: u64,
    },
}

/// The statements of a migration: comments dropped, split on `;`.
pub fn statements(script: &str) -> Vec<String> {
    let mut text = String::with_capacity(script.len());
    for line in script.lines() {
        let line = match line.find("--") {
            Some(i) => &line[..i],
            None => line,
        };
        text.push_str(line);
        text.push('\n');
    }
    text.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Deserialize)]
struct Kv {
    k: String,
    v: String,
}

#[derive(Deserialize)]
struct T {
    t: Option<i64>,
}

/// Records a pending wake-up for `purpose` at `at`, keeping an earlier one. Numbers go into `meta` as
/// text bound from Rust: the runtime binds a JS number as REAL, whose text form is `1.0`.
pub fn set_wake<S: Sql>(tx: &S, purpose: &str, at: i64) -> PResult<()> {
    tx.exec(
        "INSERT INTO meta (k, v) VALUES ('alarm:' || ?1, ?2) ON CONFLICT (k) DO UPDATE SET v = CAST(MIN(CAST(v AS INTEGER), CAST(?2 AS INTEGER)) AS TEXT)",
        &[purpose.into(), at.to_string().into()],
    )?;
    Ok(())
}

/// The owner and flags in `meta`.
#[derive(Default)]
struct Meta {
    tenant_id: Option<String>,
    identity_id: Option<String>,
    erased: bool,
    is_system: bool,
}

pub struct Mailbox<'a, P: Platform, S: Sql, St: ObjectState> {
    pub p: &'a P,
    pub sql: &'a S,
    pub state: &'a St,
    /// This object's ID (hex).
    pub object_id: &'a str,
}

impl<P: Platform, S: Sql, St: ObjectState> Mailbox<'_, P, S, St> {
    fn schema_version(&self) -> i64 {
        self.sql
            .first::<Kv>("SELECT k, v FROM meta WHERE k = 'schema_version'", &[])
            .ok()
            .flatten()
            .and_then(|r| r.v.parse().ok())
            .unwrap_or(0)
    }

    /// Schema on wake (Design conventions § 4, rule 6; J9): every statement is idempotent, so a
    /// migration whose `schema_version` write was lost runs again without an error.
    pub fn migrate(&self) -> PResult<()> {
        if self.schema_version() >= SCHEMA_VERSION {
            return Ok(());
        }
        // Ignored inside a transaction; Durable Object SQLite enforces foreign keys already.
        let _ = self.sql.exec("PRAGMA foreign_keys = ON", &[]);
        self.sql.transaction_sync(|s| -> PResult<()> {
            for st in statements(V1) {
                s.exec(&st, &[])?;
            }
            s.exec(
                "INSERT INTO meta (k, v) VALUES ('schema_version', ?1) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
                &[SCHEMA_VERSION.to_string().into()],
            )?;
            s.exec(
                "INSERT OR IGNORE INTO meta (k, v) VALUES ('event_seq', '0'), ('erased', '0')",
                &[],
            )?;
            Ok(())
        })
    }

    fn meta(&self) -> PResult<Meta> {
        let rows: Vec<Kv> = self.sql.query(
            "SELECT k, v FROM meta WHERE k IN ('tenant_id', 'identity_id', 'erased', 'is_system')",
            &[],
        )?;
        let mut m = Meta::default();
        for r in rows {
            match r.k.as_str() {
                "tenant_id" => m.tenant_id = Some(r.v),
                "identity_id" => m.identity_id = Some(r.v),
                "erased" => m.erased = r.v == "1",
                "is_system" => m.is_system = r.v == "1",
                _ => {}
            }
        }
        Ok(m)
    }

    /// Arms the alarm at the earliest pending wake-up when that is earlier than the current alarm.
    async fn rearm(&self) -> PResult<()> {
        let t: Option<T> = self.sql.first(
            "SELECT MIN(CAST(v AS INTEGER)) AS t FROM meta WHERE k LIKE 'alarm:%'",
            &[],
        )?;
        let Some(at) = t.and_then(|t| t.t) else {
            return Ok(());
        };
        match self.state.get_alarm().await? {
            Some(cur) if cur <= at => Ok(()),
            _ => self.state.set_alarm(at).await,
        }
    }

    /// One RPC: the envelope in, `{"ok": …}` or `{"err": …}` out.
    pub async fn handle(&self, envelope: &Value) -> Value {
        let request_id = envelope["request_id"].as_str().unwrap_or("").to_owned();
        let docs = self.p.config().docs_url.clone();
        let out = match self.run(envelope).await {
            Ok(v) => serde_json::json!({ "ok": v }),
            Err(e) => serde_json::to_value(error_body::<()>(&e, &request_id, &docs))
                .unwrap_or(Value::Null),
        };
        if let Err(e) = self.rearm().await {
            crate::log::warn(
                "mailbox_rearm_failed",
                &[("kind", format!("{:?}", e.kind).into())],
            );
        }
        out
    }

    async fn run(&self, envelope: &Value) -> Result<Value, ApiError> {
        self.migrate()?;
        let env: RpcEnvelope<MailboxRequest> = serde_json::from_value(envelope.clone())
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "Unknown mailbox request."))?;
        let now = self.p.clock().now_ms();
        if now > env.deadline_ms {
            return Err(ApiError::new(
                ErrorCode::Timeout,
                "The request's deadline passed.",
            ));
        }
        let meta = self.meta()?;
        if meta.erased {
            return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
        }
        let sent_t = env.tenant_id.as_deref();
        let sent_i = env.identity_id.as_deref();
        match env.req {
            MailboxRequest::Init(init) => {
                if meta.tenant_id.is_some() {
                    check_owner(self.p, meta.tenant_id.as_deref(), sent_t, "IdentityMailbox")?;
                    check_owner(
                        self.p,
                        meta.identity_id.as_deref(),
                        sent_i,
                        "IdentityMailbox",
                    )?;
                    return Ok(Value::Null);
                }
                let (Some(tenant_id), Some(identity_id)) = (sent_t, sent_i) else {
                    return Err(ApiError::invalid("identity_id", "Init names its owner."));
                };
                self.sql.transaction_sync(|s| -> Result<(), ApiError> {
                    // Re-checked inside: another Init may have won meanwhile.
                    let won = s.exec(
                        "INSERT OR IGNORE INTO meta (k, v) VALUES ('tenant_id', ?1)",
                        &[tenant_id.into()],
                    )?;
                    if won.changes == 0 {
                        return Ok(());
                    }
                    s.exec(
                        "INSERT INTO meta (k, v) VALUES ('identity_id', ?1), ('created_at', ?2), ('is_system', ?3)",
                        &[
                            identity_id.into(),
                            init.created_at.to_string().into(),
                            if init.is_system { "1" } else { "0" }.into(),
                        ],
                    )?;
                    if let Some(h) = init.message_hours {
                        s.exec(
                            "INSERT OR REPLACE INTO meta (k, v) VALUES ('message_hours', ?1)",
                            &[h.to_string().into()],
                        )?;
                    }
                    let ev = &init.created_event;
                    outbox::append(
                        s,
                        &outbox::Owner { tenant_id, identity_id },
                        &ev.event_id,
                        &ev.event_type,
                        ev.occurred_at,
                        &ev.data,
                        now,
                    )?;
                    Ok(())
                })?;
                Ok(Value::Null)
            }
            req => {
                let (Some(tenant_id), Some(identity_id)) =
                    (meta.tenant_id.as_deref(), meta.identity_id.as_deref())
                else {
                    return Err(ApiError::new(
                        ErrorCode::Unavailable,
                        "The mailbox is not initialised yet.",
                    ));
                };
                check_owner(self.p, Some(tenant_id), sent_t, "IdentityMailbox")?;
                check_owner(self.p, Some(identity_id), sent_i, "IdentityMailbox")?;
                let owner = outbox::Owner {
                    tenant_id,
                    identity_id,
                };
                self.dispatch_op(req, &owner, now, meta.is_system).await
            }
        }
    }

    async fn dispatch_op(
        &self,
        req: MailboxRequest,
        owner: &outbox::Owner<'_>,
        now: i64,
        is_system: bool,
    ) -> Result<Value, ApiError> {
        match req {
            MailboxRequest::Init(_) => Ok(Value::Null),
            MailboxRequest::Ingest(mut inp) => {
                inp.token = Some(
                    self.check_token(owner.identity_id, inp.detail.as_deref())
                        .await,
                );
                let out = self
                    .sql
                    .transaction_sync(|s| ingest::ingest(self.p, s, owner, *inp, now, is_system))?;
                Ok(serde_json::to_value(out).unwrap_or(Value::Null))
            }
            MailboxRequest::NoteParseAttempt { message_id } => {
                let key = format!("parse:{message_id}");
                let n = self.sql.transaction_sync(|s| -> PResult<i64> {
                    let cur: Option<Kv> =
                        s.first("SELECT k, v FROM meta WHERE k = ?1", &[key.clone().into()])?;
                    let n = cur.and_then(|c| c.v.parse::<i64>().ok()).unwrap_or(0) + 1;
                    s.exec(
                        "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
                        &[key.clone().into(), n.to_string().into()],
                    )?;
                    Ok(n)
                })?;
                Ok(serde_json::json!({ "attempts": n }))
            }
            MailboxRequest::ConfirmStored { message_id } => {
                let erased: Option<Kv> = self.sql.first(
                    "SELECT message_id AS k, '' AS v FROM erased_ids WHERE message_id = ?1",
                    &[message_id.clone().into()],
                )?;
                let exists: Option<Kv> = self.sql.first(
                    "SELECT id AS k, '' AS v FROM messages WHERE id = ?1",
                    &[message_id.into()],
                )?;
                Ok(serde_json::json!({ "stored": erased.is_none() && exists.is_some() }))
            }
            MailboxRequest::GetEvents { event_ids } => {
                #[derive(Deserialize, Serialize)]
                struct E {
                    event_id: String,
                    payload_json: String,
                    occurred_at: i64,
                }
                let rows: Vec<E> = self.sql.query(
                    "SELECT event_id, payload_json, occurred_at FROM outbox WHERE event_id IN (SELECT value FROM json_each(?1))",
                    &[serde_json::to_string(&event_ids).unwrap_or_default().into()],
                )?;
                Ok(serde_json::json!({ "events": rows }))
            }
            MailboxRequest::PendingAttachmentText { message_id } => {
                Ok(attachments::pending(self.sql, &message_id)?)
            }
            MailboxRequest::ReparseSource { message_id } => {
                Ok(attachments::reparse_source(self.sql, &message_id)?)
            }
            MailboxRequest::Reparse(r) => {
                let event_id = outbox::new_event_id(self.p);
                Ok(self.sql.transaction_sync(|s| {
                    attachments::reparse(s, owner, &r, &event_id, is_system, now)
                })?)
            }
            MailboxRequest::AttachmentTextReady(r) => Ok(self
                .sql
                .transaction_sync(|s| attachments::ready(s, &r, is_system))?),
            MailboxRequest::Read(r) => {
                read::handle(self.p, self.sql, owner, r, now, is_system).await
            }
            MailboxRequest::Triage(op) => {
                triage::handle(self.p, self.sql, owner, op, now, is_system)
            }
            MailboxRequest::Privacy(privacy::PrivacyOp::WipeAll) => {
                // Everything goes; a three-row tombstone then refuses every later request.
                self.state.delete_all().await?;
                self.migrate()?;
                self.sql.exec(
                    "INSERT OR REPLACE INTO meta (k, v) VALUES ('erased', '1'), ('tenant_id', ?1), ('identity_id', ?2)",
                    &[owner.tenant_id.into(), owner.identity_id.into()],
                )?;
                Ok(Value::Null)
            }
            MailboxRequest::Privacy(op) => Ok(privacy::handle(
                self.sql,
                owner,
                op,
                now,
                &self.p.secrets().hash_key,
            )?),
            MailboxRequest::Search(op) => Ok(search::handle(
                self.sql,
                op,
                owner.identity_id,
                now,
                is_system,
            )?),
            MailboxRequest::EmitEvent(ev) => {
                let seq = self.sql.transaction_sync(|s| {
                    outbox::append(
                        s,
                        owner,
                        &ev.event_id,
                        &ev.event_type,
                        ev.occurred_at,
                        &ev.data,
                        now,
                    )
                })?;
                Ok(serde_json::json!({ "sequence": seq }))
            }
            MailboxRequest::Submit(inp) => {
                let mut inp = *inp;
                inp.ctx.identity.is_system |= is_system;
                self.submit(owner, inp, now).await
            }
            MailboxRequest::OutboundFacts { message_id } => {
                Ok(deliveries::facts(self.sql, &message_id)?)
            }
            MailboxRequest::BeginTransport(inp) => {
                let test = self.is_test_tenant().await;
                let mut answer = self
                    .sql
                    .transaction_sync(|s| deliveries::begin(self.p, s, owner, &inp, test, now))?;
                if let deliveries::BeginAnswer::Go(g) = &mut answer
                    && g.reply_from_platform
                {
                    let row = deliveries::by_id(self.sql, &inp.message_id)?;
                    let seq = row.map_or(0, |r| r.thread_seq);
                    let platform = crate::send::AddressCtx {
                        address: g.from.0.clone(),
                        status: "active".into(),
                        role: "alias".into(),
                        domain_id: String::new(),
                        domain: pylota_identity_core::address::domain_of(&g.from.0).to_owned(),
                        domain_kind: "platform".into(),
                        state: "healthy".into(),
                        transport: "cloudflare".into(),
                        reply_token: "subaddress".into(),
                        sending_paused_at: None,
                        delivery_events: None,
                    };
                    g.reply_to = self.reply_to(owner, &platform, seq).await;
                }
                Ok(serde_json::to_value(answer).unwrap_or(Value::Null))
            }
            MailboxRequest::ClaimTransport { message_id } => {
                let t = self
                    .sql
                    .transaction_sync(|s| deliveries::claim(self.p, s, &message_id, now))?;
                Ok(serde_json::to_value(t).unwrap_or(Value::Null))
            }
            MailboxRequest::MarkSuppressed { message_id, items } => {
                let left = self.sql.transaction_sync(|s| {
                    deliveries::mark_suppressed(s, &message_id, &items, now)
                })?;
                Ok(serde_json::to_value(left).unwrap_or(Value::Null))
            }
            MailboxRequest::AllSuppressed { message_id } => {
                self.sql.transaction_sync(|s| {
                    deliveries::all_suppressed(self.p, s, owner, &message_id, now)
                })?;
                Ok(Value::Null)
            }
            MailboxRequest::RecordTransportOutcome {
                message_id,
                claim_token,
                provider,
                result,
            } => {
                let r = self.sql.transaction_sync(|s| {
                    deliveries::record(
                        self.p,
                        s,
                        owner,
                        &message_id,
                        &claim_token,
                        &provider,
                        &result,
                        now,
                    )
                })?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            MailboxRequest::ApplyDeliveryEvent(e) => {
                let r = self
                    .sql
                    .transaction_sync(|s| deliveries::apply(self.p, s, owner, &e, now))?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            MailboxRequest::Cancel { message_id } => {
                let (out, ids) = self.sql.transaction_sync(|s| -> Result<_, ApiError> {
                    let out = deliveries::cancel(self.p, s, owner, &message_id, now)?;
                    // A canceled message leaves the semantic index (Search § 6.1).
                    let ids = search::vector_ids(s, &message_id)?;
                    s.exec(
                        "DELETE FROM chunks WHERE message_rowid = (SELECT rowid FROM messages WHERE id = ?1)",
                        &[message_id.clone().into()],
                    )?;
                    Ok((out, ids))
                })?;
                if !ids.is_empty() {
                    use pylota_identity_platform::QueueProducer as _;
                    let jobs: Vec<_> = ids
                        .chunks(500)
                        .map(
                            |c| pylota_identity_api_types::internal::IndexJob::DeleteVectors {
                                tenant_id: owner.tenant_id.to_owned(),
                                vector_ids: c.to_vec(),
                                also_next: self.p.vectors_next().is_some(),
                            },
                        )
                        .collect();
                    let _ = self
                        .p
                        .queues()
                        .send_batch(pylota_identity_platform::queues::QueueName::Index, &jobs, 0)
                        .await;
                }
                Ok(out)
            }
            MailboxRequest::Resolve {
                message_id,
                outcome,
            } => self.sql.transaction_sync(|s| {
                deliveries::resolve(self.p, s, owner, &message_id, &outcome, now)
            }),
            #[cfg(feature = "itest-hooks")]
            MailboxRequest::TestSchemaBack => {
                self.sql.exec(
                    "UPDATE meta SET v = CAST(CAST(v AS INTEGER) - 1 AS TEXT) WHERE k = 'schema_version'",
                    &[],
                )?;
                Ok(Value::Null)
            }
            #[cfg(feature = "itest-hooks")]
            MailboxRequest::TestAlarm => {
                self.alarm().await;
                Ok(Value::Null)
            }
            #[cfg(feature = "itest-hooks")]
            MailboxRequest::TestQuery { sql } => {
                let rows: Vec<Value> = self.sql.query(&sql, &[])?;
                Ok(Value::Array(rows))
            }
            #[cfg(feature = "itest-hooks")]
            MailboxRequest::TestBulkSeed { start, count, seed } => {
                let rows = self
                    .sql
                    .transaction_sync(|s| seed::bulk_seed(s, start, count, seed, now))?;
                Ok(Value::Array(rows))
            }
        }
    }

    /// Runs every due purpose, then re-arms (Design conventions § 4, rule 5).
    pub async fn alarm(&self) {
        if let Err(e) = self.alarm_purposes().await {
            crate::log::warn(
                "mailbox_alarm_failed",
                &[("kind", format!("{:?}", e.kind).into())],
            );
        }
        if let Err(e) = self.rearm().await {
            crate::log::warn(
                "mailbox_rearm_failed",
                &[("kind", format!("{:?}", e.kind).into())],
            );
        }
    }

    async fn alarm_purposes(&self) -> PResult<()> {
        self.migrate()?;
        let meta = self.meta()?;
        let (Some(tenant_id), Some(identity_id)) = (meta.tenant_id, meta.identity_id) else {
            return Ok(());
        };
        let now = self.p.clock().now_ms();
        let owner = outbox::Owner {
            tenant_id: &tenant_id,
            identity_id: &identity_id,
        };
        let due = |purpose: &str| -> PResult<bool> {
            Ok(self
                .sql
                .first::<Kv>(
                    "SELECT k, v FROM meta WHERE k = ?1 AND CAST(v AS INTEGER) <= ?2",
                    &[format!("alarm:{purpose}").into(), now.into()],
                )?
                .is_some())
        };
        if due("claim")? {
            alarms::claims(self.p, self.sql, &owner, now)?;
        }
        if due("dispatch")? {
            alarms::dispatch(self.p, self.sql, &owner, now).await?;
        }
        if due("maintenance")? {
            alarms::maintenance(self.p, self.sql, &owner, now).await?;
        }
        if !due("outbox")? {
            return Ok(());
        }
        let done = outbox::dispatch(self.p, self.sql, &owner, self.object_id, now).await?;
        self.sql.transaction_sync(|s| -> PResult<()> {
            match done {
                outbox::Dispatched::Done(_) => {
                    s.exec(
                        "DELETE FROM meta WHERE k IN ('alarm:outbox', 'outbox_backoff')",
                        &[],
                    )?;
                }
                outbox::Dispatched::More => {
                    s.exec(
                        "INSERT OR REPLACE INTO meta (k, v) VALUES ('alarm:outbox', ?1)",
                        &[now.to_string().into()],
                    )?;
                }
                outbox::Dispatched::Failed => {
                    let n: Option<Kv> = s.first(
                        "SELECT k, v FROM meta WHERE k = 'outbox_backoff'",
                        &[],
                    )?;
                    let n: u32 = n.and_then(|r| r.v.parse().ok()).unwrap_or(0);
                    s.exec(
                        "INSERT OR REPLACE INTO meta (k, v) VALUES ('outbox_backoff', ?1), ('alarm:outbox', ?2)",
                        &[(n + 1).to_string().into(), (now + outbox::backoff_ms(n)).to_string().into()],
                    )?;
                }
            }
            Ok(())
        })
    }
}

impl<P: Platform, S: Sql, St: ObjectState> Mailbox<'_, P, S, St> {
    /// Whether the owner tenant is a test tenant (`mode = 'test'`), read from D1.
    async fn is_test_tenant(&self) -> bool {
        use pylota_identity_platform::ControlDb as _;
        let Ok(Some(t)) = self.meta().map(|m| m.tenant_id) else {
            return false;
        };
        #[derive(Deserialize)]
        struct M {
            mode: String,
        }
        self.p
            .db()
            .first::<M>(pylota_identity_platform::d1::Stmt::new(
                "SELECT mode FROM tenants WHERE id = ?1",
                vec![t.into()],
            ))
            .await
            .ok()
            .flatten()
            .is_some_and(|m| m.mode == "test")
    }

    /// Threading § 2.3: the envelope detail checked against the thread keyring before the ingest
    /// transaction (its brute-force limits are applied inside it).
    async fn check_token(&self, identity_id: &str, detail: Option<&str>) -> ingest::TokenCheckWire {
        use pylota_identity_core::thread_token::{self, ThreadKey, ThreadKeyring, TokenCheck};
        let Some(detail) = detail else {
            return ingest::TokenCheckWire::Absent;
        };
        let kid = |k: &crate::keyring::Key| k.kid.bytes().next().unwrap_or(b'0');
        let keys = crate::keyring::keys(self.p, crate::keyring::Purpose::Thread)
            .await
            .unwrap_or_default();
        let Some(current) = keys.iter().find(|k| k.verify_until.is_none()) else {
            // No keyring: a token cannot verify, whatever it looks like.
            return match thread_token::verify(
                &ThreadKeyring {
                    current: ThreadKey {
                        kid: 0,
                        key: &[0; 32],
                    },
                    verifying: &[],
                },
                identity_id,
                detail,
            ) {
                TokenCheck::Absent => ingest::TokenCheckWire::Absent,
                _ => ingest::TokenCheckWire::Invalid,
            };
        };
        let verifying: Vec<ThreadKey<'_>> = keys
            .iter()
            .filter(|k| k.verify_until.is_some())
            .map(|k| ThreadKey {
                kid: kid(k),
                key: &k.secret,
            })
            .collect();
        let ring = ThreadKeyring {
            current: ThreadKey {
                kid: kid(current),
                key: &current.secret,
            },
            verifying: &verifying,
        };
        match thread_token::verify(&ring, identity_id, detail) {
            TokenCheck::Absent => ingest::TokenCheckWire::Absent,
            TokenCheck::Valid { seq, kid, current } => {
                ingest::TokenCheckWire::Valid { seq, kid, current }
            }
            TokenCheck::Invalid | TokenCheck::Unchecked => ingest::TokenCheckWire::Invalid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v1.sql is Data model § 2 with only the header and the `PRAGMA` line changed.
    #[test]
    fn schema_matches_data_model() {
        let doc = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/src/project/design/data-model.md"),
        )
        .unwrap();
        // Every SQL block of § 2.
        let section = &doc[doc.find("## 2. `IdentityMailbox`").unwrap()..];
        let section = &section[..section.find("\n## 3.").unwrap()];
        let mut from_doc = Vec::new();
        for block in section.split("```sql").skip(1) {
            from_doc.extend(statements(&block[..block.find("```").unwrap()]));
        }
        let ours = statements(V1);
        assert_eq!(from_doc[0], "PRAGMA foreign_keys = ON");
        assert_eq!(&from_doc[1..], &ours[..]);
        assert!(
            ours.iter().all(|s| s.contains("IF NOT EXISTS")),
            "every statement is idempotent"
        );
    }
}
