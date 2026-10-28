//! M10 on fakes: keyword search through the route, the mailbox's FTS5 tables and the cursor.

use pylota_identity_platform::queues::QueueName;
use serde_json::{Value, json};

use super::Harness;
use super::inbound::{drain, email, plain, setup};

fn searcher(h: &Harness, tid: &str, perms: &[&str]) -> String {
    h.seed_key("tenant", perms, Some(tid), None, None, None)
}

fn search(h: &Harness, key: &str, idn: &str, body: Value) -> super::Resp {
    h.call(
        "POST",
        &format!("/v1/identities/{idn}/search"),
        Some(key),
        Some(body),
        &[],
    )
}

#[test]
fn keyword_hits_snippets_facets_and_cursor() {
    let h = Harness::new();
    let (tid, idn, address, _) = setup(&h);
    for (i, (subject, body)) in [
        (
            "Invoice 4411 for the hire",
            "Please find the invoice attached for plate AB12 CDE.",
        ),
        (
            "Booking question",
            "Is the car free on Friday? Invoice later.",
        ),
        ("Lunch", "Nothing to do with cars."),
        ("Weather", "Rain all week."),
        ("Parking", "The garage opens at nine."),
        ("Keys", "Leave them at the desk."),
        ("Fuel", "Full tank on return."),
    ]
    .iter()
    .enumerate()
    {
        email(
            &h,
            "alex@sender.example",
            &address,
            &plain(&address, &format!("s{i}@sender.example"), subject, body),
        );
    }
    drain(&h, QueueName::Inbound);
    let key = searcher(&h, &tid, &["search:read"]);
    let r = search(
        &h,
        &key,
        &idn,
        json!({ "q": "invoice", "mode": "keyword", "limit": 1 }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let hits = r.body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["subject"], "Invoice 4411 for the hire");
    assert!(
        hits[0]["snippet"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("invoice")
    );
    assert!(
        hits[0]["why"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().starts_with("text:\"invoice\"")),
        "{}",
        hits[0]["why"]
    );
    assert_eq!(
        r.body["query"],
        json!({ "parsed": "invoice", "mode": "keyword" })
    );
    assert_eq!(r.body["semantic_coverage"], Value::Null);
    assert_eq!(r.body["facets"]["sender"]["alex@sender.example"], 2);
    assert_eq!(r.body["facets"].as_object().unwrap().len(), 6);
    let next = r.body["next_cursor"]
        .as_str()
        .expect("a second page")
        .to_owned();
    let r2 = search(
        &h,
        &key,
        &idn,
        json!({ "q": "invoice", "mode": "keyword", "limit": 1, "cursor": next }),
    );
    assert_eq!(r2.status, 200, "{}", r2.body);
    assert_eq!(r2.body["hits"][0]["subject"], "Booking question");
    assert_eq!(r2.body["facets"], Value::Null);
    assert_eq!(r2.body["next_cursor"], Value::Null);
    // A cursor for another query.
    let r3 = search(
        &h,
        &key,
        &idn,
        json!({ "q": "lunch", "mode": "keyword", "cursor": next }),
    );
    assert_eq!(r3.status, 400);
    // A reference, written either way.
    let r4 = search(
        &h,
        &key,
        &idn,
        json!({ "q": "ref:4411", "mode": "keyword" }),
    );
    assert_eq!(r4.body["hits"].as_array().unwrap().len(), 1, "{}", r4.body);
    assert_eq!(r4.body["hits"][0]["why"][0], "ref:4411 (subject)");
    // Filter only: everything, newest first, score 1.
    let all = search(
        &h,
        &key,
        &idn,
        json!({ "q": "", "mode": "keyword", "group_by": "thread", "limit": 50 }),
    );
    assert_eq!(
        all.body["hits"].as_array().unwrap().len(),
        7,
        "{}",
        all.body
    );
    assert!(all.body["hits"][0]["top_message_id"].is_string());
    // No search:read.
    let none = searcher(&h, &tid, &["messages:read"]);
    assert_eq!(search(&h, &none, &idn, json!({ "q": "x" })).status, 403);
    // Agentic waits for M11.
    let ag = searcher(&h, &tid, &["search:read", "search:agentic"]);
    let r5 = search(&h, &ag, &idn, json!({ "q": "x", "mode": "agentic" }));
    assert_eq!(r5.code(), "agentic_disabled");
    // Hybrid without Workers AI degrades to keyword; require_mode refuses.
    let r6 = search(&h, &key, &idn, json!({ "q": "invoice" }));
    assert_eq!(r6.status, 200, "{}", r6.body);
    assert_eq!(r6.body["degraded"], true);
    let r7 = search(
        &h,
        &key,
        &idn,
        json!({ "q": "invoice", "require_mode": true }),
    );
    assert_eq!(r7.code(), "search_degraded");
}
