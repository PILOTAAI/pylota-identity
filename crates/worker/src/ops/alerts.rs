//! The alert list as data and the state alert evaluator (Observability § 5.3–5.4). Each alert has a
//! name, the parameter its key carries (`dlq:{queue}`), its class, its severity and its runbook anchor.
//! The every-minute cron runs [`evaluate`]: it reads the level conditions from D1 and the reports that
//! objects, consumers and jobs wrote since the last run ([`report`]), applies `core::slo::step` to each
//! key, and writes `alert.fired` and `alert.resolved` audit rows, `alert_fired` and `alert_resolved`
//! log lines and one `alert_fired` data point per notification. Its memory between runs (the firing
//! keys, their consecutive false runs and re-notifications) is the `platform_state` row
//! `alert_evaluator`, claimed by a compare-and-set on its run number so overlapping runs act once.

use std::collections::BTreeMap;

use pylota_identity_core::slo::{self, KeyState, Persistence, Severity, Transition};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{Clock as _, ControlDb as _, Ids as _, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::audit;
use crate::log::Field;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Cloudflare Custom Alerts over the SQL API.
    A,
    /// The state alert evaluator.
    B,
    /// The evaluator and a webhook event.
    BC,
    /// Webhook events only.
    C,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sev {
    Page,
    Ticket,
    /// Set per key (`job_failed` by kind, capacity by share, the breaker by stage).
    Varies,
    TenantFacing,
}

/// One row of the alert list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertDef {
    pub name: &'static str,
    /// The key's parameter (`{queue}`), empty for a deployment-wide key.
    pub param: &'static str,
    pub class: Class,
    pub severity: Sev,
    /// The anchor of the runbook in the observability design; empty when the row's own text is it.
    pub runbook: &'static str,
}

const fn a(
    name: &'static str,
    param: &'static str,
    class: Class,
    severity: Sev,
    runbook: &'static str,
) -> AlertDef {
    AlertDef {
        name,
        param,
        class,
        severity,
        runbook,
    }
}

