//! The nightly reconciliation of the semantic index ([Search § 6.6], [F14]). On the `*/15` tick of
//! 02:00–02:14 UTC one `Reconcile` job per identity goes to `pm-index` and the run's summary row is
//! written; from the 03:00 tick on, the run is evaluated once every identity has reported: the
//! index's vector count against `Σ embedded_rows`. The `vector_drift` alert fires when this run and the
//! previous one are both more than 1% away; a run still incomplete at 23:45 is left unevaluated.
//!
//! [Search § 6.6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
//! [F14]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/edge-cases.md

use pylota_identity_api_types::internal::IndexJob;
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{
    Clock as _, ControlDb as _, Platform, QueueProducer as _, VectorIndex as _,
};
use serde::Deserialize;

/// Drift beyond this many percent, two nights running, alerts.
pub const DRIFT_PCT: f64 = 1.0;
const DAY_MS: i64 = 86_400_000;

#[derive(Deserialize)]
struct Ident {
    id: String,
    tenant_id: String,
}

#[derive(Deserialize)]
struct Summary {
    queued: Option<i64>,
    index_count: Option<i64>,
    drift_pct: Option<f64>,
}

#[derive(Deserialize)]
struct Reported {
    n: i64,
    embedded: Option<i64>,
}

fn utc(now: i64) -> (String, i64, i64) {
    let date = rfc3339(now)[..10].to_owned();
    let hour = now.rem_euclid(DAY_MS) / 3_600_000;
    let minute = now.rem_euclid(3_600_000) / 60_000;
    (date, hour, minute)
}

/// One `*/15` tick.
pub async fn run<P: Platform>(p: &P) {
    let now = p.clock().now_ms();
    let (date, hour, minute) = utc(now);
    if hour == 2 && minute < 15 {
        start(p, &date, now).await;
    }
    if hour >= 3 {
        evaluate(p, &date, hour, minute, now).await;
    }
}

/// Step 1: one job per identity, then the summary row.
pub async fn start<P: Platform>(p: &P, run_date: &str, now: i64) {
    let exists: Option<Summary> = p
        .db()
        .first(Stmt::new(
            "SELECT queued, index_count, drift_pct FROM index_reconcile WHERE run_date = ?1 AND identity_id = '*'",
            vec![run_date.into()],
        ))
        .await
        .unwrap_or(None);
    if exists.is_some() {
        return;
    }
    let mut after = String::new();
    let mut queued: i64 = 0;
    loop {
        let page: Vec<Ident> = match p
            .db()
            .query(Stmt::new(
                "SELECT id, tenant_id FROM identities WHERE status IN ('active','paused') AND is_system = 0 AND mailbox_do_id <> '' AND id > ?1 ORDER BY id LIMIT 100",
                vec![after.clone().into()],
            ))
            .await
        {
            Ok(rows) => rows,
            Err(_) => return,
        };
        let Some(last) = page.last() else { break };
        after = last.id.clone();
        let jobs: Vec<IndexJob> = page
            .iter()
            .map(|i| IndexJob::Reconcile {
                tenant_id: i.tenant_id.clone(),
                identity_id: i.id.clone(),
                run_date: run_date.to_owned(),
            })
            .collect();
        if p.queues()
            .send_batch(QueueName::Index, &jobs, 0)
            .await
            .is_err()
        {
            return;
        }
        queued += jobs.len() as i64;
        if page.len() < 100 {
            break;
        }
    }
    let _ = p
        .db()
        .execute(Stmt::new(
            "INSERT OR IGNORE INTO index_reconcile (run_date, identity_id, queued, reported_at) VALUES (?1, '*', ?2, ?3)",
            vec![run_date.into(), queued.into(), now.into()],
        ))
        .await;
}

/// `(index_count − embedded) / embedded × 100`; 0 for an empty index with nothing embedded.
pub fn drift_pct(index_count: i64, embedded: i64) -> f64 {
    if embedded == 0 {
        if index_count == 0 { 0.0 } else { 100.0 }
    } else {
        (index_count - embedded) as f64 / embedded as f64 * 100.0
    }
}

