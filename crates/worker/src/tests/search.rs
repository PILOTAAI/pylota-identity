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

/// A unit vector along axis `k`.
fn axis(k: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; 1024];
    v[k] = 1.0;
    v
}

fn embedding(vs: &[Vec<f32>]) -> Value {
    json!({ "shape": [vs.len(), 1024], "data": vs })
}

#[test]
fn semantic_and_hybrid_with_coverage() {
    let h = Harness::new();
    let (tid, idn, address, mb) = setup(&h);
    let subjects = [
        "Car hire invoice",
        "Weekend availability",
        "Lunch plans",
        "Parking",
    ];
    for (i, s) in subjects.iter().enumerate() {
        email(
            &h,
            "alex@sender.example",
            &address,
            &plain(&address, &format!("v{i}@sender.example"), s, "Body text."),
        );
    }
    drain(&h, QueueName::Inbound);
    // One embedding per message, in job order: message i along axis i.
    for i in 0..subjects.len() {
        h.app
            .p
            .ai
            .answer("@cf/baai/bge-m3", Ok(embedding(&[axis(i)])));
    }
    let settled = drain(&h, QueueName::Index);
    assert!(!settled.is_empty());
    let rows = h.mailbox_sql(&mb, "SELECT status, model FROM chunks");
    assert_eq!(rows.len(), subjects.len(), "{rows:?}");
    assert!(
        rows.iter()
            .all(|r| r["status"] == "embedded" && r["model"] == "bge-m3@1"),
        "{rows:?}"
    );
    let key = searcher(&h, &tid, &["search:read"]);
    // The query lands on axis 1: "Weekend availability".
    h.app
        .p
        .ai
        .answer("@cf/baai/bge-m3", Ok(embedding(&[axis(1)])));
    let r = search(
        &h,
        &key,
        &idn,
        json!({ "q": "free on saturday", "mode": "semantic" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["hits"][0]["subject"], "Weekend availability");
    assert_eq!(r.body["hits"][0]["score"], 1.0);
    assert!(
        r.body["hits"][0]["why"]
            .as_array()
            .unwrap()
            .contains(&json!("semantic:1.00"))
    );
    assert_eq!(r.body["semantic_coverage"], 1.0);
    assert_eq!(r.body["degraded"], false);
    // Hybrid: the cached query vector, the keyword leg on "invoice", the reranker over both.
    h.app
        .p
        .ai
        .answer("@cf/baai/bge-m3", Ok(embedding(&[axis(1)])));
    h.app.p.ai.answer(
        "@cf/baai/bge-reranker-base",
        Ok(json!({ "response": [ { "id": 0, "score": 0.9 }, { "id": 1, "score": 0.2 } ] })),
    );
    let r = search(
        &h,
        &key,
        &idn,
        json!({ "q": "invoice weekend", "limit": 5 }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["degraded"], false, "{}", r.body);
    let hits = r.body["hits"].as_array().unwrap();
    assert!(
        hits[0]["why"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().starts_with("rerank:")),
        "{}",
        hits[0]
    );
    assert!(hits[0]["score"].as_f64().unwrap() >= 0.5);
    // The reranker down: RRF order, degraded.
    h.app
        .p
        .ai
        .answer("@cf/baai/bge-m3", Ok(embedding(&[axis(1)])));
    let r = search(&h, &key, &idn, json!({ "q": "invoice availability" }));
    assert_eq!(r.body["degraded"], true, "{}", r.body);
    assert!(
        r.body["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| !h["why"].to_string().contains("rerank:"))
    );
    assert!(
        r.body["hits"][0]["why"].to_string().contains("semantic:"),
        "{}",
        r.body
    );
    // A pending row lowers coverage (F4).
    h.mailbox_sql(
        &mb,
        "UPDATE chunks SET status = 'pending' WHERE rowid = (SELECT MIN(rowid) FROM chunks)",
    );
    h.app
        .p
        .ai
        .answer("@cf/baai/bge-m3", Ok(embedding(&[axis(1)])));
    let r = search(
        &h,
        &key,
        &idn,
        json!({ "q": "weekend", "mode": "semantic" }),
    );
    assert_eq!(r.body["semantic_coverage"], 0.75, "{}", r.body);
}

/// `s` as RFC 2047 encoded words, folded, so a long non-ASCII subject survives the header limits.
pub(super) fn encoded_subject(s: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let encode = |raw: &[u8]| {
        let mut b64 = String::new();
        for c in raw.chunks(3) {
            let n = c
                .iter()
                .enumerate()
                .fold(0u32, |a, (i, &b)| a | (u32::from(b) << (16 - 8 * i)));
            for i in 0..4 {
                b64.push(if i <= c.len() {
                    char::from(T[((n >> (18 - 6 * i)) & 63) as usize])
                } else {
                    '='
                });
            }
        }
        b64
    };
    let chars: Vec<char> = s.chars().collect();
    chars
        .chunks(10)
        .map(|c| {
            format!(
                "=?UTF-8?B?{}?=",
                encode(c.iter().collect::<String>().as_bytes())
            )
        })
        .collect::<Vec<_>>()
        .join("\r\n ")
}

#[test]
fn byte_cap_truncates_with_a_continuing_cursor() {
    let h = Harness::new();
    let (tid, idn, address, _) = setup(&h);
    let wide = "𝐀".repeat(990);
    let words = vec!["𝐁𝐁𝐁𝐁𝐁𝐁"; 300];
    let mut body = String::from("budget");
    for line in words.chunks(10) {
        body.push_str("\r\n");
        body.push_str(&line.join(" "));
    }
    for i in 0..60 {
        let subject = encoded_subject(&format!("{wide} {i}"));
        email(
            &h,
            "alex@sender.example",
            &address,
            &plain(&address, &format!("w{i}@sender.example"), &subject, &body),
        );
    }
    drain(&h, QueueName::Inbound);
    let key = searcher(&h, &tid, &["search:read"]);
    let q = json!({ "q": "budget", "mode": "keyword", "limit": 50, "snippet_chars": 1000 });
    let r = search(&h, &key, &idn, q.clone());
    assert_eq!(r.status, 200, "{}", r.body);
    let bytes = r.body.to_string().len();
    let first = r.body["hits"].as_array().unwrap().clone();
    assert_eq!(
        r.body["truncated"],
        true,
        "{bytes} bytes, {} hits",
        first.len()
    );
    assert!(bytes <= 262_144, "{bytes}");
    assert!(!first.is_empty() && first.len() < 50);
    assert!(first[0]["subject"].as_str().unwrap().chars().count() >= 992);
    let mut q2 = q.clone();
    q2["cursor"] = r.body["next_cursor"].clone();
    let r2 = search(&h, &key, &idn, q2);
    let second = r2.body["hits"].as_array().unwrap();
    assert!(!second.is_empty());
    assert!(
        second
            .iter()
            .all(|h| !first.iter().any(|f| f["message_id"] == h["message_id"]))
    );
}
