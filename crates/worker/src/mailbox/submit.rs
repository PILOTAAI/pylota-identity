//! `IdentityMailbox.submit` ([Outbound › The submit path]): the idempotency ledger, the policy pipeline
//! (steps 1–19), composition, the quota reservation, the thread lock, the sent copy and the accept
//! transaction; a dry run stops after step 17.
//!
//! [Outbound › The submit path]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::{domain_of, org_domain};
use pylota_identity_core::classify::{self, Kind};
use pylota_identity_core::compose::{self, ComposeInput, Disclosure, Forwarded, HeaderError};
use pylota_identity_core::days::rfc3339;
use pylota_identity_core::reply::{self, ReplyInputs};
use pylota_identity_core::thread;
use pylota_identity_platform::durable::{ObjectState, Sql};
use pylota_identity_platform::r2::{BlobBody, BlobMeta};
use pylota_identity_platform::{BlobStore as _, Clock as _, Ids as _, PResult, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::idempotency::{self, InFlight};
use super::messages::{self, Row};
use super::{Mailbox, locks, outbox, threads};
use crate::errors::ApiError;
use crate::quota::{self, QuotaRequest, ReserveAnswer, SendsHold};
use crate::send::{AddressCtx, RecipientFacts, SendContext};

/// At most this many automatic replies or ordinary sends per thread or sender a day (N35).
pub const MAX_REPLIES_PER_DAY: i64 = 25;
const DAY_MS: i64 = 86_400_000;
/// A message row's text budget.
const TEXT_MAX: usize = 512 * 1024;
const EXTRACTED_MAX: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubmitInput {
    /// `send`, `reply`, `reply_all` or `forward`.
    pub operation: String,
    /// The message answered or forwarded.
    pub target: Option<String>,
    /// hex SHA-256 of the `Idempotency-Key`; `None` on a dry run without one.
    pub key_hash: Option<String>,
    pub fingerprint: String,
    pub dry_run: bool,
    /// The key holds `quarantine:review`.
    pub review: bool,
    pub body: Value,
    pub ctx: SendContext,
    pub request_id: String,
}

/// One recipient after steps 6 and 16.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rcpt {
    pub address: String,
    pub name: String,
    pub field: String,
    /// `queued` or `suppressed`.
    pub status: String,
    /// For `suppressed`: the suppression reason or the list rule.
    pub reason: Option<String>,
    pub scope: Option<String>,
    pub smtp_response: Option<String>,
    pub first_reply: bool,
}

#[derive(Clone, Debug)]
struct PlannedAtt {
    id: String,
    filename: String,
    content_type: String,
    bytes: Vec<u8>,
    content_id: Option<String>,
    disposition: String,
    sniffed: String,
    sha256: String,
    linked: bool,
}

/// A reply's anchor: header ID, its references and its in-reply-to.
type Anchor = (String, Vec<String>, Option<String>);

/// Everything decided before the reservation.
struct Plan {
    message_id: String,
    kind: String,
    subject: String,
    recipients: Vec<Rcpt>,
    from: AddressCtx,
    from_name: String,
    thread: Option<(i64, String)>,
    in_reply_to: Option<String>,
    references: Vec<String>,
    hop: u32,
    headers: Vec<(String, String)>,
    text: Option<String>,
    html: Option<String>,
    link_note: Vec<String>,
    attachments: Vec<PlannedAtt>,
    labels: Vec<String>,
    metadata: Value,
    unsubscribe: Option<(String, Option<String>)>,
    forward: Option<Forwarded>,
    disclosure: Option<Disclosure>,
}

#[derive(Deserialize)]
struct N {
    n: i64,
}

#[derive(Deserialize)]
struct T {
    t: Option<i64>,
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}

fn err(code: ErrorCode, msg: &str) -> ApiError {
    ApiError::new(code, msg)
}

/// The `[{address, name}]` column form.
fn addr_json(list: &[(String, String)]) -> String {
    Value::Array(
        list.iter()
            .map(|(a, n)| json!({ "address": a, "name": n }))
            .collect(),
    )
    .to_string()
}

fn cut(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_owned(), false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_owned(), true)
}

