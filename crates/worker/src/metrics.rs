//! Metrics (Observability § 3): every data point goes to `METRICS` with one layout, `index1` the
//! tenant (or `platform`), `blob1` the name, `blob2` `PM_ENV`, `blob3…` the catalogued labels in
//! order, `double1` the value. Counters are summed in memory by `(index, name, labels)` and written
//! when the invocation flushes (the entry points call [`flush`]); observations are one point each. A
//! flush writes at most [`CAP`] points and counts the rest in `metrics_dropped_total`. With
//! `PM_ENV = local` each point is also a `metric` log line at debug level. Writes never fail a request.

use std::cell::RefCell;
use std::collections::BTreeMap;

use pylota_identity_platform::config::DeployEnv;
use pylota_identity_platform::metrics::DataPoint;
use pylota_identity_platform::{Metrics as _, Platform};

/// Points per flush (the platform allows 250 per invocation).
pub const CAP: usize = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Counter,
    Observation,
}

/// One catalogue entry: name, kind and labels in `blob3…` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metric {
    pub name: &'static str,
    pub kind: Kind,
    pub labels: &'static [&'static str],
}

const fn m(name: &'static str, kind: Kind, labels: &'static [&'static str]) -> Metric {
    Metric { name, kind, labels }
}

/// The catalogue of Observability § 3.2, in its order.
pub const CATALOGUE: &[Metric] = &[
    m(
        "http_requests_total",
        Kind::Counter,
        &["route", "method", "status_class", "code"],
    ),
    m("http_ms", Kind::Observation, &["route"]),
    m("rate_limited_total", Kind::Counter, &["bucket"]),
    m("inbound_received_total", Kind::Counter, &["result"]),
    m("inbound_r2_retries_total", Kind::Counter, &[]),
    m("inbound_processed_total", Kind::Counter, &["outcome"]),
    m("inbound_ingest_ms", Kind::Observation, &[]),
    m("inbound_lost_total", Kind::Counter, &[]),
    m("inbound_raw_missing_total", Kind::Counter, &[]),
    m("inbound_orphan_raw_total", Kind::Counter, &[]),
    m("inbound_staged_unroutable_total", Kind::Counter, &[]),
    m(
        "inbound_dropped_total",
        Kind::Counter,
        &["reason", "source"],
    ),
    m(
        "ses_sns_rejected_total",
        Kind::Counter,
        &["endpoint", "reason"],
    ),
    m("ses_auth_disagreement_total", Kind::Counter, &["check"]),
    m("ses_object_lost_total", Kind::Counter, &[]),
    m("scanner_error_total", Kind::Counter, &[]),
    m("auth_dns_cache_miss_total", Kind::Counter, &[]),
    m("backscatter_total", Kind::Counter, &[]),
    m("inbound_throttled_total", Kind::Counter, &["cap"]),
    m("thread_token_invalid_total", Kind::Counter, &[]),
    m("thread_token_previous_key_total", Kind::Counter, &[]),
    m("thread_token_unchecked_total", Kind::Counter, &[]),
    m("quarantine_total", Kind::Counter, &["reason"]),
    m(
        "send_api_requests_total",
        Kind::Counter,
        &["operation", "result"],
    ),
    m("send_api_ms", Kind::Observation, &["operation"]),
    m(
        "outbound_queue_to_transport_ms",
        Kind::Observation,
        &["transport"],
    ),
    m(
        "transport_outcomes_total",
        Kind::Counter,
        &["transport", "outcome", "provider_code"],
    ),
    m(
        "recipients_submitted_total",
        Kind::Counter,
        &["transport", "domain_id"],
    ),
    m(
        "bounces_total",
        Kind::Counter,
        &["domain_id", "bounce_type"],
    ),
    m("complaints_total", Kind::Counter, &["domain_id"]),
    m(
        "delivery_events_total",
        Kind::Counter,
        &["transport", "type"],
    ),
    m("delivery_orphaned_total", Kind::Counter, &[]),
    m("delivery_unknown_type_total", Kind::Counter, &[]),
    m("delivery_unroutable_total", Kind::Counter, &[]),
    m("reconcile_ambiguous_total", Kind::Counter, &[]),
    m("uncertain_total", Kind::Counter, &["transport"]),
    m("reconciled_total", Kind::Counter, &["transport"]),
    m("fallback_sends_total", Kind::Counter, &["domain_id"]),
    m("suppressed_recipients_total", Kind::Counter, &["reason"]),
    m(
        "provider_quota_errors_total",
        Kind::Counter,
        &["transport", "provider_code"],
    ),
    m("backup_objects_total", Kind::Counter, &["result"]),
    m("alert_emails_total", Kind::Counter, &["severity", "result"]),
    m("containment_total", Kind::Counter, &["rule"]),
    m("webhook_attempts_total", Kind::Counter, &["result"]),
    m(
        "webhook_delivery_latency_ms",
        Kind::Observation,
        &["event_class", "first_attempt"],
    ),
    m("webhook_dead_total", Kind::Counter, &["event_class"]),
    m("webhook_disabled_total", Kind::Counter, &["reason"]),
    m("outbox_undispatched_age_ms", Kind::Observation, &["owner"]),
    m("webhook_ssrf_blocked_total", Kind::Counter, &[]),
    m("identity_keys_total", Kind::Counter, &["op"]),
    m("signatures_total", Kind::Counter, &["kind", "result"]),
    m(
        "well_known_requests_total",
        Kind::Counter,
        &["endpoint", "status_class"],
    ),
    m("notifications_sent_total", Kind::Counter, &["kind"]),
    m(
        "notifications_failed_total",
        Kind::Counter,
        &["kind", "reason"],
    ),
    m("notifications_deferred_total", Kind::Counter, &["reason"]),
    m(
        "notification_unsubscribes_total",
        Kind::Counter,
        &["kind", "result"],
    ),
    m(
        "usage_alerts_total",
        Kind::Counter,
        &["feature", "threshold"],
    ),
    m("quota_hold_denied_total", Kind::Counter, &["feature"]),
    m("quota_hold_expired_total", Kind::Counter, &["feature"]),
    m("quota_consumed_without_hold_total", Kind::Counter, &[]),
    m("quota_count_drift_total", Kind::Counter, &["feature"]),
    m("stripe_webhook_total", Kind::Counter, &["type", "outcome"]),
    m("stripe_webhook_rejected_total", Kind::Counter, &[]),
    m("stripe_api_errors_total", Kind::Counter, &["call"]),
    m("stripe_duplicate_subscriptions_total", Kind::Counter, &[]),
    m("billing_increase_held_total", Kind::Counter, &["feature"]),
    m(
        "system_mail_budget_denied_total",
        Kind::Counter,
        &["class", "scope"],
    ),
    m("ses_control_throttled_total", Kind::Counter, &[]),
    m(
        "search_requests_total",
        Kind::Counter,
        &["mode", "scope", "result"],
    ),
    m("search_ms", Kind::Observation, &["mode", "scope", "fanout"]),
    m("agentic_requests_total", Kind::Counter, &["status"]),
    m("agentic_ms", Kind::Observation, &[]),
    m("agentic_first_evidence_ms", Kind::Observation, &[]),
    m("ai_calls_total", Kind::Counter, &["purpose", "result"]),
    m("index_jobs_total", Kind::Counter, &["kind", "result"]),
    m("vector_count_drift", Kind::Observation, &[]),
    m("triage_total", Kind::Counter, &["status"]),
    m(
        "job_steps_total",
        Kind::Counter,
        &["kind", "step", "result"],
    ),
    m("erasure_ms", Kind::Observation, &["scope"]),
    m("retention_purged_total", Kind::Counter, &["store"]),
    m(
        "domain_checks_total",
        Kind::Counter,
        &["resolver", "outcome"],
    ),
    m("domain_transitions_total", Kind::Counter, &["from", "to"]),
    m("mailbox_size_bytes", Kind::Observation, &[]),
    m("dlq_items", Kind::Observation, &["queue"]),
    m("rpc_owner_mismatch_total", Kind::Counter, &["class"]),
    m("alert_fired", Kind::Counter, &["alert", "severity"]),
    m("panics_total", Kind::Counter, &[]),
    m("config_invalid_total", Kind::Counter, &[]),
    m("metrics_dropped_total", Kind::Counter, &[]),
    m("inbound_dictionary_guard_total", Kind::Counter, &[]),
    m("role_mail_failed_total", Kind::Counter, &["reason"]),
    m("dsn_untrusted_total", Kind::Counter, &["reason"]),
    m("r2_orphans_deleted_total", Kind::Counter, &[]),
    m("loopback_refused_total", Kind::Counter, &[]),
    m("stale_claim_outcome_total", Kind::Counter, &[]),
    m("index_job_unimplemented_total", Kind::Counter, &["kind"]),
    m("ai_deferred_total", Kind::Counter, &["kind"]),
    m("ai_rate_limited_total", Kind::Counter, &["kind"]),
    m("triage_model_cap_total", Kind::Counter, &["kind"]),
    m("drafts_written_total", Kind::Counter, &["op"]),
    m("draft_sends_total", Kind::Counter, &["origin", "outcome"]),
    m("draft_send_failures_total", Kind::Counter, &["reason"]),
    m(
        "draft_decisions_total",
        Kind::Counter,
        &["decision", "actor", "send"],
    ),
    m(
        "draft_approval_rerequested_total",
        Kind::Counter,
        &["cause"],
    ),
    m("draft_fire_lag_ms", Kind::Observation, &[]),
    m("draft_claims_expired_total", Kind::Counter, &["origin"]),
    m("drafts_alarm_missed_total", Kind::Counter, &[]),
    m("draft_index_write_failures_total", Kind::Counter, &[]),
    m("send_approval_required_total", Kind::Counter, &["level"]),
    m("event_log_append_total", Kind::Counter, &["result"]),
    m(
        "event_log_append_failures_total",
        Kind::Counter,
        &["reason"],
    ),
    m("event_log_lag_ms", Kind::Observation, &[]),
    m("event_log_pruned_total", Kind::Counter, &["reason"]),
    m("event_log_erase_failures_total", Kind::Counter, &[]),
    m("event_log_payload_bytes", Kind::Observation, &[]),
    m(
        "event_feed_reads_total",
        Kind::Counter,
        &["surface", "result"],
    ),
    m("event_consumer_backlog", Kind::Observation, &["state"]),
    m("event_stream_connections", Kind::Observation, &[]),
    m(
        "event_stream_opened_total",
        Kind::Counter,
        &["auth", "result"],
    ),
    m("event_stream_closed_total", Kind::Counter, &["code", "by"]),
    m("event_stream_sent_total", Kind::Counter, &[]),
    m("event_stream_tokens_total", Kind::Counter, &["result"]),
    m(
        "event_stream_revalidation_failures_total",
        Kind::Counter,
        &[],
    ),
    m(
        "screening_verdict_total",
        Kind::Counter,
        &["mode", "status", "reason"],
    ),
    m("screening_latency_ms", Kind::Observation, &["mode"]),
    m("screening_deadline_lag_ms", Kind::Observation, &[]),
    m("screening_model_calls_total", Kind::Counter, &["outcome"]),
    m("screening_model_capped_total", Kind::Counter, &["scope"]),
    m("screening_model_ms", Kind::Observation, &[]),
    m("screening_deferred_total", Kind::Counter, &["cause"]),
    m("screening_queue_wait_ms", Kind::Observation, &[]),
    m("screening_late_signal_total", Kind::Counter, &["outcome"]),
    m("screening_late_verdict_total", Kind::Counter, &["verdict"]),
    m("screening_deadline_extended_total", Kind::Counter, &[]),
    m(
        "screening_feedback_total",
        Kind::Counter,
        &["screening_status", "verdict"],
    ),
    m("screening_test_header_ignored_total", Kind::Counter, &[]),
    m(
        "calendar_objects_total",
        Kind::Counter,
        &["method", "object_status"],
    ),
    m(
        "calendar_verification_total",
        Kind::Counter,
        &["method", "result"],
    ),
    m(
        "calendar_tz_resolution_total",
        Kind::Counter,
        &["resolution"],
    ),
    m("calendar_events_total", Kind::Counter, &["trigger"]),
    m("calendar_sends_total", Kind::Counter, &["method", "result"]),
    m(
        "calendar_replies_total",
        Kind::Counter,
        &["partstat", "result"],
    ),
    m("calendar_vtimezones_added_total", Kind::Counter, &[]),
    m("calendar_properties_removed_total", Kind::Counter, &[]),
    m("uploads_created_total", Kind::Counter, &[]),
    m("upload_puts_total", Kind::Counter, &["result"]),
    m("upload_bytes", Kind::Observation, &[]),
    m("upload_put_ms", Kind::Observation, &[]),
    m("upload_classify_wait_total", Kind::Counter, &["result"]),
    m("upload_scanner_error_total", Kind::Counter, &[]),
    m("uploads_expired_total", Kind::Counter, &["reason"]),
    m("upload_attachments_total", Kind::Counter, &["mode"]),
    m("upload_copy_ms", Kind::Observation, &[]),
    m(
        "metrics_requests_total",
        Kind::Counter,
        &["route", "scope", "status_class"],
    ),
    m("metrics_ms", Kind::Observation, &["route"]),
    m("metrics_fanout_partial_total", Kind::Counter, &[]),
    m("rates_tripped_reads_total", Kind::Counter, &["action"]),
    m("agent_signups_total", Kind::Counter, &["result"]),
    m("agent_signup_codes_total", Kind::Counter, &["result"]),
    m("agent_signup_verified_total", Kind::Counter, &["via"]),
    m("agent_signup_ended_total", Kind::Counter, &["reason"]),
    m("agent_signup_mail_total", Kind::Counter, &["result"]),
    m("agent_signup_refusals_total", Kind::Counter, &["reason"]),
    m(
        "api_client_requests_total",
        Kind::Counter,
        &["client", "client_version"],
    ),
    m("messages_batch_total", Kind::Counter, &["op", "result"]),
    m("messages_batch_ids", Kind::Observation, &["op"]),
    m("label_events_total", Kind::Counter, &["source"]),
    m(
        "list_decisions_total",
        Kind::Counter,
        &["direction", "scope", "kind"],
    ),
    m("arc_rescue_total", Kind::Counter, &["result", "source"]),
    m("retention_expired_total", Kind::Counter, &["store"]),
    m("retention_expiry_lag_ms", Kind::Observation, &[]),
    m("retention_expiry_errors_total", Kind::Counter, &["step"]),
    m("webhook_header_updates_total", Kind::Counter, &["result"]),
    m("zone_exports_total", Kind::Counter, &["method"]),
    m("phone_webhook_total", Kind::Counter, &["type", "result"]),
    m("phone_webhook_ack_ms", Kind::Observation, &[]),
    m("phone_event_mismatch_total", Kind::Counter, &[]),
    m("phone_request_total", Kind::Counter, &["kind", "status"]),
    m("phone_order_attempt_total", Kind::Counter, &["outcome"]),
    m("phone_order_uncertain_total", Kind::Counter, &[]),
    m("phone_numbers_live", Kind::Observation, &["status"]),
    m("phone_facet_failures_total", Kind::Counter, &["facet"]),
    m("phone_buy_total", Kind::Counter, &["result"]),
    m("phone_buy_inline_ms", Kind::Observation, &[]),
    m("phone_budget_covered_total", Kind::Counter, &["result"]),
    m(
        "phone_setup_status",
        Kind::Observation,
        &["country", "status"],
    ),
    m(
        "phone_capability_wait_seconds",
        Kind::Observation,
        &["capability"],
    ),
    m("phone_expiring_release_total", Kind::Counter, &["result"]),
    m("sms_send_total", Kind::Counter, &["result"]),
    m("sms_dispatch_ms", Kind::Observation, &[]),
    m("sms_receipt_total", Kind::Counter, &["status"]),
    m("sms_sender_substituted_total", Kind::Counter, &[]),
    m("sms_inbound_total", Kind::Counter, &["status"]),
    m("sms_wait_total", Kind::Counter, &["result"]),
    m("sms_vault_code_total", Kind::Counter, &["result"]),
    m(
        "call_total",
        Kind::Counter,
        &["direction", "route", "outcome"],
    ),
    m("phone_spend_pence_total", Kind::Counter, &["kind"]),
    m("phone_spend_cap_denied_total", Kind::Counter, &["scope"]),
    m("phone_charge_drift_pence", Kind::Observation, &[]),
    m("phone_provider_balance_usd", Kind::Observation, &[]),
    m("phone_provider_errors_total", Kind::Counter, &["code"]),
    m("phone_reconcile_drift_total", Kind::Counter, &["step"]),
    m("phone_webhook_missed_total", Kind::Counter, &[]),
    m("phone_port_out_open", Kind::Observation, &[]),
    m("vault_uses_total", Kind::Counter, &["operation", "outcome"]),
    m(
        "vault_redemptions_total",
        Kind::Counter,
        &["phase", "result"],
    ),
    m("vault_redeem_ms", Kind::Observation, &["phase"]),
    m("vault_consumer_reports_total", Kind::Counter, &["outcome"]),
    m("vault_value_exposed_total", Kind::Counter, &["source"]),
    m("vault_placeholder_misplaced_total", Kind::Counter, &[]),
    m("vault_unlock_failures_total", Kind::Counter, &[]),
    m("vault_audit_append_failures_total", Kind::Counter, &[]),
    m("vault_decrypt_failed_total", Kind::Counter, &["where"]),
    m("vault_consumer_auth_failures_total", Kind::Counter, &[]),
];

pub fn lookup(name: &str) -> Option<&'static Metric> {
    CATALOGUE.iter().find(|m| m.name == name)
}

