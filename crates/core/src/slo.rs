//! Alert rules, pure ([Observability §4–5]): the state alert evaluator's transitions, the burn-rate
//! targets of the Custom Alerts, and the SLIs computed from the metric catalogue. The Worker's
//! `ops::alerts` reads the conditions and writes what these functions decide.
//!
//! [Observability §4–5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/observability.md

use serde::{Deserialize, Serialize};

/// A firing key re-notifies this often (another `alert_fired` point and, for `page`, another email).
pub const RENOTIFY_MS: i64 = 6 * 3_600_000;
/// A firing key resolves after its condition was false on this many consecutive runs.
pub const RESOLVE_AFTER_FALSE_RUNS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Page,
    Ticket,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Page => "page",
            Severity::Ticket => "ticket",
        }
    }
}

/// How a condition is read (§5.4 step 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persistence {
    /// Read from D1 or the objects on every run; true while the state lasts.
    Level,
    /// Reported by an object or cron: true only in the run that reads the report.
    Reported,
    /// Reported, and resolved only by its reporter (`erasure_stalled`, `billing_cancel_failed`).
    SelfResolved,
}

/// What the evaluator remembers of one firing key between runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyState {
    pub severity: Severity,
    pub fired_at: i64,
    /// The last notification (the fire or a re-notification).
    pub notified_at: i64,
    /// Re-notifications so far (`-{n}` of the alert email's idempotency key).
    pub notices: u32,
    /// Consecutive runs with the condition false.
    pub false_runs: u32,
    #[serde(default)]
    pub tenant_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transition {
    /// Write `alert.fired` (unless the reporter did), log, one `alert_fired` point, email for `page`.
    Fire,
    /// One more `alert_fired` point and, for `page`, another email; no audit row.
    Renotify {
        n: u32,
    },
    /// Write `alert.resolved`.
    Resolve,
    None,
}

/// One run for one key: the state before, whether the condition holds now, and how it is read.
/// Returns the state after (`None` when not firing) and the transition to apply.
pub fn step(
    prev: Option<&KeyState>,
    condition: bool,
    severity: Severity,
    persistence: Persistence,
    tenant_id: Option<&str>,
    now: i64,
) -> (Option<KeyState>, Transition) {
    match (prev, condition) {
        (None, false) => (None, Transition::None),
        (None, true) => (
            Some(KeyState {
                severity,
                fired_at: now,
                notified_at: now,
                notices: 0,
                false_runs: 0,
                tenant_id: tenant_id.map(str::to_owned),
            }),
            Transition::Fire,
        ),
        (Some(s), true) => {
            let mut s = s.clone();
            s.false_runs = 0;
            // A rising severity (a capacity alert passing 80%, the breaker's stage 2) notifies at once.
            let raised = severity == Severity::Page && s.severity == Severity::Ticket;
            s.severity = severity;
            if raised || now - s.notified_at >= RENOTIFY_MS {
                s.notices += 1;
                s.notified_at = now;
                let n = s.notices;
                (Some(s), Transition::Renotify { n })
            } else {
                (Some(s), Transition::None)
            }
        }
        (Some(s), false) => {
            if persistence == Persistence::SelfResolved {
                return (Some(s.clone()), Transition::None);
            }
            let mut s = s.clone();
            s.false_runs += 1;
            if s.false_runs >= RESOLVE_AFTER_FALSE_RUNS {
                (None, Transition::Resolve)
            } else {
                (Some(s), Transition::None)
            }
        }
    }
}

/// The alert email's `Idempotency-Key`: `alert-{key}-{fired_at}`, and `-{n}` for a re-notification.
pub fn alert_email_key(alert_key: &str, fired_at: i64, notice: u32) -> String {
    if notice == 0 {
        format!("alert-{alert_key}-{fired_at}")
    } else {
        format!("alert-{alert_key}-{fired_at}-{notice}")
    }
}

