//! `DomainMonitor` ([Identities and domains › Domain health]): one object per domain. Its alarm runs a
//! check of every expected record on both DoH resolvers, adds the weekly NS and RDAP rows, and feeds the
//! two outcomes to `core::domain_fsm::step`; the object carries out the step's actions (D1, addresses,
//! removal jobs, ownership tokens) and its events (FR-DOM-4, FR-DOM-5, H1, H4, H7, H9).
//!
//! [Identities and domains › Domain health]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::address::org_domain;
use pylota_identity_core::domain_fsm::{
    self, Action, Check, DnsIssue, Domain, Event, Input, Outcome, OwnershipReason, Refusal, State,
    Step,
};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::dns::{Dns as _, RType, Resolver};
use pylota_identity_platform::durable::{DoClass, ObjectState, RpcEnvelope, RpcResult, Sql};
use pylota_identity_platform::http::HttpRequest;
use pylota_identity_platform::{
    Clock as _, ControlDb as _, HttpClient as _, ObjectClient as _, PResult, Platform,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::cloudflare_api::Cf;
use super::records::{self, Facts, Issue, ResolverResult};
use super::ses_api::{Ses, SesError};
use super::ses_control::SesCaller;
use crate::db::domains as ddb;
use crate::errors::ApiError;
use crate::rpc::{envelope_for, error_body};

const DAY: i64 = 86_400_000;
const WEEK: i64 = 7 * DAY;
const HOUR: i64 = 3_600_000;
/// Rows of `checks` kept.
const CHECKS_KEPT: i64 = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DomainRequest {
    /// The row is inserted: start verification.
    Init {
        domain_id: String,
    },
    /// A check now (an address change, a transport change).
    Check {
        domain_id: String,
    },
    /// `POST …/verify`: a check now, at most once a minute.
    Verify {
        domain_id: String,
    },
    Reprove {
        domain_id: String,
    },
    Delete {
        domain_id: String,
        live_addresses: bool,
        job_failed: bool,
    },
    Evict {
        domain_id: String,
    },
    JobCompleted {
        domain_id: String,
        reason: String,
    },
    /// Removal finished: delete everything, answer `Retired` from then on.
    Retire {
        domain_id: String,
    },
    Health {
        domain_id: String,
    },
    /// Role mail relay, step 3: one role message to this domain from the sender with `sender_hash`.
    AdmitRoleMail {
        domain_id: String,
        sender_hash: String,
        now: i64,
    },
    #[cfg(feature = "itest-hooks")]
    TestAlarm {
        domain_id: String,
    },
}

impl DomainRequest {
    fn domain_id(&self) -> &str {
        match self {
            DomainRequest::Init { domain_id }
            | DomainRequest::Check { domain_id }
            | DomainRequest::Verify { domain_id }
            | DomainRequest::Reprove { domain_id }
            | DomainRequest::Delete { domain_id, .. }
            | DomainRequest::Evict { domain_id }
            | DomainRequest::JobCompleted { domain_id, .. }
            | DomainRequest::Retire { domain_id }
            | DomainRequest::Health { domain_id }
            | DomainRequest::AdmitRoleMail { domain_id, .. } => domain_id,
            #[cfg(feature = "itest-hooks")]
            DomainRequest::TestAlarm { domain_id } => domain_id,
        }
    }
}

#[derive(Deserialize)]
struct Kv {
    v: String,
}

/// One stored check row.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CheckRow {
    resolver: String,
    outcome: String,
    results_json: String,
    at: i64,
}

/// The second of the day (UTC) of a domain's daily SES identity check: the first 8 bytes of
/// SHA-256(`domain_id`), big-endian, modulo 86,400.
pub fn ses_check_offset_s(domain_id: &str) -> i64 {
    let h = pylota_identity_core::ses::sha256_hex(domain_id.as_bytes());
    (u64::from_str_radix(&h[..16], 16).unwrap_or(0) % 86_400) as i64
}

/// The first daily check time of `domain_id` after `now`.
pub fn next_ses_check(domain_id: &str, now: i64) -> i64 {
    let at = now.div_euclid(86_400_000) * 86_400_000 + ses_check_offset_s(domain_id) * 1_000;
    if at > now { at } else { at + 86_400_000 }
}

/// Role mail relay, step 3.
pub const ROLE_PER_DOMAIN: u32 = 30;
pub const ROLE_PER_SENDER: u32 = 5;

pub struct Monitor<'a, P: Platform, S: Sql, St: ObjectState> {
    pub p: &'a P,
    pub sql: &'a S,
    pub state: &'a St,
}

fn outcome_of(r: &ResolverResult) -> Outcome {
    match r.outcome() {
        "error" => Outcome::Error,
        "fail" => Outcome::Fail,
        "degraded" => Outcome::Degraded,
        "ownership_changed" => {
            let code = r
                .issues
                .iter()
                .find(|i| i.level == "ownership")
                .map_or("", |i| i.code.as_str());
            Outcome::OwnershipChanged(match code {
                "nameservers_changed" => OwnershipReason::NameserversChanged,
                "registration_changed" => OwnershipReason::RegistrationChanged,
                _ => OwnershipReason::OwnershipRecordMissing,
            })
        }
        _ => Outcome::Pass,
    }
}