/// Observability § 5.3, in its order (the SLO burn rules, class A, are the Custom Alerts of § 5.2).
pub const ALERTS: &[AlertDef] = &[
    a("dlq", "{queue}", Class::B, Sev::Page, "dlq-growth"),
    a("vector_drift", "", Class::B, Sev::Ticket, ""),
    a(
        "bounce_rate",
        "{tenant_id}:{domain_id}",
        Class::B,
        Sev::Page,
        "bounce-spike",
    ),
    a(
        "complaint_rate",
        "{tenant_id}:{domain_id}",
        Class::B,
        Sev::Page,
        "complaint-spike",
    ),
    a(
        "inbound_reject_spike",
        "",
        Class::A,
        Sev::Ticket,
        "domain-failing",
    ),
    a("inbound_tempfail", "", Class::A, Sev::Page, "dlq-growth"),
    a("inbound_lost", "", Class::B, Sev::Page, "restore-from-pitr"),
    a(
        "ses_object_lost",
        "",
        Class::B,
        Sev::Page,
        "ses-account-and-receiving",
    ),
    a(
        "ses_sending_paused",
        "",
        Class::B,
        Sev::Page,
        "ses-account-and-receiving",
    ),
    a(
        "ses_rule_missing",
        "",
        Class::B,
        Sev::Page,
        "ses-account-and-receiving",
    ),
    a(
        "ses_identities_90pct",
        "",
        Class::B,
        Sev::Ticket,
        "ses-account-and-receiving",
    ),
    a(
        "ses_rule_sync_failed",
        "",
        Class::B,
        Sev::Ticket,
        "ses-account-and-receiving",
    ),
    a(
        "webhook_failing",
        "{webhook_id}",
        Class::B,
        Sev::Ticket,
        "integrator-api-down",
    ),
    a(
        "webhook_disabled",
        "{webhook_id}",
        Class::BC,
        Sev::Ticket,
        "integrator-api-down",
    ),
    a("provider_quota", "", Class::A, Sev::Page, "quota-exhausted"),
    a(
        "provider_quota_80",
        "",
        Class::B,
        Sev::Ticket,
        "quota-exhausted",
    ),
    a(
        "quota_warning",
        "",
        Class::C,
        Sev::TenantFacing,
        "quota-exhausted",
    ),
    a(
        "domain_failing",
        "{domain_id}",
        Class::BC,
        Sev::Ticket,
        "domain-failing",
    ),
    a(
        "platform_domain_failing",
        "",
        Class::B,
        Sev::Page,
        "domain-failing",
    ),
    a(
        "domain_remove_failed",
        "{domain_id}",
        Class::B,
        Sev::Ticket,
        "domain-failing",
    ),
    a(
        "delivery_events_manual",
        "{domain_id}",
        Class::B,
        Sev::Ticket,
        "",
    ),
    a(
        "inbound_throttled",
        "",
        Class::A,
        Sev::Ticket,
        "abusive-identity",
    ),
    a(
        "stripe_webhook_errors",
        "",
        Class::B,
        Sev::Ticket,
        "billing.md#stripe-integration",
    ),
    a(
        "mailbox_size",
        "{identity_id}",
        Class::B,
        Sev::Ticket,
        "abusive-identity",
    ),
    a(
        "abuse_pause",
        "{identity_id}",
        Class::BC,
        Sev::Ticket,
        "abusive-identity",
    ),
    a(
        "signup_ramp_review",
        "{tenant_id}",
        Class::B,
        Sev::Ticket,
        "abusive-identity",
    ),
    a(
        "system_mail_blocked",
        "",
        Class::B,
        Sev::Page,
        "domain-failing",
    ),
    a(
        "billing_dispute",
        "{tenant_id}",
        Class::B,
        Sev::Page,
        "billing-dispute",
    ),
    a(
        "shared_domain_breaker",
        "",
        Class::B,
        Sev::Varies,
        "quota-exhausted",
    ),
    a(
        "system_mail_budget",
        "",
        Class::A,
        Sev::Ticket,
        "abusive-identity",
    ),
    a(
        "billing_cancel_failed",
        "{tenant_id}",
        Class::B,
        Sev::Page,
        "erasure-failure",
    ),
    a(
        "billing_cancelled_after_erasure",
        "{tenant_id}",
        Class::B,
        Sev::Ticket,
        "",
    ),
    a(
        "erasure_stalled",
        "{erasure_id}",
        Class::B,
        Sev::Page,
        "erasure-failure",
    ),
    a(
        "erasure_failed",
        "{erasure_id}",
        Class::BC,
        Sev::Page,
        "erasure-failure",
    ),
    a(
        "erasure_overdue",
        "{erasure_id}",
        Class::B,
        Sev::Page,
        "erasure-failure",
    ),
    a("job_failed", "{kind}", Class::B, Sev::Varies, "job-failed"),
    a(
        "break_glass_key_used",
        "",
        Class::B,
        Sev::Page,
        "57-break-glass-record",
    ),
    a("heartbeat_missing", "", Class::B, Sev::Page, "heartbeat"),
    a(
        "capacity_70",
        "{resource}",
        Class::B,
        Sev::Ticket,
        "capacity",
    ),
    a("capacity_80", "{resource}", Class::B, Sev::Page, "capacity"),
    a(
        "containment",
        "{rule}",
        Class::B,
        Sev::Page,
        "automatic-containment",
    ),
    a(
        "rpc_owner_mismatch",
        "",
        Class::B,
        Sev::Page,
        "compromised-key",
    ),
    a(
        "uncertain_spike",
        "",
        Class::A,
        Sev::Page,
        "email-sending-outage",
    ),
    a(
        "transport_breaker_open",
        "{scope}",
        Class::B,
        Sev::Page,
        "email-sending-outage",
    ),
    a(
        "sending_pause",
        "{tenant_id or domain_id}",
        Class::B,
        Sev::Page,
        "abusive-identity",
    ),
    a("ai_backlog", "", Class::B, Sev::Ticket, ""),
    a(
        "delivery_orphaned",
        "",
        Class::A,
        Sev::Ticket,
        "email-sending-outage",
    ),
    a("panics", "", Class::A, Sev::Ticket, "parser-bug"),
    a("config_invalid", "", Class::A, Sev::Page, ""),
    a(
        "webhook_secret_unavailable",
        "",
        Class::A,
        Sev::Page,
        "security.md#62-rotation-procedures",
    ),
    a(
        "webhook_ssrf_blocked",
        "",
        Class::A,
        Sev::Ticket,
        "integrator-api-down",
    ),
    a(
        "notification_send_failures",
        "",
        Class::A,
        Sev::Ticket,
        "domain-failing",
    ),
    a("drafts_overdue", "", Class::B, Sev::Page, "drafts-overdue"),
    a("draft_index_stuck", "", Class::B, Sev::Ticket, ""),
    a("draft_send_interrupted", "", Class::A, Sev::Ticket, ""),
    a(
        "event_log_append_failing",
        "",
        Class::A,
        Sev::Page,
        "event-log",
    ),
    a("event_log_lag", "", Class::A, Sev::Varies, "event-log"),
    a(
        "event_log_size",
        "{tenant_id}",
        Class::B,
        Sev::Ticket,
        "event-log",
    ),
    a(
        "event_stream_errors",
        "",
        Class::A,
        Sev::Ticket,
        "event-log",
    ),
    a(
        "screening_unavailable",
        "{tenant_id}",
        Class::B,
        Sev::Page,
        "screening-unavailable",
    ),
    a(
        "screening_model_failing",
        "",
        Class::B,
        Sev::Page,
        "screening-unavailable",
    ),
    a(
        "screening_model_cap",
        "{tenant_id}",
        Class::B,
        Sev::Ticket,
        "screening-unavailable",
    ),
    a(
        "screening_stuck",
        "{identity_id}",
        Class::B,
        Sev::Page,
        "screening-unavailable",
    ),
    a(
        "screening_deadline_lag",
        "",
        Class::A,
        Sev::Ticket,
        "screening-unavailable",
    ),
    a(
        "screening_queue_wait",
        "",
        Class::A,
        Sev::Ticket,
        "screening-unavailable",
    ),
    a(
        "screening_suspect_spike",
        "",
        Class::A,
        Sev::Ticket,
        "screening-unavailable",
    ),
    a(
        "calendar_parse_failures",
        "",
        Class::A,
        Sev::Ticket,
        "parser-bug",
    ),
    a(
        "calendar_raw_refused",
        "",
        Class::A,
        Sev::Ticket,
        "index.md#spikes",
    ),
    a("upload_object_missing", "", Class::B, Sev::Ticket, ""),
    a(
        "agent_signup_spike",
        "",
        Class::B,
        Sev::Ticket,
        "agent-sign-up-abuse",
    ),
    a(
        "agent_signup_cap_80",
        "",
        Class::B,
        Sev::Ticket,
        "agent-sign-up-abuse",
    ),
    a(
        "retention_expiry_overdue",
        "{identity_id}",
        Class::B,
        Sev::Varies,
        "erasure-failure",
    ),
    a(
        "arc_rescue_refused_spike",
        "",
        Class::A,
        Sev::Ticket,
        "abusive-identity",
    ),
    a(
        "phone_port_out_pending",
        "",
        Class::B,
        Sev::Page,
        "phone-provider",
    ),
    a(
        "phone_port_out_deadline",
        "",
        Class::B,
        Sev::Page,
        "phone-provider",
    ),
    a(
        "phone_provider_balance_low",
        "",
        Class::B,
        Sev::Page,
        "phone-provider",
    ),
    a(
        "phone_provider_auth",
        "",
        Class::B,
        Sev::Page,
        "phone-provider",
    ),
    a(
        "phone_unplaced_outbound_call",
        "",
        Class::B,
        Sev::Page,
        "phone-provider",
    ),
    a(
        "phone_event_mismatch",
        "",
        Class::B,
        Sev::Page,
        "phone-provider",
    ),
    a(
        "phone_order_uncertain_stuck",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_reconcile_drift",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_orphan_number",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_status_unknown",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_requirement_unmapped",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_review_long",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_webhook_signature_failures",
        "",
        Class::A,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_fx_drift",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_provider_limit_hit",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_pin_missing",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_recording_undeclared",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_release_unattributable",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_order_duplicate",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_registration_slow",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "phone_expiring_release_stuck",
        "",
        Class::B,
        Sev::Ticket,
        "phone-provider",
    ),
    a(
        "vault_value_exposed",
        "",
        Class::B,
        Sev::Page,
        "vault-incident",
    ),
    a(
        "vault_audit_append_failed",
        "",
        Class::B,
        Sev::Page,
        "vault-incident",
    ),
    a(
        "vault_decrypt_failed",
        "",
        Class::B,
        Sev::Page,
        "vault-incident",
    ),
    a(
        "vault_placeholder_misplaced",
        "",
        Class::B,
        Sev::Ticket,
        "vault-incident",
    ),
    a(
        "vault_unlock_failures",
        "",
        Class::A,
        Sev::Ticket,
        "vault-incident",
    ),
    a(
        "vault_consumer_auth_spike",
        "",
        Class::A,
        Sev::Ticket,
        "vault-incident",
    ),
];

