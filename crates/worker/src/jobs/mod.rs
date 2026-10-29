//! `JobRunner` ([Privacy and erasure § 4]): one Durable Object per job, a state machine over named
//! steps driven by its alarm. Each alarm runs one slice of the first unfinished step, stores its cursor
//! and counts, and re-arms at once while work remains; a failed slice backs off (`core::jobs`). A job
//! that fails or is canceled runs its kind's `finalise` before its status is mirrored to D1.
//!
//! The steps of each kind live in their own module: `erasure`, `retention`, `export`, `maintenance`
//! (`reparse`, `reindex`, `reembed`), `backup` and `domain_remove`.
//!
//! [Privacy and erasure § 4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

pub mod backup;
pub mod domain_remove;
pub mod erasure;
pub mod export;
pub mod maintenance;
pub mod retention;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::jobs as rules;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::{DoClass, ObjectState, RpcEnvelope, RpcResult, Sql};
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{
    Clock as _, ControlDb as _, ObjectClient as _, PResult, Platform, QueueProducer as _,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::errors::ApiError;
use crate::rpc::{envelope_for, error_body};
use crate::webhooks::envelope::{OwnerKind, WebhookJob, envelope};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum JobRequest {
    Start {
        job_id: String,
        kind: String,
        tenant_id: Option<String>,
        params: Value,
        resume_job_id: Option<String>,
    },
    /// A resumption's copy of this job's progress (Privacy § 6.1).
    Snapshot,
    /// Tenant erasure supersedes the job: it stops at its next slice, runs `finalise`, ends `canceled`.
    Cancel,
    /// The itest build: run one alarm now and answer the runner's state.
    #[cfg(feature = "itest-hooks")]
    TestAlarm,
}

/// Step errors that end the job at once (an export superseded twice by an erasure, § 6.11).
const FATAL: &[&str] = &["superseded_by_erasure"];

/// What one slice of a step did.
pub enum Slice {
    /// Work remains: store the cursor and run again at once.
    More { cursor: String, counts: Value },
    /// The step is finished.
    Done { counts: Value },
    /// Nothing to do yet (an export waiting for an erasure): run again after this long, without
    /// counting an attempt.
    Wait { after_ms: i64 },
}

impl Slice {
    pub fn done() -> Self {
        Slice::Done { counts: json!({}) }
    }
}

/// A failed slice: a machine code, never content.
#[derive(Debug)]
pub struct StepErr(pub String);

impl StepErr {
    pub fn new(code: &str) -> Self {
        StepErr(code.to_owned())
    }
}

impl From<ApiError> for StepErr {
    fn from(e: ApiError) -> Self {
        StepErr(e.code.as_str().to_owned())
    }
}

impl From<pylota_identity_platform::PlatformError> for StepErr {
    fn from(_: pylota_identity_platform::PlatformError) -> Self {
        StepErr::new("platform_error")
    }
}

pub type StepResult = Result<Slice, StepErr>;

#[derive(Deserialize)]
struct Kv {
    v: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepRow {
    pub name: String,
    pub status: String,
    pub cursor: Option<String>,
    pub counts_json: String,
    pub attempts: i64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeldRow {
    pub thread_id: String,
    pub identity_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Snap {
    steps: Vec<StepRow>,
    held: Vec<HeldRow>,
    deleted_vectors: Vec<String>,
}

#[derive(Deserialize)]
struct IdRow {
    id: String,
}

/// What a step sees: the platform, the runner's own storage and the job's identity.
pub struct Ctx<'a, P: Platform, S: Sql> {
    pub p: &'a P,
    pub sql: &'a S,
    pub job_id: String,
    pub kind: String,
    pub tenant_id: Option<String>,
    pub params: Value,
    pub now: i64,
}

impl<P: Platform, S: Sql> Ctx<'_, P, S> {
    pub fn get(&self, k: &str) -> PResult<Option<String>> {
        Ok(self
            .sql
            .first::<Kv>("SELECT v FROM meta WHERE k = ?1", &[k.into()])?
            .map(|r| r.v))
    }

    pub fn set(&self, k: &str, v: &str) -> PResult<()> {
        self.sql.exec(
            "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            &[k.into(), v.into()],
        )?;
        Ok(())
    }

    pub fn del(&self, k: &str) -> PResult<()> {
        self.sql
            .exec("DELETE FROM meta WHERE k = ?1", &[k.into()])?;
        Ok(())
    }

    pub fn tenant(&self) -> &str {
        self.tenant_id.as_deref().unwrap_or("")
    }

    pub fn hold(&self, thread_id: &str, identity_id: &str, reason: &str) -> PResult<()> {
        self.sql.exec(
            "INSERT OR IGNORE INTO held (thread_id, identity_id, reason) VALUES (?1, ?2, ?3)",
            &[thread_id.into(), identity_id.into(), reason.into()],
        )?;
        Ok(())
    }

    pub fn held(&self) -> PResult<Vec<HeldRow>> {
        self.sql.query(
            "SELECT thread_id, identity_id, reason FROM held ORDER BY thread_id",
            &[],
        )
    }

    pub fn note_vectors(&self, ids: &[String]) -> PResult<()> {
        for id in ids {
            self.sql.exec(
                "INSERT OR IGNORE INTO deleted_vectors (id) VALUES (?1)",
                &[id.as_str().into()],
            )?;
        }
        Ok(())
    }

    pub fn deleted_vectors(&self) -> PResult<Vec<String>> {
        #[derive(Deserialize)]
        struct V {
            id: String,
        }
        Ok(self
            .sql
            .query::<V>("SELECT id FROM deleted_vectors ORDER BY id", &[])?
            .into_iter()
            .map(|v| v.id)
            .collect())
    }

    /// The summed counts of every step so far.
    pub fn counts(&self) -> PResult<Map<String, Value>> {
        let rows: Vec<StepRow> = self.sql.query(
            "SELECT name, status, cursor, counts_json, attempts, last_error FROM steps",
            &[],
        )?;
        let mut total = Map::new();
        for r in rows {
            rules::add_counts(
                &mut total,
                &serde_json::from_str(&r.counts_json).unwrap_or_default(),
            );
        }
        Ok(total)
    }

    /// The step `name`'s own counts so far.
    pub fn step_counts(&self, name: &str) -> PResult<Value> {
        Ok(self
            .sql
            .first::<StepRow>(
                "SELECT name, status, cursor, counts_json, attempts, last_error FROM steps WHERE name = ?1",
                &[name.into()],
            )?
            .and_then(|r| serde_json::from_str(&r.counts_json).ok())
            .unwrap_or_else(|| json!({})))
    }

    /// Sends `req` to the mailbox `mailbox_do_id` of `identity_id`, as the job's tenant.
    pub async fn mailbox(
        &self,
        identity_id: &str,
        mailbox_do_id: &str,
        req: crate::mailbox::MailboxRequest,
    ) -> Result<Result<Value, ApiError>, StepErr> {
        let env = envelope_for(
            self.p,
            self.tenant_id.as_deref(),
            Some(identity_id),
            &format!("req_{}", &self.job_id[4..]),
            None,
            req,
        );
        match self
            .p
            .objects()
            .call::<_, Value>(DoClass::Mailbox, mailbox_do_id, &env)
            .await
        {
            Ok(RpcResult::Ok(v)) => Ok(Ok(v)),
            Ok(RpcResult::Err(e)) => Ok(Err(ApiError::new(e.error.code, e.error.message))),
            Err(_) => Err(StepErr::new("mailbox_unreachable")),
        }
    }
}

/// The steps of a job kind, in order.
pub fn steps_for(kind: &str, params: &Value) -> Vec<&'static str> {
    match kind {
        "erasure" => erasure::steps(params),
        "retention" => retention::steps(params),
        "export" => export::STEPS.to_vec(),
        "reparse" | "reindex" | "reembed" => maintenance::STEPS.to_vec(),
        "backup" => backup::STEPS.to_vec(),
        "domain_remove" => domain_remove::STEPS.to_vec(),
        _ => Vec::new(),
    }
}

async fn run_step<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    step: &str,
    cursor: Option<&str>,
) -> StepResult {
    match c.kind.as_str() {
        "erasure" => erasure::run(c, step, cursor).await,
        "retention" => retention::run(c, step, cursor).await,
        "export" => export::run(c, step, cursor).await,
        "reparse" | "reindex" | "reembed" => maintenance::run(c, step, cursor).await,
        "backup" => backup::run(c, step, cursor).await,
        "domain_remove" => domain_remove::run(c, step, cursor).await,
        _ => Err(StepErr::new("unknown_kind")),
    }
}