/// `vector_drift`: this run and the previous one both more than 1% away from zero.
pub fn drift_alert(this: f64, previous: Option<f64>) -> bool {
    this.abs() > DRIFT_PCT && previous.is_some_and(|p| p.abs() > DRIFT_PCT)
}

/// Step 5.
pub async fn evaluate<P: Platform>(p: &P, run_date: &str, hour: i64, minute: i64, now: i64) {
    let summary: Option<Summary> = p
        .db()
        .first(Stmt::new(
            "SELECT queued, index_count, drift_pct FROM index_reconcile WHERE run_date = ?1 AND identity_id = '*'",
            vec![run_date.into()],
        ))
        .await
        .unwrap_or(None);
    let Some(summary) = summary.filter(|s| s.index_count.is_none()) else {
        return;
    };
    let reported: Option<Reported> = p
        .db()
        .first(Stmt::new(
            "SELECT COUNT(*) AS n, SUM(embedded_rows) AS embedded FROM index_reconcile WHERE run_date = ?1 AND identity_id <> '*'",
            vec![run_date.into()],
        ))
        .await
        .unwrap_or(None);
    let reported = reported.unwrap_or(Reported {
        n: 0,
        embedded: None,
    });
    if reported.n < summary.queued.unwrap_or(0) {
        if hour == 23 && minute >= 45 {
            crate::log::warn(
                "index_reconcile_incomplete",
                &[
                    ("run_date", run_date.into()),
                    ("reported", reported.n.into()),
                ],
            );
        }
        return;
    }
    let Ok(info) = p.vectors().describe().await else {
        return;
    };
    let index_count = info.vector_count as i64;
    let embedded = reported.embedded.unwrap_or(0);
    let drift = drift_pct(index_count, embedded);
    crate::log::info(
        "vector_count_drift",
        &[
            ("run_date", run_date.into()),
            ("drift", (index_count - embedded).into()),
            ("index_count", index_count.into()),
            ("embedded_rows", embedded.into()),
        ],
    );
    let _ = p
        .db()
        .execute(Stmt::new(
            "UPDATE index_reconcile SET index_count = ?2, embedded_rows = ?3, drift_pct = ?4, reported_at = ?5 WHERE run_date = ?1 AND identity_id = '*'",
            vec![
                run_date.into(),
                index_count.into(),
                embedded.into(),
                drift.into(),
                now.into(),
            ],
        ))
        .await;
    let yesterday = rfc3339(now - DAY_MS)[..10].to_owned();
    let prev: Option<Summary> = p
        .db()
        .first(Stmt::new(
            "SELECT queued, index_count, drift_pct FROM index_reconcile WHERE run_date = ?1 AND identity_id = '*'",
            vec![yesterday.into()],
        ))
        .await
        .unwrap_or(None);
    let previous = prev.and_then(|s| s.drift_pct);
    if drift_alert(drift, previous) {
        crate::log::error(
            "alert",
            &[
                ("alert", "vector_drift".into()),
                ("run_date", run_date.into()),
                ("drift_pct", format!("{drift:.2}").into()),
            ],
        );
        crate::ops::alerts::report(
            p,
            "vector_drift",
            None,
            None,
            &serde_json::json!({ "drift_pct": drift, "previous_drift_pct": previous }),
            "index_reconcile",
        )
        .await;
    }
    let week_ago = rfc3339(now - 7 * DAY_MS)[..10].to_owned();
    let _ = p
        .db()
        .execute(Stmt::new(
            "DELETE FROM index_reconcile WHERE run_date < ?1",
            vec![week_ago.into()],
        ))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_and_drift() {
        // 2026-10-27T02:05:00Z
        let (d, h, m) = utc(1_793_066_700_000);
        assert_eq!((d.as_str(), h, m), ("2026-10-27", 2, 5));
        assert_eq!(drift_pct(102, 100), 2.0);
        assert_eq!(drift_pct(0, 0), 0.0);
        assert!(drift_pct(99, 100).abs() <= DRIFT_PCT);
        assert!(!drift_alert(2.0, None));
        assert!(!drift_alert(2.0, Some(0.5)));
        assert!(drift_alert(-2.0, Some(2.0)));
    }
}
