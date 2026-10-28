//! `TenantQuota`: one object per tenant ([Outbound › TenantQuota], [Plans, metering and billing]). M5
//! declares the whole `QuotaRequest` interface and answers every variant as the build plan's stub table
//! says, so a milestone calling a variant before the one that owns it gets a well-formed answer; later
//! milestones replace behaviour, never a signature. (`GetRates` and `RecordOutcome.thresholds` arrive
//! with M35.)
//!
//! [Outbound › TenantQuota]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md
//! [Plans, metering and billing]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/billing.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{Clock as _, Ids as _, Platform, PlatformError, Sql};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::errors::ApiError;
use crate::rpc::{RpcEnvelope, check_owner, error_body};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageMetric {
    Inbound,
    Outbound,
    Search,
    AiNeurons,
    Assertions,
    HttpSignatures,
    CfRecipients,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    Inboxes,
    Sends,
    Triage,
    CustomDomains,
    StorageGb,
    Seats,
}

impl Feature {
    pub const ALL: [Feature; 6] = [
        Feature::Inboxes,
        Feature::Sends,
        Feature::Triage,
        Feature::CustomDomains,
        Feature::StorageGb,
        Feature::Seats,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Feature::Inboxes => "inboxes",
            Feature::Sends => "sends",
            Feature::Triage => "triage",
            Feature::CustomDomains => "custom_domains",
            Feature::StorageGb => "storage_gb",
            Feature::Seats => "seats",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingMode {
    Metered,
    Exempt,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Delivered,
    Bounced,
    Complained,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapScope {
    Identity,
    Tenant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendsHold {
    pub units: u32,
    pub r#ref: String,
    pub gates: Vec<Feature>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendStop {
    BillingDispute,
    SharedDomain { resets_at: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemMailClass {
    Signin,
    Invitation,
    Notification,
    Account,
    Signup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapReachedScope {
    Identity,
    Tenant,
    SharedDomain,
    BillingDispute,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allowances {
    pub inboxes: Option<u32>,
    pub sends: Option<u32>,
    pub triage: Option<u32>,
    pub custom_domains: Option<u32>,
    pub storage_gb: Option<u32>,
    pub seats: Option<u32>,
}

/// Every request of `TenantQuota`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum QuotaRequest {
    Init {
        tenant_id: String,
    },
    Reserve {
        identity_id: String,
        tz: String,
        identity_cap: u32,
        tenant_cap: Option<u32>,
        hold: Option<SendsHold>,
        stop: Option<SendStop>,
    },
    SystemMail {
        class: SystemMailClass,
        keys: Vec<String>,
        day: String,
    },
    DeploymentReserve {
        recipients: u32,
        restricted: bool,
        quota: u32,
    },
    DeploymentStatus {
        quota: u32,
    },
    Release {
        identity_id: String,
        day: String,
        tenant_counted: bool,
    },
    RecordOutcome {
        identity_id: String,
        domain_id: Option<String>,
        outcome: Outcome,
        at: i64,
        event_id: String,
        counted: bool,
    },
    OutcomeRates {
        since: i64,
    },
    CountAgentic {
        tz: String,
        cap: u32,
    },
    CountTriageModel {
        tz: String,
        cap: u32,
        r#ref: String,
    },
    CountScreenModel {
        tz: String,
        cap: u32,
        sender: String,
        known: bool,
        r#ref: String,
    },
    CountScreenFailure {
        at: i64,
    },
    CountSignupOutcome {
        outcome: Outcome,
        at: i64,
        event_id: String,
    },
    AdmitInbound {
        identity_id: String,
        bucket: String,
        at: i64,
        cap: u32,
    },
    RecordUsage {
        metric: UsageMetric,
        n: u64,
    },
    ForgetIdentity {
        identity_id: String,
    },
    Hold {
        feature: Feature,
        units: u32,
        r#ref: String,
        gates: Vec<Feature>,
    },
    Settle {
        feature: Feature,
        r#ref: String,
        consume: u32,
        keep: u32,
    },
    Extend {
        feature: Feature,
        r#ref: String,
        until: i64,
    },
    Adjust {
        feature: Feature,
        delta: i64,
        r#ref: String,
    },
    SetPlan {
        mode: BillingMode,
        granted: Allowances,
        period_start: i64,
        period_end: i64,
        catalog_hash: String,
    },
    SetMeasured {
        storage_bytes: u64,
    },
    Reconcile {
        inboxes: u32,
        custom_domains: u32,
        seats: u32,
        read_at: i64,
    },
    GetUsage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaWarning {
    pub scope: CapScope,
    pub threshold: u8,
    pub used: u32,
    pub cap: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub hold_id: String,
    pub remaining: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Denied {
    pub feature: Feature,
    pub granted: u32,
    pub used: u32,
    pub resets_at: Option<i64>,
    pub first_in_period: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReserveAnswer {
    Ok {
        day: String,
        identity_used: u32,
        tenant_used: u32,
        warnings: Vec<QuotaWarning>,
        held: Option<Held>,
    },
    Denied(Denied),
    CapReached {
        scope: CapReachedScope,
        resets_at: Option<i64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseCause {
    AbuseThreshold,
    AbuseBurst,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeDecision {
    pub pause_identity: bool,
    pub pause_tenant: Option<PauseCause>,
    pub pause_domain: bool,
    pub bounce_alert: bool,
    pub complaint_alert: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemMailAnswer {
    Ok,
    Spent { scope: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoldAnswer {
    Held(Held),
    Denied(Denied),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CountAgenticAnswer {
    Ok { used: u32 },
    CapReached { resets_at: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CountAnswer {
    Ok,
    CapReached,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdmitAnswer {
    Admit,
    Throttle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordUsageAnswer {
    pub day: String,
    pub total: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeRatesAnswer {
    pub outcomes: u64,
    pub bounced: u64,
    pub complained: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowanceRow {
    pub feature: Feature,
    pub granted: Option<u32>,
    pub used: u32,
    pub held: u32,
    pub remaining: Option<u32>,
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageAnswer {
    pub mode: BillingMode,
    pub period_start: Option<i64>,
    pub period_end: Option<i64>,
    pub allowances: Vec<AllowanceRow>,
}

/// The object's schema (Data model § 3), applied on wake under `meta.schema_version`.
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS counters (metric TEXT NOT NULL, window TEXT NOT NULL, value INTEGER NOT NULL, PRIMARY KEY (metric, window));
CREATE TABLE IF NOT EXISTS outcome_events (event_id TEXT PRIMARY KEY, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS allowances (feature TEXT PRIMARY KEY CHECK (feature IN ('inboxes','sends','triage','custom_domains','storage_gb','seats')), granted INTEGER, used INTEGER NOT NULL DEFAULT 0, held INTEGER NOT NULL DEFAULT 0, resets_at INTEGER);
CREATE TABLE IF NOT EXISTS holds (id TEXT PRIMARY KEY, feature TEXT NOT NULL, units INTEGER NOT NULL, ref TEXT NOT NULL, expires_at INTEGER NOT NULL, UNIQUE (feature, ref));
CREATE TABLE IF NOT EXISTS outcomes (identity_id TEXT NOT NULL, seq INTEGER NOT NULL, outcome TEXT NOT NULL CHECK (outcome IN ('delivered','bounced','complained','other')), at INTEGER NOT NULL, PRIMARY KEY (identity_id, seq));
INSERT OR IGNORE INTO meta (k, v) VALUES ('schema_version', '1');";

/// The default abuse thresholds of tenant policy: identity complaint and bounce rates, tenant
/// complaint and bounce rates (Configuration › Tenant policy).
const ABUSE: (f64, f64, f64, f64) = (0.003, 0.05, 0.001, 0.05);

/// One request to a tenant's `TenantQuota`; an object that does not answer is `503 unavailable`.
pub async fn call<P: Platform>(
    p: &P,
    quota_do_id: &str,
    tenant_id: &str,
    request_id: &str,
    req: QuotaRequest,
) -> Result<Value, ApiError> {
    use pylota_identity_platform::ObjectClient as _;
    use pylota_identity_platform::durable::DoClass;
    let env = crate::rpc::envelope_for(p, Some(tenant_id), None, request_id, None, req);
    match p
        .objects()
        .call::<_, Value>(DoClass::Quota, quota_do_id, &env)
        .await
    {
        Ok(crate::rpc::RpcResult::Ok(v)) => Ok(v),
        _ => Err(ApiError::new(
            ErrorCode::Unavailable,
            "The tenant's quota did not answer; retry.",
        )),
    }
}

/// How long a stub hold lives before the M22 expiry would release it.
const HOLD_MS: i64 = 86_400_000;

/// The logic of one `TenantQuota`, over its SQL storage.
pub struct Quota<'a, P: Platform, S: Sql> {
    pub p: &'a P,
    pub sql: &'a S,
}

#[derive(Deserialize)]
struct V {
    v: String,
}

#[derive(Deserialize)]
struct N {
    n: i64,
}

impl<P: Platform, S: Sql> Quota<'_, P, S> {
    /// Applies the schema; idempotent.
    pub fn migrate(&self) -> Result<(), PlatformError> {
        self.sql
            .transaction_sync(|s| s.exec(SCHEMA, &[]).map(|_| ()))
    }

    fn meta(&self, k: &str) -> Result<Option<String>, PlatformError> {
        Ok(self
            .sql
            .first::<V>("SELECT v FROM meta WHERE k = ?1", &[k.into()])?
            .map(|r| r.v))
    }

    fn counter(&self, metric: &str, window: &str) -> Result<i64, PlatformError> {
        Ok(self
            .sql
            .first::<N>(
                "SELECT value AS n FROM counters WHERE metric = ?1 AND window = ?2",
                &[metric.into(), window.into()],
            )?
            .map_or(0, |r| r.n))
    }

    fn add(s: &S, metric: &str, window: &str, delta: i64) -> Result<(), PlatformError> {
        s.exec(
            "INSERT INTO counters (metric, window, value) VALUES (?1, ?2, MAX(?3, 0)) ON CONFLICT (metric, window) DO UPDATE SET value = MAX(value + ?3, 0)",
            &[metric.into(), window.into(), delta.into()],
        )?;
        Ok(())
    }

    fn hold(
        &self,
        s: &S,
        feature: Feature,
        units: u32,
        r#ref: &str,
    ) -> Result<Held, PlatformError> {
        let id = self.p.ids().new_id(IdPrefix::Hold);
        let expires = self.p.clock().now_ms() + HOLD_MS;
        let inserted = s.exec(
            "INSERT OR IGNORE INTO holds (id, feature, units, ref, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            &[id.clone().into(), feature.as_str().into(), units.into(), r#ref.into(), expires.into()],
        )?;
        if inserted.changes > 0 {
            s.exec(
                "INSERT INTO allowances (feature, granted, used, held) VALUES (?1, NULL, 0, ?2) ON CONFLICT (feature) DO UPDATE SET held = held + ?2",
                &[feature.as_str().into(), units.into()],
            )?;
            return Ok(Held {
                hold_id: id,
                remaining: None,
            });
        }
        let existing: Option<V> = s.first(
            "SELECT id AS v FROM holds WHERE feature = ?1 AND ref = ?2",
            &[feature.as_str().into(), r#ref.into()],
        )?;
        Ok(Held {
            hold_id: existing.map_or(id, |r| r.v),
            remaining: None,
        })
    }

    fn counter_in(s: &S, metric: &str, window: &str) -> Result<i64, PlatformError> {
        Ok(s.first::<N>(
            "SELECT value AS n FROM counters WHERE metric = ?1 AND window = ?2",
            &[metric.into(), window.into()],
        )?
        .map_or(0, |r| r.n))
    }

    fn sum_since(s: &S, metric: &str, from_window: &str) -> Result<i64, PlatformError> {
        Ok(s.first::<N>(
            "SELECT COALESCE(SUM(value), 0) AS n FROM counters WHERE metric = ?1 AND window >= ?2",
            &[metric.into(), from_window.into()],
        )?
        .map_or(0, |r| r.n))
    }

    /// The day of the daily caps (Day boundaries): `meta.tz`, switching to `tz_next` at its `from`; a
    /// request with another zone records `tz_next` for the next local midnight of the current zone.
    fn cap_day(&self, s: &S, now: i64, tz: &str) -> Result<(String, String), PlatformError> {
        let get = |k: &str| -> Result<Option<String>, PlatformError> {
            Ok(
                s.first::<V>("SELECT v FROM meta WHERE k = ?1", &[k.into()])?
                    .map(|r| r.v),
            )
        };
        let set = |k: &str, v: &str| {
            s.exec(
                "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
                &[k.into(), v.into()],
            )
        };
        let mut zone = match get("tz")? {
            Some(z) => z,
            None => {
                set("tz", tz)?;
                tz.to_owned()
            }
        };
        if let Some(next) = get("tz_next")?
            && let Some((z, from)) = next.split_once('|')
            && from.parse::<i64>().is_ok_and(|f| now >= f)
        {
            set("tz", z)?;
            s.exec("DELETE FROM meta WHERE k = 'tz_next'", &[])?;
            zone = z.to_owned();
        }
        if tz != zone {
            let pending = get("tz_next")?;
            if pending
                .as_deref()
                .and_then(|p| p.split_once('|'))
                .map(|(z, _)| z)
                != Some(tz)
            {
                let from = days::next_local_midnight(now, &zone);
                set("tz_next", &format!("{tz}|{from}"))?;
            }
        }
        Ok((days::local_day(now, &zone), zone))
    }

    /// A `quota.warning` the first time `used` reaches 80% and 100% of `cap` on `day`.
    fn warn(
        s: &S,
        out: &mut Vec<QuotaWarning>,
        scope: CapScope,
        metric: &str,
        day: &str,
        used: i64,
        cap: u32,
    ) -> Result<(), PlatformError> {
        for threshold in [80u8, 100] {
            if used * 100 >= i64::from(cap) * i64::from(threshold) {
                let first = s.exec(
                    "INSERT OR IGNORE INTO counters (metric, window, value) VALUES (?1, ?2, 1)",
                    &[format!("warned:{metric}:{threshold}").into(), day.into()],
                )?;
                if first.changes > 0 {
                    out.push(QuotaWarning {
                        scope,
                        threshold,
                        used: used as u32,
                        cap,
                    });
                }
            }
        }
        Ok(())
    }

    /// The abuse rules (Outbound › Abuse auto-pause, G12, Per-domain outcome counters), with the
    /// default thresholds of tenant policy until `RecordOutcome.thresholds` arrives (M35).
    fn evaluate(
        &self,
        s: &S,
        identity_id: &str,
        domain_id: Option<&str>,
        now: i64,
    ) -> Result<OutcomeDecision, PlatformError> {
        let count = |sql: &str| -> Result<i64, PlatformError> {
            Ok(s.first::<N>(sql, &[identity_id.into()])?.map_or(0, |r| r.n))
        };
        let complaints = count(
            "SELECT COUNT(*) AS n FROM (SELECT outcome FROM outcomes WHERE identity_id = ?1 ORDER BY seq DESC LIMIT 1000) WHERE outcome = 'complained'",
        )?;
        let bounces = count(
            "SELECT COUNT(*) AS n FROM (SELECT outcome FROM outcomes WHERE identity_id = ?1 ORDER BY seq DESC LIMIT 200) WHERE outcome = 'bounced'",
        )?;
        let mut d = OutcomeDecision {
            pause_identity: complaints as f64 / 1000.0 > ABUSE.0
                || bounces as f64 / 200.0 > ABUSE.1,
            ..OutcomeDecision::default()
        };
        let week = days::utc_day(now - 6 * 86_400_000);
        let rates = |outcomes: i64, bounced: i64, complained: i64| {
            outcomes >= 500
                && (complained as f64 / outcomes as f64 >= ABUSE.2
                    || bounced as f64 / outcomes as f64 >= ABUSE.3)
        };
        if rates(
            Self::sum_since(s, "outcomes", &week)?,
            Self::sum_since(s, "bounced", &week)?,
            Self::sum_since(s, "complained", &week)?,
        ) {
            d.pause_tenant = Some(PauseCause::AbuseThreshold);
        }
        if let Some(dom) = domain_id {
            let m = |k: &str| format!("{k}:dom:{dom}");
            let week_h = format!("{week}T00");
            d.pause_domain = rates(
                Self::sum_since(s, &m("outcomes"), &week_h)?,
                Self::sum_since(s, &m("bounced"), &week_h)?,
                Self::sum_since(s, &m("complained"), &week_h)?,
            );
            let hour = days::utc_hour(now);
            let day_h = days::utc_hour(now - 23 * 3_600_000);
            let (ho, hb) = (
                Self::counter_in(s, &m("outcomes"), &hour)?,
                Self::counter_in(s, &m("bounced"), &hour)?,
            );
            let (dayo, dayc) = (
                Self::sum_since(s, &m("outcomes"), &day_h)?,
                Self::sum_since(s, &m("complained"), &day_h)?,
            );
            d.bounce_alert = ho >= 50 && hb as f64 / ho as f64 > 0.02;
            d.complaint_alert = dayo >= 200 && dayc as f64 / dayo as f64 > 0.001;
            if d.pause_tenant.is_none()
                && ((ho >= 200 && hb as f64 / ho as f64 > 0.10)
                    || (dayo >= 1000 && dayc as f64 / dayo as f64 > 0.006))
            {
                d.pause_tenant = Some(PauseCause::AbuseBurst);
            }
        }
        Ok(d)
    }

    /// Answers one envelope (already decoded as JSON); never panics on a bad body.
    pub fn handle(&self, envelope: &Value) -> Value {
        let request_id = envelope["request_id"].as_str().unwrap_or("").to_owned();
        let docs = self.p.config().docs_url.clone();
        let fail = |e: ApiError| {
            serde_json::to_value(error_body::<()>(&e, &request_id, &docs)).unwrap_or(Value::Null)
        };
        let env: RpcEnvelope<QuotaRequest> = match serde_json::from_value(envelope.clone()) {
            Ok(e) => e,
            Err(_) => {
                return fail(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "Unknown quota request.",
                ));
            }
        };
        if env.deadline_ms < self.p.clock().now_ms() {
            return fail(ApiError::new(
                ErrorCode::Timeout,
                "The request's deadline passed.",
            ));
        }
        match self.run(env) {
            Ok(v) => json!({ "ok": v }),
            Err(e) => fail(e),
        }
    }

    fn run(&self, env: RpcEnvelope<QuotaRequest>) -> Result<Value, ApiError> {
        self.migrate()?;
        let owner = self.meta("tenant_id")?;
        let sent = env.tenant_id.as_deref();
        match (&owner, &env.req) {
            (None, QuotaRequest::Init { tenant_id }) => {
                if sent != Some(tenant_id.as_str()) {
                    check_owner(self.p, Some(tenant_id), sent, "TenantQuota")?;
                }
                self.sql.exec(
                    "INSERT OR IGNORE INTO meta (k, v) VALUES ('tenant_id', ?1)",
                    &[tenant_id.clone().into()],
                )?;
                return Ok(Value::Null);
            }
            (None, _) => {
                return Err(ApiError::new(
                    ErrorCode::InternalError,
                    "The object has no owner yet.",
                ));
            }
            (Some(o), _) => check_owner(self.p, Some(o), sent, "TenantQuota")?,
        }
        let now = self.p.clock().now_ms();
        let utc = days::utc_day(now);
        let to_v = |v: &dyn erased::Ser| v.json();
        let out: Value = match env.req {
            QuotaRequest::Init { .. } => Value::Null, // the owner matched: a repeated Init
            QuotaRequest::Reserve {
                identity_id,
                tz,
                identity_cap,
                tenant_cap,
                hold,
                stop,
            } => {
                if let Some(stop) = stop {
                    return Ok(to_v(&match stop {
                        SendStop::BillingDispute => ReserveAnswer::CapReached {
                            scope: CapReachedScope::BillingDispute,
                            resets_at: None,
                        },
                        SendStop::SharedDomain { resets_at } => ReserveAnswer::CapReached {
                            scope: CapReachedScope::SharedDomain,
                            resets_at: Some(resets_at),
                        },
                    }));
                }
                let answer = self
                    .sql
                    .transaction_sync(|s| -> Result<ReserveAnswer, ApiError> {
                        let (day, zone) = self.cap_day(s, now, &tz)?;
                        let resets_at = Some(days::next_local_midnight(now, &zone));
                        let ikey = format!("sends:{identity_id}");
                        let iused = Self::counter_in(s, &ikey, &day)?;
                        let tused = Self::counter_in(s, "sends", &day)?;
                        if iused + 1 > i64::from(identity_cap) {
                            return Ok(ReserveAnswer::CapReached {
                                scope: CapReachedScope::Identity,
                                resets_at,
                            });
                        }
                        if let Some(cap) = tenant_cap
                            && tused + 1 > i64::from(cap)
                        {
                            return Ok(ReserveAnswer::CapReached {
                                scope: CapReachedScope::Tenant,
                                resets_at,
                            });
                        }
                        let held = match &hold {
                            Some(h) => Some(self.hold(s, Feature::Sends, h.units, &h.r#ref)?),
                            None => None,
                        };
                        Self::add(s, &ikey, &day, 1)?;
                        let mut warnings = Vec::new();
                        Self::warn(
                            s,
                            &mut warnings,
                            CapScope::Identity,
                            &ikey,
                            &day,
                            iused + 1,
                            identity_cap,
                        )?;
                        if let Some(cap) = tenant_cap {
                            Self::add(s, "sends", &day, 1)?;
                            Self::warn(
                                s,
                                &mut warnings,
                                CapScope::Tenant,
                                "sends",
                                &day,
                                tused + 1,
                                cap,
                            )?;
                        }
                        Ok(ReserveAnswer::Ok {
                            identity_used: (iused + 1) as u32,
                            tenant_used: (tused + i64::from(tenant_cap.is_some())) as u32,
                            day,
                            warnings,
                            held,
                        })
                    })?;
                to_v(&answer)
            }
            QuotaRequest::Release {
                identity_id,
                day,
                tenant_counted,
            } => {
                self.sql
                    .transaction_sync(|s| -> Result<(), PlatformError> {
                        Self::add(s, &format!("sends:{identity_id}"), &day, -1)?;
                        if tenant_counted {
                            Self::add(s, "sends", &day, -1)?;
                        }
                        Ok(())
                    })?;
                Value::Null
            }
            QuotaRequest::RecordOutcome {
                identity_id,
                domain_id,
                outcome,
                at,
                event_id,
                counted,
            } => {
                if !counted {
                    return Ok(to_v(&OutcomeDecision::default()));
                }
                let decision = self.sql.transaction_sync(|s| -> Result<OutcomeDecision, PlatformError> {
                    let new = s.exec(
                        "INSERT OR IGNORE INTO outcome_events (event_id, at) VALUES (?1, ?2)",
                        &[event_id.clone().into(), at.into()],
                    )?;
                    let outcome_s = match outcome {
                        Outcome::Delivered => "delivered",
                        Outcome::Bounced => "bounced",
                        Outcome::Complained => "complained",
                        Outcome::Other => "other",
                    };
                    if new.changes > 0 {
                        s.exec(
                            "INSERT INTO outcomes (identity_id, seq, outcome, at) VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM outcomes WHERE identity_id = ?1), ?2, ?3)",
                            &[identity_id.clone().into(), outcome_s.into(), at.into()],
                        )?;
                        s.exec(
                            "DELETE FROM outcomes WHERE identity_id = ?1 AND seq <= (SELECT MAX(seq) FROM outcomes WHERE identity_id = ?1) - 1000",
                            &[identity_id.clone().into()],
                        )?;
                        let day = days::utc_day(at);
                        Self::add(s, "outcomes", &day, 1)?;
                        if matches!(outcome, Outcome::Bounced | Outcome::Complained) {
                            Self::add(s, outcome_s, &day, 1)?;
                        }
                        if let Some(d) = &domain_id {
                            let hour = days::utc_hour(at);
                            Self::add(s, &format!("outcomes:dom:{d}"), &hour, 1)?;
                            if matches!(outcome, Outcome::Bounced | Outcome::Complained) {
                                Self::add(s, &format!("{outcome_s}:dom:{d}"), &hour, 1)?;
                            }
                            s.exec(
                                "DELETE FROM counters WHERE metric LIKE ?1 AND window < ?2",
                                &[format!("%:dom:{d}").into(), days::utc_hour(now - 8 * 86_400_000).into()],
                            )?;
                        }
                    }
                    self.evaluate(s, &identity_id, domain_id.as_deref(), now)
                })?;
                to_v(&decision)
            }
            // The per-tenant inbound cap (Inbound › Inbound volume caps, D13): one counter per UTC hour.
            QuotaRequest::AdmitInbound { at, cap, .. } => {
                let n = self.sql.transaction_sync(|s| -> Result<i64, PlatformError> {
                    Self::add(s, "inbound_hour", &days::utc_hour(at), 1)?;
                    let v: Option<N> = s.first(
                        "SELECT value AS n FROM counters WHERE metric = 'inbound_hour' AND window = ?1",
                        &[days::utc_hour(at).into()],
                    )?;
                    Ok(v.map_or(0, |v| v.n))
                })?;
                to_v(&if n > i64::from(cap) {
                    AdmitAnswer::Throttle
                } else {
                    AdmitAnswer::Admit
                })
            }
            QuotaRequest::CountTriageModel { tz, r#ref, .. } => {
                let day = days::local_day(now, &tz);
                self.sql
                    .transaction_sync(|s| -> Result<(), PlatformError> {
                        let first = s.exec(
                            "INSERT OR IGNORE INTO meta (k, v) VALUES (?1, ?2)",
                            &[format!("tm:{ref}").into(), day.clone().into()],
                        )?;
                        if first.changes > 0 {
                            Self::add(s, "triage_model", &day, 1)?;
                        }
                        Ok(())
                    })?;
                to_v(&CountAnswer::Ok)
            }
            QuotaRequest::CountScreenModel { tz, r#ref, .. } => {
                let day = days::local_day(now, &tz);
                self.sql
                    .transaction_sync(|s| -> Result<(), PlatformError> {
                        let first = s.exec(
                            "INSERT OR IGNORE INTO meta (k, v) VALUES (?1, ?2)",
                            &[format!("sm:{ref}").into(), day.clone().into()],
                        )?;
                        if first.changes > 0 {
                            Self::add(s, "screen_model", &day, 1)?;
                        }
                        Ok(())
                    })?;
                json!({ "kind": "ok", "used": self.counter("screen_model", &day)? })
            }
            QuotaRequest::CountScreenFailure { at } => {
                let hour = days::utc_hour(at);
                self.sql
                    .transaction_sync(|s| Self::add(s, "screen_fail", &hour, 1))?;
                json!({ "count": self.counter("screen_fail", &hour)? })
            }
            QuotaRequest::OutcomeRates { .. } => to_v(&OutcomeRatesAnswer::default()),
            QuotaRequest::CountSignupOutcome { .. } => to_v(&OutcomeRatesAnswer::default()),
            QuotaRequest::SystemMail { .. } => to_v(&SystemMailAnswer::Ok),
            QuotaRequest::DeploymentReserve { .. } => json!({ "kind": "ok" }),
            QuotaRequest::DeploymentStatus { .. } => json!({ "count": 0, "stage": "none" }),
            // One agentic search against the tenant's daily cap (Search § 11.2 Init): a spent cap
            // counts nothing and answers when it resets.
            QuotaRequest::CountAgentic { tz, cap } => {
                let answer = self.sql.transaction_sync(
                    |s| -> Result<CountAgenticAnswer, PlatformError> {
                        let (day, zone) = self.cap_day(s, now, &tz)?;
                        let used = s
                            .first::<N>(
                                "SELECT value AS n FROM counters WHERE metric = 'agentic' AND window = ?1",
                                &[day.clone().into()],
                            )?
                            .map_or(0, |r| r.n);
                        if used >= i64::from(cap) {
                            return Ok(CountAgenticAnswer::CapReached {
                                resets_at: days::next_local_midnight(now, &zone),
                            });
                        }
                        Self::add(s, "agentic", &day, 1)?;
                        Self::add(s, "usage:agentic", &utc, 1)?;
                        Ok(CountAgenticAnswer::Ok {
                            used: (used + 1) as u32,
                        })
                    },
                )?;
                to_v(&answer)
            }
            QuotaRequest::RecordUsage { metric, n } => {
                let name = format!(
                    "usage:{}",
                    serde_json::to_value(metric)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default()
                );
                self.sql
                    .transaction_sync(|s| Self::add(s, &name, &utc, n as i64))?;
                to_v(&RecordUsageAnswer {
                    total: self.counter(&name, &utc)? as u64,
                    day: utc,
                })
            }
            QuotaRequest::ForgetIdentity { identity_id } => {
                self.sql
                    .transaction_sync(|s| -> Result<(), PlatformError> {
                        s.exec(
                            "DELETE FROM outcomes WHERE identity_id = ?1",
                            &[identity_id.clone().into()],
                        )?;
                        s.exec(
                            "DELETE FROM counters WHERE metric = ?1",
                            &[format!("sends:{identity_id}").into()],
                        )?;
                        Ok(())
                    })?;
                Value::Null
            }
            QuotaRequest::Hold {
                feature,
                units,
                r#ref,
                ..
            } => {
                let held = self
                    .sql
                    .transaction_sync(|s| self.hold(s, feature, units, &r#ref))?;
                to_v(&HoldAnswer::Held(held))
            }
            QuotaRequest::Settle {
                feature,
                r#ref,
                consume,
                keep,
            } => {
                self.sql.transaction_sync(|s| -> Result<(), PlatformError> {
                    let units: Option<N> = s.first("SELECT units AS n FROM holds WHERE feature = ?1 AND ref = ?2", &[feature.as_str().into(), r#ref.clone().into()])?;
                    let consume = i64::from(consume);
                    let Some(units) = units else {
                        // No hold (an uncertain send resolved later): consume without one (W5).
                        if consume > 0 {
                            s.exec(
                                "INSERT INTO allowances (feature, granted, used, held) VALUES (?1, NULL, ?2, 0) ON CONFLICT (feature) DO UPDATE SET used = used + ?2",
                                &[feature.as_str().into(), consume.into()],
                            )?;
                        }
                        return Ok(());
                    };
                    let consume = consume.min(units.n);
                    let keep = i64::from(keep).min(units.n - consume);
                    s.exec(
                        "UPDATE allowances SET used = used + ?2, held = MAX(held - ?3, 0) WHERE feature = ?1",
                        &[feature.as_str().into(), consume.into(), (units.n - keep).into()],
                    )?;
                    if keep > 0 {
                        s.exec("UPDATE holds SET units = ?3 WHERE feature = ?1 AND ref = ?2", &[feature.as_str().into(), r#ref.clone().into(), keep.into()])?;
                    } else {
                        s.exec("DELETE FROM holds WHERE feature = ?1 AND ref = ?2", &[feature.as_str().into(), r#ref.clone().into()])?;
                    }
                    if feature == Feature::Sends && consume > 0 {
                        Self::add(s, "usage:sends", &utc, consume)?;
                    }
                    Ok(())
                })?;
                Value::Null
            }
            QuotaRequest::Extend {
                feature,
                r#ref,
                until,
            } => {
                self.sql.exec(
                    "UPDATE holds SET expires_at = MAX(expires_at, ?3) WHERE feature = ?1 AND ref = ?2",
                    &[feature.as_str().into(), r#ref.into(), until.into()],
                )?;
                Value::Null
            }
            QuotaRequest::Adjust { feature, delta, .. } => {
                self.sql.transaction_sync(|s| {
                    s.exec(
                        "INSERT INTO allowances (feature, granted, used, held) VALUES (?1, NULL, MAX(?2, 0), 0) ON CONFLICT (feature) DO UPDATE SET used = MAX(used + ?2, 0)",
                        &[feature.as_str().into(), delta.into()],
                    )
                    .map(|_| ())
                })?;
                Value::Null
            }
            QuotaRequest::SetPlan { .. }
            | QuotaRequest::SetMeasured { .. }
            | QuotaRequest::Reconcile { .. } => Value::Null,
            QuotaRequest::GetUsage => {
                #[derive(Deserialize)]
                struct A {
                    feature: String,
                    used: i64,
                    held: i64,
                }
                let rows: Vec<A> = self
                    .sql
                    .query("SELECT feature, used, held FROM allowances", &[])?;
                let allowances = Feature::ALL
                    .iter()
                    .map(|f| {
                        let r = rows.iter().find(|r| r.feature == f.as_str());
                        AllowanceRow {
                            feature: *f,
                            granted: None,
                            used: r.map_or(0, |r| r.used as u32),
                            held: r.map_or(0, |r| r.held as u32),
                            remaining: None,
                            resets_at: None,
                        }
                    })
                    .collect();
                to_v(&UsageAnswer {
                    mode: BillingMode::Disabled,
                    period_start: None,
                    period_end: None,
                    allowances,
                })
            }
        };
        Ok(out)
    }
}

/// Object-safe serialisation for the answers above.
mod erased {
    pub trait Ser {
        fn json(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> Ser for T {
        fn json(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
        }
    }
}

#[cfg(test)]
mod tests {
    use pylota_identity_platform::fakes::{FakePlatform, FakeSql};

    use super::*;

    fn env(tenant: &str, req: Value) -> Value {
        json!({ "v": 1, "tenant_id": tenant, "identity_id": null, "request_id": "req_1", "actor_key_id": null,
                "deadline_ms": i64::MAX, "req": req })
    }

    /// Init stores the owner; a mismatched owner is refused; every other variant answers as the stub
    /// table of build plan M5 says.
    #[test]
    fn stub_answers_every_variant() {
        let p = FakePlatform::new();
        let sql = FakeSql::new();
        let q = Quota { p: &p, sql: &sql };
        let t = "ten_01ARYZ6S410000000000000000";
        let ok = |v: Value| -> Value {
            assert!(v.get("ok").is_some(), "{v}");
            v["ok"].clone()
        };
        // Before Init, only Init is accepted.
        assert_eq!(
            q.handle(&env(t, json!({ "op": "get_usage" })))["err"]["error"]["code"],
            "internal_error"
        );
        ok(q.handle(&env(t, json!({ "op": "init", "tenant_id": t }))));
        ok(q.handle(&env(t, json!({ "op": "init", "tenant_id": t }))));
        let mismatch = q.handle(&env("ten_other", json!({ "op": "get_usage" })));
        assert_eq!(mismatch["err"]["error"]["code"], "internal_error");
        let logs = pylota_identity_platform::console::take();
        assert!(logs.iter().any(|(_, l)| l.contains("rpc_owner_mismatch")));

        let r = ok(q.handle(&env(
            t,
            json!({ "op": "reserve", "identity_id": "idn_1", "tz": "UTC", "identity_cap": 1,
            "tenant_cap": 1, "hold": { "units": 1, "ref": "msg_1", "gates": [] }, "stop": null }),
        )));
        assert_eq!(
            (
                r["kind"].clone(),
                r["identity_used"].clone(),
                r["tenant_used"].clone()
            ),
            (json!("ok"), json!(1), json!(1))
        );
        // From M9 the caps hold: over the identity cap is `cap_reached`, within it is ok.
        let r = ok(q.handle(&env(
            t,
            json!({ "op": "reserve", "identity_id": "idn_1", "tz": "UTC", "identity_cap": 1,
            "tenant_cap": null, "hold": null, "stop": null }),
        )));
        assert_eq!(
            (r["kind"].clone(), r["scope"].clone()),
            (json!("cap_reached"), json!("identity"))
        );
        let r = ok(q.handle(&env(
            t,
            json!({ "op": "reserve", "identity_id": "idn_1", "tz": "UTC", "identity_cap": 2,
            "tenant_cap": null, "hold": null, "stop": null }),
        )));
        assert_eq!(
            (r["identity_used"].clone(), r["tenant_used"].clone()),
            (json!(2), json!(1))
        );
        let day = r["day"].as_str().unwrap().to_owned();
        ok(q.handle(&env(
            t,
            json!({ "op": "release", "identity_id": "idn_1", "day": day, "tenant_counted": false }),
        )));

        let d = ok(q.handle(&env(t, json!({ "op": "record_outcome", "identity_id": "idn_1", "domain_id": null,
            "outcome": "bounced", "at": 1_791_540_000_000i64, "event_id": "evt_1", "counted": true }))));
        assert_eq!(
            d,
            json!({ "pause_identity": false, "pause_tenant": null, "pause_domain": false, "bounce_alert": false, "complaint_alert": false })
        );
        ok(q.handle(&env(t, json!({ "op": "record_outcome", "identity_id": "idn_1", "domain_id": null,
            "outcome": "bounced", "at": 1_791_540_000_000i64, "event_id": "evt_1", "counted": true }))));
        let n: Vec<Value> = sql
            .query("SELECT COUNT(*) AS n FROM outcomes", &[])
            .unwrap();
        assert_eq!(n[0]["n"], 1, "counted once per event_id");

        assert_eq!(ok(q.handle(&env(t, json!({ "op": "admit_inbound", "identity_id": "idn_1", "bucket": "b", "at": 1, "cap": 1 }))))["kind"], "admit");
        assert_eq!(
            ok(q.handle(&env(
                t,
                json!({ "op": "count_triage_model", "tz": "UTC", "cap": 1, "ref": "msg_1" })
            )))["kind"],
            "ok"
        );
        assert_eq!(
            ok(q.handle(&env(t, json!({ "op": "outcome_rates", "since": 0 })))),
            json!({ "outcomes": 0, "bounced": 0, "complained": 0 })
        );
        assert_eq!(ok(q.handle(&env(t, json!({ "op": "system_mail", "class": "invitation", "keys": [], "day": "2026-10-28" }))))["kind"], "ok");
        assert_eq!(
            ok(q.handle(&env(
                t,
                json!({ "op": "count_agentic", "tz": "UTC", "cap": 1 })
            ))),
            json!({ "kind": "ok", "used": 1 })
        );
        assert_eq!(
            ok(q.handle(&env(
                t,
                json!({ "op": "record_usage", "metric": "inbound", "n": 3 })
            )))["total"],
            3
        );
        ok(q.handle(&env(
            t,
            json!({ "op": "forget_identity", "identity_id": "idn_1" }),
        )));
        let n: Vec<Value> = sql
            .query("SELECT COUNT(*) AS n FROM outcomes", &[])
            .unwrap();
        assert_eq!(n[0]["n"], 0);

        let h = ok(q.handle(&env(
            t,
            json!({ "op": "hold", "feature": "triage", "units": 2, "ref": "r1", "gates": [] }),
        )));
        assert_eq!(
            (h["kind"].clone(), h["remaining"].clone()),
            (json!("held"), Value::Null)
        );
        ok(q.handle(&env(
            t,
            json!({ "op": "settle", "feature": "triage", "ref": "r1", "consume": 1, "keep": 0 }),
        )));
        ok(q.handle(&env(
            t,
            json!({ "op": "adjust", "feature": "triage", "delta": -5, "ref": "r2" }),
        )));
        for op in [
            json!({ "op": "extend", "feature": "sends", "ref": "r", "until": 1 }),
            json!({ "op": "set_plan", "mode": "metered", "granted": {}, "period_start": 0, "period_end": 1, "catalog_hash": "h" }),
            json!({ "op": "set_measured", "storage_bytes": 1 }),
            json!({ "op": "reconcile", "inboxes": 1, "custom_domains": 0, "seats": 1, "read_at": 1 }),
        ] {
            assert_eq!(ok(q.handle(&env(t, op))), Value::Null);
        }
        let u = ok(q.handle(&env(t, json!({ "op": "get_usage" }))));
        assert_eq!(u["mode"], "disabled");
        assert_eq!(u["allowances"].as_array().unwrap().len(), 6);
        let triage = u["allowances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["feature"] == "triage")
            .unwrap()
            .clone();
        assert_eq!(
            (
                triage["used"].clone(),
                triage["held"].clone(),
                triage["granted"].clone()
            ),
            (json!(0), json!(0), Value::Null)
        );
        for op in [
            json!({ "op": "deployment_reserve", "recipients": 1, "restricted": false, "quota": 10 }),
            json!({ "op": "deployment_status", "quota": 10 }),
            json!({ "op": "count_screen_model", "tz": "UTC", "cap": 1, "sender": "s", "known": true, "ref": "m" }),
            json!({ "op": "count_screen_failure", "at": 1 }),
            json!({ "op": "count_signup_outcome", "outcome": "delivered", "at": 1, "event_id": "e" }),
        ] {
            ok(q.handle(&env(t, op)));
        }
        assert_eq!(
            q.handle(&env(t, json!({ "op": "nope" })))["err"]["error"]["code"],
            "invalid_request"
        );
    }
}