/// The kind's cleanup on every end (Privacy § 4, "Finalise on every end"): job-local copies of
/// personal data, and an export's open upload.
async fn finalise<P: Platform, S: Sql>(c: &Ctx<'_, P, S>, outcome: &str) -> Result<(), StepErr> {
    if c.kind == "export" {
        export::finalise(c, outcome).await?;
    }
    c.del("target_address")?;
    Ok(())
}

/// The runner, over its storage and alarm.
pub struct Runner<'a, P: Platform, S: Sql, St: ObjectState> {
    pub p: &'a P,
    pub sql: &'a S,
    pub state: &'a St,
}

impl<P: Platform, S: Sql, St: ObjectState> Runner<'_, P, S, St> {
    fn migrate(&self) -> PResult<()> {
        for s in [
            "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS steps (name TEXT PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending','running','done','failed','skipped')), cursor TEXT, counts_json TEXT NOT NULL DEFAULT '{}', attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT, updated_at INTEGER NOT NULL)",
            "CREATE TABLE IF NOT EXISTS held (thread_id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, reason TEXT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS deleted_vectors (id TEXT PRIMARY KEY)",
        ] {
            self.sql.exec(s, &[])?;
        }
        Ok(())
    }

    fn get(&self, k: &str) -> PResult<Option<String>> {
        Ok(self
            .sql
            .first::<Kv>("SELECT v FROM meta WHERE k = ?1", &[k.into()])?
            .map(|r| r.v))
    }

    fn set(&self, k: &str, v: &str) -> PResult<()> {
        self.sql.exec(
            "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            &[k.into(), v.into()],
        )?;
        Ok(())
    }

    fn steps(&self) -> PResult<Vec<StepRow>> {
        self.sql.query(
            "SELECT name, status, cursor, counts_json, attempts, last_error FROM steps ORDER BY rowid",
            &[],
        )
    }

    fn ctx(&self) -> PResult<Option<Ctx<'_, P, S>>> {
        let Some(job_id) = self.get("job_id")? else {
            return Ok(None);
        };
        Ok(Some(Ctx {
            p: self.p,
            sql: self.sql,
            job_id,
            kind: self.get("kind")?.unwrap_or_default(),
            tenant_id: self.get("tenant_id")?.filter(|t| !t.is_empty()),
            params: self
                .get("params")?
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_else(|| json!({})),
            now: self.p.clock().now_ms(),
        }))
    }

    pub async fn handle(&self, envelope: &Value) -> Value {
        let request_id = envelope["request_id"].as_str().unwrap_or("").to_owned();
        let docs = self.p.config().docs_url.clone();
        let fail = |e: ApiError| {
            serde_json::to_value(error_body::<()>(&e, &request_id, &docs)).unwrap_or(Value::Null)
        };
        let env: RpcEnvelope<JobRequest> = match serde_json::from_value(envelope.clone()) {
            Ok(e) => e,
            Err(_) => {
                return fail(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "Unknown JobRunner request.",
                ));
            }
        };
        if let Err(e) = self.migrate() {
            return fail(e.into());
        }
        // The owner: the tenant written by Start ('' for a deployment-wide job).
        if !matches!(env.req, JobRequest::Start { .. }) {
            let stored = self.get("tenant_id").ok().flatten();
            let sent = env.tenant_id.clone().unwrap_or_default();
            if let Err(e) =
                crate::rpc::check_owner(self.p, stored.as_deref(), Some(sent.as_str()), "JobRunner")
            {
                return fail(e);
            }
        }
        match self.run(env.req).await {
            Ok(v) => json!({ "ok": v }),
            Err(e) => fail(e),
        }
    }

    async fn run(&self, req: JobRequest) -> Result<Value, ApiError> {
        let now = self.p.clock().now_ms();
        match req {
            JobRequest::Start {
                job_id,
                kind,
                tenant_id,
                params,
                resume_job_id,
            } => {
                if self.get("job_id")?.is_some() {
                    return Ok(json!({ "started": false }));
                }
                self.set("job_id", &job_id)?;
                self.set("kind", &kind)?;
                self.set("tenant_id", tenant_id.as_deref().unwrap_or(""))?;
                let mut stored = params.clone();
                // The clear address stays in meta.target_address only (Privacy § 6.1).
                if let Some(a) = params["address"].as_str() {
                    self.set("target_address", a)?;
                    if let Some(o) = stored.as_object_mut() {
                        o.remove("address");
                    }
                }
                self.set("params", &stored.to_string())?;
                for name in steps_for(&kind, &stored) {
                    self.sql.exec(
                        "INSERT OR IGNORE INTO steps (name, status, updated_at) VALUES (?1, 'pending', ?2)",
                        &[name.into(), now.into()],
                    )?;
                }
                if kind == "erasure" {
                    #[derive(Deserialize)]
                    struct D {
                        deadline_at: i64,
                    }
                    let d: Option<D> = self
                        .p
                        .db()
                        .first(Stmt::new(
                            "SELECT deadline_at FROM erasure_requests WHERE job_id = ?1",
                            vec![job_id.clone().into()],
                        ))
                        .await?;
                    let deadline = d.map_or(now + crate::db::jobs::ERASURE_DEADLINE_MS, |d| {
                        d.deadline_at
                    });
                    self.set(
                        "retry_until",
                        &rules::retry_until(deadline, now).to_string(),
                    )?;
                }
                if let Some(old) = resume_job_id {
                    self.set("resume_job_id", &old)?;
                    self.copy_from(&old, tenant_id.as_deref()).await?;
                }
                self.state.set_alarm(now).await?;
                Ok(json!({ "started": true }))
            }
            JobRequest::Snapshot => {
                let held: Vec<HeldRow> = self
                    .sql
                    .query("SELECT thread_id, identity_id, reason FROM held", &[])?;
                #[derive(Deserialize)]
                struct V {
                    id: String,
                }
                let deleted_vectors = self
                    .sql
                    .query::<V>("SELECT id FROM deleted_vectors", &[])?
                    .into_iter()
                    .map(|v| v.id)
                    .collect();
                Ok(serde_json::to_value(Snap {
                    steps: self.steps()?,
                    held,
                    deleted_vectors,
                })
                .unwrap_or(Value::Null))
            }
            JobRequest::Cancel => {
                if self.get("ended")?.is_none() {
                    self.set("cancel", "1")?;
                    self.state.set_alarm(now).await?;
                }
                Ok(json!({ "canceling": self.get("ended")?.is_none() }))
            }
            #[cfg(feature = "itest-hooks")]
            JobRequest::TestAlarm => {
                self.alarm().await;
                #[derive(Deserialize, Serialize)]
                struct M {
                    k: String,
                    v: String,
                }
                let meta: Vec<M> = self.sql.query("SELECT k, v FROM meta ORDER BY k", &[])?;
                Ok(json!({
                    "meta": meta.into_iter().map(|m| (m.k, Value::String(m.v))).collect::<Map<_, _>>(),
                    "steps": self.steps()?,
                    "alarm": self.state.get_alarm().await?,
                }))
            }
        }
    }

    /// Copies the failed job's steps, held threads and deleted vector IDs, with attempts reset.
    async fn copy_from(&self, old_job: &str, tenant_id: Option<&str>) -> Result<(), ApiError> {
        #[derive(Deserialize)]
        struct R {
            runner_do_id: String,
        }
        let Some(r) = self
            .p
            .db()
            .first::<R>(Stmt::new(
                "SELECT runner_do_id FROM jobs WHERE id = ?1",
                vec![old_job.into()],
            ))
            .await?
        else {
            return Ok(());
        };
        let env = envelope_for(
            self.p,
            tenant_id,
            None,
            &crate::request_id::new(self.p),
            None,
            JobRequest::Snapshot,
        );
        let snap: Snap = match self
            .p
            .objects()
            .call::<_, Snap>(DoClass::Job, &r.runner_do_id, &env)
            .await
        {
            Ok(RpcResult::Ok(s)) => s,
            _ => return Ok(()),
        };
        for s in snap.steps {
            // A done step stays done; the failed one continues from its cursor.
            let status = if s.status == "done" || s.status == "skipped" {
                s.status
            } else {
                "pending".to_owned()
            };
            self.sql.exec(
                "INSERT INTO steps (name, status, cursor, counts_json, attempts, updated_at) VALUES (?1, ?2, ?3, ?4, 0, ?5) \
                 ON CONFLICT (name) DO UPDATE SET status = excluded.status, cursor = excluded.cursor, counts_json = excluded.counts_json, attempts = 0",
                &[s.name.into(), status.into(), s.cursor.into(), s.counts_json.into(), self.p.clock().now_ms().into()],
            )?;
        }
        for h in snap.held {
            self.sql.exec(
                "INSERT OR IGNORE INTO held (thread_id, identity_id, reason) VALUES (?1, ?2, ?3)",
                &[h.thread_id.into(), h.identity_id.into(), h.reason.into()],
            )?;
        }
        for v in snap.deleted_vectors {
            self.sql.exec(
                "INSERT OR IGNORE INTO deleted_vectors (id) VALUES (?1)",
                &[v.into()],
            )?;
        }
        Ok(())
    }

    /// The alarm: one slice of the first unfinished step.
    pub async fn alarm(&self) {
        if let Err(e) = self.alarm_inner().await {
            crate::log::warn("job_alarm_failed", &[("code", e.code.as_str().into())]);
            let _ = self
                .state
                .set_alarm(self.p.clock().now_ms() + rules::BACKOFF_BASE_MS)
                .await;
        }
    }

    async fn alarm_inner(&self) -> Result<(), ApiError> {
        self.migrate()?;
        if self.get("ended")?.is_some() {
            return Ok(());
        }
        let Some(c) = self.ctx()? else {
            return Ok(());
        };
        if self.get("cancel")?.is_some() {
            return self.end(&c, "canceled", None).await;
        }
        let steps = self.steps()?;
        let Some(step) = steps
            .iter()
            .find(|s| !matches!(s.status.as_str(), "done" | "skipped"))
            .cloned()
        else {
            return self.end(&c, "completed", None).await;
        };
        if self.get("mirrored_running")?.is_none() {
            mirror_running(self.p, &c).await?;
            self.set("mirrored_running", "1")?;
        }
        let now = c.now;
        match run_step(&c, &step.name, step.cursor.as_deref()).await {
            Ok(Slice::More { cursor, counts }) => {
                self.save(&step, "running", Some(&cursor), &counts, 0, None)?;
                self.resolve_stalled(&c).await?;
                self.state.set_alarm(now).await?;
            }
            Ok(Slice::Done { counts }) => {
                self.save(&step, "done", None, &counts, 0, None)?;
                self.resolve_stalled(&c).await?;
                self.state.set_alarm(now).await?;
            }
            Ok(Slice::Wait { after_ms }) => {
                self.state.set_alarm(now + after_ms).await?;
            }
            Err(StepErr(code)) => {
                let attempts = step.attempts + 1;
                self.save(
                    &step,
                    &step.status,
                    step.cursor.as_deref(),
                    &json!({}),
                    attempts,
                    Some(&code),
                )?;
                let erasure = c.kind == "erasure";
                let retry_until = self.get("retry_until")?.and_then(|v| v.parse().ok());
                if FATAL.contains(&code.as_str())
                    || rules::fails_job(erasure, attempts, retry_until, now)
                {
                    return self.end(&c, "failed", Some((&step.name, &code))).await;
                }
                if erasure && attempts == rules::STALLED_AT {
                    crate::ops::alerts::report(
                        self.p,
                        &stalled_key(&c),
                        c.tenant_id.as_deref(),
                        None,
                        &json!({ "job_id": c.job_id, "step": step.name, "code": code, "attempts": attempts }),
                        &c.job_id,
                    )
                    .await;
                    self.set("stalled", "1")?;
                }
                self.state
                    .set_alarm(now + rules::backoff_ms(attempts))
                    .await?;
            }
        }
        Ok(())
    }

    /// `erasure_stalled` resolves when the step next succeeds or the job ends (Observability § 5.4).
    async fn resolve_stalled(&self, c: &Ctx<'_, P, S>) -> PResult<()> {
        if self.get("stalled")?.is_some() {
            crate::ops::alerts::resolve_reported(
                self.p,
                &stalled_key(c),
                c.tenant_id.as_deref(),
                &c.job_id,
            )
            .await;
            self.sql.exec("DELETE FROM meta WHERE k = 'stalled'", &[])?;
        }
        Ok(())
    }

    fn save(
        &self,
        step: &StepRow,
        status: &str,
        cursor: Option<&str>,
        counts: &Value,
        attempts: i64,
        error: Option<&str>,
    ) -> PResult<()> {
        let mut total: Map<String, Value> =
            serde_json::from_str(&step.counts_json).unwrap_or_default();
        rules::add_counts(&mut total, counts);
        self.sql.exec(
            "UPDATE steps SET status = ?2, cursor = ?3, counts_json = ?4, attempts = ?5, last_error = ?6, updated_at = ?7 WHERE name = ?1",
            &[
                step.name.as_str().into(),
                status.into(),
                cursor.map(str::to_owned).into(),
                Value::Object(total).to_string().into(),
                attempts.into(),
                error.map(str::to_owned).into(),
                self.p.clock().now_ms().into(),
            ],
        )?;
        Ok(())
    }

    /// Every end: `finalise` first, then the kind's result, then the D1 mirror.
    async fn end(
        &self,
        c: &Ctx<'_, P, S>,
        outcome: &str,
        failed: Option<(&str, &str)>,
    ) -> Result<(), ApiError> {
        if let Err(StepErr(code)) = finalise(c, outcome).await {
            // finalise touches only the runner's storage and R2; a crash or fault re-runs it.
            crate::log::warn("job_finalise_failed", &[("code", code.into())]);
            self.state.set_alarm(c.now + rules::BACKOFF_BASE_MS).await?;
            return Ok(());
        }
        let counts = c.counts()?;
        let result = match c.kind.as_str() {
            "erasure" => erasure::on_end(c, outcome, &counts, failed).await?,
            "export" => export::on_end(c, outcome, failed).await?,
            _ => {
                let mut r = Value::Object(counts);
                if let Some((step, code)) = failed {
                    r["step"] = step.into();
                    r["error"] = code.into();
                }
                r
            }
        };
        // `job_failed:{kind}` and `domain_remove_failed` are read from D1 by the alert evaluator.
        if outcome == "failed" {
            crate::log::error(
                "job_failed",
                &[
                    ("job_id", c.job_id.clone().into()),
                    ("detail", c.kind.clone().into()),
                ],
            );
        }
        if self.get("stalled")?.is_some() {
            crate::ops::alerts::resolve_reported(
                self.p,
                &stalled_key(c),
                c.tenant_id.as_deref(),
                &c.job_id,
            )
            .await;
        }
        self.p
            .db()
            .execute(Stmt::new(
                "UPDATE jobs SET status = ?2, result_json = ?3, completed_at = ?4, updated_at = ?4 WHERE id = ?1",
                vec![
                    c.job_id.clone().into(),
                    outcome.into(),
                    result.to_string().into(),
                    c.now.into(),
                ],
            ))
            .await?;
        self.set("ended", outcome)?;
        Ok(())
    }
}