pub fn def(name: &str) -> Option<&'static AlertDef> {
    ALERTS.iter().find(|a| a.name == name)
}

/// The alert's name: the key up to its first `:`.
pub fn name_of(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

/// How the evaluator reads a key (§ 5.4 step 3).
pub fn persistence(name: &str) -> Persistence {
    match name {
        "erasure_stalled" | "billing_cancel_failed" => Persistence::SelfResolved,
        "mailbox_size"
        | "abuse_pause"
        | "sending_pause"
        | "transport_breaker_open"
        | "ai_backlog"
        | "rpc_owner_mismatch"
        | "inbound_lost"
        | "ses_object_lost"
        | "system_mail_blocked"
        | "bounce_rate"
        | "complaint_rate"
        | "billing_cancelled_after_erasure"
        | "vector_drift"
        | "signup_ramp_review"
        | "billing_dispute"
        | "shared_domain_breaker"
        | "ses_sending_paused"
        | "ses_rule_missing"
        | "ses_rule_sync_failed"
        | "drafts_overdue"
        | "draft_index_stuck"
        | "event_log_size"
        | "screening_unavailable"
        | "screening_stuck"
        | "screening_model_failing"
        | "screening_model_cap"
        | "upload_object_missing"
        | "retention_expiry_overdue" => Persistence::Reported,
        _ => Persistence::Level,
    }
}

/// The severity of `key` when the caller does not set one.
pub fn default_severity(key: &str) -> Severity {
    let name = name_of(key);
    if name == "job_failed" {
        let kind = key.split(':').nth(1).unwrap_or("");
        return if matches!(kind, "retention" | "domain_remove" | "restore_reconcile") {
            Severity::Page
        } else {
            Severity::Ticket
        };
    }
    match def(name).map(|d| d.severity) {
        Some(Sev::Ticket) => Severity::Ticket,
        _ => Severity::Page,
    }
}

/// The runbook's URL in the published docs.
pub fn runbook_url(docs_url: &str, name: &str) -> String {
    let anchor = def(name).map_or("", |d| d.runbook);
    match anchor.split_once(".md#") {
        Some((page, a)) => format!("{docs_url}/project/design/{page}.html#{a}"),
        None if anchor.is_empty() => {
            format!("{docs_url}/project/design/observability.html#53-alert-list")
        }
        None => format!("{docs_url}/project/design/observability.html#{anchor}"),
    }
}

/// A condition reported by an object, consumer, cron or job (§ 5.4, the reporting table): its
/// `alert.fired` audit row, which the next evaluator run reads. `values` holds IDs and numbers only.
pub async fn report<P: Platform>(
    p: &P,
    key: &str,
    tenant_id: Option<&str>,
    severity: Option<Severity>,
    values: &Value,
    request_id: &str,
) {
    let severity = severity.unwrap_or_else(|| default_severity(key));
    let now = p.clock().now_ms();
    let r = p
        .db()
        .execute(audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            tenant_id,
            None,
            "alert.fired",
            "alert",
            key,
            &json!({ "severity": severity.as_str(), "values": values, "reported": true }),
            request_id,
            now,
        ))
        .await;
    if r.is_err() {
        crate::log::error(
            "alert_report_failed",
            &[
                ("detail", key.to_owned().into()),
                ("code", "unavailable".into()),
            ],
        );
    }
}

/// A reported condition that its reporter resolves itself (`erasure_stalled`, `billing_cancel_failed`).
pub async fn resolve_reported<P: Platform>(
    p: &P,
    key: &str,
    tenant_id: Option<&str>,
    request_id: &str,
) {
    let now = p.clock().now_ms();
    let _ = p
        .db()
        .execute(audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            tenant_id,
            None,
            "alert.resolved",
            "alert",
            key,
            &json!({ "reported": true }),
            request_id,
            now,
        ))
        .await;
}

/// The evaluator's memory between runs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvaluatorState {
    pub run: u64,
    pub last_run_at: i64,
    #[serde(default)]
    pub keys: BTreeMap<String, KeyState>,
    /// The alert email budget of the current hour and the last daily summary.
    #[serde(default)]
    pub email: super::alert_email::Budget,
    /// The last audit row (D1 rowid) read for reports, so a clock step never hides one.
    #[serde(default)]
    pub report_rowid: i64,
    /// The last capacity measurement (every 15th run).
    #[serde(default)]
    pub capacity: Option<Capacity>,
}

/// D1's size, the vector count and the tenants that are not erased (§ 5.4, J30, J31).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capacity {
    pub d1_bytes: Option<u64>,
    pub vectors: Option<u64>,
    pub tenants: u64,
    pub measured_at: i64,
}

/// The limits capacity is measured against ([Limits › Storage]).
pub const D1_LIMIT_BYTES: u64 = 10_000_000_000;
pub const VECTORS_LIMIT: u64 = 20_000_000;
pub const NAMESPACES_LIMIT: u64 = 50_000;