fn state_str(s: State) -> &'static str {
    match s {
        State::Pending => "pending",
        State::Verifying => "verifying",
        State::Healthy => "healthy",
        State::Degraded => "degraded",
        State::Failing => "failing",
        State::Suspended => "suspended",
        State::Removing => "removing",
        State::Removed => "removed",
    }
}

fn parse_state(s: &str) -> State {
    match s {
        "verifying" => State::Verifying,
        "healthy" => State::Healthy,
        "degraded" => State::Degraded,
        "failing" => State::Failing,
        "suspended" => State::Suspended,
        "removing" => State::Removing,
        "removed" => State::Removed,
        _ => State::Pending,
    }
}

/// The registrar fingerprint of an RDAP domain object (§ RDAP).
pub fn rdap_fingerprint(v: &Value) -> Option<String> {
    let entities = v["entities"].as_array()?;
    let handle = |role: &str| {
        entities
            .iter()
            .find(|e| {
                e["roles"]
                    .as_array()
                    .is_some_and(|r| r.iter().any(|x| x == role))
            })
            .and_then(|e| e["handle"].as_str())
            .unwrap_or("")
            .to_owned()
    };
    let registrar = handle("registrar");
    if registrar.is_empty() {
        return None;
    }
    let date = v["events"]
        .as_array()
        .and_then(|a| a.iter().find(|e| e["eventAction"] == "registration"))
        .and_then(|e| e["eventDate"].as_str())
        .unwrap_or("");
    Some(pylota_identity_core::ses::sha256_hex(
        format!("{registrar}{}{date}", handle("registrant")).as_bytes(),
    ))
}