/// `queued` → `running` on the job and its request (Privacy § 4, "Mirroring").
async fn mirror_running<P: Platform, S: Sql>(p: &P, c: &Ctx<'_, P, S>) -> Result<(), ApiError> {
    p.db()
        .batch(vec![
            Stmt::new(
                "UPDATE jobs SET status = 'running', updated_at = ?2 WHERE id = ?1 AND status = 'queued'",
                vec![c.job_id.clone().into(), c.now.into()],
            ),
            Stmt::new(
                "UPDATE erasure_requests SET status = 'running' WHERE job_id = ?1 AND status = 'queued'",
                vec![c.job_id.clone().into()],
            ),
            Stmt::new(
                "UPDATE exports SET status = 'running' WHERE job_id = ?1 AND status = 'queued'",
                vec![c.job_id.clone().into()],
            ),
        ])
        .await?;
    Ok(())
}

/// The `erasure_stalled` key of an erasure job: its erasure request's ID.
fn stalled_key<P: Platform, S: Sql>(c: &Ctx<'_, P, S>) -> String {
    format!(
        "erasure_stalled:{}",
        c.params["erasure_request_id"].as_str().unwrap_or(&c.job_id)
    )
}

/// A job event: written to D1 `event_index` (`owner_kind = 'job'`, the payload kept in the index)
/// and fanned out like a domain's.
pub async fn emit<P: Platform>(
    p: &P,
    tenant_id: Option<&str>,
    identity_id: Option<&str>,
    owner_id: &str,
    event_type: &str,
    data: &Value,
    now: i64,
) -> Result<(), ApiError> {
    let event_id = pylota_identity_core::ids::derived(
        IdPrefix::Event,
        now,
        &p.secrets().hash_key,
        &format!("{event_type}:{owner_id}"),
    );
    let body = envelope(
        &event_id,
        event_type,
        now,
        tenant_id,
        identity_id,
        None,
        data,
    );
    p.db()
        .execute(Stmt::new(
            "INSERT OR IGNORE INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, partner_id, payload_json, occurred_at) VALUES (?1, ?2, ?3, ?4, 'job', ?5, (SELECT partner_id FROM tenants WHERE id = ?2), ?6, ?7)",
            vec![
                event_id.clone().into(),
                tenant_id.map(str::to_owned).into(),
                identity_id.map(str::to_owned).into(),
                event_type.into(),
                owner_id.into(),
                body.into(),
                now.into(),
            ],
        ))
        .await?;
    let job = WebhookJob::Fanout {
        v: 1,
        event_id,
        event_type: event_type.to_owned(),
        tenant_id: tenant_id.map(str::to_owned),
        identity_id: identity_id.map(str::to_owned),
        owner_kind: OwnerKind::Job,
        owner_id: owner_id.to_owned(),
        occurred_at: now,
    };
    let _ = p.queues().send(QueueName::Webhooks, &job, 0).await;
    Ok(())
}