/// The Custom Alert target of an SLO-detection burn-rate rule (§5.2): `100 × (1 − burn × (1 − SLO))`,
/// with the SLO as a fraction (0.999).
pub fn burn_rate_target(slo: f64, burn: f64) -> f64 {
    100.0 * (1.0 - burn * (1.0 - slo))
}

/// A burn-rate rule of §5.2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BurnRule {
    pub slo: &'static str,
    pub target: f64,
    pub burn: f64,
    pub long_min: u32,
    pub short_min: u32,
    pub severity: Severity,
}

/// The availability burn-rate rules (§5.2).
pub fn burn_rules() -> Vec<BurnRule> {
    let mut out = Vec::new();
    for (slo, target) in [("NFR-REL-2", 0.999), ("NFR-REL-4", 0.9999)] {
        for (burn, long_min, short_min, severity) in [
            (14.4, 60, 5, Severity::Page),
            (6.0, 360, 30, Severity::Page),
            (3.0, 1_440, 120, Severity::Ticket),
        ] {
            out.push(BurnRule {
                slo,
                target,
                burn,
                long_min,
                short_min,
                severity,
            });
        }
    }
    out
}

/// The good share of a ratio SLI, `None` without events.
pub fn ratio(good: f64, total: f64) -> Option<f64> {
    (total > 0.0).then(|| good / total)
}

/// The share of `observations` at or below `threshold` (a latency SLI), `None` without points.
pub fn share_within(observations: &[f64], threshold: f64) -> Option<f64> {
    let n = observations.len();
    (n > 0).then(|| observations.iter().filter(|v| **v <= threshold).count() as f64 / n as f64)
}

/// Whether a rate alert's bar is crossed: `numerator / total > threshold` with at least `min` events.
pub fn rate_crossed(numerator: u64, total: u64, threshold: f64, min: u64) -> bool {
    total >= min && total > 0 && numerator as f64 / total as f64 > threshold
}