/// Measures capacity: D1's size from the `meta.size_after` of the tenant-count query, the vector count
/// from the latest reconciliation summary.
async fn measure<P: Platform>(p: &P, now: i64) -> Option<Capacity> {
    #[derive(Deserialize)]
    struct N {
        n: i64,
    }
    #[derive(Deserialize)]
    struct V {
        index_count: Option<i64>,
    }
    let count = Stmt::new(
        "SELECT COUNT(*) AS n FROM tenants WHERE status <> 'erased'",
        vec![],
    );
    let size = p.db().execute(count.clone()).await.ok()?.size_after;
    let tenants = p.db().first::<N>(count).await.ok()??.n.max(0) as u64;
    let vectors = p
        .db()
        .first::<V>(Stmt::new(
            "SELECT index_count FROM index_reconcile WHERE identity_id = '*' AND index_count IS NOT NULL ORDER BY run_date DESC LIMIT 1",
            vec![],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|v| v.index_count)
        .map(|n| n.max(0) as u64);
    Some(Capacity {
        d1_bytes: size,
        vectors,
        tenants,
        measured_at: now,
    })
}

/// Whether `capacity_80:vectors` or `capacity_80:namespaces` fires (J31).
pub async fn vectors_full<P: Platform>(p: &P) -> bool {
    load_state(p).await.is_some_and(|(s, _)| {
        s.keys.contains_key("capacity_80:vectors") || s.keys.contains_key("capacity_80:namespaces")
    })
}

/// The capacity alerts of a measurement: `capacity_70:{resource}` (ticket) and `capacity_80:{resource}`
/// (page).
pub fn capacity_conditions(c: &Capacity) -> Vec<Condition> {
    let mut out = Vec::new();
    for (resource, used, limit) in [
        ("d1", c.d1_bytes, D1_LIMIT_BYTES),
        ("vectors", c.vectors, VECTORS_LIMIT),
        ("namespaces", Some(c.tenants), NAMESPACES_LIMIT),
    ] {
        let Some(used) = used else { continue };
        for pct in [70, 80] {
            if slo::capacity_at(used, limit, pct) {
                let mut cond = Condition::new(
                    format!("capacity_{pct}:{resource}"),
                    None,
                    json!({ "used": used, "limit": limit, "pct": used as f64 * 100.0 / limit as f64 }),
                );
                cond.severity = if pct == 80 {
                    Severity::Page
                } else {
                    Severity::Ticket
                };
                out.push(cond);
            }
        }
    }
    out
}

/// One true condition of this run.
#[derive(Clone, Debug)]
pub struct Condition {
    pub key: String,
    pub tenant_id: Option<String>,
    pub severity: Severity,
    pub values: Value,
    /// The reporter wrote the `alert.fired` row.
    pub reported: bool,
}

impl Condition {
    pub fn new(key: impl Into<String>, tenant_id: Option<String>, values: Value) -> Self {
        let key = key.into();
        Condition {
            severity: default_severity(&key),
            key,
            tenant_id,
            values,
            reported: false,
        }
    }
}

pub(crate) async fn load_state<P: Platform>(p: &P) -> Option<(EvaluatorState, bool)> {
    #[derive(Deserialize)]
    struct R {
        value_json: String,
    }
    match p
        .db()
        .first::<R>(Stmt::new(
            "SELECT value_json FROM platform_state WHERE key = 'alert_evaluator'",
            vec![],
        ))
        .await
    {
        Ok(Some(r)) => Some((
            serde_json::from_str(&r.value_json).unwrap_or_default(),
            true,
        )),
        Ok(None) => Some((EvaluatorState::default(), false)),
        Err(_) => None,
    }
}

/// Claims run `prev.run + 1` by writing `next` only where the stored run is still `prev.run`.
async fn claim<P: Platform>(
    p: &P,
    prev: &EvaluatorState,
    existed: bool,
    next: &EvaluatorState,
) -> bool {
    let now = p.clock().now_ms();
    let json = serde_json::to_string(next).unwrap_or_default();
    let stmt = if existed {
        Stmt::new(
            "UPDATE platform_state SET value_json = ?1, updated_at = ?2 WHERE key = 'alert_evaluator' AND json_extract(value_json, '$.run') = ?3",
            vec![json.into(), now.into(), (prev.run as i64).into()],
        )
    } else {
        Stmt::new(
            "INSERT OR IGNORE INTO platform_state (key, value_json, updated_at) VALUES ('alert_evaluator', ?1, ?2)",
            vec![json.into(), now.into()],
        )
    };
    matches!(p.db().execute(stmt).await, Ok(m) if m.changes == 1)
}

/// One evaluator run (§ 5.4).
pub async fn evaluate<P: Platform>(p: &P, request_id: &str) {
    let now = p.clock().now_ms();
    let Some((prev, existed)) = load_state(p).await else {
        return;
    };
    let mut conditions = match conditions(p, &prev, now).await {
        Ok(c) => c,
        Err(_) => {
            crate::log::warn("alert_evaluator_skipped", &[("code", "unavailable".into())]);
            return;
        }
    };
    let (reported, resolved_by_reporter, last_rowid) = reports_since(p, prev.report_rowid).await;
    // Capacity: measured on the first run and every 15th after (§ 5.4); its alerts are levels between.
    let capacity = if prev.capacity.is_none() || prev.run % 15 == 0 {
        let measured = measure(p, now).await.or_else(|| prev.capacity.clone());
        if let Some(m) = &measured
            && m.d1_bytes
                .is_some_and(|b| slo::capacity_at(b, D1_LIMIT_BYTES, 90))
            && !crate::ops::switches::away_from_default(p, "emergency_prune").await
        {
            // `emergency_prune` (§ 5.6): retention keeps 7 days of events from the next run.
            if crate::ops::switches::set(
                p,
                "emergency_prune",
                "on",
                "auto",
                "capacity_d1_90",
                None,
                request_id,
            )
            .await
            .is_ok()
            {
                crate::metrics::count_with(p, "containment_total", None, &["emergency_prune"], 1);
            }
        }
        measured
    } else {
        prev.capacity.clone()
    };
    if let Some(c) = &capacity {
        conditions.extend(capacity_conditions(c));
    }
    conditions.extend(reported);

    // The keys of this run: every true condition and every key still firing.
    let mut by_key: BTreeMap<String, Condition> = BTreeMap::new();
    for c in conditions {
        by_key
            .entry(c.key.clone())
            .and_modify(|e| e.reported &= c.reported)
            .or_insert(c);
    }
    let mut next = EvaluatorState {
        run: prev.run + 1,
        last_run_at: now,
        keys: BTreeMap::new(),
        email: prev.email.clone(),
        report_rowid: last_rowid.max(prev.report_rowid),
        capacity: capacity.clone(),
    };
    let mut effects: Vec<(String, Transition, Option<Condition>, KeyState)> = Vec::new();
    let mut keys: Vec<String> = prev.keys.keys().cloned().collect();
    keys.extend(by_key.keys().cloned());
    keys.sort();
    keys.dedup();
    for key in keys {
        if resolved_by_reporter.contains(&key) && !by_key.contains_key(&key) {
            continue;
        }
        let cond = by_key.get(&key);
        let before = prev.keys.get(&key);
        let severity = cond
            .map(|c| c.severity)
            .or_else(|| before.map(|b| b.severity))
            .unwrap_or_else(|| default_severity(&key));
        let tenant = cond
            .and_then(|c| c.tenant_id.as_deref())
            .or_else(|| before.and_then(|b| b.tenant_id.as_deref()));
        let (after, t) = slo::step(
            before,
            cond.is_some(),
            severity,
            persistence(name_of(&key)),
            tenant,
            now,
        );
        let record = after.clone().or_else(|| before.cloned());
        if let Some(a) = after {
            next.keys.insert(key.clone(), a);
        }
        if t != Transition::None
            && let Some(state) = record
        {
            effects.push((key, t, cond.cloned(), state));
        }
    }
    let emails = plan_emails(p, &mut next, &effects, now);
    if !claim(p, &prev, existed, &next).await {
        return;
    }
    for (key, t, cond, state) in effects {
        apply(p, &key, &t, cond.as_ref(), &state, request_id, now).await;
    }
    for e in emails {
        super::alert_email::send(p, &e, request_id).await;
    }
}

/// The alert emails of this run (§ 5.5): one per page notification, at most 20 an hour and then one
/// that says how many more; and in the 08:00 UTC hour, once a day, the summary of the firing tickets.
fn plan_emails<P: Platform>(
    p: &P,
    next: &mut EvaluatorState,
    effects: &[(String, Transition, Option<Condition>, KeyState)],
    now: i64,
) -> Vec<super::alert_email::Email> {
    use super::alert_email::{PER_HOUR, capped_email, page_email, summary_email};
    let cfg = p.config();
    let env = match cfg.env {
        pylota_identity_platform::config::DeployEnv::Production => "production",
        pylota_identity_platform::config::DeployEnv::Staging => "staging",
        pylota_identity_platform::config::DeployEnv::Local => "local",
    };
    let hour = pylota_identity_core::days::utc_hour(now);
    if next.email.hour != hour {
        next.email.hour = hour.clone();
        next.email.sent = 0;
        next.email.capped = false;
    }
    let mut out = Vec::new();
    let mut over = 0;
    for (key, t, cond, state) in effects {
        let notice = match t {
            Transition::Fire => 0,
            Transition::Renotify { n } => *n,
            _ => continue,
        };
        if state.severity != Severity::Page {
            continue;
        }
        if next.email.sent >= PER_HOUR {
            over += 1;
            continue;
        }
        next.email.sent += 1;
        let values = cond.as_ref().map_or(Value::Null, |c| c.values.clone());
        out.push(page_email(
            env,
            &cfg.docs_url,
            key,
            &values,
            state.fired_at,
            notice,
        ));
    }
    if over > 0 && !next.email.capped {
        next.email.capped = true;
        out.push(capped_email(env, &hour, over));
    }
    let day = pylota_identity_core::days::utc_day(now);
    if hour.ends_with("T08") && next.email.summary_day != day {
        next.email.summary_day = day.clone();
        let tickets: Vec<(String, i64)> = next
            .keys
            .iter()
            .filter(|(_, s)| s.severity == Severity::Ticket)
            .map(|(k, s)| (k.clone(), s.fired_at))
            .collect();
        if !tickets.is_empty() {
            out.push(summary_email(env, &day, &tickets));
        }
    }
    out
}

async fn apply<P: Platform>(
    p: &P,
    key: &str,
    t: &Transition,
    cond: Option<&Condition>,
    state: &KeyState,
    request_id: &str,
    now: i64,
) {
    let name = name_of(key).to_owned();
    let sev = state.severity.as_str();
    let tenant = state.tenant_id.as_deref();
    let values = cond.map_or(Value::Null, |c| c.values.clone());
    match t {
        Transition::Fire | Transition::Renotify { .. } => {
            if *t == Transition::Fire && !cond.is_some_and(|c| c.reported) {
                let _ = p
                    .db()
                    .execute(audit::insert(
                        &p.ids().new_id(IdPrefix::Audit),
                        tenant,
                        None,
                        "alert.fired",
                        "alert",
                        key,
                        &json!({ "severity": sev, "values": values }),
                        request_id,
                        now,
                    ))
                    .await;
            }
            let notice = match t {
                Transition::Renotify { n } => i64::from(*n),
                _ => 0,
            };
            crate::log::error(
                "alert_fired",
                &[
                    ("detail", key.to_owned().into()),
                    ("severity", sev.into()),
                    ("count", Field::Int(notice)),
                ],
            );
            crate::metrics::count_with(p, "alert_fired", None, &[&name, sev], 1);
        }
        Transition::Resolve => {
            let _ = p
                .db()
                .execute(audit::insert(
                    &p.ids().new_id(IdPrefix::Audit),
                    tenant,
                    None,
                    "alert.resolved",
                    "alert",
                    key,
                    &json!({ "severity": sev }),
                    request_id,
                    now,
                ))
                .await;
            crate::log::info(
                "alert_resolved",
                &[("detail", key.to_owned().into()), ("severity", sev.into())],
            );
        }
        Transition::None => {}
    }
}

/// The `alert.fired` reports written after audit row `since_rowid`, the keys their reporters resolved,
/// and the last rowid read.
async fn reports_since<P: Platform>(p: &P, since_rowid: i64) -> (Vec<Condition>, Vec<String>, i64) {
    #[derive(Deserialize)]
    struct R {
        rid: i64,
        action: String,
        target_id: String,
        tenant_id: Option<String>,
        details_json: Option<String>,
    }
    let rows: Vec<R> = p
        .db()
        .query(Stmt::new(
            "SELECT rowid AS rid, action, target_id, tenant_id, details_json FROM audit_log WHERE rowid > ?1 AND action IN ('alert.fired','alert.resolved') AND json_extract(details_json, '$.reported') = 1 ORDER BY rowid LIMIT 1000",
            vec![since_rowid.into()],
        ))
        .await
        .unwrap_or_default();
    let mut fired = Vec::new();
    let mut resolved = Vec::new();
    let mut last = since_rowid;
    for r in rows {
        last = last.max(r.rid);
        let d: Value = r
            .details_json
            .as_deref()
            .and_then(|d| serde_json::from_str(d).ok())
            .unwrap_or(Value::Null);
        if r.action == "alert.resolved" {
            fired.retain(|c: &Condition| c.key != r.target_id);
            resolved.push(r.target_id);
            continue;
        }
        resolved.retain(|k| *k != r.target_id);
        let severity = match d["severity"].as_str() {
            Some("ticket") => Severity::Ticket,
            Some("page") => Severity::Page,
            _ => default_severity(&r.target_id),
        };
        fired.push(Condition {
            key: r.target_id,
            tenant_id: r.tenant_id,
            severity,
            values: d["values"].clone(),
            reported: true,
        });
    }
    (fired, resolved, last)
}

/// The DLQ alert's age bar.
const DLQ_AGE_MS: i64 = 15 * 60_000;
const DAY_MS: i64 = 86_400_000;

/// The level conditions of § 5.4 step 1 that hold now. A D1 error skips the run (nothing transitions).
pub async fn conditions<P: Platform>(
    p: &P,
    _prev: &EvaluatorState,
    now: i64,
) -> Result<Vec<Condition>, pylota_identity_platform::PlatformError> {
    let db = p.db();
    let mut out = Vec::new();

    // Dead-letter items: the per-queue count is also the `dlq_items` observation.
    #[derive(Deserialize)]
    struct Dlq {
        queue: String,
        n: i64,
        oldest: i64,
    }
    let dlq: Vec<Dlq> = db
        .query(Stmt::new(
            "SELECT queue, COUNT(*) AS n, MIN(first_seen_at) AS oldest FROM dlq_items WHERE redriven_at IS NULL GROUP BY queue",
            vec![],
        ))
        .await?;
    for d in dlq {
        crate::metrics::observe(p, "dlq_items", None, &[&d.queue], d.n as f64);
        if now - d.oldest > DLQ_AGE_MS {
            out.push(Condition::new(
                format!("dlq:{}", d.queue),
                None,
                json!({ "open": d.n, "oldest_age_s": (now - d.oldest) / 1000 }),
            ));
        }
    }

    // Webhook endpoints failing, or disabled for failing in the last minute.
    #[derive(Deserialize)]
    struct Wh {
        id: String,
        tenant_id: String,
        enabled: i64,
        consecutive_failures: i64,
    }
    let wh: Vec<Wh> = db
        .query(Stmt::new(
            "SELECT id, tenant_id, enabled, consecutive_failures FROM webhook_endpoints WHERE (enabled = 1 AND consecutive_failures >= 10) OR (enabled = 0 AND disabled_reason = 'failing' AND updated_at > ?1)",
            vec![(now - 60_000).into()],
        ))
        .await?;
    for w in wh {
        let name = if w.enabled == 1 {
            "webhook_failing"
        } else {
            "webhook_disabled"
        };
        out.push(Condition::new(
            format!("{name}:{}", w.id),
            Some(w.tenant_id),
            json!({ "consecutive_failures": w.consecutive_failures }),
        ));
    }

    // Domains: failing or suspended, the platform domain, removals that failed, manual events.
    #[derive(Deserialize)]
    struct Dom {
        id: String,
        tenant_id: Option<String>,
        kind: String,
        state: String,
        state_reason: Option<String>,
        manual: i64,
        remove_failed: i64,
    }
    let doms: Vec<Dom> = db
        .query(Stmt::new(
            "SELECT d.id, d.tenant_id, d.kind, d.state, d.state_reason, \
             (d.transport = 'cloudflare' AND d.kind <> 'platform' AND d.sending = 1 AND d.event_subscription_id IS NULL AND d.state NOT IN ('removing','removed')) AS manual, \
             (d.state = 'removing' AND (SELECT j.status FROM jobs j WHERE j.kind = 'domain_remove' AND json_extract(j.params_json, '$.domain_id') = d.id ORDER BY j.created_at DESC LIMIT 1) = 'failed') AS remove_failed \
             FROM domains d WHERE d.state IN ('failing','suspended','removing') OR d.kind = 'platform' \
             OR (d.transport = 'cloudflare' AND d.sending = 1 AND d.event_subscription_id IS NULL)",
            vec![],
        ))
        .await?;
    for d in doms {
        let reason = d.state_reason.clone().unwrap_or_default();
        if d.kind == "platform" {
            if d.state == "failing"
                || matches!(reason.as_str(), "dns_unresolvable" | "dnssec_bogus")
            {
                out.push(Condition::new(
                    "platform_domain_failing",
                    None,
                    json!({ "domain_id": d.id, "state": d.state, "reason": reason }),
                ));
            }
        } else if matches!(d.state.as_str(), "failing" | "suspended") {
            out.push(Condition::new(
                format!("domain_failing:{}", d.id),
                d.tenant_id.clone(),
                json!({ "state": d.state }),
            ));
        }
        if d.remove_failed == 1 {
            out.push(Condition::new(
                format!("domain_remove_failed:{}", d.id),
                d.tenant_id.clone(),
                json!({}),
            ));
        }
        if d.manual == 1 {
            out.push(Condition::new(
                format!("delivery_events_manual:{}", d.id),
                d.tenant_id.clone(),
                json!({}),
            ));
        }
    }

    // Erasure requests failed without a resumption, or past `deadline_at − 4 h`.
    #[derive(Deserialize)]
    struct Era {
        id: String,
        tenant_id: Option<String>,
        status: String,
        deadline_at: i64,
    }
    let eras: Vec<Era> = db
        .query(Stmt::new(
            "SELECT e.id, e.tenant_id, e.status, e.deadline_at FROM erasure_requests e WHERE (e.status = 'failed' AND NOT EXISTS (SELECT 1 FROM erasure_requests r WHERE r.resumes_id = e.id)) OR (e.status IN ('queued','running') AND e.deadline_at - 14400000 < ?1)",
            vec![now.into()],
        ))
        .await?;
    for e in eras {
        let name = if e.status == "failed" {
            "erasure_failed"
        } else {
            "erasure_overdue"
        };
        out.push(Condition::new(
            format!("{name}:{}", e.id),
            e.tenant_id,
            json!({ "deadline_at": e.deadline_at }),
        ));
    }

    // Jobs that failed in the last 7 days with no later completed job of the kind and tenant.
    #[derive(Deserialize)]
    struct Job {
        id: String,
        kind: String,
        tenant_id: Option<String>,
    }
    let jobs: Vec<Job> = db
        .query(Stmt::new(
            "SELECT j.id, j.kind, j.tenant_id FROM jobs j WHERE j.kind <> 'erasure' AND j.status = 'failed' AND j.updated_at > ?1 \
             AND NOT EXISTS (SELECT 1 FROM jobs k WHERE k.kind = j.kind AND k.tenant_id IS j.tenant_id AND k.status = 'completed' AND k.created_at > j.created_at) \
             ORDER BY j.kind, j.created_at LIMIT 200",
            vec![(now - 7 * DAY_MS).into()],
        ))
        .await?;
    let mut by_kind: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for j in jobs {
        let e = by_kind.entry(j.kind).or_default();
        e.0.push(j.id);
        if let Some(t) = j.tenant_id
            && !e.1.contains(&t)
        {
            e.1.push(t);
        }
    }
    for (kind, (ids, tenants)) in by_kind {
        out.push(Condition::new(
            format!("job_failed:{kind}"),
            None,
            json!({ "job_ids": ids, "tenant_ids": tenants }),
        ));
    }

    // The external heartbeat's key (§ 5.5).
    if let Some(key_id) = p.config().heartbeat_key_id.as_deref() {
        #[derive(Deserialize)]
        struct K {
            last_used_at: Option<i64>,
            created_at: i64,
            revoked_at: Option<i64>,
        }
        let k: Option<K> = db
            .first(Stmt::new(
                "SELECT last_used_at, created_at, revoked_at FROM api_keys WHERE id = ?1",
                vec![key_id.into()],
            ))
            .await?;
        let missing = match &k {
            None => true,
            Some(k) => {
                k.revoked_at.is_some() || now - k.last_used_at.unwrap_or(k.created_at) > 3_600_000
            }
        };
        if missing {
            out.push(Condition::new(
                "heartbeat_missing",
                None,
                json!({ "last_used_at": k.and_then(|k| k.last_used_at) }),
            ));
        }
    }

    // Automatic containment (§ 5.6): `partner_suspended` acts here, from the audit rows.
    partner_containment(p, now).await?;
    // `containment:{rule}` is a level: true while its effect lasts.
    for (name, rule) in [
        ("read_only", "read_only"),
        ("emergency_prune", "emergency_prune"),
        ("free_sending", "free_sending_off"),
        ("agent_signup", "agent_signup_off"),
    ] {
        if crate::ops::switches::away_from_default(p, name).await {
            out.push(Condition::new(
                format!("containment:{rule}"),
                None,
                json!({ "switch": name }),
            ));
        }
    }
    #[derive(Deserialize)]
    struct Ids {
        ids: Option<String>,
    }
    let paused: Option<Ids> = db
        .first(Stmt::new(
            "SELECT group_concat(id) AS ids FROM tenants WHERE sending_paused_at IS NOT NULL AND sending_pause_reason = 'abuse_burst'",
            vec![],
        ))
        .await?;
    if let Some(ids) = paused.and_then(|p| p.ids) {
        out.push(Condition::new(
            "containment:tenant_paused",
            None,
            json!({ "tenant_ids": ids.split(',').collect::<Vec<_>>() }),
        ));
    }
    let suspended: Option<Ids> = db
        .first(Stmt::new(
            "SELECT group_concat(p.id) AS ids FROM partners p WHERE p.status = 'suspended' AND              (SELECT a.action FROM audit_log a WHERE a.target_id = p.id AND a.action IN ('partner.auto_suspend','partner.update') ORDER BY a.created_at DESC, a.id DESC LIMIT 1) = 'partner.auto_suspend'",
            vec![],
        ))
        .await?;
    if let Some(ids) = suspended.and_then(|p| p.ids) {
        out.push(Condition::new(
            "containment:partner_suspended",
            None,
            json!({ "partner_ids": ids.split(',').collect::<Vec<_>>() }),
        ));
    }

    // `PM_DAILY_SEND_QUOTA`: the default tenant's `deployment_day` at 80% (G3).
    if let Some(quota) = p.config().daily_send_quota {
        #[derive(Deserialize)]
        struct T {
            id: String,
            quota_do_id: String,
        }
        let t: Option<T> = db
            .first(Stmt::new(
                "SELECT id, quota_do_id FROM tenants WHERE address_suffix = '' AND quota_do_id <> '' LIMIT 1",
                vec![],
            ))
            .await?;
        if let Some(t) = t
            && let Ok(v) = crate::quota::call(
                p,
                &t.quota_do_id,
                &t.id,
                "alert_evaluator",
                crate::quota::QuotaRequest::DeploymentStatus { quota },
            )
            .await
        {
            let count = v["count"].as_u64().unwrap_or(0);
            if slo::capacity_at(count, u64::from(quota), 80) {
                out.push(Condition::new(
                    "provider_quota_80",
                    None,
                    json!({ "count": count, "quota": quota }),
                ));
            }
        }
    }
    Ok(out)
}

/// `partner_suspended` (§ 5.6): a partner with 5 or more `identity.auto_pause` rows in the last hour
/// over at least 3 of its tenants, or 3 or more of its tenants sending-paused automatically in the last
/// 24 hours, is suspended (`partner.auto_suspend`, with the trigger). One tenant's abuse alone never
/// suspends its partner, and rows from before the partner's last update (a platform key's
/// reactivation) do not count again.
async fn partner_containment<P: Platform>(
    p: &P,
    now: i64,
) -> Result<(), pylota_identity_platform::PlatformError> {
    #[derive(Deserialize)]
    struct Hit {
        partner_id: String,
        rows: i64,
        tenants: i64,
        rule: String,
    }
    let hits: Vec<Hit> = p
        .db()
        .query(Stmt::new(
            "SELECT t.partner_id, COUNT(*) AS rows, COUNT(DISTINCT a.tenant_id) AS tenants, 'identity_pauses' AS rule              FROM audit_log a JOIN tenants t ON t.id = a.tenant_id JOIN partners pt ON pt.id = t.partner_id              WHERE a.action = 'identity.auto_pause' AND a.created_at > ?1 AND pt.status <> 'suspended' AND a.created_at > COALESCE((SELECT MAX(u.created_at) FROM audit_log u WHERE u.action = 'partner.update' AND u.target_id = t.partner_id), 0) GROUP BY t.partner_id HAVING COUNT(*) >= 5 AND COUNT(DISTINCT a.tenant_id) >= 3              UNION ALL              SELECT t.partner_id, COUNT(*) AS rows, COUNT(DISTINCT a.tenant_id) AS tenants, 'tenant_pauses' AS rule              FROM audit_log a JOIN tenants t ON t.id = a.tenant_id JOIN partners pt ON pt.id = t.partner_id              WHERE a.action = 'tenant.auto_pause' AND a.created_at > ?2 AND pt.status <> 'suspended' AND a.created_at > COALESCE((SELECT MAX(u.created_at) FROM audit_log u WHERE u.action = 'partner.update' AND u.target_id = t.partner_id), 0) GROUP BY t.partner_id HAVING COUNT(DISTINCT a.tenant_id) >= 3",
            vec![(now - 3_600_000).into(), (now - DAY_MS).into()],
        ))
        .await?;
    for h in hits {
        let changed = p
            .db()
            .batch(vec![
                Stmt::new(
                    "UPDATE partners SET status = 'suspended', updated_at = ?2 WHERE id = ?1 AND status <> 'suspended'",
                    vec![h.partner_id.clone().into(), now.into()],
                ),
                audit::insert(
                    &p.ids().new_id(IdPrefix::Audit),
                    None,
                    None,
                    "partner.auto_suspend",
                    "partner",
                    &h.partner_id,
                    &json!({ "rule": h.rule, "rows": h.rows, "tenants": h.tenants }),
                    "containment",
                    now,
                ),
            ])
            .await?;
        if changed.first().is_some_and(|m| m.changes == 1) {
            crate::log::security(
                "containment_partner_suspended",
                &[("detail", h.partner_id.clone().into())],
            );
            crate::metrics::count_with(p, "containment_total", None, &["partner_suspended"], 1);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The alert list is the design's § 5.3 table: the same keys in order, with their class,
    /// severity and runbook anchor; every anchor exists in the document.
    #[test]
    fn table_matches_design() {
        let doc = include_str!("../../../../docs/src/project/design/observability.md");
        let start = doc.find("### 5.3 Alert list").unwrap();
        let end = doc.find("### 5.4 The state alert evaluator").unwrap();
        let rows: Vec<&str> = doc[start..end]
            .lines()
            .filter(|l| l.starts_with("| `"))
            .collect();
        assert_eq!(rows.len(), ALERTS.len());
        for (row, a) in rows.iter().zip(ALERTS) {
            let key = if a.param.is_empty() {
                format!("`{}`", a.name)
            } else {
                format!("`{}:{}`", a.name, a.param)
            };
            assert!(row.starts_with(&format!("| {key} |")), "{row}");
            let class = match a.class {
                Class::A => "| A |",
                Class::B => "| B |",
                Class::BC => "| B + C |",
                Class::C => "| C |",
            };
            assert!(row.contains(class), "{} class: {row}", a.name);
            if a.runbook.contains(".md#") {
                assert!(row.contains(&format!("({})", a.runbook)), "{}", a.name);
            } else if !a.runbook.is_empty() {
                assert!(row.contains(&format!("(#{})", a.runbook)), "{}", a.name);
                let slug = |h: &str| -> String {
                    h.to_lowercase()
                        .chars()
                        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
                        .collect::<String>()
                        .replace(' ', "-")
                };
                assert!(
                    doc.lines()
                        .filter_map(|l| l.strip_prefix("### "))
                        .any(|h| slug(h) == a.runbook),
                    "{} runbook heading",
                    a.name
                );
            }
        }
    }

    /// `worker::ops::alerts::capacity_rules` (J30, J31): 70% of a limit is a ticket, 80% a page, for D1,
    /// vectors and namespaces (tenants that are not erased).
    #[test]
    fn capacity_rules() {
        let keys = |c: &Capacity| -> Vec<(String, Severity)> {
            capacity_conditions(c)
                .into_iter()
                .map(|c| (c.key, c.severity))
                .collect()
        };
        let none = Capacity {
            d1_bytes: Some(1),
            vectors: None,
            tenants: 10,
            measured_at: 0,
        };
        assert!(keys(&none).is_empty());
        let c = Capacity {
            d1_bytes: Some(8_000_000_000),
            vectors: Some(14_000_000),
            tenants: 35_000,
            measured_at: 0,
        };
        assert_eq!(
            keys(&c),
            [
                ("capacity_70:d1".to_owned(), Severity::Ticket),
                ("capacity_80:d1".to_owned(), Severity::Page),
                ("capacity_70:vectors".to_owned(), Severity::Ticket),
                ("capacity_70:namespaces".to_owned(), Severity::Ticket),
            ]
        );
    }

    #[test]
    fn keys_and_severities() {
        assert_eq!(name_of("dlq:pm-inbound"), "dlq");
        assert_eq!(default_severity("dlq:pm-inbound"), Severity::Page);
        assert_eq!(default_severity("webhook_failing:whk_1"), Severity::Ticket);
        assert_eq!(default_severity("job_failed:retention"), Severity::Page);
        assert_eq!(default_severity("job_failed:reindex"), Severity::Ticket);
        assert_eq!(persistence("erasure_stalled"), Persistence::SelfResolved);
        assert_eq!(persistence("vector_drift"), Persistence::Reported);
        assert_eq!(persistence("dlq"), Persistence::Level);
        assert!(runbook_url("https://x/docs", "dlq").ends_with("observability.html#dlq-growth"));
    }
}
