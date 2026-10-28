//! Agentic search ([Search § 11], ADR 0007): a planner model chooses read-only tool calls, code runs
//! them in the caller's scope, and code verifies every citation. The run is the state machine of
//! § 11.2: Seed (a hybrid search on the question), Plan and Act until the planner is ready or the judge
//! stops it, then one Answer call and the verifier. The model is never trusted with scope, IDs or the
//! final wording: tools take no scope arguments, open only IDs seen in earlier results, and a sentence
//! survives only if its citations, quotes and references check out against what the planner saw.
//!
//! [Search § 11]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

pub mod answer;
pub mod judge;
pub mod planner;
pub mod prompts;
pub mod sse;
pub mod tools;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::citations::{EvidenceItem, EvidenceSet, SourceAuth};
use pylota_identity_core::injection::{self, Nonce, fence};
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{BlobStore as _, Clock as _, ObjectClient as _, Platform, Rng as _};
use serde_json::{Value, json};

use self::answer::Status;
use self::judge::Next;
use self::planner::ModelError;
use self::tools::{Args, Refusal};
use super::{Mode, Params, Scope};
use crate::errors::ApiError;
use crate::mailbox::MailboxRequest;
use crate::mailbox::read::{Include, ReadRequest};
use crate::mailbox::search::{ContactsIn, SearchOp};
use crate::rpc::{RpcResult, envelope_for};

/// Tool calls per step, and in total.
pub const CALLS_PER_STEP: usize = 4;
pub const CALLS_TOTAL: u32 = 16;
/// Evidence items kept (first seen).
pub const EVIDENCE_MAX: usize = 40;
/// Characters of tool results in the conversation; older results shrink to their first line.
pub const CONVERSATION_CHARS: usize = 48_000;
/// The answer call's timeout.
pub const ANSWER_TIMEOUT_MS: u32 = 2_500;
/// Each identity's deadline for the `contacts` fan-out at tenant scope.
const CONTACTS_DEADLINE_MS: u32 = 900;

/// A validated agentic request.
#[derive(Clone, Debug)]
pub struct Request {
    pub q: String,
    pub direction: Option<String>,
    pub labels: Vec<String>,
    pub after_ms: Option<i64>,
    pub before_ms: Option<i64>,
    /// Already narrowed to keys with `quarantine:review`.
    pub include_quarantined: bool,
    pub review: bool,
    pub max_steps: u8,
    pub max_seconds: u8,
}

impl Request {
    /// The request filters in query syntax, for the planner's first message.
    fn filters_text(&self) -> String {
        let mut f = Vec::new();
        if let Some(d) = &self.direction {
            f.push(format!("in:{d}"));
        }
        for l in &self.labels {
            f.push(format!("label:{l}"));
        }
        if let Some(a) = self.after_ms {
            f.push(format!("after:{}", pylota_identity_core::days::rfc3339(a)));
        }
        if let Some(b) = self.before_ms {
            f.push(format!("before:{}", pylota_identity_core::days::rfc3339(b)));
        }
        f.join(" ")
    }
}

/// One evidence message: its identity, its hit for the response, and what the verifier needs.
struct Ev {
    idx: usize,
    hit: Value,
    item: EvidenceItem,
}

/// The run's result.
pub struct Run {
    pub body: Value,
    pub status: Status,
}

/// A fresh fence nonce: 16 Crockford base32 characters from the platform RNG.
pub fn nonce<P: Platform>(p: &P) -> Nonce {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let bytes: [u8; 16] = p.rng().bytes();
    let s: String = bytes
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(b % 32)]))
        .collect();
    Nonce::new(&s).expect("16 Crockford characters")
}

pub struct Agent<P: Platform> {
    p: Rc<P>,
    scope: Scope,
    req: Request,
    key_id: String,
    request_id: String,
    nonce: Nonce,
    start: i64,
    deadline: i64,
    ev: Vec<Ev>,
    /// Threads the planner may open, by ID: the identity index.
    threads: HashMap<String, usize>,
    /// Attachments the planner saw, by ID: their message.
    attachments: HashMap<String, String>,
    trace: Vec<Value>,
    model_calls: u32,
    tool_calls: u32,
    done_calls: HashMap<String, u8>,
    searches: Vec<(String, usize)>,
    failed: BTreeSet<String>,
    messages: Vec<Value>,
    sse: Option<Rc<sse::Sse>>,
    /// Seed hits, for a degraded answer.
    seed_hits: Option<Vec<Value>>,
}

