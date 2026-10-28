//! The `pm-index` consumer (Search § 6): attachment text (M7), and `Embed`, `Reconcile` and the
//! Workers AI fair queue (M10).

use std::time::Duration;

use crate::{eventually, get, inject, mailbox_fixture, mailbox_query, wait_stored};

fn with_pdf(to: &str) -> String {
    format!(
        "From: a@pdf.example\r\nTo: {to}\r\nSubject: Scan\r\nMessage-ID: <{}@it.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n--b\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"scan.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQKJcOkw7zDtsOfCjEgMCBvYmoKPDw+PgplbmRvYmoKdHJhaWxlcgo8PD4+CiUlRU9GCg==\r\n--b--\r\n",
        crate::slug("pdf")
    )
}

/// B12: the converter fails on every attempt (workerd has no Workers AI): after the third the
/// attachment is `unavailable`, and still fetchable.
#[test]
fn b12_extraction_failure() {
    let (_, idn, addr, key) = mailbox_fixture("b12");
    assert_eq!(inject("a@pdf.example", &addr, &with_pdf(&addr)), 200);
    let m = wait_stored(&idn, &key, "received", 1);
    let mid = m[0]["id"].as_str().unwrap().to_owned();
    let att = eventually(40, "text_status unavailable", || {
        let rows = mailbox_query(&idn, "SELECT id, text_status FROM attachments");
        (rows[0]["text_status"] == "unavailable")
            .then(|| rows[0]["id"].as_str().unwrap().to_owned())
    });
    let r = get(
        &format!("/v1/identities/{idn}/messages/{mid}/attachments/{att}"),
        &key,
    );
    assert_eq!(r.status, 200);
    let t = get(
        &format!("/v1/identities/{idn}/messages/{mid}/attachments/{att}/text"),
        &key,
    );
    assert_eq!(t.body["status"], "unavailable");
}

/// `Triage` acks without work until M12: triage stays `pending`, nothing is dead-lettered.
#[test]
fn job_kinds_before_milestone() {
    let (_, idn, addr, key) = mailbox_fixture("kinds");
    assert_eq!(
        inject(
            "a@k.example",
            &addr,
            &crate::mail("a@k.example", &addr, "Kinds", "x")
        ),
        200
    );
    let m = wait_stored(&idn, &key, "received", 1);
    std::thread::sleep(Duration::from_secs(6));
    let full = get(
        &format!(
            "/v1/identities/{idn}/messages/{}",
            m[0]["id"].as_str().unwrap()
        ),
        &key,
    );
    assert_eq!(full.body["triage"]["status"], "pending", "{}", full.body);
    // Only this test's jobs: an earlier test's fault can dead-letter its own.
    assert!(
        !crate::fakes::queued("pm-index-dlq")
            .iter()
            .any(|j| j["identity_id"] == idn.as_str())
    );
}