type CounterKey = (String, &'static str, Vec<String>);

#[derive(Default)]
struct Buffer {
    counters: BTreeMap<CounterKey, f64>,
    observations: Vec<(String, &'static str, Vec<String>, f64)>,
}

thread_local! {
    static BUFFER: RefCell<Buffer> = RefCell::new(Buffer::default());
}

/// Counts `n` of `name` for `tenant` (or the platform), without labels.
pub fn count<P: Platform>(p: &P, name: &'static str, tenant: Option<&str>, n: u32) {
    count_with(p, name, tenant, &[], n);
}

/// Counts `n` of `name` with `labels` in catalogue order (missing trailing labels are empty).
pub fn count_with<P: Platform>(
    _p: &P,
    name: &'static str,
    tenant: Option<&str>,
    labels: &[&str],
    n: u32,
) {
    if n == 0 {
        return;
    }
    let key = (
        tenant.unwrap_or("platform").to_owned(),
        name,
        labels.iter().map(|l| (*l).to_owned()).collect(),
    );
    BUFFER.with(|b| *b.borrow_mut().counters.entry(key).or_insert(0.0) += f64::from(n));
}

/// One observation of `name` (a duration in milliseconds, a size in bytes, a count).
pub fn observe<P: Platform>(
    _p: &P,
    name: &'static str,
    tenant: Option<&str>,
    labels: &[&str],
    value: f64,
) {
    let tenant = tenant.unwrap_or("platform").to_owned();
    let labels = labels.iter().map(|l| (*l).to_owned()).collect();
    BUFFER.with(|b| {
        b.borrow_mut()
            .observations
            .push((tenant, name, labels, value))
    });
}

/// The data point of one metric.
pub fn point(env: DeployEnv, tenant: &str, name: &str, labels: &[String], value: f64) -> DataPoint {
    let mut blobs = vec![name.to_owned(), env_name(env).to_owned()];
    blobs.extend(labels.iter().take(6).cloned());
    DataPoint {
        indexes: [tenant.to_owned()],
        blobs,
        doubles: vec![value],
    }
}

fn env_name(env: DeployEnv) -> &'static str {
    match env {
        DeployEnv::Production => "production",
        DeployEnv::Staging => "staging",
        DeployEnv::Local => "local",
    }
}

/// Writes what the invocation counted and observed: counters first, then observations, at most
/// [`CAP`] points, the overflow counted in `metrics_dropped_total`.
pub fn flush<P: Platform>(p: &P) {
    let Buffer {
        counters,
        observations,
    } = BUFFER.with(|b| std::mem::take(&mut *b.borrow_mut()));
    let env = p.config().env;
    let mut points: Vec<(String, &'static str, Vec<String>, f64)> = counters
        .into_iter()
        .map(|((t, n, l), v)| (t, n, l, v))
        .collect();
    points.extend(observations);
    let dropped = points.len().saturating_sub(CAP);
    if dropped > 0 {
        points.truncate(CAP - 1);
        points.push((
            "platform".to_owned(),
            "metrics_dropped_total",
            Vec::new(),
            dropped as f64 + 1.0,
        ));
    }
    for (tenant, name, labels, value) in points {
        if env == DeployEnv::Local {
            let mut fields: Vec<(&'static str, crate::log::Field)> = vec![
                ("name", name.into()),
                ("tenant_id", tenant.clone().into()),
                ("value", crate::log::Field::Num(value)),
            ];
            if let Some(m) = lookup(name) {
                for (k, v) in m.labels.iter().zip(&labels) {
                    fields.push((k, v.clone().into()));
                }
            }
            crate::log::debug("metric", &fields);
        }
        p.metrics().write(point(env, &tenant, name, &labels, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalogue is the design's § 3.2 table: the same names, in order, with the same kinds; at
    /// most six labels each (`blob3` … `blob8`).
    #[test]
    fn catalogue_matches_design() {
        let doc = include_str!("../../../docs/src/project/design/observability.md");
        let start = doc.find("### 3.2 Catalogue").unwrap();
        let end = doc.find("## 4. Service-level objectives").unwrap();
        let mut want = Vec::new();
        for line in doc[start..end].lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let kind = if cells[1] == "counter" {
                Kind::Counter
            } else {
                Kind::Observation
            };
            for name in cells[0].split('`').skip(1).step_by(2) {
                want.push((name.to_owned(), kind));
            }
        }
        let got: Vec<(String, Kind)> = CATALOGUE
            .iter()
            .map(|m| (m.name.to_owned(), m.kind))
            .collect();
        assert_eq!(got, want);
        assert!(CATALOGUE.iter().all(|m| m.labels.len() <= 6));
    }

    /// Every name the Worker counts or observes is catalogued, with no more labels than it has.
    #[test]
    fn every_name_in_source_is_catalogued() {
        let mut files = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src"
        ))];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("src/metrics.rs")
                {
                    files.push(p);
                }
            }
        }
        let mut unknown = Vec::new();
        for f in files {
            let src = std::fs::read_to_string(&f)
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            for call in [
                "metrics::count(",
                "metrics::count_with(",
                "metrics::observe(",
            ] {
                for (i, _) in src.match_indices(call) {
                    let rest = &src[i + call.len()..];
                    let Some(q) = rest.find('"') else { continue };
                    if q > 40 {
                        continue;
                    }
                    let name = &rest[q + 1..q + 1 + rest[q + 1..].find('"').unwrap()];
                    if lookup(name).is_none() {
                        unknown.push(format!("{}: {name}", f.display()));
                    }
                }
            }
        }
        assert!(unknown.is_empty(), "not in the catalogue: {unknown:#?}");
    }

    #[test]
    fn point_layout() {
        let p = point(
            DeployEnv::Production,
            "ten_1",
            "bounces_total",
            &["dom_1".into(), "hard".into()],
            3.0,
        );
        assert_eq!(p.indexes, ["ten_1".to_owned()]);
        assert_eq!(p.blobs, ["bounces_total", "production", "dom_1", "hard"]);
        assert_eq!(p.doubles, [3.0]);
    }
}