/// Sends `Start` to a job's runner. A lost call is repaired by the every-minute cron.
pub async fn start<P: Platform>(
    p: &P,
    runner_do_id: &str,
    req: JobRequest,
    tenant_id: Option<&str>,
) -> bool {
    let env = envelope_for(p, tenant_id, None, &crate::request_id::new(p), None, req);
    matches!(
        p.objects()
            .call::<_, Value>(DoClass::Job, runner_do_id, &env)
            .await,
        Ok(RpcResult::Ok(_))
    )
}

/// Sends `Cancel` to a job's runner.
pub async fn cancel<P: Platform>(p: &P, runner_do_id: &str, tenant_id: Option<&str>) -> bool {
    let env = envelope_for(
        p,
        tenant_id,
        None,
        &crate::request_id::new(p),
        None,
        JobRequest::Cancel,
    );
    matches!(
        p.objects()
            .call::<_, Value>(DoClass::Job, runner_do_id, &env)
            .await,
        Ok(RpcResult::Ok(_))
    )
}

/// A new job: its row and runner ID. The caller writes the statements in its D1 batch, then calls
/// [`start`].
pub struct NewJob {
    pub id: String,
    pub runner_do_id: String,
}

pub fn new_job<P: Platform>(p: &P) -> Result<NewJob, ApiError> {
    use pylota_identity_platform::Ids as _;
    Ok(NewJob {
        id: p.ids().new_id(IdPrefix::Job),
        runner_do_id: p.objects().new_object_id(DoClass::Job)?,
    })
}

