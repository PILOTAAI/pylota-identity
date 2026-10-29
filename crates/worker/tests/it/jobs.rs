//! `JobRunner` ends through workerd (Privacy and erasure § 4): every failed or canceled job runs its
//! `finalise` (I13).

use serde_json::{Value, json};

use crate::erasure::{erase, eraser, job_alarm, job_of, run};
use crate::send::open_sender;
use crate::{d1, eventually, fakes, get, hook, inbound, mail, platform, post};

fn r2_list(prefix: &str) -> Value {
    hook(
        "POST",
        "/__test/r2",
        Some(json!({ "op": "list", "key": prefix })),
    )
    .body["keys"]
        .clone()
}

/// I13: a counterparty erasure that failed past its deadline and one canceled by a tenant erasure
/// end with no address in the runner; a failed and a canceled export leave no part and no ZIP.
#[test]
fn i13_finalise_on_fail_and_cancel() {
    let _reset = fakes::Reset;
    let s = open_sender("i13 finalise", "live");
    let cp = "ola@i13.example";
    let (o, _) = inbound(cp, &s.addr, &mail(cp, &s.addr, "i13 hello", "x"));
    assert_eq!(o, "accepted");
    crate::wait_stored(&s.iid, &s.key, "received", 1);
    fakes::fault("r2.delete", "BLOBS", "error", 100_000);
    let body = json!({ "tenant_id": s.tid, "scope": "counterparty", "counterparty_address": cp });
    let era = erase(&platform(), body.clone()).body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let job = job_of(&era);
    let j = eventually(60, "a failed attempt", || {
        let j = job_alarm(&job);
        (j["meta"]["target_address"] == cp && j["steps"].to_string().contains("\"attempts\":1"))
            .then_some(j)
    });
    assert_eq!(j["meta"]["target_address"], cp);
    fakes::advance(21 * 3_600_000);
    assert_eq!(run(&era)["status"], "failed");
    let j = job_alarm(&job);
    assert!(j["meta"].get("target_address").is_none(), "{j}");
    // Canceled by a tenant erasure.
    let era2 = erase(&platform(), body).body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let job2 = job_of(&era2);
    eventually(60, "the second job to start", || {
        (job_alarm(&job2)["meta"]["target_address"] == cp).then_some(())
    });
    fakes::reset();
    let t = erase(
        &eraser(&s.tid),
        json!({ "tenant_id": s.tid, "scope": "tenant" }),
    );
    assert_eq!(t.status, 202, "{}", t.body);
    eventually(60, "the cancel", || {
        job_alarm(&job_of(t.body["id"].as_str().unwrap()));
        let j = job_alarm(&job2);
        (j["meta"]["ended"] == "canceled").then_some(())
    });
    let j = job_alarm(&job2);
    assert!(j["meta"].get("target_address").is_none(), "{j}");
    let r = get(&format!("/v1/erasure-requests/{era2}"), &platform());
    assert_eq!(r.body["status"], "canceled");
    assert_eq!(run(t.body["id"].as_str().unwrap())["status"], "completed");
}

/// I13 for exports: a canceled export (stopped while its `init` waits for an erasure) leaves no
/// part and no ZIP.
#[test]
fn i13_export_cancel_leaves_nothing() {
    let s = open_sender("i13 export", "live");
    d1(&format!(
        "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_at, updated_at) VALUES ('job_i13{0}', '{0}', 'erasure', 'running', 'x', '{{}}', 1, 1); \
         INSERT INTO erasure_requests (id, tenant_id, job_id, scope, reason, status, deadline_at, created_at) VALUES ('era_i13{0}', '{0}', 'job_i13{0}', 'tenant', 'test', 'running', 1, 1)",
        s.tid
    ));
    let r = post(
        "/v1/exports",
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "identity", "identity_id": s.iid }),
    );
    let exp = r.body["id"].as_str().unwrap().to_owned();
    let job = d1(&format!("SELECT job_id FROM exports WHERE id = '{exp}'"))[0]["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    job_alarm(&job);
    let r = crate::delete(&format!("/v1/exports/{exp}"), &platform());
    assert_eq!(r.status, 204);
    eventually(30, "the canceled end", || {
        (job_alarm(&job)["meta"]["ended"] == "canceled").then_some(())
    });
    assert_eq!(
        get(&format!("/v1/exports/{exp}"), &platform()).body["status"],
        "canceled"
    );
    assert_eq!(r2_list(&format!("t/{}/exports/", s.tid)), json!([]));
}

/// `it::jobs::j3_reparse` (J3): a `reparse` job started with `POST /v1/platform/jobs` (`platform:ops`)
/// re-parses a stored inbound message from its raw MIME: the parsed text, `parser_version` and the
/// keyword index come back from raw, the message carries the `reprocessed` flag, and
/// `message.received` is emitted again with `reprocessed: true`. A non-platform key gets `403`.
#[test]
fn j3_reparse() {
    let (tid, idn, addr, key) = crate::mailbox_fixture("j3reparse");
    crate::inbound(
        "ana@sender.example",
        &addr,
        &crate::mail(
            "ana@sender.example",
            &addr,
            "J3 invoice",
            "Invoice INV-7731 attached",
        ),
    );
    let m = crate::wait_stored(&idn, &key, "received", 1);
    let mid = m[0]["id"].as_str().unwrap().to_owned();
    // A parser bug: the stored text is wrong and the parser version old.
    crate::mailbox_query(
        &idn,
        &format!(
            "UPDATE messages SET text = 'garbled', extracted_text = 'garbled', parser_version = 0 WHERE id = '{mid}'"
        ),
    );
    let before = crate::send::events_of(&idn, "message.received").len();

    let tenant_key = crate::owner_key(&tid, &["messages:read"]);
    let denied = crate::post(
        "/v1/platform/jobs",
        &tenant_key,
        json!({ "kind": "reparse", "tenant_id": tid }),
    );
    assert_eq!(denied.status, 403, "{}", denied.body);
    let r = crate::post(
        "/v1/platform/jobs",
        &crate::platform(),
        json!({ "kind": "reparse", "tenant_id": tid, "identity_ids": [idn] }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let job = r.body["id"].as_str().unwrap().to_owned();
    crate::eventually(60, "the reparse job to complete", || {
        let j = crate::get(&format!("/v1/platform/jobs/{job}"), &crate::platform()).body;
        (j["status"] == "completed").then_some(j)
    });
    let row = crate::eventually(60, "the re-parsed message", || {
        let rows = crate::mailbox_query(
            &idn,
            &format!("SELECT text, parser_version, flags_json FROM messages WHERE id = '{mid}'"),
        );
        (rows[0]["parser_version"] == 1).then(|| rows[0].clone())
    });
    assert!(row["text"].as_str().unwrap().contains("INV-7731"), "{row}");
    assert!(row["flags_json"].as_str().unwrap().contains("reprocessed"));
    let events = crate::send::events_of(&idn, "message.received");
    assert_eq!(events.len(), before + 1);
    let last = events.last().unwrap();
    assert_eq!(last["reprocessed"], true);
    assert_eq!(last["message"]["id"], mid.as_str());
    // The keyword index matches the re-parsed text again.
    let hits = crate::post(
        &format!("/v1/identities/{idn}/search"),
        &key,
        json!({ "q": "INV-7731", "mode": "keyword" }),
    );
    assert_eq!(hits.status, 200, "{}", hits.body);
    assert!(hits.body.to_string().contains(&mid), "{}", hits.body);
}