impl<P: Platform, S: Sql, St: ObjectState> Monitor<'_, P, S, St> {
    fn migrate(&self) -> PResult<()> {
        self.sql.exec(
            "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
            &[],
        )?;
        self.sql.exec(
            "CREATE TABLE IF NOT EXISTS checks (id INTEGER PRIMARY KEY, resolver TEXT NOT NULL, outcome TEXT NOT NULL, results_json TEXT NOT NULL, at INTEGER NOT NULL)",
            &[],
        )?;
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

    fn del(&self, k: &str) -> PResult<()> {
        self.sql
            .exec("DELETE FROM meta WHERE k = ?1", &[k.into()])?;
        Ok(())
    }

    fn fsm(&self) -> PResult<Option<Domain>> {
        Ok(self.get("fsm")?.and_then(|f| serde_json::from_str(&f).ok()))
    }

    fn save_fsm(&self, d: &Domain) -> PResult<()> {
        self.set("fsm", &serde_json::to_string(d).unwrap_or_default())
    }

    pub async fn handle(&self, envelope: &Value) -> Value {
        let request_id = envelope["request_id"].as_str().unwrap_or("").to_owned();
        let docs = self.p.config().docs_url.clone();
        let fail = |e: ApiError| {
            serde_json::to_value(error_body::<()>(&e, &request_id, &docs)).unwrap_or(Value::Null)
        };
        let env: RpcEnvelope<DomainRequest> = match serde_json::from_value(envelope.clone()) {
            Ok(e) => e,
            Err(_) => {
                return fail(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "Unknown monitor request.",
                ));
            }
        };
        if let Err(e) = self.migrate() {
            return fail(e.into());
        }
        // A retired monitor answers before any owner check.
        if self.get("retired").ok().flatten().is_some() {
            return json!({ "ok": { "retired": true } });
        }
        let sent = env.req.domain_id().to_owned();
        let mut stored = self.get("domain_id").ok().flatten();
        // A lost `Init` never leaves the domain unmonitored: the first request initialises the object.
        if stored.is_none() && !matches!(env.req, DomainRequest::Init { .. }) {
            if let Err(e) = self
                .run(DomainRequest::Init {
                    domain_id: sent.clone(),
                })
                .await
            {
                return fail(e);
            }
            stored = Some(sent.clone());
        }
        if !matches!(env.req, DomainRequest::Init { .. })
            && let Err(e) =
                crate::rpc::check_owner(self.p, stored.as_deref(), Some(&sent), "DomainMonitor")
        {
            return fail(e);
        }
        match self.run(env.req).await {
            Ok(v) => json!({ "ok": v }),
            Err(e) => fail(e),
        }
    }

    async fn run(&self, req: DomainRequest) -> Result<Value, ApiError> {
        let now = self.p.clock().now_ms();
        match req {
            DomainRequest::Init { domain_id } => {
                if let Some(existing) = self.get("domain_id")? {
                    if existing != domain_id {
                        return Err(ApiError::internal());
                    }
                    return Ok(Value::Null);
                }
                let row = self.row(&domain_id).await?;
                self.set("domain_id", &domain_id)?;
                let mut d = Domain::new(row.kind == "platform", false, row.created_at);
                d.state = parse_state(&row.state);
                d.ownership_verified_at_ms = row.ownership_verified_at;
                d.state_changed_at_ms = row.state_changed_at;
                let s = domain_fsm::step(&mut d, Input::Init { zone_active: true }, now);
                self.save_fsm(&d)?;
                if row.ses_identity.is_some() {
                    self.set("ses_check_at", &next_ses_check(&domain_id, now).to_string())?;
                }
                self.apply(&row, &d, &s, now).await?;
                self.state.set_alarm(now).await?;
                Ok(Value::Null)
            }
            DomainRequest::Check { .. } => {
                self.set("check_at", &now.to_string())?;
                self.state.set_alarm(now).await?;
                Ok(json!({ "scheduled": true }))
            }
            DomainRequest::Verify { .. } => {
                let last = self
                    .get("verify_at")?
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(i64::MIN / 2);
                if now - last < 60_000 {
                    return Ok(json!({ "retry_after_ms": 60_000 - (now - last) }));
                }
                self.set("verify_at", &now.to_string())?;
                self.set("check_at", &now.to_string())?;
                self.state.set_alarm(now).await?;
                Ok(json!({ "scheduled": true }))
            }
            DomainRequest::Reprove { domain_id } => {
                self.input(&domain_id, Input::Reprove, now).await
            }
            DomainRequest::Delete {
                domain_id,
                live_addresses,
                job_failed,
            } => {
                self.input(
                    &domain_id,
                    Input::Delete {
                        live_addresses,
                        job_failed,
                    },
                    now,
                )
                .await
            }
            DomainRequest::Evict { domain_id } => self.input(&domain_id, Input::Evict, now).await,
            DomainRequest::JobCompleted { domain_id, reason } => {
                let r = match reason.as_str() {
                    "evicted" => domain_fsm::RemoveReason::Evicted,
                    "unverified_expired" => domain_fsm::RemoveReason::UnverifiedExpired,
                    _ => domain_fsm::RemoveReason::Requested,
                };
                self.input(&domain_id, Input::JobCompleted(r), now).await
            }
            DomainRequest::Retire { .. } => {
                self.state.delete_all().await?;
                self.migrate()?;
                self.set("retired", &now.to_string())?;
                Ok(json!({ "retired": true }))
            }
            DomainRequest::Health { .. } => {
                let issues: Value = self
                    .get("issues")?
                    .and_then(|i| serde_json::from_str(&i).ok())
                    .unwrap_or_else(|| json!([]));
                let checks: Vec<CheckRow> = self.sql.query(
                    "SELECT resolver, outcome, results_json, at FROM checks ORDER BY id DESC LIMIT 2",
                    &[],
                )?;
                let status: Value = checks
                    .iter()
                    .find(|c| c.resolver == "first")
                    .and_then(|c| serde_json::from_str::<ResolverResult>(&c.results_json).ok())
                    .map_or_else(|| json!([]), |r| json!(r.status));
                Ok(
                    json!({ "issues": issues, "status": status, "checks": checks.iter().map(|c| json!({
                    "resolver": c.resolver, "outcome": c.outcome, "at": pylota_identity_core::days::rfc3339(c.at),
                })).collect::<Vec<_>>() }),
                )
            }
            DomainRequest::AdmitRoleMail {
                sender_hash, now, ..
            } => self.admit_role_mail(&sender_hash, now),
            #[cfg(feature = "itest-hooks")]
            DomainRequest::TestAlarm { .. } => {
                self.alarm().await;
                Ok(
                    json!({ "ses_check_at": self.get("ses_check_at")?.and_then(|v| v.parse::<i64>().ok()) }),
                )
            }
        }
    }

    /// At most [`ROLE_PER_DOMAIN`] role messages an hour to the domain and [`ROLE_PER_SENDER`] from
    /// one sender, in hourly windows kept for 48 hours.
    fn admit_role_mail(&self, sender_hash: &str, now: i64) -> Result<Value, ApiError> {
        let hour = now.div_euclid(3_600_000);
        let count = |k: &str| -> PResult<u32> {
            Ok(self.get(k)?.and_then(|v| v.parse().ok()).unwrap_or(0))
        };
        let dk = format!("role:{hour}");
        let sk = format!("role:{hour}:{sender_hash}");
        let (d, s) = (count(&dk)?, count(&sk)?);
        if d >= ROLE_PER_DOMAIN || s >= ROLE_PER_SENDER {
            return Ok(json!({ "admitted": false }));
        }
        self.set(&dk, &(d + 1).to_string())?;
        self.set(&sk, &(s + 1).to_string())?;
        // Windows older than 48 hours: keys sort as text, so compare the parsed hour.
        let old: Vec<Kv> = self
            .sql
            .query("SELECT k AS v FROM meta WHERE k LIKE 'role:%'", &[])?;
        for k in old {
            let h = k.v[5..]
                .split(':')
                .next()
                .and_then(|h| h.parse::<i64>().ok());
            if h.is_some_and(|h| h < hour - 48) {
                self.del(&k.v)?;
            }
        }
        Ok(json!({ "admitted": true }))
    }

    async fn row(&self, domain_id: &str) -> Result<ddb::Row, ApiError> {
        self.p
            .db()
            .first::<ddb::Row>(ddb::get(domain_id))
            .await?
            .ok_or_else(|| ApiError::not_found(ErrorCode::DomainNotFound))
    }

    /// One non-check input: step, persist, apply. Answers the new state, or the refusal.
    async fn input(&self, domain_id: &str, input: Input, now: i64) -> Result<Value, ApiError> {
        let row = self.row(domain_id).await?;
        let mut d = self.fsm()?.ok_or_else(ApiError::internal)?;
        let s = domain_fsm::step(&mut d, input, now);
        if let Some(r) = s.refused {
            return Ok(json!({ "refused": match r {
                Refusal::ScopeDenied => "scope_denied",
                Refusal::DomainInUse => "domain_in_use",
                Refusal::NotApplicable => "not_applicable",
            } }));
        }
        self.save_fsm(&d)?;
        self.apply(&row, &d, &s, now).await?;
        Ok(json!({ "state": state_str(d.state) }))
    }

    /// The alarm: a check when one is due.
    pub async fn alarm(&self) {
        if self.p.config().frozen {
            let _ = self
                .state
                .set_alarm(self.p.clock().now_ms() + 300_000)
                .await;
            return;
        }
        if let Err(e) = self.alarm_inner().await {
            crate::log::warn(
                "domain_monitor_alarm_failed",
                &[("code", e.code.as_str().into())],
            );
            let _ = self
                .state
                .set_alarm(self.p.clock().now_ms() + domain_fsm::RETRY_AFTER_MS)
                .await;
        }
    }

    async fn alarm_inner(&self) -> Result<(), ApiError> {
        self.migrate()?;
        if self.get("retired")?.is_some() {
            return Ok(());
        }
        let Some(domain_id) = self.get("domain_id")? else {
            return Ok(());
        };
        let now = self.p.clock().now_ms();
        self.ses_check(&domain_id, now).await?;
        let due = self
            .get("check_at")?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        if now < due {
            self.arm(due).await?;
            return Ok(());
        }
        let row = self.row(&domain_id).await?;
        let Some(mut d) = self.fsm()? else {
            return Ok(());
        };
        if matches!(d.state, State::Removing | State::Removed) {
            return Ok(());
        }
        let rule_failed = self.retry_rules(&row, now).await?;
        let (mut results, check) = self.check(&row, &d, now).await?;
        if rule_failed {
            for r in &mut results {
                r.issues.push(Issue {
                    code: "routing_rule_failed".into(),
                    level: "info".into(),
                    record: None,
                });
            }
        }
        let s = domain_fsm::step(&mut d, Input::Check(check), now);
        self.save_fsm(&d)?;
        // The issues of this cycle (the agreed resolver's, or the first's), and the step's own.
        // The step's own issues (`dns_unresolvable`, `dnssec_bogus`) lead: they decided the outcome.
        let mut issues: Vec<Issue> = s
            .issues
            .iter()
            .map(|code| Issue {
                code: (*code).to_owned(),
                level: "fail".into(),
                record: None,
            })
            .collect();
        for i in &results[0].issues {
            if !issues.iter().any(|x| x.code == i.code) {
                issues.push(i.clone());
            }
        }
        if s.agreed.is_some() || results.iter().all(|r| r.errors.is_empty()) {
            self.set(
                "issues",
                &serde_json::to_string(&issues).unwrap_or_default(),
            )?;
        }
        self.apply_with_issues(&row, &d, &s, now, &issues).await?;
        let next = now + s.next_check_ms.unwrap_or(domain_fsm::CHECK_EVERY_MS);
        self.set("check_at", &next.to_string())?;
        self.arm(next).await?;
        Ok(())
    }

    /// The alarm at `at`, or at the daily SES check when that comes first.
    async fn arm(&self, at: i64) -> Result<(), ApiError> {
        let ses = self
            .get("ses_check_at")?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(i64::MAX);
        self.state.set_alarm(at.min(ses)).await?;
        Ok(())
    }

    /// `alarm:ses_check`: once a day at the domain's offset, `GetEmailIdentity` behind a token of
    /// `SesControl`; its DKIM status is kept as `ses_dkim`. A busy bucket moves the check to its retry.
    async fn ses_check(&self, domain_id: &str, now: i64) -> Result<(), ApiError> {
        let Some(at) = self
            .get("ses_check_at")?
            .and_then(|v| v.parse::<i64>().ok())
        else {
            return Ok(());
        };
        if now < at {
            return Ok(());
        }
        let row = self.row(domain_id).await?;
        let (Some(identity), Some(ses)) = (
            row.ses_identity.as_deref(),
            Ses::new(self.p, SesCaller::DomainCheck),
        ) else {
            return self.del("ses_check_at").map_err(Into::into);
        };
        let next = match ses.get_identity(identity).await {
            Ok(v) => {
                self.set(
                    "ses_dkim",
                    v["DkimAttributes"]["Status"].as_str().unwrap_or(""),
                )?;
                next_ses_check(domain_id, now)
            }
            Err(SesError::Busy { retry_after_ms }) => now + retry_after_ms.max(1_000),
            Err(_) => now + 3_600_000,
        };
        self.set("ses_check_at", &next.to_string())?;
        Ok(())
    }

    /// H6: the pending addresses of a literal domain without a rule get one; an address whose rule now
    /// exists is activated when the domain is verified. Answers whether a create failed (the issue
    /// `routing_rule_failed`, informational: it never changes the state).
    async fn retry_rules(&self, row: &ddb::Row, now: i64) -> Result<bool, ApiError> {
        if row.routing_mode != "literal" {
            return Ok(false);
        }
        let pending: Vec<crate::db::addresses::Row> = self
            .p
            .db()
            .query(Stmt::new(
                "SELECT id, address, local_part, domain_id, tenant_id, identity_id, role, status, routing_rule_id, retire_at, retired_at, ses_bounce_rule, forwarding, forwarding_checked_at, created_at FROM addresses WHERE domain_id = ?1 AND status = 'pending' AND routing_rule_id IS NULL",
                vec![row.id.clone().into()],
            ))
            .await?;
        let mut failed = false;
        for a in pending {
            if crate::handlers::addresses::create_rule(self.p, row, &a)
                .await
                .is_none()
            {
                failed = true;
            }
        }
        if matches!(row.state.as_str(), "healthy" | "degraded") {
            self.activate_addresses(row, now).await?;
        }
        Ok(failed)
    }

    /// One check: both resolvers, the weekly NS and RDAP rows, the `cd=1` re-query when due.
    async fn check(
        &self,
        row: &ddb::Row,
        d: &Domain,
        now: i64,
    ) -> Result<([ResolverResult; 2], Check), ApiError> {
        let recs = records::parse(&row.records_json);
        let ns_next = self
            .get("ns_next_at")?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let ns_bad = self.get("ns_bad")?.is_some();
        let ns_due = now >= ns_next || ns_bad;
        let zone_name = match (&row.zone_id, row.kind.as_str()) {
            (Some(_), _) => Some(self.zone_name(row).await),
            _ => None,
        };
        let facts = Facts {
            name: row.name.clone(),
            platform: row.kind == "platform",
            receiving: row.receiving == 1,
            sending: row.sending == 1,
            inbound: row.inbound.clone(),
            transport: row.transport.clone(),
            mail_from_domain: row.mail_from_domain.clone(),
            ownership_token: row.ownership_token.clone(),
            zone_name,
            expected_ns: row
                .expected_ns_json
                .as_deref()
                .and_then(|j| serde_json::from_str(j).ok())
                .unwrap_or_default(),
        };
        let mut results = [
            records::check(self.p.dns(), Resolver::First, &facts, &recs, ns_due).await,
            records::check(self.p.dns(), Resolver::Second, &facts, &recs, ns_due).await,
        ];
        if ns_due {
            if now >= ns_next {
                self.set("ns_next_at", &(now + WEEK).to_string())?;
            }
            let bad = results
                .iter()
                .any(|r| r.issues.iter().any(|i| i.code == "nameservers_changed"));
            if bad {
                self.set("ns_bad", "1")?;
            } else if results.iter().all(|r| r.errors.is_empty()) {
                self.del("ns_bad")?;
            }
        }
        // RDAP (weekly; zone, delegated and external).
        if row.kind != "platform" {
            self.rdap(row, d, now).await?;
            if self.get("rdap_changed")?.is_some() {
                for r in &mut results {
                    r.issues.push(Issue {
                        code: "registration_changed".into(),
                        level: "ownership".into(),
                        record: None,
                    });
                }
            }
        }
        for (i, r) in results.iter().enumerate() {
            self.sql.exec(
                "INSERT INTO checks (resolver, outcome, results_json, at) VALUES (?1, ?2, ?3, ?4)",
                &[
                    (if i == 0 { "first" } else { "second" }).into(),
                    r.outcome().into(),
                    serde_json::to_string(r).unwrap_or_default().into(),
                    now.into(),
                ],
            )?;
        }
        self.sql.exec(
            "DELETE FROM checks WHERE id <= (SELECT MAX(id) FROM checks) - ?1",
            &[CHECKS_KEPT.into()],
        )?;
        let outcomes = [outcome_of(&results[0]), outcome_of(&results[1])];
        let mut check = Check::of(outcomes);
        if d.state == State::Suspended {
            check.new_ownership_seen = results.iter().all(|r| {
                r.errors.is_empty()
                    && !r
                        .issues
                        .iter()
                        .any(|i| i.code == "ownership_record_missing")
            });
        }
        if domain_fsm::cd1_due(d, outcomes, now) {
            let names: Vec<(String, String)> =
                results.iter().flat_map(|r| r.errors.clone()).collect();
            let mut bogus = false;
            for (n, _) in &names {
                for r in [Resolver::First, Resolver::Second] {
                    if self
                        .p
                        .dns()
                        .query_cd(r, n, RType::Txt)
                        .await
                        .is_ok_and(|a| {
                            matches!(
                                a.status,
                                pylota_identity_platform::dns::DnsStatus::NoError
                                    | pylota_identity_platform::dns::DnsStatus::NxDomain
                            )
                        })
                    {
                        bogus = true;
                    }
                }
            }
            check.cd1 = Some(if bogus {
                DnsIssue::DnssecBogus
            } else {
                DnsIssue::DnsUnresolvable
            });
        }
        Ok((results, check))
    }

    async fn zone_name(&self, row: &ddb::Row) -> String {
        if let Some(z) = self.get("zone_name").ok().flatten() {
            return z;
        }
        let name = match (Cf::new(self.p), &row.zone_id) {
            (Some(cf), Some(z)) => cf.get_zone(z).await.ok().map(|z| z.name),
            _ => None,
        }
        .unwrap_or_else(|| org_domain(&row.name).to_owned());
        let _ = self.set("zone_name", &name);
        name
    }

    /// The weekly RDAP row: a change counts once two queries an hour apart agree.
    async fn rdap(&self, row: &ddb::Row, d: &Domain, now: i64) -> Result<(), ApiError> {
        let next = self
            .get("rdap_next_at")?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        if now < next {
            return Ok(());
        }
        let Some(fp) = self.fetch_rdap(&row.name).await else {
            self.set("rdap_next_at", &(now + WEEK).to_string())?;
            return Ok(());
        };
        let stored: Option<String> = self
            .p
            .db()
            .first::<Kv>(Stmt::new(
                "SELECT COALESCE(rdap_fingerprint, '') AS v FROM domains WHERE id = ?1",
                vec![row.id.clone().into()],
            ))
            .await?
            .map(|r| r.v)
            .filter(|v| !v.is_empty());
        match stored {
            None if matches!(d.state, State::Healthy | State::Degraded) => {
                self.p
                    .db()
                    .execute(Stmt::new(
                        "UPDATE domains SET rdap_fingerprint = ?2 WHERE id = ?1",
                        vec![row.id.clone().into(), fp.into()],
                    ))
                    .await?;
                self.set("rdap_next_at", &(now + WEEK).to_string())?;
            }
            Some(s) if s != fp => {
                let pending = self.get("rdap_pending")?;
                if pending.as_deref() == Some(fp.as_str()) {
                    self.set("rdap_changed", "1")?;
                    self.del("rdap_pending")?;
                    self.set("rdap_next_at", &(now + WEEK).to_string())?;
                } else {
                    self.set("rdap_pending", &fp)?;
                    self.set("rdap_next_at", &(now + HOUR).to_string())?;
                }
            }
            _ => {
                self.del("rdap_pending")?;
                self.set("rdap_next_at", &(now + WEEK).to_string())?;
            }
        }
        Ok(())
    }

    async fn fetch_rdap(&self, name: &str) -> Option<String> {
        let get = |url: String| {
            let mut r = HttpRequest::get(url);
            r.max_body_bytes = 256 * 1024;
            r
        };
        let boot = self
            .p
            .http()
            .send(get("https://data.iana.org/rdap/dns.json".into()))
            .await
            .ok()?;
        let b: Value = serde_json::from_slice(&boot.body).ok()?;
        let registrable = org_domain(name).to_owned();
        let tld = registrable.rsplit('.').next()?.to_owned();
        let base = b["services"].as_array()?.iter().find_map(|s| {
            s[0].as_array()?
                .iter()
                .any(|t| t.as_str() == Some(tld.as_str()))
                .then(|| s[1][0].as_str().map(str::to_owned))
                .flatten()
        })?;
        let r = self
            .p
            .http()
            .send(get(format!("{base}domain/{registrable}")))
            .await
            .ok()?;
        if r.status != 200 {
            return None;
        }
        rdap_fingerprint(&serde_json::from_slice(&r.body).ok()?)
    }

    async fn apply(&self, row: &ddb::Row, d: &Domain, s: &Step, now: i64) -> Result<(), ApiError> {
        let issues: Vec<Issue> = self
            .get("issues")?
            .and_then(|i| serde_json::from_str(&i).ok())
            .unwrap_or_default();
        self.apply_with_issues(row, d, s, now, &issues).await
    }

    /// The step's D1 update, actions and events.
    async fn apply_with_issues(
        &self,
        row: &ddb::Row,
        d: &Domain,
        s: &Step,
        now: i64,
        issues: &[Issue],
    ) -> Result<(), ApiError> {
        let p = self.p;
        let changed = s.to != s.from;
        let reason: Option<String> = s.reason.map(str::to_owned).or_else(|| {
            (s.to != State::Healthy)
                .then(|| issues.first().map(|i| i.code.clone()))
                .flatten()
        });
        p.db()
            .execute(Stmt::new(
                "UPDATE domains SET state = ?2, state_reason = ?3, state_changed_at = CASE WHEN ?4 THEN ?5 ELSE state_changed_at END, ownership_verified_at = COALESCE(ownership_verified_at, ?6), updated_at = ?5 WHERE id = ?1",
                vec![
                    row.id.clone().into(),
                    state_str(s.to).into(),
                    (if s.to == State::Healthy && changed { None } else { reason.clone() }).into(),
                    i64::from(changed).into(),
                    now.into(),
                    d.ownership_verified_at_ms.into(),
                ],
            ))
            .await?;
        for a in &s.actions {
            match a {
                Action::CheckNow => {
                    self.set("check_at", &now.to_string())?;
                    self.state.set_alarm(now).await?;
                }
                Action::ActivateAddresses => self.activate_addresses(row, now).await?,
                Action::NewOwnershipToken => self.new_token(row).await?,
                Action::UpdateRecords | Action::AuditEvict | Action::RequeryCd1 => {}
                Action::RerecordBaselines => {
                    self.rerecord(row).await?;
                    self.del("rdap_changed")?;
                    self.del("ns_bad")?;
                }
                Action::RereadZone => self.rerecord(row).await?,
                Action::StorePendingRules => {}
                Action::DeletePendingAddresses => {}
                Action::CreateRemoveJob(reason) => {
                    let pending: Vec<(String, String)> = p
                        .db()
                        .query::<PendingRule>(Stmt::new(
                            "SELECT COALESCE(d.zone_id, '') AS zone_id, a.routing_rule_id FROM addresses a JOIN domains d ON d.id = a.domain_id WHERE a.domain_id = ?1 AND a.status = 'pending' AND a.routing_rule_id IS NOT NULL",
                            vec![row.id.clone().into()],
                        ))
                        .await?
                        .into_iter()
                        .map(|r| (r.zone_id, r.routing_rule_id))
                        .collect();
                    p.db()
                        .batch(vec![
                            super::remove_job(
                                p,
                                &row.id,
                                row.tenant_id.as_deref(),
                                reason.code(),
                                &pending,
                                now,
                            )?,
                            Stmt::new(
                                "DELETE FROM addresses WHERE domain_id = ?1 AND status = 'pending'",
                                vec![row.id.clone().into()],
                            ),
                        ])
                        .await?;
                    crate::jobs::domain_remove::start_for(p, &row.id).await;
                }
                Action::RestartRemoveJob => {
                    let reason = row
                        .state_reason
                        .clone()
                        .unwrap_or_else(|| "requested".into());
                    p.db()
                        .execute(super::remove_job(
                            p,
                            &row.id,
                            row.tenant_id.as_deref(),
                            &reason,
                            &[],
                            now,
                        )?)
                        .await?;
                    crate::jobs::domain_remove::start_for(p, &row.id).await;
                }
                Action::Alert(name) => {
                    crate::log::warn(
                        "alert",
                        &[
                            ("alert", (*name).into()),
                            ("domain_id", row.id.clone().into()),
                        ],
                    );
                }
            }
        }
        if s.events.is_empty() {
            return Ok(());
        }
        let fresh = self.row(&row.id).await?;
        let issues_json: Vec<Value> = issues
            .iter()
            .map(|i| json!({ "code": i.code, "record": i.record }))
            .collect();
        for e in &s.events {
            let (kind, extra) = match e {
                Event::Verified => ("domain.verified", json!({})),
                Event::Degraded => ("domain.degraded", json!({ "issues": issues_json })),
                Event::Failing => (
                    "domain.failing",
                    json!({ "issues": issues_json, "fallback_active": fresh.kind != "platform" }),
                ),
                Event::Recovered { from } => (
                    "domain.recovered",
                    json!({ "from_state": state_str(*from) }),
                ),
                Event::Suspended { reason } => ("domain.suspended", json!({ "reason": reason })),
                Event::Removed { reason } => ("domain.removed", json!({ "reason": reason })),
                Event::Reminder(_) => (
                    "domain.reminder",
                    json!({ "hours_in_state": (now - d.state_changed_at_ms) / HOUR }),
                ),
            };
            let mut data = super::domain_data(&fresh);
            if let (Some(m), Some(x)) = (data.as_object_mut(), extra.as_object()) {
                m.extend(x.clone());
            }
            super::emit(
                p,
                &fresh,
                kind,
                &data,
                &format!("{}:{now}", d.state_changed_at_ms),
                now,
            )
            .await?;
        }
        Ok(())
    }

    /// Activates the domain's routed `pending` addresses, with `identity.address_activated`.
    async fn activate_addresses(&self, row: &ddb::Row, now: i64) -> Result<(), ApiError> {
        let p = self.p;
        let rows: Vec<crate::db::addresses::Row> = p
            .db()
            .query(Stmt::new(
                "SELECT id, address, local_part, domain_id, tenant_id, identity_id, role, status, routing_rule_id, retire_at, retired_at, ses_bounce_rule, forwarding, forwarding_checked_at, created_at FROM addresses WHERE domain_id = ?1 AND status = 'pending' AND (?2 <> 'literal' OR routing_rule_id IS NOT NULL)",
                vec![row.id.clone().into(), row.routing_mode.clone().into()],
            ))
            .await?;
        for a in rows {
            p.db()
                .execute(Stmt::new(
                    "UPDATE addresses SET status = 'active', updated_at = ?2 WHERE id = ?1 AND status = 'pending'",
                    vec![a.id.clone().into(), now.into()],
                ))
                .await?;
            crate::handlers::addresses::emit_identity_event(
                p,
                &a.tenant_id,
                &a.identity_id,
                "identity.address_activated",
                &json!({ "identity_id": a.identity_id, "address": a.json() }),
                &format!("activated:{}", a.id),
                now,
            )
            .await;
        }
        Ok(())
    }

    async fn new_token(&self, row: &ddb::Row) -> Result<(), ApiError> {
        let token = super::ownership_token(self.p);
        let mut recs = records::parse(&row.records_json);
        recs.retain(|r| r.purpose != "ownership");
        recs.push(records::ownership(&row.name, &token));
        // The Worker publishes its own TXT where it can.
        if let (Some(cf), Some(z)) = (Cf::new(self.p), &row.zone_id) {
            let _ = cf
                .create_record(
                    z,
                    "TXT",
                    &records::ownership_name(&row.name),
                    &format!("pm-verify={token}"),
                )
                .await;
        }
        self.p
            .db()
            .execute(Stmt::new(
                "UPDATE domains SET ownership_token = ?2, records_json = ?3 WHERE id = ?1",
                vec![
                    row.id.clone().into(),
                    token.into(),
                    serde_json::to_string(&recs).unwrap_or_default().into(),
                ],
            ))
            .await?;
        Ok(())
    }

    /// Re-records `expected_ns_json` from the zone, and clears the RDAP baseline so the next query
    /// records it again.
    async fn rerecord(&self, row: &ddb::Row) -> Result<(), ApiError> {
        if let (Some(cf), Some(z)) = (Cf::new(self.p), &row.zone_id)
            && let Ok(zone) = cf.get_zone(z).await
            && zone.status == "active"
        {
            self.p
                .db()
                .execute(Stmt::new(
                    "UPDATE domains SET expected_ns_json = ?2, rdap_fingerprint = NULL WHERE id = ?1",
                    vec![row.id.clone().into(), json!(zone.name_servers).to_string().into()],
                ))
                .await?;
        }
        self.del("rdap_next_at")?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct PendingRule {
    zone_id: String,
    routing_rule_id: String,
}

/// One request to a domain's monitor.
pub async fn call<P: Platform>(
    p: &P,
    monitor_do_id: &str,
    req: DomainRequest,
) -> Result<Value, ApiError> {
    let env = envelope_for(p, None, None, &crate::request_id::new(p), None, req);
    match p
        .objects()
        .call::<_, Value>(DoClass::Domain, monitor_do_id, &env)
        .await
    {
        Ok(RpcResult::Ok(v)) => Ok(v),
        Ok(RpcResult::Err(e)) => Err(ApiError::new(e.error.code, e.error.message)),
        Err(_) => Err(ApiError::new(
            ErrorCode::Unavailable,
            "The domain's monitor did not answer; retry.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The daily SES checks of 1,000 domains fall at their hash offsets and, through the bucket, are
    /// granted at most one per second, each within its background wait (domain-connections § 4.8).
    #[test]
    fn ses_daily_checks_spread() {
        use pylota_identity_platform::fakes::{FakePlatform, FakeSql};

        use crate::domains::ses_control::{BACKGROUND_WAIT_MS, SesControl, TOKEN_MS};
        let p = FakePlatform::new();
        let sql = FakeSql::new();
        let c = SesControl { p: &p, sql: &sql };
        let day = p.clock().now_ms().div_euclid(86_400_000) * 86_400_000 + 86_400_000;
        let mut due: Vec<i64> = (0..1000)
            .map(|i| {
                let id = format!("dom_{i:026}");
                let at = next_ses_check(&id, day - 1);
                assert_eq!(at, day + ses_check_offset_s(&id) * 1000);
                at
            })
            .collect();
        due.sort_unstable();
        // Spread across the day, not bunched at midnight.
        assert!(due.iter().filter(|&&d| d < day + 3_600_000).count() < 100);
        let mut last = day - TOKEN_MS;
        for at in due {
            if p.clock().now_ms() < at {
                p.clock.set(at);
            }
            let now = p.clock().now_ms();
            let v = c.handle(&json!({ "v": 1, "tenant_id": null, "identity_id": null, "request_id": "r",
                "actor_key_id": null, "deadline_ms": i64::MAX,
                "req": { "op": "acquire", "caller": "domain_check", "deadline_ms": now + BACKGROUND_WAIT_MS } }));
            let granted = v["ok"]["at_ms"].as_i64().expect("granted within the wait");
            assert!(granted >= at && granted - at <= BACKGROUND_WAIT_MS);
            assert!(granted - last >= TOKEN_MS);
            last = granted;
        }
    }

    #[test]
    fn rdap_fingerprint_of_roles() {
        let v = json!({ "entities": [
            { "handle": "REG-1", "roles": ["registrar"] }, { "handle": "ME", "roles": ["registrant"] } ],
            "events": [{ "eventAction": "registration", "eventDate": "2020-01-01T00:00:00Z" }] });
        let a = rdap_fingerprint(&v).unwrap();
        let mut w = v.clone();
        w["entities"][0]["handle"] = "REG-2".into();
        assert_ne!(a, rdap_fingerprint(&w).unwrap());
        assert!(rdap_fingerprint(&json!({ "entities": [] })).is_none());
    }
}