const DAY: i64 = 86_400_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Moves the Worker's clock to the first `hh:mm` UTC at least `days` days and `ahead_ms` ahead of its
/// current time; answers the UTC date there.
fn travel(days: i64, hh: i64, mm: i64, ahead_ms: i64, offset: &mut i64) -> String {
    let now = now_ms() + *offset;
    let day_start = now - now.rem_euclid(DAY);
    let mut target = day_start + days * DAY + hh * 3_600_000 + mm * 60_000;
    while target < now + ahead_ms.max(1) {
        target += DAY;
    }
    crate::fakes::advance(target - now);
    *offset += target - now;
    let secs = target / 1000;
    let days_since = secs.div_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days_since + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn chunk_states(idn: &str) -> Vec<(i64, String)> {
    mailbox_query(
        idn,
        "SELECT message_rowid, status FROM chunks ORDER BY message_rowid",
    )
    .iter()
    .map(|r| {
        (
            r["message_rowid"].as_i64().unwrap(),
            r["status"].as_str().unwrap().to_owned(),
        )
    })
    .collect()
}

fn all_embedded(idn: &str, messages: usize) -> bool {
    let rows = chunk_states(idn);
    let mut ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
    ids.dedup();
    ids.len() == messages && rows.iter().all(|r| r.1 == "embedded")
}

/// Waits until the run's every queued identity has reported.
fn reported(run_date: &str) -> i64 {
    eventually(120, "every identity reported", || {
        let s = crate::d1(&format!(
            "SELECT queued FROM index_reconcile WHERE run_date = '{run_date}' AND identity_id = '*'"
        ));
        let queued = s.as_array()?.first()?["queued"].as_i64()?;
        let n = crate::d1(&format!(
            "SELECT COUNT(*) AS n, SUM(embedded_rows) AS e FROM index_reconcile WHERE run_date = '{run_date}' AND identity_id <> '*'"
        ));
        (n[0]["n"].as_i64() == Some(queued)).then(|| n[0]["e"].as_i64().unwrap_or(0))
    })
}

/// `pct` percent more vectors than `embedded`, at least one more.
fn over(embedded: i64, pct: f64) -> u64 {
    let n = embedded.max(0) as u64;
    n + ((n as f64 * pct / 100.0).ceil() as u64).max(1)
}

fn summary(run_date: &str) -> serde_json::Value {
    crate::d1(&format!(
        "SELECT queued, index_count, embedded_rows, drift_pct FROM index_reconcile WHERE run_date = '{run_date}' AND identity_id = '*'"
    ))[0]
        .clone()
}

/// F14: a failed upsert is retried by the job; the nightly reconciliation re-enqueues failed rows,
/// messages without rows and vectors Vectorize lost; a retried `Reconcile` job does not count twice;
/// the drift alert's condition holds on the second night over 1% and not the first; an incomplete run
/// is left unevaluated.
#[test]
fn f14_retry_and_reconcile() {
    let _reset = crate::fakes::Reset;
    let (tid, idn, addr, key) = mailbox_fixture("f14rec");
    // One failed upsert: the job marks the rows failed, re-enqueues itself, and the retry embeds.
    crate::fakes::vector_fail("upsert", 1);
    for (s, b) in [
        ("Fleet audit", "The audit is due."),
        ("Tyre change", "Winter tyres fitted."),
        ("Key handover", "Keys at the desk."),
    ] {
        let from = "ops@fleet.example";
        assert_eq!(inject(from, &addr, &crate::mail(from, &addr, s, b)), 200);
    }
    wait_stored(&idn, &key, "received", 3);
    eventually(60, "embedded after the retry", || {
        all_embedded(&idn, 3).then_some(())
    });
    assert!(
        crate::fakes::queued("pm-index")
            .iter()
            .any(|j| j["kind"] == "embed" && j["attempt"] == 1),
        "a retry with attempt 1"
    );
    // Damage: a failed row, a message without rows, a vector Vectorize lost.
    let rowids: Vec<i64> = chunk_states(&idn).iter().map(|r| r.0).collect();
    mailbox_query(
        &idn,
        &format!(
            "UPDATE chunks SET status = 'failed' WHERE message_rowid = {}",
            rowids[0]
        ),
    );
    mailbox_query(
        &idn,
        &format!("DELETE FROM chunks WHERE message_rowid = {}", rowids[1]),
    );
    let lost = mailbox_query(
        &idn,
        &format!(
            "SELECT vector_id FROM chunks WHERE message_rowid = {}",
            rowids[2]
        ),
    )[0]["vector_id"]
        .as_str()
        .unwrap()
        .to_owned();
    crate::fakes::vector_lose("VECTORS", &lost);
    // Night one, 02:05 UTC.
    let mut offset = 0;
    // Two hours ahead at least, so the mail received now is older than an hour there.
    let night1 = travel(0, 2, 5, 2 * 3_600_000, &mut offset);
    crate::cron("*/15 * * * *");
    let embedded1 = reported(&night1);
    eventually(60, "repaired", || {
        (all_embedded(&idn, 3) && crate::fakes::vectors("VECTORS").contains_key(&lost))
            .then_some(())
    });
    // A retried Reconcile job overwrites its own row.
    crate::queue_send(
        "pm-index",
        serde_json::json!({ "kind": "reconcile", "tenant_id": tid, "identity_id": idn, "run_date": night1 }),
    );
    std::thread::sleep(Duration::from_secs(3));
    let mine = crate::d1(&format!(
        "SELECT COUNT(*) AS n FROM index_reconcile WHERE run_date = '{night1}' AND identity_id = '{idn}'"
    ));
    assert_eq!(mine[0]["n"], 1);
    // 03:05: the index reports 2% more vectors than the rows say.
    let embedded1 = reported(&night1).max(embedded1);
    crate::fakes::vector_count(Some(over(embedded1, 2.0)));
    travel(0, 3, 5, 0, &mut offset);
    crate::cron("*/15 * * * *");
    let s1 = eventually(30, "night one evaluated", || {
        let s = summary(&night1);
        s["drift_pct"].as_f64().map(|_| s)
    });
    assert!(s1["drift_pct"].as_f64().unwrap() > 1.0, "{s1}");
    // Night two: over 1% again, so both nights are over and the alert's condition holds.
    crate::fakes::vector_count(None);
    let night2 = travel(0, 2, 5, 0, &mut offset);
    crate::cron("*/15 * * * *");
    let embedded2 = reported(&night2);
    crate::fakes::vector_count(Some(over(embedded2, 3.0)));
    travel(0, 3, 5, 0, &mut offset);
    crate::cron("*/15 * * * *");
    let s2 = eventually(30, "night two evaluated", || {
        let s = summary(&night2);
        s["drift_pct"].as_f64().map(|_| s)
    });
    assert!(s2["drift_pct"].as_f64().unwrap() > 1.0, "{s2}");
    // An incomplete run: queued more than reported; it stays unevaluated.
    let night3 = travel(0, 3, 5, 0, &mut offset);
    crate::d1(&format!(
        "INSERT INTO index_reconcile (run_date, identity_id, queued, reported_at) VALUES ('{night3}', '*', 999999, 1)"
    ));
    crate::cron("*/15 * * * *");
    std::thread::sleep(Duration::from_secs(2));
    let s3 = summary(&night3);
    assert!(
        s3["index_count"].is_null() && s3["drift_pct"].is_null(),
        "{s3}"
    );
}

/// F16: over its `RL_EMBED` share a tenant's `Embed` jobs are deferred (`deferrals` up, `attempt`
/// unchanged) while another tenant's embed at once; past 120 deferrals the job acks and leaves its
/// rows `pending`; a Workers AI rate-limit answer defers background jobs and degrades interactive
/// hybrid search to keyword.
#[test]
fn f16_ai_fair_queue() {
    let _reset = crate::fakes::Reset;
    let (ta, ia, aa, ka) = mailbox_fixture("f16busy");
    let (_, ib, ab, kb) = mailbox_fixture("f16calm");
    crate::fakes::fault("rl.allow", &format!("RL_EMBED:{ta}"), "deny", 2);
    for (idn, addr) in [(&ia, &aa), (&ib, &ab)] {
        let from = "ops@fair.example";
        assert_eq!(
            inject(
                from,
                addr,
                &crate::mail(from, addr, &format!("Fair {idn}"), "Share test.")
            ),
            200
        );
    }
    wait_stored(&ia, &ka, "received", 1);
    wait_stored(&ib, &kb, "received", 1);
    eventually(30, "the calm tenant embedded", || {
        all_embedded(&ib, 1).then_some(())
    });
    eventually(30, "the busy tenant deferred, then embedded", || {
        all_embedded(&ia, 1).then_some(())
    });
    let deferred: Vec<_> = crate::fakes::queued("pm-index")
        .into_iter()
        .filter(|j| j["kind"] == "embed" && j["identity_id"] == ia.as_str())
        .collect();
    assert!(
        deferred
            .iter()
            .any(|j| j["deferrals"] == 2 && j["attempt"] == 0),
        "{deferred:?}"
    );
    // Past 120 deferrals: acked, rows left pending, no new job.
    mailbox_query(&ia, "UPDATE chunks SET status = 'pending'");
    let mid = mailbox_query(&ia, "SELECT id FROM messages")[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    crate::fakes::fault("rl.allow", &format!("RL_EMBED:{ta}"), "deny", 1);
    let before = crate::fakes::queued("pm-index").len();
    crate::queue_send(
        "pm-index",
        serde_json::json!({ "kind": "embed", "tenant_id": ta, "identity_id": ia, "message_id": mid, "deferrals": 120 }),
    );
    std::thread::sleep(Duration::from_secs(4));
    assert!(chunk_states(&ia).iter().all(|r| r.1 == "pending"));
    assert!(
        !crate::fakes::queued("pm-index")[before..]
            .iter()
            .any(|j| j["deferrals"] == 121),
        "no job past 120 deferrals"
    );
    // A Workers AI rate-limit answer: the background job is deferred, not failed.
    crate::fakes::ai_fail("bge-m3", &[429]);
    crate::queue_send(
        "pm-index",
        serde_json::json!({ "kind": "embed", "tenant_id": ta, "identity_id": ia, "message_id": mid }),
    );
    eventually(30, "embedded after the rate-limit deferral", || {
        all_embedded(&ia, 1).then_some(())
    });
    assert!(crate::fakes::queued("pm-index").iter().any(|j| {
        j["kind"] == "embed"
            && j["message_id"] == mid.as_str()
            && j["deferrals"] == 1
            && j["attempt"] == 0
    }));
    // Interactive: never queued; hybrid falls back to keyword with `degraded: true`.
    crate::fakes::ai_fail("bge-m3", &[429]);
    let r = crate::post(
        &format!("/v1/identities/{ia}/search"),
        &ka,
        serde_json::json!({ "q": "share test" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["degraded"], true, "{}", r.body);
}
