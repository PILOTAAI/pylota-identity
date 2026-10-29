//! `it::erasure`: objects no row names are swept (I9); erasure of every scope through workerd
//! (Privacy and erasure › Tests): holds, probes, receipts, resumption and the tenant freeze.

use serde_json::{Value, json};

use crate::send::{Sender, open_sender, send, sender_in};
use crate::{
    alarm, d1, eventually, fakes, get, hook, inbound, mail, mailbox_query, messages_of, platform,
    post,
};

fn exists(key: &str) -> bool {
    let r = hook(
        "POST",
        "/__test/r2",
        Some(json!({ "op": "head", "key": key })),
    );
    !r.body["size"].is_null()
}

#[test]
fn orphan_objects_swept() {
    let s = open_sender("i9 orphans", "live");
    let _f = fakes::Reset;
    // A submit killed after its sent copy leaves a compose marker and an object.
    fakes::fault_any("submit.after_put", "error", 1);
    let r = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "killed", "text": "x" }),
    );
    assert_eq!(r.status, 503, "{}", r.body);
    let prefix = format!("t/{}/i/{}/out/", s.tid, s.iid);
    let listed = hook(
        "POST",
        "/__test/r2",
        Some(json!({ "op": "list", "key": prefix })),
    );
    let key = listed.body["keys"][0].as_str().unwrap().to_owned();
    assert!(exists(&key));
    // An object no row names, older than 15 days by its ID.
    let old = pylota_identity_api_types::ids::format(
        pylota_identity_api_types::ids::IdPrefix::Message,
        1_700_000_000_000,
        7,
    );
    let stray = format!("t/{}/i/{}/m/{old}/raw.eml", s.tid, s.iid);
    assert_eq!(
        hook(
            "POST",
            "/__test/r2",
            Some(json!({ "op": "put", "key": stray, "body_base64": "eA==" }))
        )
        .status,
        200
    );
    // A send that is stored keeps its copy.
    let kept = send(
        &s,
        json!({ "to": ["jo@example.net"], "subject": "kept", "text": "x" }),
    );
    assert_eq!(kept.status, 202);
    fakes::advance(2 * 86_400_000);
    alarm(&s.iid);
    assert!(
        !exists(&key),
        "the stale compose marker's object is deleted"
    );
    assert!(!exists(&stray), "an old orphan is deleted");
    let kept_key = format!("{prefix}{}.eml", kept.body["id"].as_str().unwrap());
    assert!(exists(&kept_key));
}

/// `POST /v1/erasure-requests` with `key`.
pub fn erase(key: &str, body: Value) -> crate::Resp {
    post("/v1/erasure-requests", key, body)
}