/// Whether a share of a capacity is at or above `percent`.
pub fn capacity_at(used: u64, limit: u64, percent: u64) -> bool {
    limit > 0 && used.saturating_mul(100) >= limit.saturating_mul(percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3_600_000;

    /// `core::slo::alert_state_machine` (Observability §5.4 step 3): a true condition fires once;
    /// a firing key re-notifies every 6 hours (no audit row) and at once when its severity rises;
    /// it resolves after two consecutive false runs, and one true run in between restarts the count;
    /// a reported condition resolves the same way after two runs without a report; a self-resolved
    /// one (`erasure_stalled`) stays firing until its reporter resolves it.
    #[test]
    fn alert_state_machine() {
        let sm = |prev: Option<&KeyState>, c: bool, now: i64| {
            step(prev, c, Severity::Page, Persistence::Level, None, now)
        };
        assert_eq!(sm(None, false, 0), (None, Transition::None));
        let (s, t) = sm(None, true, 0);
        assert_eq!(t, Transition::Fire);
        let s = s.unwrap();
        assert_eq!((s.fired_at, s.notices, s.false_runs), (0, 0, 0));

        // Still true: nothing until 6 hours, then one re-notification per 6 hours.
        let (s, t) = sm(Some(&s), true, 60_000);
        assert_eq!(t, Transition::None);
        let (s, t) = sm(s.as_ref(), true, 6 * H - 1);
        assert_eq!(t, Transition::None);
        let (s, t) = sm(s.as_ref(), true, 6 * H);
        assert_eq!(t, Transition::Renotify { n: 1 });
        let (s, t) = sm(s.as_ref(), true, 6 * H + 60_000);
        assert_eq!(t, Transition::None);
        let (s, t) = sm(s.as_ref(), true, 12 * H);
        assert_eq!(t, Transition::Renotify { n: 2 });
        assert_eq!(
            alert_email_key("dlq:pm-inbound", 0, 2),
            "alert-dlq:pm-inbound-0-2"
        );
        assert_eq!(
            alert_email_key("dlq:pm-inbound", 0, 0),
            "alert-dlq:pm-inbound-0"
        );

        // One false run is not enough; a true run in between restarts the count.
        let (s, t) = sm(s.as_ref(), false, 12 * H + 60_000);
        assert_eq!((t, s.as_ref().unwrap().false_runs), (Transition::None, 1));
        let (s, t) = sm(s.as_ref(), true, 12 * H + 120_000);
        assert_eq!((t, s.as_ref().unwrap().false_runs), (Transition::None, 0));
        let (s, _) = sm(s.as_ref(), false, 12 * H + 180_000);
        let (s, t) = sm(s.as_ref(), false, 12 * H + 240_000);
        assert_eq!((s, t), (None, Transition::Resolve));

        // A ticket that becomes a page notifies at once.
        let (s, _) = step(
            None,
            true,
            Severity::Ticket,
            Persistence::Level,
            Some("ten_1"),
            0,
        );
        assert_eq!(s.as_ref().unwrap().tenant_id.as_deref(), Some("ten_1"));
        let (s, t) = step(
            s.as_ref(),
            true,
            Severity::Page,
            Persistence::Level,
            None,
            60_000,
        );
        assert_eq!(t, Transition::Renotify { n: 1 });
        assert_eq!(s.unwrap().severity, Severity::Page);

        // Reported: true in the run that reads the report only.
        let rep = |prev: Option<&KeyState>, c: bool, now: i64| {
            step(prev, c, Severity::Ticket, Persistence::Reported, None, now)
        };
        let (s, t) = rep(None, true, 0);
        assert_eq!(t, Transition::Fire);
        let (s, _) = rep(s.as_ref(), false, 60_000);
        let (s, t) = rep(s.as_ref(), false, 120_000);
        assert_eq!((s, t), (None, Transition::Resolve));

        // Self-resolved: no report for days and it still fires.
        let sr = |prev: Option<&KeyState>, c: bool, now: i64| {
            step(
                prev,
                c,
                Severity::Page,
                Persistence::SelfResolved,
                None,
                now,
            )
        };
        let (mut s, _) = sr(None, true, 0);
        for run in 1..10 {
            let (next, t) = sr(s.as_ref(), false, run * 60_000);
            assert_eq!(t, Transition::None);
            s = next;
        }
        assert!(s.is_some());
    }

    /// The §5.2 targets: 14.4×, 6× and 3× of NFR-REL-2 and NFR-REL-4.
    #[test]
    fn burn_rate_targets() {
        let t: Vec<(String, String)> = burn_rules()
            .iter()
            .map(|r| {
                (
                    r.slo.to_owned(),
                    format!(
                        "{}",
                        (burn_rate_target(r.target, r.burn) * 1000.0).round() / 1000.0
                    ),
                )
            })
            .collect();
        let want = [
            ("NFR-REL-2", "98.56"),
            ("NFR-REL-2", "99.4"),
            ("NFR-REL-2", "99.7"),
            ("NFR-REL-4", "99.856"),
            ("NFR-REL-4", "99.94"),
            ("NFR-REL-4", "99.97"),
        ];
        assert_eq!(
            t,
            want.iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn sli_helpers() {
        assert_eq!(ratio(999.0, 1000.0), Some(0.999));
        assert_eq!(ratio(0.0, 0.0), None);
        assert_eq!(
            share_within(&[100.0, 200.0, 600.0, 400.0], 500.0),
            Some(0.75)
        );
        assert!(!rate_crossed(2, 49, 0.02, 50), "too few outcomes");
        assert!(!rate_crossed(1, 50, 0.02, 50), "exactly 2% is not over");
        assert!(rate_crossed(2, 50, 0.02, 50));
        assert!(capacity_at(7_000_000_000, 10_000_000_000, 70));
        assert!(!capacity_at(6_999_999_999, 10_000_000_000, 70));
    }
}
