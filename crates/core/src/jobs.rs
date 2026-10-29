//! The pure rules of the `JobRunner` ([Privacy and erasure § 4, § 10]): the step backoff, when a job
//! fails, and the erasure receipt built from the steps' counts.
//!
//! [Privacy and erasure § 4, § 10]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

use serde_json::{Map, Value, json};

/// The first retry delay of a failed slice; it doubles per attempt up to [`BACKOFF_CAP_MS`].
pub const BACKOFF_BASE_MS: i64 = 30_000;
pub const BACKOFF_CAP_MS: i64 = 3_600_000;
/// A job other than erasure fails on this many failed attempts of one step.
pub const MAX_ATTEMPTS: i64 = 10;
/// An erasure's automatic retries end this long before its deadline, and last at least this long.
pub const RETRY_MARGIN_MS: i64 = 4 * 3_600_000;
/// `erasure_stalled` fires on this many consecutive failed attempts of one step.
pub const STALLED_AT: i64 = 3;

/// The delay before attempt `attempts + 1`, after `attempts` (≥ 1) failed ones.
pub fn backoff_ms(attempts: i64) -> i64 {
    let n = attempts.clamp(1, 32) - 1;
    BACKOFF_BASE_MS
        .saturating_mul(1i64 << n.min(20))
        .min(BACKOFF_CAP_MS)
}

/// `meta.retry_until` of an erasure job started at `now`: `deadline_at − 4 h`, or 4 hours after the
/// start when that is later (a resumption of a request that failed near its deadline).
pub fn retry_until(deadline_at: i64, now: i64) -> i64 {
    (deadline_at - RETRY_MARGIN_MS).max(now + RETRY_MARGIN_MS)
}

/// Whether a step that just failed for the `attempts`-th time in a row ends the job.
pub fn fails_job(erasure: bool, attempts: i64, retry_until: Option<i64>, now: i64) -> bool {
    if erasure {
        retry_until.is_some_and(|r| now >= r)
    } else {
        attempts >= MAX_ATTEMPTS
    }
}

/// The receipt's count fields, in the order of Privacy § 10.
pub const COUNT_FIELDS: &[&str] = &[
    "messages_deleted",
    "attachments_deleted",
    "r2_objects_deleted",
    "fts_rows_deleted",
    "refs_deleted",
    "vectors_deleted",
    "events_deleted",
    "log_events_deleted",
    "drafts_deleted",
    "uploads_deleted",
    "screenings_deleted",
    "screening_feedback_deleted",
    "list_entries_deleted",
];

/// Adds every numeric field of `delta` into `into`.
pub fn add_counts(into: &mut Map<String, Value>, delta: &Value) {
    if let Some(d) = delta.as_object() {
        for (k, v) in d {
            if let Some(n) = v.as_i64() {
                let cur = into.get(k).and_then(Value::as_i64).unwrap_or(0);
                into.insert(k.clone(), Value::from(cur + n));
            }
        }
    }
}

/// The receipt from the steps' summed counts, the affected identities, the held threads and the
/// probe (`None` when the job ended before probing). Answers `(receipt, status)`, where a held thread
/// makes the status `completed_with_holds`.
pub fn receipt(
    counts: &Map<String, Value>,
    identities_affected: &[String],
    held: &[(String, String)],
    probe: Option<(i64, i64, i64)>,
) -> (Value, &'static str) {
    let mut r = Map::new();
    for f in COUNT_FIELDS {
        r.insert(
            (*f).to_owned(),
            Value::from(counts.get(*f).and_then(Value::as_i64).unwrap_or(0)),
        );
    }
    for (k, v) in counts {
        if !COUNT_FIELDS.contains(&k.as_str()) && v.is_i64() {
            r.insert(k.clone(), v.clone());
        }
    }
    let mut ids = identities_affected.to_vec();
    ids.sort();
    ids.dedup();
    r.insert("identities_affected".into(), json!(ids));
    r.insert(
        "held".into(),
        Value::Array(
            held.iter()
                .map(|(t, reason)| json!({ "thread_id": t, "reason": reason }))
                .collect(),
        ),
    );
    r.insert(
        "probe".into(),
        match probe {
            Some((k, s, l)) => json!({ "keyword_hits": k, "semantic_hits": s, "log_hits": l }),
            None => json!({ "keyword_hits": null, "semantic_hits": null, "log_hits": null }),
        },
    );
    let status = if held.is_empty() {
        "completed"
    } else {
        "completed_with_holds"
    };
    (Value::Object(r), status)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Privacy § 4: 30 s doubling to a 1-hour cap; a non-erasure job fails after 10 attempts; an
    /// erasure job only on the first failed attempt after `deadline_at − 4 h`.
    #[test]
    fn backoff_schedule() {
        assert_eq!(backoff_ms(1), 30_000);
        assert_eq!(backoff_ms(2), 60_000);
        assert_eq!(backoff_ms(7), 1_920_000);
        assert_eq!(backoff_ms(8), 3_600_000);
        assert_eq!(backoff_ms(40), 3_600_000);
        assert!(!fails_job(false, 9, None, 0));
        assert!(fails_job(false, 10, None, 0));
        let deadline = 24 * 3_600_000;
        let until = retry_until(deadline, 0);
        assert_eq!(until, 20 * 3_600_000);
        assert!(!fails_job(true, 50, Some(until), until - 1));
        assert!(fails_job(true, 1, Some(until), until));
        // A resumption past its window still gets four hours.
        assert_eq!(retry_until(deadline, deadline), deadline + RETRY_MARGIN_MS);
        assert_eq!(STALLED_AT, 3);
    }

    /// Privacy § 10: counts summed per field, `held` forces `completed_with_holds`.
    #[test]
    fn receipt_builder() {
        let mut c = Map::new();
        add_counts(&mut c, &json!({ "messages_deleted": 2, "refs_deleted": 3 }));
        add_counts(&mut c, &json!({ "messages_deleted": 1, "cursor": "x" }));
        let (r, s) = receipt(
            &c,
            &["idn_b".into(), "idn_a".into(), "idn_b".into()],
            &[],
            Some((0, 0, 0)),
        );
        assert_eq!(s, "completed");
        assert_eq!(r["messages_deleted"], 3);
        assert_eq!(r["attachments_deleted"], 0);
        assert_eq!(r["identities_affected"], json!(["idn_a", "idn_b"]));
        assert_eq!(r["probe"]["semantic_hits"], 0);
        let (r, s) = receipt(&c, &[], &[("thr_1".into(), "PCN".into())], None);
        assert_eq!(s, "completed_with_holds");
        assert_eq!(r["held"][0]["reason"], "PCN");
        assert!(r["probe"]["keyword_hits"].is_null());
    }
}