fn ms(p: &impl Platform) -> i64 {
    p.clock().now_ms()
}

impl<P: Platform> Agent<P> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        p: Rc<P>,
        scope: Scope,
        req: Request,
        key_id: &str,
        request_id: &str,
        nonce: Nonce,
        sse: Option<Rc<sse::Sse>>,
    ) -> Self {
        let start = ms(p.as_ref());
        let deadline = start + i64::from(req.max_seconds) * 1000;
        Self {
            p,
            scope,
            req,
            key_id: key_id.to_owned(),
            request_id: request_id.to_owned(),
            nonce,
            start,
            deadline,
            ev: Vec::new(),
            threads: HashMap::new(),
            attachments: HashMap::new(),
            trace: Vec::new(),
            model_calls: 0,
            tool_calls: 0,
            done_calls: HashMap::new(),
            searches: Vec::new(),
            failed: BTreeSet::new(),
            messages: Vec::new(),
            sse,
            seed_hits: None,
        }
    }

    fn remaining(&self) -> i64 {
        self.deadline - ms(self.p.as_ref())
    }

    fn stopped(&self) -> bool {
        self.sse.as_ref().is_some_and(|s| !s.alive())
    }

    fn idx_of(&self, identity_id: &str) -> usize {
        self.scope
            .targets
            .iter()
            .position(|t| t.id == identity_id)
            .unwrap_or(0)
    }

    async fn emit(&self, name: &str, data: &Value) {
        if let Some(s) = &self.sse {
            s.send(self.p.as_ref(), name, data).await;
        }
    }

    async fn step(&mut self, entry: Value) {
        self.emit("step", &entry).await;
        self.trace.push(entry);
    }

    // ---- Evidence ---------------------------------------------------------------------------------

    /// Adds one message (a hit, or a read message as a hit) with the untrusted texts the planner saw;
    /// scans every new text for steering (§ 11.9). New messages are streamed as `evidence`.
    async fn observe(
        &mut self,
        step: u8,
        idx: usize,
        hit: Value,
        texts: Vec<String>,
        refs: Vec<String>,
    ) {
        let Some(id) = hit["message_id"].as_str().map(str::to_owned) else {
            return;
        };
        let thread = hit["thread_id"].as_str().unwrap_or("").to_owned();
        if !thread.is_empty() {
            self.threads.entry(thread.clone()).or_insert(idx);
        }
        let mut signals: BTreeSet<injection::Signal> = BTreeSet::new();
        for t in &texts {
            signals.extend(injection::scan(t));
        }
        let pos = self.ev.iter().position(|e| e.item.message_id == id);
        let new = pos.is_none();
        let suspected_before = pos.is_some_and(|i| self.ev[i].item.steering_suspected);
        match pos {
            Some(i) => {
                let e = &mut self.ev[i];
                for t in texts {
                    if !e.item.texts_seen.contains(&t) {
                        e.item.texts_seen.push(t);
                    }
                }
                for r in refs {
                    if !e.item.refs.contains(&r) {
                        e.item.refs.push(r);
                    }
                }
                e.item.steering_suspected |= !signals.is_empty();
            }
            None if self.ev.len() < EVIDENCE_MAX => {
                let auth = if hit["direction"] == "outbound" {
                    SourceAuth::Authenticated
                } else {
                    match hit["trust"]["verdict"].as_str() {
                        Some("pass") => SourceAuth::Authenticated,
                        Some("arc_rescued") => SourceAuth::Rescued,
                        _ => SourceAuth::Unauthenticated,
                    }
                };
                self.ev.push(Ev {
                    idx,
                    item: EvidenceItem {
                        message_id: id.clone(),
                        thread_id: thread,
                        identity_id: hit["identity_id"].as_str().unwrap_or("").to_owned(),
                        texts_seen: texts,
                        refs,
                        auth,
                        steering_suspected: !signals.is_empty(),
                    },
                    hit,
                });
            }
            None => return,
        }
        if !signals.is_empty() && !suspected_before {
            let entry = json!({ "step": step.max(1), "action": "steering_suspected", "message_id": id, "signals": signals });
            self.step(entry).await;
        }
        if new {
            let data = self.evidence_json(self.ev.len() - 1, &BTreeMap::new());
            self.emit("evidence", &data).await;
        }
    }

    fn evidence_json(&self, i: usize, quotes: &BTreeMap<String, Vec<String>>) -> Value {
        let e = &self.ev[i];
        let mut v = e.hit.clone();
        v["quotes"] = json!(quotes.get(&e.item.message_id).cloned().unwrap_or_default());
        v["steering_suspected"] = Value::Bool(e.item.steering_suspected);
        v
    }

    fn evidence_set(&self) -> EvidenceSet {
        EvidenceSet {
            items: self.ev.iter().map(|e| e.item.clone()).collect(),
        }
    }

    // ---- Mailbox calls ----------------------------------------------------------------------------

    async fn read(&self, idx: usize, req: ReadRequest) -> Result<Value, ApiError> {
        let idn = &self.scope.targets[idx];
        let env = envelope_for(
            self.p.as_ref(),
            Some(&idn.tenant_id),
            Some(&idn.id),
            &self.request_id,
            None,
            MailboxRequest::Read(req),
        );
        match self
            .p
            .objects()
            .call::<_, Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
            .await?
        {
            RpcResult::Ok(v) => Ok(v),
            RpcResult::Err(e) => Err(ApiError::new(e.error.code, e.error.message)),
        }
    }

    fn params(&self, q: &str, mode: Mode, limit: u32, thread: bool) -> Params {
        Params {
            q: q.to_owned(),
            mode,
            thread,
            limit,
            snippet_chars: 240,
            facets: true,
            include_quarantined: self.req.include_quarantined,
            review: self.req.review,
            require_mode: false,
            cursor: None,
            direction: self.req.direction.clone(),
            labels: self.req.labels.clone(),
            after_ms: self.req.after_ms,
            before_ms: self.req.before_ms,
            key_id: self.key_id.clone(),
            request_id: self.request_id.clone(),
            safe_why: true,
        }
    }

    /// One search in the caller's scope with the request filters; answers the response body.
    async fn search(
        &mut self,
        q: &str,
        mode: Mode,
        limit: u32,
        thread: bool,
    ) -> Result<Value, ApiError> {
        let plan = super::plan(
            self.p.as_ref(),
            self.scope.clone(),
            self.params(q, mode, limit, thread),
        )
        .await?;
        let body = super::run(self.p.as_ref(), &plan).await?;
        for f in body["failed_identities"].as_array().into_iter().flatten() {
            if let Some(f) = f.as_str() {
                self.failed.insert(f.to_owned());
            }
        }
        Ok(body)
    }

    // ---- Tools ------------------------------------------------------------------------------------

    /// Renders a search body as a tool result and adds its hits to the evidence.
    async fn search_result(&mut self, step: u8, call: &str, body: &Value) -> (String, usize) {
        let hits = body["hits"].as_array().cloned().unwrap_or_default();
        let mut out = format!(
            "RESULT search step={step} call={call} hits={}\n",
            hits.len()
        );
        if let Some(f) = tools::facets_line(&body["facets"]) {
            out.push_str(&f);
            out.push('\n');
        }
        for (n, h) in hits.iter().enumerate() {
            let mut h = h.clone();
            if h.get("top_message_id").is_some() {
                // A thread row: its best message stands for it.
                h["message_id"] = h["top_message_id"].clone();
            }
            out.push_str(&tools::hit(&self.nonce, n + 1, &h));
            let idx = self.idx_of(h["identity_id"].as_str().unwrap_or(""));
            if h["identity_id"].is_null() {
                h["identity_id"] = json!(self.scope.targets[idx].id);
            }
            let texts = tools::hit_texts(&h);
            for a in h["attachment_hits"].as_array().into_iter().flatten() {
                if let (Some(att), Some(msg)) =
                    (a["attachment_id"].as_str(), h["message_id"].as_str())
                {
                    self.attachments.insert(att.to_owned(), msg.to_owned());
                }
            }
            if h.get("top_message_id").is_none() {
                self.observe(step, idx, h, texts, Vec::new()).await;
            } else if let Some(t) = h["thread_id"].as_str() {
                self.threads.entry(t.to_owned()).or_insert(idx);
            }
        }
        (out, hits.len())
    }

    /// Runs one validated call; answers the tool result text and the trace entry.
    async fn act(&mut self, step: u8, call: &str, args: Args) -> (String, Value) {
        let started = ms(self.p.as_ref());
        let mut entry = json!({ "step": step, "action": args.name() });
        let text = match &args {
            Args::Search {
                q,
                mode,
                limit,
                group_by,
            } => {
                let m = match mode.as_str() {
                    "keyword" => Mode::Keyword,
                    "semantic" => Mode::Semantic,
                    _ => Mode::Hybrid,
                };
                entry["q"] = json!(q);
                entry["mode"] = json!(mode);
                match self.search(q, m, *limit, group_by == "thread").await {
                    Ok(body) => {
                        let (text, n) = self.search_result(step, call, &body).await;
                        entry["hits"] = json!(n);
                        self.searches.push((q.clone(), n));
                        text
                    }
                    Err(e) if e.code == ErrorCode::InvalidQuery => {
                        self.searches.push((q.clone(), 0));
                        entry["hits"] = json!(0);
                        let d = e.details.clone().unwrap_or(Value::Null);
                        format!(
                            "INVALID_QUERY at {}: expected {}",
                            d["position"].as_i64().unwrap_or(0),
                            d["expected"].as_str().unwrap_or("a valid query")
                        )
                    }
                    Err(_) => {
                        self.searches.push((q.clone(), 0));
                        "ERROR: search unavailable".to_owned()
                    }
                }
            }
            Args::ReadThread {
                thread_id,
                max_messages,
            } => {
                entry["thread_id"] = json!(thread_id);
                match self.threads.get(thread_id).copied() {
                    None => self.probe(step, "read_thread").await,
                    Some(idx) => match self
                        .read(
                            idx,
                            ReadRequest::GetThread {
                                thread_id: thread_id.clone(),
                                messages_limit: *max_messages,
                                cursor: None,
                                include: Include::default(),
                                review: self.req.include_quarantined,
                            },
                        )
                        .await
                    {
                        Ok(out) => {
                            let msgs: Vec<Value> = out["items"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(|i| i["value"].clone())
                                .take(*max_messages as usize)
                                .collect();
                            let mut text = format!(
                                "RESULT read_thread step={step} call={call} thread_id={thread_id} messages={}\n",
                                msgs.len()
                            );
                            for m in &msgs {
                                text.push_str(&tools::message(&self.nonce, m, false));
                                self.read_message_evidence(step, idx, m, false).await;
                            }
                            text
                        }
                        Err(_) => "NOT_FOUND: the thread is not available".to_owned(),
                    },
                }
            }
            Args::ReadMessage {
                message_id,
                include_quoted,
            } => {
                entry["message_id"] = json!(message_id);
                match self
                    .ev
                    .iter()
                    .find(|e| e.item.message_id == *message_id)
                    .map(|e| e.idx)
                {
                    None => self.probe(step, "read_message").await,
                    Some(idx) => match self
                        .read(
                            idx,
                            ReadRequest::GetMessage {
                                message_id: message_id.clone(),
                                include: Include {
                                    quoted: *include_quoted,
                                    ..Include::default()
                                },
                                review: self.req.include_quarantined,
                            },
                        )
                        .await
                    {
                        Ok(m) => {
                            self.read_message_evidence(step, idx, &m, *include_quoted)
                                .await;
                            format!(
                                "RESULT read_message step={step} call={call}\n{}",
                                tools::message(&self.nonce, &m, *include_quoted)
                            )
                        }
                        Err(_) => "NOT_FOUND: the message is not available".to_owned(),
                    },
                }
            }
            Args::ReadAttachmentText {
                message_id,
                attachment_id,
                pages,
            } => {
                entry["message_id"] = json!(message_id);
                let idx = self
                    .ev
                    .iter()
                    .find(|e| e.item.message_id == *message_id)
                    .map(|e| e.idx);
                match (idx, self.attachments.get(attachment_id)) {
                    (Some(idx), Some(owner)) if owner == message_id => {
                        self.attachment_text(step, call, idx, message_id, attachment_id, *pages)
                            .await
                    }
                    _ => self.probe(step, "read_attachment_text").await,
                }
            }
            Args::FindRelated { message_id, limit } => {
                entry["message_id"] = json!(message_id);
                match self
                    .ev
                    .iter()
                    .find(|e| e.item.message_id == *message_id)
                    .map(|e| e.idx)
                {
                    None => self.probe(step, "find_related").await,
                    Some(idx) => {
                        let scope = Scope {
                            tenant: self.scope.tenant.clone(),
                            targets: vec![self.scope.targets[idx].clone()],
                            tenant_scope: false,
                        };
                        match super::related::related(
                            self.p.as_ref(),
                            scope,
                            message_id,
                            *limit,
                            self.req.review,
                            &self.key_id,
                            &self.request_id,
                            true,
                        )
                        .await
                        {
                            Ok(body) => {
                                let mut b = json!({ "hits": body["data"] });
                                b["facets"] = Value::Null;
                                let (text, n) = self.search_result(step, call, &b).await;
                                entry["hits"] = json!(n);
                                text.replacen("RESULT search", "RESULT find_related", 1)
                            }
                            Err(_) => "NOT_FOUND: the message is not available".to_owned(),
                        }
                    }
                }
            }
            Args::Contacts { q, limit } => {
                let (text, n) = self.contacts(step, call, q, *limit).await;
                entry["hits"] = json!(n);
                text
            }
        };
        // A refused ID has its `scope_probe` entry already; the ID the model made up is not echoed.
        if text.starts_with("UNKNOWN_ID") {
            return (text, Value::Null);
        }
        entry["ms"] = json!(ms(self.p.as_ref()) - started);
        (text, entry)
    }

    /// An ID the planner never saw: refused, and recorded as a scope probe (§ 11.9).
    async fn probe(&mut self, step: u8, tool: &str) -> String {
        self.step(json!({ "step": step, "action": "scope_probe", "tool": tool }))
            .await;
        "UNKNOWN_ID: open only IDs from earlier results".to_owned()
    }

    async fn read_message_evidence(&mut self, step: u8, idx: usize, m: &Value, quoted: bool) {
        let identity = self.scope.targets[idx].id.clone();
        for a in m["attachments"].as_array().into_iter().flatten() {
            if let (Some(att), Some(msg)) = (a["id"].as_str(), m["id"].as_str()) {
                self.attachments.insert(att.to_owned(), msg.to_owned());
            }
        }
        let refs: Vec<String> = m["refs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["value"].as_str().map(str::to_owned))
            .collect();
        let mut hit = tools::hit_of_message(m, &identity);
        if let Some(existing) = self
            .ev
            .iter()
            .find(|e| e.item.message_id == hit["message_id"])
        {
            hit = existing.hit.clone();
        }
        let suspect =
            m["screening"]["verdict"] == "suspect" || m["screening"]["status"] == "suspect";
        self.observe(step, idx, hit, tools::message_texts(m, quoted), refs)
            .await;
        if suspect && let Some(e) = self.ev.iter_mut().find(|e| e.item.message_id == m["id"]) {
            e.item.steering_suspected = true;
        }
    }

    async fn attachment_text(
        &mut self,
        step: u8,
        call: &str,
        idx: usize,
        message_id: &str,
        attachment_id: &str,
        (first, last): (u32, u32),
    ) -> String {
        let row = match self
            .read(
                idx,
                ReadRequest::Attachment {
                    message_id: message_id.to_owned(),
                    attachment_id: attachment_id.to_owned(),
                    review: self.req.include_quarantined,
                },
            )
            .await
        {
            Ok(r) => r,
            Err(_) => return "NOT_FOUND: the attachment is not available".to_owned(),
        };
        let status = row["text_status"].as_str().unwrap_or("skipped").to_owned();
        let mut out = format!(
            "RESULT read_attachment_text step={step} call={call} attachment_id={attachment_id} text_status={status}\n"
        );
        let md = match row["text_r2_key"].as_str() {
            Some(key) if status == "ready" => match self.p.blobs().get(key).await {
                Ok(Some(b)) => String::from_utf8_lossy(&b.body.to_vec()).into_owned(),
                _ => return out,
            },
            _ => return out,
        };
        let mut used = 0usize;
        let mut seen = Vec::new();
        for (i, text) in pylota_identity_core::attach_text::split_rendered(&md)
            .into_iter()
            .enumerate()
        {
            let n = i as u32 + 1;
            if n < first || n > last || used >= tools::ATTACHMENT_CHARS {
                continue;
            }
            let part: String = text.chars().take(tools::ATTACHMENT_CHARS - used).collect();
            used += part.chars().count();
            out.push_str(&fence(
                &self.nonce,
                "attachment_text",
                &[("page", &n.to_string())],
                &part,
            ));
            out.push('\n');
            seen.push(part);
        }
        if let Some(e) = self.ev.iter().find(|e| e.item.message_id == message_id) {
            let hit = e.hit.clone();
            self.observe(step, idx, hit, seen, Vec::new()).await;
        }
        out
    }

    /// `contacts` (§ 11.5): every identity in scope, merged by address.
    async fn contacts(&mut self, step: u8, call: &str, q: &str, limit: u32) -> (String, usize) {
        let folded = super::contacts::fold_q(q).unwrap_or_default();
        let p = self.p.clone();
        let futs: Vec<_> = self
            .scope
            .targets
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.mailbox_do_id.is_empty())
            .map(|(idx, t)| {
                let env = envelope_for(
                    p.as_ref(),
                    Some(&t.tenant_id),
                    Some(&t.id),
                    &self.request_id,
                    None,
                    MailboxRequest::Search(SearchOp::Contacts(ContactsIn {
                        q: folded.clone(),
                        limit,
                        review: self.req.include_quarantined,
                        after: None,
                    })),
                );
                let object = t.mailbox_do_id.clone();
                let p = p.clone();
                async move {
                    let call = p
                        .objects()
                        .call::<_, Value>(DoClass::Mailbox, &object, &env);
                    let r = super::join::before(call, p.clock().sleep(CONTACTS_DEADLINE_MS)).await;
                    (idx, r)
                }
            })
            .collect();
        let mut merged: BTreeMap<String, (Value, usize)> = BTreeMap::new();
        let mut failed = Vec::new();
        for (idx, r) in super::join::bounded(futs, super::IN_FLIGHT).await {
            let rows = match r {
                Some(Ok(RpcResult::Ok(Value::Array(rows)))) => rows,
                _ => {
                    failed.push(self.scope.targets[idx].id.clone());
                    continue;
                }
            };
            for row in rows {
                let Some(addr) = row["address"].as_str().map(str::to_owned) else {
                    continue;
                };
                match merged.get_mut(&addr) {
                    None => {
                        merged.insert(addr, (row, idx));
                    }
                    Some((m, midx)) => {
                        let sum =
                            |k: &str| m[k].as_i64().unwrap_or(0) + row[k].as_i64().unwrap_or(0);
                        let (inb, outb) = (sum("inbound_count"), sum("outbound_count"));
                        let first = m["first_seen_at"]
                            .as_i64()
                            .unwrap_or(i64::MAX)
                            .min(row["first_seen_at"].as_i64().unwrap_or(i64::MAX));
                        let rank = m["match_rank"]
                            .as_i64()
                            .unwrap_or(0)
                            .max(row["match_rank"].as_i64().unwrap_or(0));
                        if row["last_seen_at"].as_i64() > m["last_seen_at"].as_i64() {
                            *m = row.clone();
                            *midx = idx;
                        }
                        m["inbound_count"] = inb.into();
                        m["outbound_count"] = outb.into();
                        m["first_seen_at"] = first.into();
                        m["match_rank"] = rank.into();
                        m["weight"] = (inb + 2 * outb).into();
                    }
                }
            }
        }
        let mut rows: Vec<(Value, usize)> = merged.into_values().collect();
        rows.sort_by(|(a, _), (b, _)| {
            let k = |v: &Value| {
                (
                    v["match_rank"].as_i64().unwrap_or(0),
                    v["weight"].as_i64().unwrap_or(0),
                    v["last_seen_at"].as_i64().unwrap_or(0),
                )
            };
            k(b).cmp(&k(a))
                .then(a["address"].as_str().cmp(&b["address"].as_str()))
        });
        rows.truncate(limit as usize);
        let mut out = format!(
            "RESULT contacts step={step} call={call} contacts={}\n",
            rows.len()
        );
        for f in &failed {
            self.failed.insert(f.clone());
        }
        let mut signals = BTreeSet::new();
        for (n, (row, idx)) in rows.iter().enumerate() {
            let rendered = super::contacts::render(row);
            out.push_str(&tools::contact(&self.nonce, n + 1, &rendered));
            if let Some(t) = rendered["last_thread_id"].as_str() {
                self.threads.entry(t.to_owned()).or_insert(*idx);
            }
            for t in [row["name"].as_str(), row["address"].as_str()]
                .into_iter()
                .flatten()
            {
                signals.extend(injection::scan(t));
            }
        }
        if !signals.is_empty() {
            self.step(json!({ "step": step, "action": "steering_suspected", "signals": signals }))
                .await;
        }
        (out, rows.len())
    }

    // ---- The loop ---------------------------------------------------------------------------------

    fn system(&self, step: u8) -> Value {
        let tz = super::tz_of(&self.scope.tenant);
        let today =
            pylota_identity_core::days::local_day(ms(self.p.as_ref()), &self.scope.tenant.timezone);
        json!({ "role": "system", "content": prompts::system(&prompts::Fill {
            tenant_scope: self.scope.tenant_scope,
            today: &today,
            timezone: tz.name(),
            nonce: self.nonce.as_str(),
            max_steps: self.req.max_steps,
            max_seconds: self.req.max_seconds,
            step,
        }) })
    }

    /// The conversation for a call: this step's system prompt, then the rest.
    fn conversation(&self, step: u8) -> Vec<Value> {
        let mut m = vec![self.system(step)];
        m.extend(self.messages.iter().cloned());
        m
    }

    /// Keeps tool results within the conversation budget: the oldest shrink to their first line.
    fn trim(&mut self) {
        let size = |m: &[Value]| -> usize {
            m.iter()
                .filter(|x| x["role"] == "tool")
                .map(|x| x["content"].as_str().map_or(0, |c| c.chars().count()))
                .sum()
        };
        while size(&self.messages) > CONVERSATION_CHARS {
            let Some(m) = self.messages.iter_mut().find(|x| {
                x["role"] == "tool" && x["content"].as_str().is_some_and(|c| c.contains('\n'))
            }) else {
                break;
            };
            let head = m["content"]
                .as_str()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_owned();
            m["content"] = json!(format!("{head} [summarised]"));
        }
    }

    /// Seed (§ 11.2 step 2): a hybrid search on the question, given to the planner as its first result.
    async fn seed(&mut self) {
        let q = self.req.q.clone();
        let started = ms(self.p.as_ref());
        let result = match self.search(&q, Mode::Hybrid, 8, false).await {
            Ok(body) => {
                self.seed_hits = Some(body["hits"].as_array().cloned().unwrap_or_default());
                let (text, n) = self.search_result(1, "seed", &body).await;
                self.searches.push((q.clone(), n));
                let entry = json!({ "step": 1, "action": "search", "q": q, "mode": "hybrid", "hits": n,
                    "ms": ms(self.p.as_ref()) - started });
                self.step(entry).await;
                text
            }
            Err(_) => {
                "RESULT search step=1 call=seed hits=0\nERROR: the first search could not run"
                    .to_owned()
            }
        };
        self.messages.push(json!({ "role": "user", "content": prompts::question(&self.req.q, &self.req.filters_text()) }));
        self.messages.push(json!({ "role": "assistant", "content": null, "tool_calls": [ { "id": "seed", "type": "function",
            "function": { "name": "search", "arguments": json!({ "q": q, "mode": "hybrid" }).to_string() } } ] }));
        self.messages.push(json!({ "role": "tool", "tool_call_id": "seed", "content": tools::cut(&result, tools::RESULT_CHARS) }));
    }

    /// One planning call, with the retry rule of § 11.2 step 3.
    async fn plan_call(&mut self, step: u8) -> Result<planner::Reply, ()> {
        for attempt in 0..2 {
            let remaining = self.remaining();
            let timeout = (remaining - 2_500).min(3_000);
            if timeout <= 0 {
                return Err(());
            }
            self.model_calls += 1;
            let input = planner::plan_input(&self.conversation(step));
            match planner::call(
                self.p.as_ref(),
                &self.scope.tenant.id,
                &input,
                timeout as u32,
            )
            .await
            {
                Ok(r) => return Ok(r),
                Err(ModelError::RateLimited) => return Err(()),
                Err(ModelError::Failed) if step == 1 || attempt == 1 => return Err(()),
                Err(ModelError::Failed) => {}
            }
        }
        Err(())
    }

    /// The answer call, retried once.
    async fn answer_call(&mut self, step: u8) -> Option<String> {
        for _ in 0..2 {
            self.model_calls += 1;
            let input = planner::answer_input(&self.conversation(step));
            if let Ok(r) = planner::call(
                self.p.as_ref(),
                &self.scope.tenant.id,
                &input,
                ANSWER_TIMEOUT_MS,
            )
            .await
                && let Some(c) = r.content.filter(|c| !c.trim().is_empty())
            {
                return Some(c);
            }
        }
        None
    }

    /// Runs the state machine to its end.
    pub async fn run(mut self) -> Run {
        self.seed().await;
        let mut step: u8 = 1;
        // `Some(forced)` once the loop hands over to the answer call.
        let mut answer: Option<bool> = None;
        let mut status: Option<Status> = None;
        while answer.is_none() && status.is_none() {
            if self.stopped() {
                status = Some(Status::Degraded);
                break;
            }
            let reply = match self.plan_call(step).await {
                Ok(r) => r,
                Err(()) if step == 1 || self.ev.is_empty() => {
                    status = Some(Status::Degraded);
                    break;
                }
                Err(()) => {
                    answer = Some(true);
                    break;
                }
            };
            if reply.tool_calls.is_empty() {
                answer = Some(false);
                break;
            }
            self.messages.push(reply.message.clone());
            for (i, c) in reply.tool_calls.iter().enumerate() {
                let text = if i >= CALLS_PER_STEP || self.tool_calls >= CALLS_TOTAL {
                    "BUDGET: no more tool calls in this step".to_owned()
                } else {
                    self.tool_calls += 1;
                    match tools::validate(&c.name, &c.arguments) {
                        Err(Refusal { text, scope_probe }) => {
                            let action = if scope_probe {
                                "scope_probe"
                            } else {
                                "invalid_call"
                            };
                            let entry = json!({ "step": step, "action": action, "tool": c.name });
                            self.step(entry).await;
                            text
                        }
                        Ok(args) => match self.done_calls.get(&args.canonical()) {
                            Some(n) => {
                                format!("DUPLICATE_CALL: already run at step {n}; refine the query")
                            }
                            None => {
                                self.done_calls.insert(args.canonical(), step);
                                let (text, entry) = self.act(step, &c.id, args).await;
                                if !entry.is_null() {
                                    self.step(entry).await;
                                }
                                text
                            }
                        },
                    }
                };
                self.messages.push(json!({ "role": "tool", "tool_call_id": c.id, "content": tools::cut(&text, tools::RESULT_CHARS) }));
            }
            self.trim();
            match judge::judge(
                step,
                self.req.max_steps,
                self.remaining(),
                self.ev.is_empty(),
                &self.searches,
            ) {
                Next::Plan => step += 1,
                Next::Answer => answer = Some(true),
                Next::Insufficient => status = Some(Status::InsufficientEvidence),
                Next::Exhausted => status = Some(Status::BudgetExhausted),
            }
        }
        let mut answer_json = Value::Null;
        let mut quotes = BTreeMap::new();
        if let Some(forced) = answer {
            let step = step.saturating_add(1);
            match self.answer_call(step).await {
                None => status = Some(Status::Degraded),
                Some(content) => match answer::outcome(&content, &self.evidence_set(), forced) {
                    None => status = Some(Status::Degraded),
                    Some(o) => {
                        let mut entry = json!({ "step": step, "action": "answer" });
                        if let (Some(e), Some(o_tr)) = (entry.as_object_mut(), o.trace.as_object())
                        {
                            e.extend(o_tr.clone());
                        }
                        self.step(entry).await;
                        if !o.answer.is_null() {
                            self.emit("answer", &o.answer).await;
                        }
                        answer_json = o.answer;
                        quotes = o.quotes;
                        status = Some(o.status);
                    }
                },
            }
        }
        let status = status.unwrap_or(Status::InsufficientEvidence);
        if status == Status::Degraded {
            answer_json = Value::Null;
            self.degraded_evidence().await;
        }
        let evidence: Vec<Value> = (0..self.ev.len())
            .map(|i| self.evidence_json(i, &quotes))
            .collect();
        let mut body = json!({
            "status": status.as_str(),
            "answer": answer_json,
            "evidence": evidence,
            "trace": self.trace,
            "degraded": status == Status::Degraded,
            "usage": { "steps": self.model_calls, "ms": ms(self.p.as_ref()) - self.start, "model": self.p.config().agent_model },
        });
        if self.scope.tenant_scope {
            body["partial"] = Value::Bool(!self.failed.is_empty());
            body["failed_identities"] = json!(self.failed);
        }
        crate::log::info(
            "agentic_search",
            &[
                ("tenant_id", self.scope.tenant.id.clone().into()),
                ("status", status.as_str().into()),
                ("steps", i64::from(self.model_calls).into()),
                ("tool_calls", i64::from(self.tool_calls).into()),
                ("ms", (ms(self.p.as_ref()) - self.start).into()),
                (
                    "prompt_version",
                    i64::from(prompts::PLANNER_PROMPT_VERSION).into(),
                ),
            ],
        );
        Run { body, status }
    }

    /// A degraded run returns hybrid hits for the question: the seed's, or a fresh search.
    async fn degraded_evidence(&mut self) {
        if self.seed_hits.is_some() || !self.ev.is_empty() {
            return;
        }
        let q = self.req.q.clone();
        if let Ok(body) = self.search(&q, Mode::Hybrid, 8, false).await {
            let _ = self.search_result(1, "degraded", &body).await;
        }
    }
}