/// The recipients of a body field (`[address | {address, name}]`).
fn body_recipients(v: &Value, field: &str) -> Vec<(String, String)> {
    v[field]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| match r {
                    Value::String(s) => (s.clone(), String::new()),
                    o => (
                        o["address"].as_str().unwrap_or("").to_owned(),
                        o["name"].as_str().unwrap_or("").to_owned(),
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn stored_list(json: &str) -> Vec<(String, String)> {
    serde_json::from_str::<Vec<Value>>(json)
        .unwrap_or_default()
        .iter()
        .filter_map(|a| {
            Some((
                a["address"].as_str()?.to_owned(),
                a["name"].as_str().unwrap_or("").to_owned(),
            ))
        })
        .collect()
}

/// The hop stored on a message (`automated_json.hop`, 0 when absent).
fn hop_of(row: &Row) -> u32 {
    row.automated_json
        .as_deref()
        .and_then(|a| serde_json::from_str::<Value>(a).ok())
        .and_then(|v| v["hop"].as_u64())
        .unwrap_or(0) as u32
}

impl<P: Platform, S: Sql, St: ObjectState> Mailbox<'_, P, S, St> {
    /// The whole submit; answers the `202` body (with `deduplicated`) or the dry-run result.
    pub async fn submit(
        &self,
        owner: &outbox::Owner<'_>,
        inp: SubmitInput,
        now: i64,
    ) -> Result<Value, ApiError> {
        let key_hash = inp.key_hash.clone().filter(|_| !inp.dry_run);
        let _guard = match &key_hash {
            Some(kh) => {
                if let Some(e) = idempotency::lookup(self.sql, kh, now)? {
                    return self.replay(owner, &inp, &e);
                }
                Some(InFlight::enter(self.object_id, kh).ok_or_else(|| {
                    err(
                        ErrorCode::RequestInProgress,
                        "A request with this Idempotency-Key is still running.",
                    )
                })?)
            }
            None => None,
        };
        let mut plan = self.plan(owner, &inp, now).await?;
        if inp.dry_run {
            return dry_run(&plan);
        }
        let key_hash = key_hash.ok_or_else(|| {
            err(
                ErrorCode::IdempotencyKeyRequired,
                "This route needs an Idempotency-Key.",
            )
        })?;
        let ctx = &inp.ctx;
        let queued = plan
            .recipients
            .iter()
            .filter(|r| r.status == "queued")
            .count() as u32;

        // Step 18: the sends hold and the daily caps, in one TenantQuota request.
        let identity_cap = ctx.identity.send_policy["daily_cap"]
            .as_u64()
            .or_else(|| ctx.pol("identity_daily_send_cap").as_u64())
            .unwrap_or(500) as u32;
        let tenant_cap = (!ctx.identity.is_system)
            .then(|| ctx.pol("tenant_daily_send_cap").as_u64().unwrap_or(5000) as u32);
        let reserve = QuotaRequest::Reserve {
            identity_id: owner.identity_id.to_owned(),
            tz: ctx.tenant.timezone.clone(),
            identity_cap,
            tenant_cap,
            hold: (queued > 0).then(|| SendsHold {
                units: queued,
                r#ref: plan.message_id.clone(),
                gates: Vec::new(),
            }),
            stop: None,
        };
        let answer = quota::call(
            self.p,
            &ctx.tenant.quota_do_id,
            owner.tenant_id,
            &inp.request_id,
            reserve,
        )
        .await?;
        let answer: ReserveAnswer = serde_json::from_value(answer)
            .map_err(|_| err(ErrorCode::Unavailable, "The quota did not answer."))?;
        let (day, warnings) = match answer {
            ReserveAnswer::Ok { day, warnings, .. } => (day, warnings),
            ReserveAnswer::Denied(d) => {
                return Err(
                    err(ErrorCode::BillingLimit, "The plan's allowance is spent.").details(
                        json!({ "feature": d.feature, "granted": d.granted, "used": d.used,
                        "resets_at": d.resets_at.map(rfc3339), "upgrade_url": null }),
                    ),
                );
            }
            ReserveAnswer::CapReached { scope, resets_at } => {
                return Err(
                    err(ErrorCode::DailyCapReached, "The daily send cap is reached.")
                        .details(json!({ "cap": scope, "resets_at": resets_at.map(rfc3339) })),
                );
            }
        };
        // Step 19 and submit step 5: the lock (and a new thread) with the compose marker.
        let started = self.p.clock().now_ms();
        let seq = loop {
            let now = self.p.clock().now_ms();
            let got = self
                .sql
                .transaction_sync(|tx| -> PResult<Option<(i64, String)>> {
                    let (seq, id) = match &plan.thread {
                        Some((seq, id)) => {
                            if !locks::acquire(tx, *seq, &plan.message_id, now, locks::LEASE_MS)? {
                                return Ok(None);
                            }
                            (*seq, id.clone())
                        }
                        None => {
                            let id = self.p.ids().new_id(IdPrefix::Thread);
                            let seq = threads::insert(tx, &id, "", now, None)?;
                            locks::acquire(tx, seq, &plan.message_id, now, locks::LEASE_MS)?;
                            (seq, id)
                        }
                    };
                    tx.exec(
                        "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
                        &[
                            format!("compose:{}", plan.message_id).into(),
                            now.to_string().into(),
                        ],
                    )?;
                    Ok(Some((seq, id)))
                })?;
            if let Some(t) = got {
                break t;
            }
            if now - started >= locks::WAIT_MS {
                let seq = plan.thread.as_ref().map_or(0, |t| t.0);
                self.release(
                    owner,
                    ctx,
                    &inp.request_id,
                    &day,
                    tenant_cap.is_some(),
                    &plan.message_id,
                )
                .await;
                return Err(err(
                    ErrorCode::ThreadBusy,
                    "Another send in this thread is in progress.",
                )
                .details(json!({ "retry_after": locks::retry_after(self.sql, seq, now) })));
            }
            self.p.clock().sleep(locks::RETRY_MS).await;
        };
        plan.thread = Some(seq.clone());

        // Compose with the thread token, then submit step 6: the sent copy.
        let reply_to = self.reply_to(owner, &plan.from, seq.0).await;
        let composed = compose::compose(&compose_input(&plan, ctx, reply_to.clone(), now, &inp));
        let ten = owner.tenant_id;
        let idn = owner.identity_id;
        let eml_key = format!("t/{ten}/i/{idn}/out/{}.eml", plan.message_id);
        let mut written = vec![eml_key.clone()];
        let blobs = self.p.blobs();
        let meta = BlobMeta {
            content_type: Some("message/rfc822".into()),
            custom: vec![
                ("tenant".into(), ten.to_owned()),
                ("identity".into(), idn.to_owned()),
                ("message".into(), plan.message_id.clone()),
                ("idem_key_sha256".into(), key_hash.clone()),
                ("fingerprint".into(), inp.fingerprint.clone()),
                ("operation".into(), inp.operation.clone()),
            ],
        };
        let mut put_ok = blobs
            .put(&eml_key, BlobBody::Bytes(composed.mime.clone()), &meta)
            .await
            .is_ok();
        for a in &plan.attachments {
            if !put_ok {
                break;
            }
            let k = format!("t/{ten}/i/{idn}/out/{}/a/{}", plan.message_id, a.id);
            put_ok = blobs
                .put(&k, BlobBody::Bytes(a.bytes.clone()), &BlobMeta::default())
                .await
                .is_ok();
            written.push(k);
        }
        // A submit killed after the sent copy (I9): the compose marker is left for the daily sweep.
        #[cfg(all(feature = "itest-hooks", target_arch = "wasm32"))]
        if pylota_identity_platform::cf::itest::take_fault("submit.after_put", "")
            .await
            .is_some()
        {
            return Err(err(
                ErrorCode::Unavailable,
                "Fault injected after the sent copy.",
            ));
        }
        if !put_ok {
            let _ = blobs.delete(&written).await;
            self.abandon(&plan.message_id)?;
            self.release(
                owner,
                ctx,
                &inp.request_id,
                &day,
                tenant_cap.is_some(),
                &plan.message_id,
            )
            .await;
            return Err(err(
                ErrorCode::Unavailable,
                "The sent copy could not be stored; retry.",
            ));
        }

        // Submit step 7: the accept transaction.
        let now = self.p.clock().now_ms();
        let stored = self.sql.transaction_sync(|tx| {
            self.store(
                tx,
                owner,
                &inp,
                &plan,
                &composed,
                reply_to.as_deref(),
                &eml_key,
                &key_hash,
                &warnings,
                &format!("{day}|{}", u8::from(tenant_cap.is_some())),
                now,
            )
        });
        let response = match stored {
            Ok(Some(r)) => r,
            Ok(None) | Err(_) => {
                let _ = blobs.delete(&written).await;
                self.abandon(&plan.message_id)?;
                self.release(
                    owner,
                    ctx,
                    &inp.request_id,
                    &day,
                    tenant_cap.is_some(),
                    &plan.message_id,
                )
                .await;
                if let Some(e) = idempotency::lookup(self.sql, &key_hash, now)? {
                    return self.replay(owner, &inp, &e);
                }
                return Err(err(
                    ErrorCode::Unavailable,
                    "The send could not be stored; retry.",
                ));
            }
        };

        // Step 8: the pointer.
        if queued > 0 {
            crate::consumers::outbound::enqueue(
                self.p,
                owner.tenant_id,
                owner.identity_id,
                &plan.message_id,
                &inp.request_id,
                0,
            )
            .await;
        }
        // The sent copy is searchable semantically too (Search § 6.1); never the system mailbox's (A15).
        if !inp.ctx.identity.is_system {
            use pylota_identity_platform::QueueProducer as _;
            let job = pylota_identity_api_types::internal::IndexJob::embed(
                owner.tenant_id,
                owner.identity_id,
                &plan.message_id,
                pylota_identity_api_types::internal::EmbedReason::New,
            );
            let _ = self
                .p
                .queues()
                .send(pylota_identity_platform::queues::QueueName::Index, &job, 0)
                .await;
        }
        Ok(response)
    }

    /// Undoes step 18: the day's reservation and the sends hold.
    async fn release(
        &self,
        owner: &outbox::Owner<'_>,
        ctx: &SendContext,
        request_id: &str,
        day: &str,
        tenant_counted: bool,
        message_id: &str,
    ) {
        let q = |req| {
            quota::call(
                self.p,
                &ctx.tenant.quota_do_id,
                owner.tenant_id,
                request_id,
                req,
            )
        };
        let _ = q(QuotaRequest::Release {
            identity_id: owner.identity_id.to_owned(),
            day: day.to_owned(),
            tenant_counted,
        })
        .await;
        let _ = q(QuotaRequest::Settle {
            feature: quota::Feature::Sends,
            r#ref: message_id.to_owned(),
            consume: 0,
            keep: 0,
        })
        .await;
    }

    /// A replay of an earlier accepted send, or `409 idempotency_conflict`.
    fn replay(
        &self,
        owner: &outbox::Owner<'_>,
        inp: &SubmitInput,
        e: &idempotency::Entry,
    ) -> Result<Value, ApiError> {
        if e.fingerprint != inp.fingerprint {
            return Err(err(
                ErrorCode::IdempotencyConflict,
                "This Idempotency-Key was used with a different request.",
            )
            .details(json!({ "original_message_id": e.message_id })));
        }
        let mut v: Value = serde_json::from_str(&e.response_json).unwrap_or(Value::Null);
        if let Some(mid) = &e.message_id
            && let Some(row) = self.sql.first::<Row>(
                &format!("{} WHERE m.id = ?1", messages::SELECT),
                &[mid.clone().into()],
            )?
        {
            v["extracted_text"] = json!(row.extracted_text);
            v["status"] = json!(row.status);
        }
        let _ = owner;
        v["deduplicated"] = json!(true);
        Ok(json!({ "replayed": true, "message": v }))
    }

    /// Deletes the compose marker and the lock of a send that will not be stored.
    fn abandon(&self, message_id: &str) -> PResult<()> {
        self.sql.transaction_sync(|tx| -> PResult<()> {
            tx.exec(
                "DELETE FROM meta WHERE k = ?1",
                &[format!("compose:{message_id}").into()],
            )?;
            locks::release(tx, message_id)
        })
    }

    /// The `Reply-To` with the thread token, for a domain with `reply_token = 'subaddress'`.
    pub(crate) async fn reply_to(
        &self,
        owner: &outbox::Owner<'_>,
        from: &AddressCtx,
        seq: i64,
    ) -> Option<String> {
        use pylota_identity_core::thread_token::{self, ThreadKey, ThreadKeyring};
        if from.reply_token != "subaddress" {
            return None;
        }
        let keys = crate::keyring::keys(self.p, crate::keyring::Purpose::Thread)
            .await
            .ok()?;
        let current = keys.iter().find(|k| k.verify_until.is_none())?;
        let ring = ThreadKeyring {
            current: ThreadKey {
                kid: current.kid.bytes().next().unwrap_or(b'0'),
                key: &current.secret,
            },
            verifying: &[],
        };
        let token = thread_token::mint(&ring, owner.identity_id, seq as u64);
        let (local, domain) = from.address.rsplit_once('@')?;
        Some(format!(
            "{}@{domain}",
            thread_token::reply_to_local(local, &token)
        ))
    }

    /// Steps 1–17.
    async fn plan(
        &self,
        owner: &outbox::Owner<'_>,
        inp: &SubmitInput,
        now: i64,
    ) -> Result<Plan, ApiError> {
        let ctx = &inp.ctx;
        let body = &inp.body;
        let op = inp.operation.as_str();
        let idn = &ctx.identity;
        // 1–4.
        if matches!(idn.status.as_str(), "deleting" | "deleted") {
            return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
        }
        if ctx.tenant.status == "suspended"
            || ctx.tenant.partner_status.as_deref() == Some("suspended")
        {
            return Err(err(ErrorCode::TenantSuspended, "The tenant is suspended."));
        }
        if idn.status == "paused" {
            return Err(err(ErrorCode::IdentityPaused, "The identity is paused.")
                .details(json!({ "reason": idn.pause_reason })));
        }
        if ctx.tenant.sending_paused_at.is_some() && !idn.is_system {
            return Err(err(
                ErrorCode::SendingPaused,
                "Sending is paused for this tenant.",
            )
            .details(json!({ "scope": "tenant", "reason": ctx.tenant.sending_pause_reason })));
        }
        if idn.owner_name.is_none() || idn.owner_email.is_none() {
            return Err(err(
                ErrorCode::IdentityOwnerRequired,
                "The identity has no accountable human (owner_name and owner_email).",
            ));
        }
        // 5. The target.
        let target: Option<Row> = match &inp.target {
            Some(mid) => {
                let row: Option<Row> = self.sql.first(
                    &format!("{} WHERE m.id = ?1", messages::SELECT),
                    &[mid.clone().into()],
                )?;
                let visible = |r: &Row| {
                    !matches!(r.status.as_str(), "hidden" | "throttled")
                        && (inp.review || !matches!(r.status.as_str(), "quarantined" | "screening"))
                };
                Some(
                    row.filter(visible)
                        .ok_or_else(|| ApiError::not_found(ErrorCode::MessageNotFound))?,
                )
            }
            None => None,
        };
        let thread_id = s(&body["thread_id"]);
        let thread: Option<(i64, String)> = match (&target, &thread_id) {
            (Some(t), _) => Some((t.thread_seq, t.thread_id.clone())),
            (None, Some(tid)) => {
                #[derive(Deserialize)]
                struct Th {
                    seq: i64,
                    id: String,
                }
                let th: Option<Th> = self.sql.first(
                    "SELECT seq, id FROM threads WHERE id = ?1",
                    &[tid.clone().into()],
                )?;
                Some(
                    th.map(|t| (t.seq, t.id))
                        .ok_or_else(|| ApiError::not_found(ErrorCode::ThreadNotFound))?,
                )
            }
            (None, None) => None,
        };
        let own: Vec<&str> = ctx.addresses.iter().map(|a| a.address.as_str()).collect();

        // 6. Recipients.
        let mut reply_target: Option<String> = None;
        let mut fields: Vec<(String, String, &'static str)> = Vec::new();
        match (op, &target) {
            ("reply" | "reply_all", Some(t)) if t.direction == "inbound" => {
                let from = t.from_address.clone().unwrap_or_default();
                let rt: Vec<String> = stored_list(&t.reply_to_json)
                    .into_iter()
                    .map(|(a, _)| a)
                    .collect();
                let contacts = ContactsOf(self.sql);
                let target_addr = reply::reply_target(
                    &ReplyInputs {
                        from: &from,
                        reply_to: &rt,
                        known_sender: t.known_sender == Some(1),
                    },
                    &contacts,
                )
                .to_owned();
                reply_target = Some(target_addr.clone());
                let name = if target_addr == from {
                    t.from_name.clone().unwrap_or_default()
                } else {
                    String::new()
                };
                fields.push((target_addr, name, "to"));
                if op == "reply_all" {
                    for (a, n) in stored_list(&t.to_json) {
                        fields.push((a, n, "to"));
                    }
                    for (a, n) in stored_list(&t.cc_json) {
                        fields.push((a, n, "cc"));
                    }
                }
            }
            ("reply" | "reply_all", Some(t)) => {
                for (a, n) in stored_list(&t.to_json) {
                    fields.push((a, n, "to"));
                }
                for (a, n) in stored_list(&t.cc_json) {
                    fields.push((a, n, "cc"));
                }
            }
            _ => {
                for f in ["to", "cc", "bcc"] {
                    let field: &'static str = match f {
                        "to" => "to",
                        "cc" => "cc",
                        _ => "bcc",
                    };
                    for (a, n) in body_recipients(body, f) {
                        fields.push((a, n, field));
                    }
                }
            }
        }
        let mut recipients: Vec<Rcpt> = Vec::new();
        for (a, name, field) in fields {
            let addr = compose::recipient(&a).map_err(|e| match e {
                compose::AddressError::Invalid => err(
                    ErrorCode::AddressInvalid,
                    "A recipient address is not valid.",
                )
                .details(json!({ "address": a })),
                compose::AddressError::Unsupported => err(
                    ErrorCode::AddressUnsupported,
                    "Addresses with a non-ASCII local part are not supported.",
                )
                .details(json!({ "address": a })),
            })?;
            if recipients.iter().any(|r| r.address == addr) {
                continue;
            }
            if op == "reply_all" && own.contains(&addr.as_str()) {
                continue;
            }
            recipients.push(Rcpt {
                address: addr,
                name,
                field: field.to_owned(),
                status: "queued".into(),
                reason: None,
                scope: None,
                smtp_response: None,
                first_reply: false,
            });
        }
        if recipients.is_empty() {
            return Err(ApiError::invalid("to", "At least one recipient is needed."));
        }
        // 7.
        let max = ctx
            .pol("max_recipients")
            .as_u64()
            .unwrap_or(10)
            .min(compose::HARD_MAX_RECIPIENTS as u64) as usize;
        if recipients.len() > max {
            return Err(err(ErrorCode::TooManyRecipients, "Too many recipients.")
                .details(json!({ "limit": max })));
        }
        let addrs: Vec<String> = recipients.iter().map(|r| r.address.clone()).collect();
        let test = ctx.tenant.mode == "test";
        let facts: RecipientFacts = crate::send::context::recipient_facts(
            self.p,
            owner.tenant_id,
            owner.identity_id,
            &addrs,
            test,
            now,
        )
        .await?;
        // 8. Test tenants (L1, L3).
        if test {
            for r in &recipients {
                let ok = r.address.ends_with("@simulator.invalid")
                    || facts.directory.iter().any(|(a, ten, mode, partner, _)| {
                        *a == r.address
                            && (ten == owner.tenant_id
                                || (mode == "test"
                                    && partner.is_some()
                                    && *partner == ctx.tenant.partner_id))
                    });
                if !ok {
                    return Err(err(
                        ErrorCode::TestModeRecipient,
                        "A test tenant sends only to the simulator and to its own identities.",
                    )
                    .details(json!({ "address": r.address })));
                }
            }
        }
        // 9.
        let kind = s(&body["kind"]).unwrap_or_else(|| "transactional".into());
        let unsubscribe = if kind == "marketing" {
            let url = s(&body["unsubscribe"]["url"]).filter(|u| u.starts_with("https://"));
            match (url, body.get("consent").filter(|c| !c.is_null())) {
                (Some(u), Some(_)) => Some((u, s(&body["unsubscribe"]["mailto"]))),
                _ => {
                    return Err(err(
                        ErrorCode::MarketingRequirementsMissing,
                        "Marketing mail needs unsubscribe.url (HTTPS) and consent.",
                    ));
                }
            }
        } else {
            None
        };
        // 10. Auto-replies (D6).
        if kind == "auto_reply" {
            let t = target
                .as_ref()
                .filter(|_| matches!(op, "reply" | "reply_all"));
            let target_kind = t.map_or(Kind::Automated, |t| match t.kind.as_str() {
                "normal" => Kind::Normal,
                "calendar" => Kind::Calendar,
                _ => Kind::Automated,
            });
            let exchanges: Option<N> = match t {
                Some(t) => self.sql.first(
                    "SELECT COUNT(*) AS n FROM messages WHERE thread_seq = ?1 AND direction = 'outbound' AND kind = 'auto_reply' AND status NOT IN ('canceled','rejected','failed','suppressed') AND rowid > COALESCE((SELECT MAX(rowid) FROM messages WHERE thread_seq = ?1 AND direction = 'outbound' AND kind <> 'auto_reply'), 0)",
                    &[t.thread_seq.into()],
                )?,
                None => None,
            };
            let cap = ctx
                .pol("auto_reply.max_automatic_exchanges")
                .as_u64()
                .unwrap_or(2) as u32;
            let allowed = ctx.pol("auto_reply.allowed").as_bool().unwrap_or(true)
                && idn.send_policy["auto_reply"].as_str() != Some("denied")
                && classify::auto_reply_allowed(
                    target_kind,
                    exchanges.map_or(0, |n| n.n as u32),
                    cap,
                );
            if t.is_none() || !allowed {
                return Err(err(
                    ErrorCode::AutoReplyNotAllowed,
                    "This auto-reply is not allowed.",
                ));
            }
        }
        // 10a. The loop guard (N13).
        let previous_hop = match (&target, &thread) {
            (Some(t), _) => Some(hop_of(t)),
            (None, Some((seq, _))) => self
                .sql
                .first::<Row>(
                    &format!("{} WHERE m.thread_seq = ?1 ORDER BY m.received_at DESC, m.rowid DESC LIMIT 1", messages::SELECT),
                    &[(*seq).into()],
                )?
                .map(|r| hop_of(&r)),
            (None, None) => None,
        };
        let hop = classify::outbound_hop(previous_hop).map_err(|hop| {
            err(
                ErrorCode::LoopDetected,
                "This exchange looks like a mail loop.",
            )
            .details(json!({ "hop": hop }))
        })?;
        // 10b. The reply rate (N35).
        if let Some((seq, _)) = &thread {
            let since = now - DAY_MS;
            let oldest_of = |rows: Option<T>| rows.and_then(|t| t.t).unwrap_or(now);
            let n: Option<N> = self.sql.first(
                "SELECT COUNT(*) AS n FROM messages WHERE thread_seq = ?1 AND direction = 'outbound' AND received_at > ?2 AND status NOT IN ('canceled','rejected','failed','suppressed')",
                &[(*seq).into(), since.into()],
            )?;
            if n.is_some_and(|n| n.n >= MAX_REPLIES_PER_DAY) {
                let oldest: Option<T> = self.sql.first(
                    "SELECT MIN(received_at) AS t FROM (SELECT received_at FROM messages WHERE thread_seq = ?1 AND direction = 'outbound' AND received_at > ?2 AND status NOT IN ('canceled','rejected','failed','suppressed') ORDER BY received_at DESC LIMIT ?3)",
                    &[(*seq).into(), since.into(), MAX_REPLIES_PER_DAY.into()],
                )?;
                let retry = ((oldest_of(oldest) + DAY_MS - now + 999) / 1000).max(1);
                return Err(err(
                    ErrorCode::LoopDetected,
                    "Too many replies in this thread today.",
                )
                .details(json!({ "reason": "thread_rate", "retry_after": retry })));
            }
            if let (Some(t), "reply" | "reply_all") = (&target, op)
                && t.direction == "inbound"
                && let Some(from) = &t.from_address
            {
                let n: Option<N> = self.sql.first(
                    "SELECT COUNT(*) AS n FROM messages WHERE from_address = ?1 AND direction = 'inbound' AND received_at > ?2",
                    &[from.clone().into(), since.into()],
                )?;
                if n.is_some_and(|n| n.n >= MAX_REPLIES_PER_DAY) {
                    let oldest: Option<T> = self.sql.first(
                        "SELECT MIN(received_at) AS t FROM (SELECT received_at FROM messages WHERE from_address = ?1 AND direction = 'inbound' AND received_at > ?2 ORDER BY received_at DESC LIMIT ?3)",
                        &[from.clone().into(), since.into(), MAX_REPLIES_PER_DAY.into()],
                    )?;
                    let retry = ((oldest_of(oldest) + DAY_MS - now + 999) / 1000).max(1);
                    return Err(err(
                        ErrorCode::LoopDetected,
                        "Too many messages from this sender today.",
                    )
                    .details(json!({ "reason": "sender_rate", "retry_after": retry })));
                }
            }
        }
        // 11. Custom headers.
        let raw_headers: Vec<(String, String)> = body["headers"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let service = usize::from(target.is_some() || thread.is_some()) * 2
            + usize::from(kind == "auto_reply")
            + usize::from(unsubscribe.is_some()) * 2;
        let headers = compose::check_headers(&raw_headers, service).map_err(|e| match e {
            HeaderError::NotAllowed(n) => {
                err(ErrorCode::HeaderNotAllowed, "This header may not be set.")
                    .details(json!({ "header": n }))
            }
            HeaderError::Invalid { path, message } => ApiError::invalid(&path, message),
        })?;
        // 12. Content.
        let text = s(&body["text"]);
        let html = s(&body["html"]);
        if text.is_none() && html.is_none() {
            return Err(ApiError::invalid("text", "text or html is required."));
        }
        let subject = match (op, &target) {
            ("reply" | "reply_all", Some(t)) => {
                thread::reply_subject(t.subject.as_deref().unwrap_or(""))
            }
            ("forward", Some(t)) => thread::forward_subject(t.subject.as_deref().unwrap_or("")),
            _ => s(&body["subject"]).unwrap_or_default(),
        };
        if subject.chars().count() > 998 {
            return Err(ApiError::invalid("subject", "At most 998 characters."));
        }
        let mut attachments: Vec<PlannedAtt> = Vec::new();
        let list = body["attachments"].as_array().cloned().unwrap_or_default();
        if list.len() > 32 {
            return Err(ApiError::invalid("attachments", "At most 32 attachments."));
        }
        let mut total = 0usize;
        for (i, a) in list.iter().enumerate() {
            if a.get("upload_id").is_some_and(|u| !u.is_null()) {
                return Err(ApiError::not_found(ErrorCode::UploadNotFound));
            }
            let path = format!("attachments[{i}]");
            let bytes = a["content_base64"]
                .as_str()
                .and_then(pylota_identity_core::links::decode_base64)
                .ok_or_else(|| {
                    ApiError::invalid(&format!("{path}.content_base64"), "Not valid base64.")
                })?;
            let filename = pylota_identity_core::mime::sanitise_filename(
                a["filename"].as_str().unwrap_or("attachment"),
            );
            let content_type = a["content_type"]
                .as_str()
                .unwrap_or("application/octet-stream")
                .to_ascii_lowercase();
            if content_type.split('/').filter(|p| !p.is_empty()).count() != 2 {
                return Err(ApiError::invalid(
                    &format!("{path}.content_type"),
                    "A type/subtype.",
                ));
            }
            let disposition = a["disposition"].as_str().unwrap_or("attachment").to_owned();
            let content_id = s(&a["content_id"]);
            if disposition == "inline" && content_id.is_none() {
                return Err(ApiError::invalid(
                    &format!("{path}.content_id"),
                    "An inline part needs content_id.",
                ));
            }
            let assess =
                pylota_identity_core::attach::assess(&bytes, Some(&filename), &content_type);
            if assess.risk.is_some_and(|r| r.quarantines()) {
                return Err(err(
                    ErrorCode::AttachmentRisky,
                    "An attachment would be quarantined as risky.",
                )
                .details(json!({ "filename": filename, "risk": assess.risk })));
            }
            total += bytes.len();
            attachments.push(PlannedAtt {
                id: self.p.ids().new_id(IdPrefix::Attachment),
                sha256: pylota_identity_core::ses::sha256_hex(&bytes),
                filename,
                content_type,
                bytes,
                content_id: (disposition == "inline").then_some(content_id).flatten(),
                disposition,
                sniffed: assess.sniffed_type.to_owned(),
                linked: false,
            });
        }
        if total > compose::MAX_ATTACHMENT_BYTES {
            return Err(
                err(ErrorCode::MessageTooLarge, "The attachments are too large.")
                    .details(json!({ "reason": "attachments_total" })),
            );
        }
        let labels: Vec<String> = body["labels"]
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if labels.len() > 64
            || labels
                .iter()
                .any(|l| !crate::handlers::messages::valid_label(l))
        {
            return Err(ApiError::invalid("labels", "At most 64 valid labels."));
        }
        let metadata = body
            .get("metadata")
            .filter(|m| m.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        if metadata.as_object().is_some_and(|m| {
            m.len() > 16 || m.values().any(|v| v.as_str().is_none_or(|s| s.len() > 512))
        }) {
            return Err(ApiError::invalid(
                "metadata",
                "At most 16 string values of 512 bytes.",
            ));
        }
        // 13. From.
        let from_address = s(&body["from_address"]).map(|a| a.to_ascii_lowercase());
        let from_invalid = || {
            ApiError::invalid(
                "from_address",
                "Not an address this identity can send from.",
            )
        };
        let from = match (&from_address, &thread) {
            (Some(a), th) => {
                let addr = ctx.address(a).ok_or_else(from_invalid)?;
                let ok = match addr.status.as_str() {
                    "active" | "pending" => true,
                    "retiring" => match th {
                        Some((seq, _)) => self.thread_uses(*seq, a)?,
                        None => false,
                    },
                    _ => false,
                };
                if !ok {
                    return Err(from_invalid());
                }
                addr.clone()
            }
            (None, Some((seq, _))) => self.select_reply_from(ctx, *seq)?,
            (None, None) => ctx.primary().cloned().ok_or_else(from_invalid)?,
        };
        // 14, 14a, 15.
        if matches!(from.state.as_str(), "pending" | "verifying") || from.status == "pending" {
            return Err(err(
                ErrorCode::DomainNotReady,
                "The sending domain is not verified yet.",
            ));
        }
        if from.sending_paused_at.is_some() {
            return Err(err(
                ErrorCode::SendingPaused,
                "Sending is paused for this domain.",
            )
            .details(json!({ "scope": "domain", "domain_id": from.domain_id })));
        }
        if from.transport == "ses" && !ctx.ses_configured {
            return Err(err(
                ErrorCode::TransportUnavailable,
                "Amazon SES is not configured.",
            )
            .details(json!({ "reason": "ses_not_configured" })));
        }
        if kind == "marketing" && from.transport == "cloudflare" {
            return Err(err(
                ErrorCode::TransportUnavailable,
                "Marketing mail needs a domain that sends through SES or an SMTP relay.",
            )
            .details(json!({ "reason": "marketing_needs_ses" })));
        }
        // 16. Recipient filters.
        let require_known = idn.send_policy["require_known_recipient"]
            .as_bool()
            .unwrap_or(!idn.is_system);
        let allowlist_only = ctx.pol("send_allowlist_only").as_bool().unwrap_or(false);
        let rule3 = target.as_ref().is_some_and(|t| {
            matches!(op, "reply" | "reply_all")
                && t.direction == "inbound"
                && t.status == "received"
                && matches!(t.verdict.as_deref(), Some("pass" | "arc_rescued"))
        });
        for r in &mut recipients {
            let written = ContactsOf(self.sql).outbound(&r.address) > 0;
            let decision = facts.decision(&r.address);
            let is_target = reply_target.as_deref() == Some(r.address.as_str());
            let suppress = |reason: &str, scope: Option<&str>, smtp: String| {
                (reason.to_owned(), scope.map(str::to_owned), smtp)
            };
            let filtered = if let Some(reason) = facts.suppression(&r.address) {
                Some(suppress(
                    reason,
                    None,
                    format!("policy: suppression ({reason})"),
                ))
            } else if let Some(d) = decision.as_ref().filter(|d| d.block) {
                let scope = if d.identity { "identity" } else { "tenant" };
                let smtp = if d.identity {
                    "policy: send_block (identity)"
                } else {
                    "policy: send_block"
                };
                Some(suppress("send_block", Some(scope), smtp.to_owned()))
            } else if allowlist_only && decision.is_none() {
                Some(suppress(
                    "not_on_allowlist",
                    None,
                    "policy: not_on_allowlist".into(),
                ))
            } else if require_known && !written && decision.is_none() && !(rule3 && is_target) {
                Some(suppress(
                    "unknown_recipient",
                    None,
                    "policy: unknown_recipient".into(),
                ))
            } else {
                None
            };
            if let Some((reason, scope, smtp)) = filtered {
                r.status = "suppressed".into();
                r.reason = Some(reason);
                r.scope = scope;
                r.smtp_response = Some(smtp);
            } else if rule3 && is_target && !written {
                r.first_reply = true;
            }
        }
        // Threading headers.
        let (in_reply_to, references) = match (&target, &thread) {
            (Some(t), _) => {
                let parent = self.anchor(t)?;
                match parent {
                    Some(p) => {
                        let refs = thread::build_references(&p.1, p.2.as_deref(), &p.0);
                        ((op != "forward").then_some(p.0), refs)
                    }
                    None => (None, Vec::new()),
                }
            }
            (None, Some((seq, _))) => {
                let latest: Option<Row> = self.sql.first(
                    &format!("{} WHERE m.thread_seq = ?1 AND m.rfc_message_id IS NOT NULL ORDER BY m.received_at DESC, m.rowid DESC LIMIT 1", messages::SELECT),
                    &[(*seq).into()],
                )?;
                match latest {
                    Some(l) => {
                        let refs: Vec<String> =
                            serde_json::from_str(&l.references_json.clone()).unwrap_or_default();
                        let id = l.rfc_message_id.clone().unwrap_or_default();
                        (
                            Some(id.clone()),
                            thread::build_references(&refs, l.in_reply_to.as_deref(), &id),
                        )
                    }
                    None => (None, Vec::new()),
                }
            }
            (None, None) => (None, Vec::new()),
        };
        // Forward (C6).
        let forward = match (op, &target) {
            ("forward", Some(t)) => Some(Forwarded {
                from: match (&t.from_name, &t.from_address) {
                    (Some(n), Some(a)) if !n.is_empty() => format!("{n} <{a}>"),
                    (_, Some(a)) => a.clone(),
                    _ => String::new(),
                },
                date: rfc2822(t.sent_at.unwrap_or(t.received_at)),
                subject: t.subject.clone().unwrap_or_default(),
                to: stored_list(&t.to_json)
                    .into_iter()
                    .map(|(a, _)| a)
                    .collect::<Vec<_>>()
                    .join(", "),
                text: t.text.clone().unwrap_or_default(),
                html: t.html_sanitized.clone(),
            }),
            _ => None,
        };
        if let (Some(t), true) = (
            &target,
            op == "forward" && body["include_attachments"].as_bool().unwrap_or(true),
        ) {
            for a in self
                .sql
                .query::<messages::AttachmentRow>(messages::ATTACHMENTS_OF, &[t.rowid.into()])?
            {
                if a.risk.is_some() {
                    continue;
                }
                let Ok(Some(blob)) = self.p.blobs().get(&a.r2_key).await else {
                    continue;
                };
                let bytes = blob.body.to_vec();
                attachments.push(PlannedAtt {
                    id: self.p.ids().new_id(IdPrefix::Attachment),
                    filename: a.filename.clone().unwrap_or_else(|| "attachment".into()),
                    content_type: a.content_type.clone(),
                    sha256: a.sha256.clone(),
                    content_id: None,
                    disposition: "attachment".into(),
                    sniffed: a
                        .sniffed_type
                        .clone()
                        .unwrap_or_else(|| a.content_type.clone()),
                    linked: false,
                    bytes,
                });
            }
        }
        let disclosure = match ctx.pol("ai_disclosure.mode").as_str() {
            Some("footer") => Some(Disclosure::Footer(
                ctx.pol("ai_disclosure.text")
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
            )),
            Some("header") => Some(Disclosure::Header),
            _ => None,
        };
        let mut plan = Plan {
            message_id: self.p.ids().new_id(IdPrefix::Message),
            kind,
            subject,
            recipients,
            from_name: idn.display_name.clone(),
            from,
            thread,
            in_reply_to,
            references,
            hop,
            headers,
            text,
            html,
            link_note: Vec::new(),
            attachments,
            labels,
            metadata,
            unsubscribe,
            forward,
            disclosure,
        };
        // 17. Size, with links for `large_attachments: "link"` (G5).
        let placeholder = Some(format!("{}@{}", "x".repeat(40), plan.from.domain));
        let size = |plan: &Plan| {
            compose::compose(&compose_input(plan, ctx, placeholder.clone(), now, inp))
                .mime
                .len()
        };
        if size(&plan) > compose::MAX_MIME_BYTES {
            if ctx.pol("large_attachments").as_str() != Some("link") {
                return Err(err(
                    ErrorCode::MessageTooLarge,
                    "The message is too large for the transport.",
                )
                .details(json!({ "reason": "transport_limit" })));
            }
            let keys = crate::keyring::keys(self.p, crate::keyring::Purpose::Link).await?;
            let key = keys
                .iter()
                .find(|k| k.verify_until.is_none())
                .ok_or_else(|| err(ErrorCode::Unavailable, "No link key yet; retry."))?;
            let ttl_h = ctx.pol("link_ttl_hours").as_i64().unwrap_or(72);
            let expires_s = now / 1000 + ttl_h * 3600;
            let mut order: Vec<usize> = (0..plan.attachments.len()).collect();
            order.sort_by_key(|i| std::cmp::Reverse(plan.attachments[*i].bytes.len()));
            for i in order {
                if size(&plan) <= compose::MAX_MIME_BYTES {
                    break;
                }
                let a = &mut plan.attachments[i];
                a.linked = true;
                let token = pylota_identity_core::links::mint(
                    &key.secret,
                    &pylota_identity_core::links::AttLink {
                        kid: key.kid.chars().next().unwrap_or('0'),
                        tenant_id: owner.tenant_id.to_owned(),
                        identity_id: owner.identity_id.to_owned(),
                        message_id: plan.message_id.clone(),
                        attachment_id: a.id.clone(),
                        expires_s,
                    },
                );
                plan.link_note.push(format!(
                    "{} ({} bytes): https://{}/v1/links/{token} (available until {})",
                    a.filename,
                    a.bytes.len(),
                    self.p.config().api_host,
                    rfc3339(expires_s * 1000)
                ));
            }
            if size(&plan) > compose::MAX_MIME_BYTES {
                return Err(err(
                    ErrorCode::MessageTooLarge,
                    "The message is too large for the transport.",
                )
                .details(json!({ "reason": "transport_limit" })));
            }
        }
        Ok(plan)
    }

    /// Whether thread `seq` already uses `address` (G7): its `reply_from_address`, or an outbound message.
    fn thread_uses(&self, seq: i64, address: &str) -> PResult<bool> {
        let n: Option<N> = self.sql.first(
            "SELECT COUNT(*) AS n FROM threads t WHERE t.seq = ?1 AND (t.reply_from_address = ?2 OR EXISTS (SELECT 1 FROM messages m WHERE m.thread_seq = ?1 AND m.direction = 'outbound' AND m.from_address = ?2))",
            &[seq.into(), address.into()],
        )?;
        Ok(n.is_some_and(|n| n.n > 0))
    }

    /// `select_reply_from` (Threading § 5).
    fn select_reply_from(&self, ctx: &SendContext, seq: i64) -> Result<AddressCtx, ApiError> {
        #[derive(Deserialize)]
        struct Th {
            reply_from_address: Option<String>,
            fallback_pinned: i64,
        }
        let th: Option<Th> = self.sql.first(
            "SELECT reply_from_address, fallback_pinned FROM threads WHERE seq = ?1",
            &[seq.into()],
        )?;
        let th = th.ok_or_else(|| ApiError::not_found(ErrorCode::ThreadNotFound))?;
        if th.fallback_pinned == 1
            && let Some(p) = ctx.platform_address()
        {
            return Ok(p.clone());
        }
        if let Some(a) = th
            .reply_from_address
            .as_deref()
            .and_then(|a| ctx.address(a))
            && matches!(a.status.as_str(), "active" | "retiring")
        {
            return Ok(a.clone());
        }
        ctx.primary().cloned().ok_or_else(|| {
            ApiError::invalid("from_address", "The identity has no primary address.")
        })
    }

    /// The anchor of a reply or forward of `t`: `(header ID, its references, its in-reply-to)` of `t` or
    /// of the latest earlier message of its thread with a known header ID (Threading § 4).
    fn anchor(&self, t: &Row) -> PResult<Option<Anchor>> {
        let row = if t.rfc_message_id.is_some() {
            Some(t.clone())
        } else {
            self.sql.first::<Row>(
                &format!("{} WHERE m.thread_seq = ?1 AND m.rowid < ?2 AND m.rfc_message_id IS NOT NULL ORDER BY m.rowid DESC LIMIT 1", messages::SELECT),
                &[t.thread_seq.into(), t.rowid.into()],
            )?
        };
        Ok(row.map(|r| {
            let refs: Vec<String> =
                serde_json::from_str(&r.references_json.clone()).unwrap_or_default();
            (
                r.rfc_message_id.clone().unwrap_or_default(),
                refs,
                r.in_reply_to.clone(),
            )
        }))
    }

    /// Submit step 7, inside one transaction. `None` when the ledger already holds the key.
    #[allow(clippy::too_many_arguments)]
    fn store(
        &self,
        tx: &S,
        owner: &outbox::Owner<'_>,
        inp: &SubmitInput,
        plan: &Plan,
        composed: &compose::Composed,
        reply_to: Option<&str>,
        eml_key: &str,
        key_hash: &str,
        warnings: &[quota::QuotaWarning],
        reserve: &str,
        now: i64,
    ) -> Result<Option<Value>, ApiError> {
        if idempotency::lookup(tx, key_hash, now)?.is_some() {
            return Ok(None);
        }
        let (seq, _) = plan.thread.clone().expect("the lock step set the thread");
        let all_suppressed = plan.recipients.iter().all(|r| r.status == "suppressed");
        let status = if all_suppressed {
            "suppressed"
        } else {
            "queued"
        };
        let by = |f: &str| -> Vec<(String, String)> {
            plan.recipients
                .iter()
                .filter(|r| r.field == f)
                .map(|r| (r.address.clone(), r.name.clone()))
                .collect()
        };
        let full_text = composed.text.clone().unwrap_or_default();
        let (text, t1) = cut(&full_text, TEXT_MAX);
        let (extracted, _) = cut(&full_text, EXTRACTED_MAX);
        let html_sanitized = composed
            .html
            .as_deref()
            .map(|h| pylota_identity_core::sanitize::parse(h).sanitized_html());
        let mut flags: Vec<&str> = Vec::new();
        if t1 {
            flags.push("body_truncated");
        }
        let (text, html_sanitized, extracted) = if inp.ctx.identity.is_system {
            flags.push("body_redacted");
            let r = pylota_identity_core::text::redact_links_and_codes;
            (r(&text), html_sanitized.map(|h| r(&h)), r(&extracted))
        } else {
            (text, html_sanitized, extracted)
        };
        let snippet = pylota_identity_core::text::snippet(&extracted);
        let rfc_id = format!(
            "{}@{}",
            plan.message_id[4..].to_ascii_lowercase(),
            plan.from.domain
        );
        let row: Option<N> = tx.first(
            "INSERT INTO messages (id, thread_seq, direction, status, provider, raw_r2_key, raw_size, from_address, from_name, sender_domain, reply_to_json, to_json, cc_json, bcc_json, subject, text, html_sanitized, extracted_text, snippet, sent_at, received_at, kind, flags_json, operation, in_reply_to, references_json, metadata_json, automated_json, rfc_message_id) \
             VALUES (?1, ?2, 'outbound', ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, NULL) RETURNING rowid AS n",
            &[
                plan.message_id.clone().into(),
                seq.into(),
                status.into(),
                eml_key.into(),
                (composed.mime.len() as i64).into(),
                plan.from.address.clone().into(),
                plan.from_name.clone().into(),
                org_domain(domain_of(&plan.from.address)).to_owned().into(),
                addr_json(&reply_to.map(|r| vec![(r.to_owned(), String::new())]).unwrap_or_default()).into(),
                addr_json(&by("to")).into(),
                addr_json(&by("cc")).into(),
                addr_json(&by("bcc")).into(),
                plan.subject.clone().into(),
                text.into(),
                html_sanitized.into(),
                extracted.clone().into(),
                snippet.into(),
                now.into(),
                plan.kind.clone().into(),
                serde_json::to_string(&flags).unwrap_or_else(|_| "[]".into()).into(),
                inp.operation.clone().into(),
                plan.in_reply_to.clone().into(),
                serde_json::to_string(&plan.references).unwrap_or_else(|_| "[]".into()).into(),
                plan.metadata.to_string().into(),
                json!({ "hop": plan.hop }).to_string().into(),
            ],
        )?;
        let _ = rfc_id;
        let rowid = row
            .map(|r| r.n)
            .ok_or_else(|| err(ErrorCode::InternalError, "No rowid."))?;
        for r in &plan.recipients {
            tx.exec(
                "INSERT INTO deliveries (message_rowid, address, field, status, smtp_response, first_reply, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                &[
                    rowid.into(),
                    r.address.clone().into(),
                    r.field.clone().into(),
                    r.status.clone().into(),
                    r.smtp_response.clone().into(),
                    i64::from(r.first_reply).into(),
                    now.into(),
                ],
            )?;
            if r.status == "queued" {
                tx.exec(
                    "INSERT INTO contacts (address, name, domain, first_seen_at, last_seen_at, outbound_count, last_thread_seq) VALUES (?1, ?2, ?3, ?4, ?4, 1, ?5) \
                     ON CONFLICT (address) DO UPDATE SET last_seen_at = MAX(contacts.last_seen_at, excluded.last_seen_at), outbound_count = contacts.outbound_count + 1, last_thread_seq = excluded.last_thread_seq",
                    &[r.address.clone().into(), r.name.clone().into(), domain_of(&r.address).to_owned().into(), now.into(), seq.into()],
                )?;
            }
        }
        for l in &plan.labels {
            tx.exec(
                "INSERT OR IGNORE INTO labels (message_rowid, label) VALUES (?1, ?2)",
                &[rowid.into(), l.clone().into()],
            )?;
        }
        let ten = owner.tenant_id;
        let idn = owner.identity_id;
        for a in &plan.attachments {
            tx.exec(
                "INSERT INTO attachments (id, message_rowid, filename, content_type, sniffed_type, size, sha256, disposition, content_id, r2_key, text_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'skipped')",
                &[
                    a.id.clone().into(),
                    rowid.into(),
                    a.filename.clone().into(),
                    a.content_type.clone().into(),
                    a.sniffed.clone().into(),
                    (a.bytes.len() as i64).into(),
                    a.sha256.clone().into(),
                    a.disposition.clone().into(),
                    a.content_id.clone().into(),
                    format!("t/{ten}/i/{idn}/out/{}/a/{}", plan.message_id, a.id).into(),
                ],
            )?;
        }
        if !inp.ctx.identity.is_system {
            let participants: Vec<(String, String)> = plan
                .recipients
                .iter()
                .map(|r| (r.name.clone(), r.address.clone()))
                .collect();
            let att_names: Vec<(String, String)> = plan
                .attachments
                .iter()
                .map(|a| (a.filename.clone(), String::new()))
                .collect();
            let doc = pylota_identity_core::search::fts_doc::fts_doc(
                &pylota_identity_core::search::fts_doc::MessageForIndex {
                    subject: &plan.subject,
                    from_name: &plan.from_name,
                    from_address: &plan.from.address,
                    recipients: &participants,
                    delivered_to: "",
                    extracted_text: &extracted,
                    text: &extracted,
                    attachments: &att_names,
                    refs: &[],
                },
            );
            tx.exec(
                "INSERT INTO fts (rowid, subject, participants, body_new, body_full, attachments, refs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                &[rowid.into(), doc.subject.clone().into(), doc.participants.clone().into(), doc.body_new.into(), doc.body_full.into(), doc.attachments.into(), doc.refs.clone().into()],
            )?;
            tx.exec(
                "INSERT INTO fts_tri (rowid, subject, participants, refs) VALUES (?1, ?2, ?3, ?4)",
                &[
                    rowid.into(),
                    doc.subject.into(),
                    doc.participants.into(),
                    doc.refs.into(),
                ],
            )?;
        }
        // The thread (Threading § 4.2).
        let addresses: Vec<(&str, &str)> = plan
            .recipients
            .iter()
            .map(|r| (r.address.as_str(), r.name.as_str()))
            .collect();
        let own: Vec<&str> = inp
            .ctx
            .addresses
            .iter()
            .map(|a| a.address.as_str())
            .collect();
        threads::recompute(tx, seq, &addresses, &own, None, false)?;
        tx.exec(
            "UPDATE threads SET last_outbound_at = ?2, reply_from_address = COALESCE(reply_from_address, ?3), needs_reply = CASE WHEN needs_reply IS NULL THEN NULL ELSE 0 END WHERE seq = ?1",
            &[seq.into(), now.into(), plan.from.address.clone().into()],
        )?;
        tx.exec(
            "DELETE FROM meta WHERE k = ?1",
            &[format!("compose:{}", plan.message_id).into()],
        )?;
        tx.exec(
            "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
            &[
                format!("reserve:{}", plan.message_id).into(),
                reserve.into(),
            ],
        )?;
        if all_suppressed {
            locks::release(tx, &plan.message_id)?;
            let addrs: Vec<&str> = plan.recipients.iter().map(|r| r.address.as_str()).collect();
            outbox::append(
                tx,
                owner,
                &outbox::new_event_id(self.p),
                "message.suppressed",
                now,
                &json!({ "message_id": plan.message_id, "recipients": addrs }),
                now,
            )?;
        } else {
            tx.exec(
                "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
                &[
                    format!("dispatch:{}", plan.message_id).into(),
                    now.to_string().into(),
                ],
            )?;
            super::set_wake(tx, "dispatch", now + super::alarms::DISPATCH_AFTER_MS)?;
        }
        super::set_wake(tx, "maintenance", now + super::alarms::MAINTENANCE_MS)?;
        for w in warnings {
            outbox::append(
                tx,
                owner,
                &outbox::new_event_id(self.p),
                "quota.warning",
                now,
                &json!({ "metric": "sends", "used": w.used, "limit": w.cap, "scope": w.scope }),
                now,
            )?;
        }
        let row: Row = tx
            .first(
                &format!("{} WHERE m.id = ?1", messages::SELECT),
                &[plan.message_id.clone().into()],
            )?
            .ok_or_else(|| err(ErrorCode::InternalError, "The message was not stored."))?;
        let mut m = super::read::message_json(
            tx,
            &row,
            owner.identity_id,
            super::read::Include::default(),
        )?;
        m["extracted_text"] = Value::Null;
        m["text"] = Value::Null;
        m["html"] = Value::Null;
        idempotency::record(
            tx,
            key_hash,
            &inp.fingerprint,
            &inp.operation,
            &plan.message_id,
            &m.to_string(),
            now,
        )?;
        m["extracted_text"] = json!(row.extracted_text);
        m["deduplicated"] = json!(false);
        Ok(Some(json!({ "replayed": false, "message": m })))
    }
}

/// The composition input of a plan.
fn compose_input(
    plan: &Plan,
    ctx: &SendContext,
    reply_to: Option<String>,
    now: i64,
    inp: &SubmitInput,
) -> ComposeInput {
    let to: Vec<String> = plan
        .recipients
        .iter()
        .filter(|r| r.field == "to")
        .map(|r| r.address.clone())
        .collect();
    let cc: Vec<String> = plan
        .recipients
        .iter()
        .filter(|r| r.field == "cc")
        .map(|r| r.address.clone())
        .collect();
    let mut text = plan.text.clone();
    let mut html = plan.html.clone();
    if !plan.link_note.is_empty() {
        let note = plan.link_note.join("\n");
        text = Some(format!(
            "{}\n\n{note}",
            text.unwrap_or_else(|| html
                .as_deref()
                .map(compose::text_of_html)
                .unwrap_or_default())
        ));
        if let Some(h) = html.as_mut() {
            for l in &plan.link_note {
                h.push_str(&format!(
                    "<p>{}</p>",
                    l.replace('&', "&amp;").replace('<', "&lt;")
                ));
            }
        }
    }
    let _ = inp;
    ComposeInput {
        message_id: format!(
            "{}@{}",
            plan.message_id[4..].to_ascii_lowercase(),
            plan.from.domain
        ),
        date_s: now / 1000,
        boundary_seed: plan.message_id[4..].to_ascii_lowercase(),
        from: (plan.from.address.clone(), plan.from_name.clone()),
        reply_to,
        to,
        cc,
        subject: plan.subject.clone(),
        text,
        html,
        signature_text: ctx.identity.signature_text.clone(),
        signature_html: ctx.identity.signature_html.clone(),
        disclosure: plan.disclosure.clone(),
        unsubscribe: plan.unsubscribe.clone(),
        list_unsubscribe: None,
        forward: plan.forward.clone(),
        in_reply_to: plan.in_reply_to.clone(),
        references: plan.references.clone(),
        auto_reply: plan.kind == "auto_reply",
        hop: plan.hop,
        custom_headers: plan.headers.clone(),
        attachments: plan
            .attachments
            .iter()
            .filter(|a| !a.linked)
            .map(|a| compose::OutAttachment {
                filename: a.filename.clone(),
                content_type: a.content_type.clone(),
                bytes: a.bytes.clone(),
                content_id: a.content_id.clone(),
            })
            .collect(),
    }
}

/// The dry-run answer (Outbound › Policy pipeline › Dry run).
fn dry_run(plan: &Plan) -> Result<Value, ApiError> {
    if let Some(b) = plan
        .recipients
        .iter()
        .find(|r| r.reason.as_deref() == Some("send_block"))
    {
        return Err(err(
            ErrorCode::RecipientBlocked,
            "A recipient is on the send-block list.",
        )
        .details(json!({ "address": b.address, "scope": b.scope })));
    }
    if plan.recipients.iter().all(|r| r.status == "suppressed") {
        return Err(err(
            ErrorCode::AllRecipientsSuppressed,
            "Every recipient would be suppressed.",
        ));
    }
    let recipients: Vec<Value> = plan
        .recipients
        .iter()
        .map(|r| {
            let mut v = json!({ "address": r.address, "field": r.field, "status": r.status });
            if let Some(reason) = &r.reason {
                v["reason"] = json!(reason);
            }
            if let Some(scope) = &r.scope {
                v["scope"] = json!(scope);
            }
            v
        })
        .collect();
    Ok(json!({ "dry_run": { "would_send": true, "recipients": recipients } }))
}

/// RFC 2822 date of a millisecond time (UTC).
fn rfc2822(ms: i64) -> String {
    chrono_like(ms)
}

fn chrono_like(ms: i64) -> String {
    // `Mon, 14 Sep 2026 08:12:00 +0000` from the RFC 3339 form.
    let r = rfc3339(ms);
    let date = &r[..10];
    let time = &r[11..19];
    let (y, m, d) = (&date[..4], &date[5..7], &date[8..10]);
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mi: usize = m.parse().unwrap_or(1);
    let days = ms.div_euclid(DAY_MS);
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    format!(
        "{wd}, {} {} {y} {time} +0000",
        d.trim_start_matches('0'),
        months[mi - 1]
    )
}

/// Contacts read from the mailbox.
struct ContactsOf<'a, S: Sql>(&'a S);

impl<S: Sql> ContactsOf<'_, S> {
    fn outbound(&self, address: &str) -> i64 {
        self.0
            .first::<N>(
                "SELECT outbound_count AS n FROM contacts WHERE address = ?1",
                &[address.into()],
            )
            .ok()
            .flatten()
            .map_or(0, |n| n.n)
    }
}

impl<S: Sql> reply::Contacts for ContactsOf<'_, S> {
    fn written_to(&self, address: &str) -> bool {
        self.outbound(address) > 0
    }
}
