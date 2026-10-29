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