/// One alarm of a job's runner now (`/__test/job-alarm`): its meta, steps and next alarm.
pub fn job_alarm(job_id: &str) -> Value {
    let r = hook(
        "POST",
        "/__test/job-alarm",
        Some(json!({ "job_id": job_id })),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["ok"].clone()
}

pub fn job_of(era: &str) -> String {
    d1(&format!(
        "SELECT job_id FROM erasure_requests WHERE id = '{era}'"
    ))[0]["job_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Drives the request's job until it ends; answers the request.
pub fn run(era: &str) -> Value {
    let job = job_of(era);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut last = Value::Null;
    loop {
        let r = get(&format!("/v1/erasure-requests/{era}"), &platform());
        assert_eq!(r.status, 200, "{}", r.body);
        if !matches!(r.body["status"].as_str(), Some("queued" | "running")) {
            return r.body;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the erasure did not end: {last}"
        );
        last = job_alarm(&job);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// A counterparty's mail in two identities of one tenant, and a reply to it.
fn two_identities(tag: &str, counterparty: &str) -> (Sender, Sender) {
    let a = open_sender(tag, "live");
    let b = sender_in(
        &a.tid,
        &format!("{tag} b"),
        json!({ "require_known_recipient": false }),
    );
    for s in [&a, &b] {
        let (o, smtp) = inbound(
            counterparty,
            &s.addr,
            &mail(
                counterparty,
                &s.addr,
                &format!("{tag} hello"),
                "Booking BK-2291 please",
            ),
        );
        assert_eq!(o, "accepted", "{smtp}");
    }
    for s in [&a, &b] {
        crate::wait_stored(&s.iid, &s.key, "received", 1);
    }
    let r = send(
        &a,
        json!({ "to": [counterparty], "subject": format!("{tag} reply"), "text": "Done" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    (a, b)
}

fn count(identity: &str, sql: &str) -> i64 {
    mailbox_query(identity, sql)[0]["n"].as_i64().unwrap_or(-1)
}

/// I1: counterparty erasure across two identities: rows, raw copies, contacts and the receipt.
#[test]
fn i1_counterparty() {
    let cp = "jo.rivera@i1.example";
    let (a, b) = two_identities("i1 cp", cp);
    // Mail from someone else stays.
    let (o, _) = inbound(
        "other@i1.example",
        &a.addr,
        &mail("other@i1.example", &a.addr, "i1 other", "x"),
    );
    assert_eq!(o, "accepted");
    eventually(30, "the other message", || {
        messages_of(&a.iid, &a.key, "")
            .into_iter()
            .find(|m| m["subject"] == "i1 other")
    });
    let r = erase(
        &platform(),
        json!({ "tenant_id": a.tid, "scope": "counterparty", "counterparty_address": cp, "reason": "DSR-1" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["status"], "queued");
    assert!(
        r.body.to_string().find(cp).is_none(),
        "the address is never returned"
    );
    let era = r.body["id"].as_str().unwrap().to_owned();
    let done = run(&era);
    assert_eq!(done["status"], "completed", "{done}");
    let rc = &done["receipt"];
    assert!(rc["messages_deleted"].as_i64().unwrap() >= 3, "{rc}");
    assert!(rc["r2_objects_deleted"].as_i64().unwrap() >= 2, "{rc}");
    assert_eq!(
        rc["probe"],
        json!({ "keyword_hits": 0, "semantic_hits": 0, "log_hits": 0 })
    );
    let affected: Vec<&str> = rc["identities_affected"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        affected.contains(&a.iid.as_str()) && affected.contains(&b.iid.as_str()),
        "{rc}"
    );
    for s in [&a, &b] {
        let left = count(
            &s.iid,
            &format!(
                "SELECT COUNT(*) AS n FROM messages WHERE from_address = '{cp}' OR to_json LIKE '%{cp}%'"
            ),
        );
        assert_eq!(left, 0);
        assert_eq!(
            count(
                &s.iid,
                &format!("SELECT COUNT(*) AS n FROM contacts WHERE address = '{cp}'")
            ),
            0
        );
        let keys = hook(
            "POST",
            "/__test/r2",
            Some(json!({ "op": "list", "key": format!("t/{}/i/{}/", s.tid, s.iid) })),
        );
        let ids = mailbox_query(&s.iid, "SELECT message_id AS id FROM erased_ids");
        for id in ids {
            let id = id["id"].as_str().unwrap();
            assert!(
                !keys.body["keys"].to_string().contains(id),
                "objects of {id} remain"
            );
        }
    }
    assert!(
        messages_of(&a.iid, &a.key, "")
            .iter()
            .any(|m| m["subject"] == "i1 other")
    );
    // The stored request keeps only the keyed hash.
    let row = d1(&format!(
        "SELECT counterparty_hash, reason FROM erasure_requests WHERE id = '{era}'"
    ));
    assert_eq!(row[0]["counterparty_hash"].as_str().unwrap().len(), 64);
}

/// A tenant key holding `tenants:erase`, as the workspace owner mints one (W35).
pub fn eraser(tenant: &str) -> String {
    crate::owner_key(
        tenant,
        &["erasure:manage", "tenants:erase", "identities:read"],
    )
}

/// The steps of the request's job, by name: `(status, attempts, last_error)`.
fn steps_of(job: &Value) -> Vec<(String, String, i64, String)> {
    job["steps"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|s| {
            (
                s["name"].as_str().unwrap_or("").to_owned(),
                s["status"].as_str().unwrap_or("").to_owned(),
                s["attempts"].as_i64().unwrap_or(0),
                s["last_error"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

/// Alarms until a step has failed at least `n` times; answers the runner.
fn until_failed(job: &str, step: &str, n: i64) -> Value {
    eventually(60, "the step to fail", || {
        let j = job_alarm(job);
        steps_of(&j)
            .iter()
            .any(|(name, _, a, _)| name == step && *a >= n)
            .then_some(j)
    })
}

fn one_message(s: &Sender, from: &str, subject: &str) -> Value {
    let (o, smtp) = inbound(
        from,
        &s.addr,
        &mail(from, &s.addr, subject, "Plate AB12CDE, booking BK-77"),
    );
    assert_eq!(o, "accepted", "{smtp}");
    eventually(30, "the message", || {
        messages_of(&s.iid, &s.key, "")
            .into_iter()
            .find(|m| m["subject"] == subject)
    })
}

fn embedded(identity: &str, message: &str) -> Vec<String> {
    eventually(60, "the message's vectors", || {
        let rows = mailbox_query(
            identity,
            &format!(
                "SELECT c.vector_id FROM chunks c JOIN messages m ON m.rowid = c.message_rowid WHERE m.id = '{message}' AND c.status = 'embedded'"
            ),
        );
        let ids: Vec<String> = rows
            .iter()
            .filter_map(|r| r["vector_id"].as_str().map(str::to_owned))
            .collect();
        (!ids.is_empty()).then_some(ids)
    })
}

fn erase_message(s: &Sender, id: &str) -> String {
    let r = erase(
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "message", "identity_id": s.iid, "message_id": id }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    r.body["id"].as_str().unwrap().to_owned()
}

fn events_of_type(t: &str, tenant: &str) -> usize {
    d1(&format!(
        "SELECT id FROM event_index WHERE type = '{t}' AND tenant_id = '{tenant}'"
    ))
    .as_array()
    .map_or(0, Vec::len)
}

/// F6: keyword, semantic and object probes are empty after an erasure; an index that keeps a
/// vector makes the job retry, then fail with `erasure.failed` past its window.
#[test]
fn f6_probe_empty() {
    let _reset = fakes::Reset;
    let s = open_sender("f6 probe", "live");
    let m = one_message(&s, "kim@f6.example", "f6 first");
    let id = m["id"].as_str().unwrap();
    let vectors = embedded(&s.iid, id);
    let era = erase_message(&s, id);
    let done = run(&era);
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(
        done["receipt"]["probe"],
        json!({ "keyword_hits": 0, "semantic_hits": 0, "log_hits": 0 })
    );
    assert!(done["receipt"]["vectors_deleted"].as_i64().unwrap() >= vectors.len() as i64);
    let r = get(&format!("/v1/identities/{}/messages/{id}", s.iid), &s.key);
    assert_eq!(r.status, 404);
    let left = hook(
        "POST",
        "/__test/r2",
        Some(json!({ "op": "list", "key": format!("t/{}/i/{}/m/{id}/", s.tid, s.iid) })),
    );
    assert_eq!(left.body["keys"], json!([]));
    // A vector the index keeps: the probe waits, re-sends, then the step fails until the window ends.
    let m2 = one_message(&s, "kim@f6.example", "f6 second");
    let id2 = m2["id"].as_str().unwrap();
    let v2 = embedded(&s.iid, id2);
    fakes::vector_keep(&v2[0]);
    let era2 = erase_message(&s, id2);
    let job = job_of(&era2);
    let j = until_failed(&job, "probe", 1);
    assert!(
        steps_of(&j)
            .iter()
            .any(|(n, _, _, e)| n == "probe" && e == "probe_semantic_hits"),
        "{j}"
    );
    fakes::advance(21 * 3_600_000);
    let done = run(&era2);
    assert_eq!(done["status"], "failed", "{done}");
    assert_eq!(events_of_type("erasure.failed", &s.tid), 1);
}

/// I2: a held thread survives an erasure and is listed with its reason; once the hold expires a new
/// request erases it.
#[test]
fn i2_hold() {
    let _reset = fakes::Reset;
    let s = open_sender("i2 hold", "live");
    let cp = "pat@i2.example";
    let held = one_message(&s, cp, "i2 held thread");
    let free = one_message(&s, cp, "i2 free thread");
    let thread = held["thread_id"].as_str().unwrap();
    let until = pylota_identity_core::days::rfc3339(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            + 2 * 86_400_000,
    );
    let r = post(
        &format!("/v1/identities/{}/threads/{thread}/hold", s.iid),
        &platform(),
        json!({ "reason": "PCN dispute WM12345678", "until": until }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["hold"]["reason"], "PCN dispute WM12345678");
    assert_eq!(r.body["hold"]["until"], until.as_str());
    assert!(
        r.body["hold"]["set_at"]
            .as_str()
            .is_some_and(|t| t.ends_with('Z'))
    );
    let e = erase(
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "counterparty", "counterparty_address": cp }),
    );
    let done = run(e.body["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed_with_holds", "{done}");
    assert_eq!(
        done["receipt"]["held"],
        json!([{ "thread_id": thread, "reason": "PCN dispute WM12345678" }])
    );
    let ids: Vec<String> = messages_of(&s.iid, &s.key, "")
        .iter()
        .filter_map(|m| m["id"].as_str().map(str::to_owned))
        .collect();
    assert!(ids.contains(&held["id"].as_str().unwrap().to_owned()));
    assert!(!ids.contains(&free["id"].as_str().unwrap().to_owned()));
    // A single-message delete in the held thread is refused.
    let r = crate::delete(
        &format!(
            "/v1/identities/{}/messages/{}",
            s.iid,
            held["id"].as_str().unwrap()
        ),
        &platform(),
    );
    assert_eq!((r.status, r.code()), (423, "legal_hold"));
    // The hold expires: the next erasure takes the thread.
    fakes::advance(3 * 86_400_000);
    let e = erase(
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "counterparty", "counterparty_address": cp }),
    );
    let done = run(e.body["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    assert!(messages_of(&s.iid, &s.key, "").is_empty());
}

/// I7: after counterparty erasure the suppression keeps its hash, hint and reason, loses note and
/// source, and still refuses a send.
#[test]
fn i7_suppression_kept_hashed() {
    let s = open_sender("i7 suppression", "live");
    let cp = "gone@i7.example";
    let r = post(
        &format!("/v1/tenants/{}/suppressions", s.tid),
        &s.key,
        json!({ "address": cp, "note": "Asked us to stop" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    one_message(&s, cp, "i7 hello");
    let e = erase(
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "counterparty", "counterparty_address": cp }),
    );
    let done = run(e.body["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    let row = &d1(&format!(
        "SELECT address_hash, address_hint, reason, note, source_message_id FROM suppressions WHERE tenant_id = '{}'",
        s.tid
    ))[0];
    assert_eq!(row["address_hash"].as_str().unwrap().len(), 64);
    assert!(row["address_hint"].as_str().unwrap().contains('*'));
    assert!(
        row["note"].is_null() && row["source_message_id"].is_null(),
        "{row}"
    );
    let r = send(
        &s,
        json!({ "to": [cp], "subject": "i7 again", "text": "x" }),
    );
    assert_eq!(r.body["status"], "suppressed", "{}", r.body);
}

/// Privacy § 6.6: routing stops first, billing second, then domains, mailboxes, D1 rows and the
/// vector sweep; the tenant ends `erased` and `erasure.completed` is recorded.
#[test]
fn tenant_scope_order() {
    let s = open_sender("tenant order", "live");
    one_message(&s, "lee@order.example", "order hello");
    let e = erase(
        &eraser(&s.tid),
        json!({ "tenant_id": s.tid, "scope": "tenant" }),
    );
    assert_eq!(e.status, 202, "{}", e.body);
    let era = e.body["id"].as_str().unwrap().to_owned();
    let job = job_of(&era);
    let order: Vec<String> = steps_of(&job_alarm(&job))
        .into_iter()
        .map(|s| s.0)
        .collect();
    assert_eq!(
        order,
        [
            "stop_routing",
            "cancel_billing",
            "remove_domains",
            "erase_identities",
            "delete_d1_rows",
            "sweep_vectors",
            "sweep_r2",
            "probe",
            "receipt"
        ]
    );
    // Routing stopped: once the directory cache (60 s) has turned over, inbound is refused.
    let _reset = fakes::Reset;
    fakes::advance(61_000);
    let (o, smtp) = inbound(
        "lee@order.example",
        &s.addr,
        &mail("lee@order.example", &s.addr, "order late", "x"),
    );
    assert_eq!((o.as_str(), &smtp[..9]), ("rejected", "550 5.1.1"));
    let done = run(&era);
    assert_eq!(done["status"], "completed", "{done}");
    let t = &d1(&format!(
        "SELECT status, name FROM tenants WHERE id = '{}'",
        s.tid
    ))[0];
    assert_eq!(
        (t["status"].as_str(), t["name"].as_str()),
        (Some("erased"), Some(""))
    );
    assert_eq!(events_of_type("erasure.completed", &s.tid), 1);
    for table in ["identities", "api_keys", "addresses", "suppressions"] {
        let n = d1(&format!(
            "SELECT COUNT(*) AS n FROM {table} WHERE tenant_id = '{}'",
            s.tid
        ))[0]["n"]
            .as_i64()
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
    let left = hook(
        "POST",
        "/__test/r2",
        Some(json!({ "op": "list", "key": format!("t/{}/", s.tid) })),
    );
    assert_eq!(left.body["keys"], json!([]));
}

/// Privacy § 6.12: tenant erasure deletes thousands of index rows in statements of at most 1,000.
#[test]
fn batched_deletes() {
    let s = open_sender("batched deletes", "live");
    d1(&format!(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 5000) \
         INSERT INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, occurred_at) \
         SELECT 'evt_bd' || i || '{0}', '{0}', NULL, 'identity.updated', 'platform', 'x', 1 FROM n",
        s.tid
    ));
    let e = erase(
        &eraser(&s.tid),
        json!({ "tenant_id": s.tid, "scope": "tenant" }),
    );
    assert_eq!(e.status, 202, "{}", e.body);
    let era = e.body["id"].as_str().unwrap().to_owned();
    let done = run(&era);
    assert_eq!(done["status"], "completed", "{done}");
    let n = d1(&format!(
        "SELECT COUNT(*) AS n FROM event_index WHERE tenant_id = '{}' AND id LIKE 'evt_bd%'",
        s.tid
    ))[0]["n"]
        .as_i64()
        .unwrap();
    assert_eq!(n, 0);
}

/// Every scope's failed request is resumed by a new request for the same scope and target.
#[test]
fn resume_failed_any_scope() {
    let _reset = fakes::Reset;
    let s = open_sender("resume any", "live");
    let m = one_message(&s, "ash@resume.example", "resume me");
    let id = m["id"].as_str().unwrap().to_owned();
    fakes::fault("r2.delete", "BLOBS", "error", 1000);
    let era = erase_message(&s, &id);
    let job = job_of(&era);
    until_failed(&job, "erase_mailbox", 1);
    fakes::advance(21 * 3_600_000);
    let failed = run(&era);
    assert_eq!(failed["status"], "failed", "{failed}");
    fakes::reset();
    fakes::advance(21 * 3_600_000);
    // Two resumptions at once make one request.
    let body =
        json!({ "tenant_id": s.tid, "scope": "message", "identity_id": s.iid, "message_id": id });
    let a = std::thread::spawn({
        let b = body.clone();
        move || erase(&platform(), b)
    });
    let r2 = erase(&platform(), body);
    let r1 = a.join().unwrap();
    let mut statuses = [r1.status, r2.status];
    statuses.sort();
    assert_eq!(statuses, [200, 202], "{} {}", r1.body, r2.body);
    let resumed = if r1.status == 202 { r1.body } else { r2.body };
    assert_eq!(resumed["resumes_id"], era);
    assert_eq!(resumed["deadline_at"], failed["deadline_at"]);
    let done = run(resumed["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    // The resumed job did not start over: its init step came done from the failed one.
    let steps = steps_of(&job_alarm(&job_of(resumed["id"].as_str().unwrap())));
    assert!(steps.iter().all(|(_, st, _, _)| st == "done"), "{steps:?}");
}

/// NFR-PRV-1: injected R2 faults retry with backoff; past its window the job fails with a partial
/// receipt and `erasure.failed`; a non-erasure job fails after 10 attempts.
#[test]
fn step_retry_and_fail() {
    let _reset = fakes::Reset;
    let s = open_sender("retry fail", "live");
    let m = one_message(&s, "bo@retry.example", "retry me");
    fakes::fault("r2.delete", "BLOBS", "error", 3);
    let era = erase_message(&s, m["id"].as_str().unwrap());
    let job = job_of(&era);
    let j = until_failed(&job, "erase_mailbox", 3);
    assert_eq!(
        j["meta"]["stalled"], "1",
        "erasure_stalled fired on the third failure"
    );
    let done = run(&era);
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(done["receipt"]["messages_deleted"], 1, "not counted twice");
    // Past the window: failed, with a partial receipt.
    let m2 = one_message(&s, "bo@retry.example", "retry never");
    fakes::fault("r2.delete", "BLOBS", "error", 1000);
    let era2 = erase_message(&s, m2["id"].as_str().unwrap());
    until_failed(&job_of(&era2), "erase_mailbox", 1);
    fakes::advance(21 * 3_600_000);
    let failed = run(&era2);
    assert_eq!(failed["status"], "failed", "{failed}");
    assert!(failed["receipt"].is_object());
    assert!(failed["receipt"]["probe"]["keyword_hits"].is_null());
    fakes::reset();
    // A non-erasure job fails on its 10th attempt.
    let r = hook(
        "POST",
        "/__test/job-create",
        Some(
            json!({ "kind": "domain_remove", "tenant_id": s.tid, "params": { "domain_id": "dom_01ARYZ6S410000000000000000" } }),
        ),
    );
    let jid = r.body["job_id"].as_str().unwrap().to_owned();
    eventually(60, "the job to fail", || {
        job_alarm(&jid);
        (d1(&format!("SELECT status FROM jobs WHERE id = '{jid}'"))[0]["status"] == "failed")
            .then_some(())
    });
    let steps = steps_of(&job_alarm(&jid));
    assert_eq!(steps[0].2, 10, "{steps:?}");
}

/// A partner whose key may erase its tenants (`tenants:erase` written as the owner flow grants it),
/// and one of its tenants with an identity: `(partner key, tenant id, identity)`.
fn partner_tenant(tag: &str) -> (String, String, Value) {
    let (pid, pk) = crate::partner(
        tag,
        &[
            "tenants:manage",
            "identities:read",
            "identities:write",
            "erasure:manage",
            "keys:manage",
            "webhooks:manage",
            "messages:read",
            "messages:send",
            "domains:write",
        ],
    );
    d1(&format!(
        "UPDATE api_keys SET permissions_json = json_insert(permissions_json, '$[#]', 'tenants:erase') WHERE partner_id = '{pid}'"
    ));
    let t = crate::tenant(&pk, tag);
    let tid = t["id"].as_str().unwrap().to_owned();
    let idn = crate::identity(&pk, &tid, &crate::slug(tag)[..9]);
    (pk, tid, idn)
}

fn mail_to(addr: &str, from: &str, subject: &str) {
    let (o, smtp) = inbound(from, addr, &mail(from, addr, subject, "Hello"));
    assert_eq!(o, "accepted", "{smtp}");
}

/// I8: while a tenant is `erasing` and after it is `erased`, its partner key may not write and its
/// own keys are revoked; reads keep working; a second request answers the first; a failed job is
/// resumed at its failed step and leaves the tenant `erased`.
#[test]
fn i8_erasing_tenant_frozen() {
    let _reset = fakes::Reset;
    let (pk, tid, idn) = partner_tenant("i8 frozen");
    let iid = idn["id"].as_str().unwrap().to_owned();
    let addr = idn["primary_address"].as_str().unwrap().to_owned();
    let (_, tk) = crate::mint(
        &pk,
        json!({ "name": "t", "level": "tenant", "tenant_id": tid, "permissions": ["identities:read"] }),
    );
    let _ = post(
        &format!("/v1/tenants/{tid}/identities"),
        &pk,
        json!({ "username": crate::slug("i8 extra")[..9].to_owned(), "display_name": "x" }),
    );
    mail_to(&addr, "max@i8.example", "i8 hello");
    eventually(30, "stored", || {
        (mailbox_query(&iid, "SELECT COUNT(*) AS n FROM messages")[0]["n"] == 1).then_some(())
    });
    fakes::fault("r2.delete", "BLOBS", "error", 10_000);
    let e = erase(&pk, json!({ "tenant_id": tid, "scope": "tenant" }));
    assert_eq!(e.status, 202, "{}", e.body);
    let era = e.body["id"].as_str().unwrap().to_owned();
    until_failed(&job_of(&era), "erase_identities", 1);
    // Writes from the partner key are refused; the tenant's own key is revoked.
    let r = post(
        &format!("/v1/tenants/{tid}/identities"),
        &pk,
        json!({ "username": "late", "display_name": "x" }),
    );
    assert_eq!((r.status, r.code()), (404, "tenant_not_found"));
    let r = crate::patch(&format!("/v1/tenants/{tid}"), &pk, json!({ "name": "x" }));
    assert_eq!(r.status, 404);
    let r = post(
        &format!("/v1/tenants/{tid}/webhooks"),
        &pk,
        json!({ "url": "https://i8.hooks.example.com/a", "events": ["identity.created"] }),
    );
    assert_eq!(r.status, 404);
    let r = get("/v1/identities", &tk);
    assert_eq!((r.status, r.code()), (401, "key_revoked"));
    // Reads keep working for the partner key; a second request answers the first.
    let r = get(&format!("/v1/tenants/{tid}"), &pk);
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (200, Some("erasing"))
    );
    assert_eq!(get(&format!("/v1/erasure-requests/{era}"), &pk).status, 200);
    let again = erase(&pk, json!({ "tenant_id": tid, "scope": "tenant" }));
    assert_eq!(
        (again.status, again.body["id"].as_str()),
        (200, Some(era.as_str()))
    );
    let r = crate::patch(
        &format!("/v1/tenants/{tid}"),
        &platform(),
        json!({ "status": "active" }),
    );
    assert_eq!((r.status, r.code()), (409, "tenant_erased"));
    // The job fails past its window; the tenant stays erasing.
    fakes::advance(21 * 3_600_000);
    let failed = run(&era);
    assert_eq!(failed["status"], "failed", "{failed}");
    assert_eq!(
        get(&format!("/v1/tenants/{tid}"), &pk).body["status"],
        "erasing"
    );
    // A new request resumes it at the failed step and finishes.
    fakes::reset();
    fakes::advance(21 * 3_600_000);
    let r = erase(&pk, json!({ "tenant_id": tid, "scope": "tenant" }));
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["resumes_id"], era);
    assert_eq!(r.body["deadline_at"], failed["deadline_at"]);
    let resumed = r.body["id"].as_str().unwrap().to_owned();
    let steps = steps_of(&job_alarm(&job_of(&resumed)));
    assert!(
        steps.iter().take(3).all(|(_, st, _, _)| st == "done"),
        "earlier steps are not re-run: {steps:?}"
    );
    let done = run(&resumed);
    assert_eq!(done["status"], "completed", "{done}");
    let r = get(&format!("/v1/tenants/{tid}"), &pk);
    assert_eq!(
        (r.body["status"].as_str(), r.body["name"].as_str()),
        (Some("erased"), Some(""))
    );
    let r = erase(&pk, json!({ "tenant_id": tid, "scope": "tenant" }));
    assert_eq!((r.status, r.code()), (409, "tenant_erased"));
    let n = d1(&format!(
        "SELECT COUNT(*) AS n FROM idempotency_records WHERE tenant_id = '{tid}' OR scope = '{tid}'"
    ))[0]["n"]
        .as_i64()
        .unwrap();
    assert_eq!(n, 0);
}

/// I2 in a tenant erasure: the held thread survives with its identity `deleting` and the tenant
/// `erasing`; the partner key reads it and releases the hold; the global job's `held_erasures`
/// step then finishes the erasure.
#[test]
fn i2_hold_tenant_scope() {
    let (pk, tid, idn) = partner_tenant("i2 tenant hold");
    let iid = idn["id"].as_str().unwrap().to_owned();
    let addr = idn["primary_address"].as_str().unwrap().to_owned();
    let other = crate::identity(&pk, &tid, &crate::slug("i2 other")[..9]);
    mail_to(&addr, "kay@i2t.example", "i2t held");
    mail_to(
        other["primary_address"].as_str().unwrap(),
        "kay@i2t.example",
        "i2t free",
    );
    let held = eventually(30, "the held message", || {
        messages_of(&iid, &pk, "")
            .into_iter()
            .find(|m| m["subject"] == "i2t held")
    });
    let thread = held["thread_id"].as_str().unwrap().to_owned();
    let r = post(
        &format!("/v1/identities/{iid}/threads/{thread}/hold"),
        &pk,
        json!({ "reason": "litigation" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let e = erase(&pk, json!({ "tenant_id": tid, "scope": "tenant" }));
    let era = e.body["id"].as_str().unwrap().to_owned();
    let done = run(&era);
    assert_eq!(done["status"], "completed_with_holds", "{done}");
    assert_eq!(done["receipt"]["held"][0]["thread_id"], thread);
    let t = &d1(&format!("SELECT status FROM tenants WHERE id = '{tid}'"))[0];
    assert_eq!(t["status"], "erasing");
    let i = &d1(&format!("SELECT status FROM identities WHERE id = '{iid}'"))[0];
    assert_eq!(i["status"], "deleting");
    // The partner key reads the held mail, may not write, and releases the hold.
    let r = get(&format!("/v1/identities/{iid}/threads/{thread}"), &pk);
    assert_eq!(r.status, 200, "{}", r.body);
    let r = post(
        &format!("/v1/tenants/{tid}/identities"),
        &pk,
        json!({ "username": "late", "display_name": "x" }),
    );
    assert_eq!(r.status, 404);
    let r = crate::delete(&format!("/v1/identities/{iid}/threads/{thread}/hold"), &pk);
    assert_eq!(r.status, 200, "{}", r.body);
    // The global job's held_erasures step starts the continuation.
    let g = hook(
        "POST",
        "/__test/job-create",
        Some(json!({ "kind": "retention", "tenant_id": null, "params": { "global": true } })),
    );
    let gid = g.body["job_id"].as_str().unwrap().to_owned();
    eventually(120, "the global job", || {
        job_alarm(&gid);
        (d1(&format!("SELECT status FROM jobs WHERE id = '{gid}'"))[0]["status"] == "completed")
            .then_some(())
    });
    let cont = d1(&format!(
        "SELECT id, reason FROM erasure_requests WHERE resumes_id = '{era}'"
    ));
    assert_eq!(cont[0]["reason"], format!("hold_released:{era}"));
    let done = run(cont[0]["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    let t = &d1(&format!("SELECT status FROM tenants WHERE id = '{tid}'"))[0];
    assert_eq!(t["status"], "erased");
}

fn r2_keys(prefix: &str, backup: bool) -> Vec<String> {
    let mut body = json!({ "op": "list", "key": prefix });
    if backup {
        body["bucket"] = "backup".into();
    }
    let r = hook("POST", "/__test/r2", Some(body));
    r.body["keys"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|k| k.as_str().map(str::to_owned))
        .collect()
}

/// Runs a job created through `/__test/job-create` to its end; answers its status.
pub fn run_job(kind: &str, tenant: Option<&str>, params: Value) -> (String, String) {
    let r = hook(
        "POST",
        "/__test/job-create",
        Some(json!({ "kind": kind, "tenant_id": tenant, "params": params })),
    );
    let id = r.body["job_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{}", r.body))
        .to_owned();
    let status = eventually(120, "the job to end", || {
        job_alarm(&id);
        let s = d1(&format!("SELECT status FROM jobs WHERE id = '{id}'"))[0]["status"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        matches!(s.as_str(), "completed" | "failed" | "canceled").then_some(s)
    });
    (id, status)
}

/// I6: with `BACKUP` bound, an erasure leaves no object under the erased prefixes in either bucket,
/// and the Worker has no other R2 binding.
#[test]
fn i6_backup_purge() {
    let s = open_sender("i6 backup", "live");
    let m = one_message(&s, "rae@i6.example", "i6 copy me");
    let id = m["id"].as_str().unwrap();
    let prefix = format!("t/{}/i/{}/m/{id}/", s.tid, s.iid);
    let (_, status) = run_job("backup", None, json!({}));
    assert_eq!(status, "completed");
    assert!(
        !r2_keys(&prefix, true).is_empty(),
        "the backup holds the raw copy"
    );
    let e = erase(
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "counterparty", "counterparty_address": "rae@i6.example" }),
    );
    let done = run(e.body["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    assert!(r2_keys(&prefix, false).is_empty());
    assert!(r2_keys(&prefix, true).is_empty());
    let tmpl = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/wrangler.toml.tmpl"),
    )
    .unwrap();
    let bindings: Vec<&str> = tmpl
        .lines()
        .filter_map(|l| l.trim().trim_start_matches("# ").strip_prefix("binding"))
        .filter_map(|l| l.split('"').nth(1))
        .filter(|b| ["BLOBS", "BACKUP"].contains(b) || b.starts_with("R2"))
        .collect();
    assert_eq!(bindings, ["BLOBS", "BACKUP"]);
}

/// I6: after a simulated restore brought erased mail back, a re-applied counterparty erasure finds
/// it by the stored keyed hash alone, and a re-applied message erasure by the stored IDs.
#[test]
fn i6_reapply_by_hash() {
    let s = open_sender("i6 reapply", "live");
    let cp = "zoe@i6r.example";
    let raw = mail(cp, &s.addr, "i6r hello", "x");
    let (o, _) = inbound(cp, &s.addr, &raw);
    assert_eq!(o, "accepted");
    crate::wait_stored(&s.iid, &s.key, "received", 1);
    let e = erase(
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "counterparty", "counterparty_address": cp }),
    );
    let era = e.body["id"].as_str().unwrap().to_owned();
    assert_eq!(run(&era)["status"], "completed");
    // The restore: the mailbox forgets the erasure and the message arrives again.
    mailbox_query(&s.iid, "DELETE FROM erased_ids RETURNING message_id");
    let (o, _) = inbound(cp, &s.addr, &raw);
    assert_eq!(o, "accepted");
    crate::wait_stored(&s.iid, &s.key, "received", 1);
    let r = post(
        &format!("/v1/platform/erasure-requests/{era}/reapply"),
        &platform(),
        json!({}),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["reason"], json!(null));
    let row = &d1(&format!(
        "SELECT reason FROM erasure_requests WHERE id = '{}'",
        r.body["id"].as_str().unwrap()
    ))[0];
    assert_eq!(row["reason"], format!("reapply_after_restore:{era}"));
    let done = run(r.body["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    assert!(done["receipt"]["messages_deleted"].as_i64().unwrap() >= 1);
    assert_eq!(done["receipt"]["probe"]["keyword_hits"], 0);
    assert!(messages_of(&s.iid, &s.key, "").is_empty());
    // A request that did not complete cannot be re-applied.
    let m = one_message(&s, "nia@i6r.example", "i6r other");
    let era2 = erase_message(&s, m["id"].as_str().unwrap());
    let r = post(
        &format!("/v1/platform/erasure-requests/{era2}/reapply"),
        &platform(),
        json!({}),
    );
    assert!(
        r.status == 202 || r.code() == "erasure_not_completed",
        "{}",
        r.body
    );
}

/// I10: an index job that runs after its message was erased writes no vector.
#[test]
fn race_with_index_jobs() {
    let s = open_sender("i10 race", "live");
    let m = one_message(&s, "lou@i10.example", "i10 race");
    let id = m["id"].as_str().unwrap().to_owned();
    embedded(&s.iid, &id);
    let era = erase_message(&s, &id);
    assert_eq!(run(&era)["status"], "completed");
    crate::queue_send(
        "pm-index",
        json!({ "kind": "embed", "tenant_id": s.tid, "identity_id": s.iid, "message_id": id, "reason": "new" }),
    );
    std::thread::sleep(std::time::Duration::from_secs(3));
    let left: Vec<String> = fakes::vectors("VECTORS")
        .keys()
        .filter(|k| k.starts_with(&id))
        .cloned()
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// I11: an erasure expires the tenant's completed exports; an export whose collection overlapped an
/// erasure restarts once and then fails.
#[test]
fn i11_inflight_copies() {
    let s = open_sender("i11 inflight", "live");
    one_message(&s, "eve@i11.example", "i11 hello");
    let create = || {
        let r = post(
            "/v1/exports",
            &platform(),
            json!({ "tenant_id": s.tid, "scope": "identity", "identity_id": s.iid }),
        );
        assert_eq!(r.status, 202, "{}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    };
    let wait_export = |exp: &str| -> Value {
        let job = d1(&format!("SELECT job_id FROM exports WHERE id = '{exp}'"))[0]["job_id"]
            .as_str()
            .unwrap()
            .to_owned();
        eventually(120, "the export to end", || {
            job_alarm(&job);
            let r = get(&format!("/v1/exports/{exp}"), &platform());
            (!matches!(r.body["status"].as_str(), Some("queued" | "running"))).then_some(r.body)
        })
    };
    let exp = create();
    assert_eq!(wait_export(&exp)["status"], "completed");
    let zip = d1(&format!("SELECT r2_key FROM exports WHERE id = '{exp}'"))[0]["r2_key"]
        .as_str()
        .unwrap()
        .to_owned();
    let m = one_message(&s, "eve@i11.example", "i11 erase me");
    let era = erase_message(&s, m["id"].as_str().unwrap());
    assert_eq!(run(&era)["status"], "completed");
    assert_eq!(
        get(&format!("/v1/exports/{exp}"), &platform()).body["status"],
        "expired"
    );
    assert!(r2_keys(&zip, false).is_empty());
    // An erasure that completes after collection started supersedes the export twice: it fails.
    d1(&format!(
        "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_at, updated_at) VALUES ('job_i11{0}', '{0}', 'erasure', 'completed', 'x', '{{}}', 1, 1); \
         INSERT INTO erasure_requests (id, tenant_id, job_id, scope, reason, status, deadline_at, created_at, completed_at) VALUES ('era_i11{0}', '{0}', 'job_i11{0}', 'tenant', 'test', 'completed', 1, 1, 9999999999999)",
        s.tid
    ));
    let exp2 = create();
    let end = wait_export(&exp2);
    assert_eq!(end["status"], "failed", "{end}");
    let result = &d1(&format!(
        "SELECT j.result_json FROM jobs j JOIN exports e ON e.job_id = j.id WHERE e.id = '{exp2}'"
    ))[0]["result_json"];
    assert!(result.as_str().unwrap().contains("superseded_by_erasure"));
    assert!(r2_keys(&format!("t/{}/exports/{exp2}", s.tid), false).is_empty());
}

/// I12: counterparty erasure deletes only the provider's `sending_domain` suppressions of the
/// tenant's own domains; account, other tenants' and read-only entries stay.
#[test]
fn i12_provider_suppressions() {
    let (s, _, zone) = crate::addresses::on_domain("i12 provider");
    let cp = "ivy@i12.example";
    fakes::cf_suppress(cp, "account");
    fakes::cf_suppress(cp, &zone);
    fakes::cf_suppress(cp, "other-tenant.example");
    fakes::cf_suppress_with(cp, &zone, true);
    one_message(&s, cp, "i12 hello");
    let e = erase(
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "counterparty", "counterparty_address": cp }),
    );
    let done = run(e.body["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    let left = fakes::cf_suppressions_of(cp);
    assert_eq!(left.len(), 3, "{left:?}");
    assert!(
        !left
            .iter()
            .any(|x| x["domain"] == zone.as_str() && x["read_only"] == false)
    );
    assert_eq!(done["receipt"]["provider_suppressions_deleted"], 1);
}