/// A deployment-wide or tenant job created and started from a cron or another job (no key).
pub async fn create<P: Platform>(
    p: &P,
    kind: &str,
    tenant_id: Option<&str>,
    params: &Value,
) -> Result<String, ApiError> {
    let j = new_job(p)?;
    let now = p.clock().now_ms();
    p.db()
        .execute(Stmt::new(
            "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_at, updated_at) VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?6, ?6)",
            vec![
                j.id.clone().into(),
                tenant_id.map(str::to_owned).into(),
                kind.into(),
                j.runner_do_id.clone().into(),
                params.to_string().into(),
                now.into(),
            ],
        ))
        .await?;
    start(
        p,
        &j.runner_do_id,
        JobRequest::Start {
            job_id: j.id.clone(),
            kind: kind.to_owned(),
            tenant_id: tenant_id.map(str::to_owned),
            params: params.clone(),
            resume_job_id: None,
        },
        tenant_id,
    )
    .await;
    Ok(j.id)
}

#[derive(Deserialize)]
struct Queued {
    id: String,
    tenant_id: Option<String>,
    kind: String,
    runner_do_id: String,
    params_json: String,
}

/// The every-minute cron: a job still `queued` after 60 seconds gets its `Start` again (a lost call).
/// A counterparty erasure's address was never stored, so such a job cannot be restarted: it waits
/// for a new request.
pub async fn restart_queued<P: Platform>(p: &P) {
    let now = p.clock().now_ms();
    let Ok(rows) = p
        .db()
        .query::<Queued>(Stmt::new(
            "SELECT id, tenant_id, kind, runner_do_id, params_json FROM jobs WHERE status = 'queued' AND created_at < ?1 ORDER BY created_at LIMIT 20",
            vec![(now - 60_000).into()],
        ))
        .await
    else {
        return;
    };
    for r in rows {
        let params: Value = serde_json::from_str(&r.params_json).unwrap_or_else(|_| json!({}));
        let resume: Option<String> = p
            .db()
            .first::<IdRow>(Stmt::new(
                "SELECT j.id FROM erasure_requests e JOIN erasure_requests f ON f.id = e.resumes_id JOIN jobs j ON j.id = f.job_id WHERE e.job_id = ?1 AND f.status = 'failed'",
                vec![r.id.clone().into()],
            ))
            .await
            .ok()
            .flatten()
            .map(|x| x.id);
        let resume = match (resume, r.kind.as_str()) {
            (None, "domain_remove") => {
                let did = params["domain_id"].as_str().unwrap_or("");
                domain_remove::resume_of(p, did, &r.id).await
            }
            (r, _) => r,
        };
        start(
            p,
            &r.runner_do_id,
            JobRequest::Start {
                job_id: r.id,
                kind: r.kind,
                tenant_id: r.tenant_id.clone(),
                params,
                resume_job_id: resume,
            },
            r.tenant_id.as_deref(),
        )
        .await;
    }
}
