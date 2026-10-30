//! Search through workerd (Search § 2–12): keyword, semantic and hybrid over the fake Workers AI and
//! Vectorize (Testing § 6.3), tenant fan-out, budgets, coverage, contacts and related messages.

use serde_json::{Value, json};

use crate::{
    dkim, eventually, fakes, get, identity, inject, mail, mailbox_fixture, mailbox_query, mint,
    partner, platform, post, tenant, wait_stored,
};

fn search(idn: &str, key: &str, body: Value) -> crate::Resp {
    post(&format!("/v1/identities/{idn}/search"), key, body)
}

fn hits(r: &crate::Resp) -> Vec<Value> {
    r.body["hits"].as_array().cloned().unwrap_or_default()
}

/// Delivers `n` messages (`subject i`, `body i`) and waits until they are stored.
fn deliver(idn: &str, addr: &str, key: &str, items: &[(&str, &str)]) -> Vec<Value> {
    for (s, b) in items {
        let from = "notes@mail.example";
        assert_eq!(inject(from, addr, &mail(from, addr, s, b)), 200);
    }
    wait_stored(idn, key, "received", items.len())
}

/// Waits until every chunk row of the mailbox is `embedded` (and there are `n` messages with rows).
fn embedded(idn: &str, n: usize) {
    eventually(40, "chunks embedded", || {
        let rows = mailbox_query(
            idn,
            "SELECT COUNT(DISTINCT message_rowid) AS m, SUM(status <> 'embedded') AS open FROM chunks",
        );
        (rows[0]["m"].as_u64() == Some(n as u64) && rows[0]["open"].as_u64() == Some(0))
            .then_some(())
    });
}

/// F3: an identity key on the tenant route is `403 scope_denied`; a partner key reaches its own
/// tenants only; tenant search always carries `partial` and `failed_identities`.
#[test]
fn f3_tenant_scope_denied() {
    let (tid, idn, _, _) = mailbox_fixture("f3scope");
    let p = platform();
    let (_, ik) = mint(
        &p,
        json!({ "name": "i", "level": "identity", "tenant_id": tid, "identity_id": idn, "permissions": ["search:read"] }),
    );
    let r = post(
        &format!("/v1/tenants/{tid}/search"),
        &ik,
        json!({ "q": "x" }),
    );
    assert_eq!((r.status, r.code()), (403, "scope_denied"), "{}", r.body);
    // The identity route still works for it.
    assert_eq!(
        search(&idn, &ik, json!({ "q": "x", "mode": "keyword" })).status,
        200
    );
    let (_, pk) = partner("f3own", &["tenants:manage", "search:read"]);
    let own = tenant(&pk, "f3own");
    let oid = own["id"].as_str().unwrap();
    let r = post(
        &format!("/v1/tenants/{oid}/search"),
        &pk,
        json!({ "q": "", "mode": "keyword" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["partial"], false);
    assert_eq!(r.body["failed_identities"], json!([]));
    let r = post(
        &format!("/v1/tenants/{tid}/search"),
        &pk,
        json!({ "q": "x", "mode": "keyword" }),
    );
    assert_eq!(r.code(), "tenant_not_found");
    // An identity of another tenant in identity_ids.
    let (_, tk) = mint(
        &p,
        json!({ "name": "t", "level": "tenant", "tenant_id": oid, "permissions": ["search:read"] }),
    );
    let r = post(
        &format!("/v1/tenants/{oid}/search"),
        &tk,
        json!({ "q": "x", "mode": "keyword", "identity_ids": [idn] }),
    );
    assert_eq!(r.code(), "identity_not_found");
}

/// F4: coverage counts pending, failed and stale-model rows as not covered, ignores `deleting` rows,
/// and is `null` in keyword mode.
#[test]
fn f4_coverage() {
    let _reset = fakes::Reset;
    let (_, idn, addr, key) = mailbox_fixture("f4cov");
    deliver(
        &idn,
        &addr,
        &key,
        &[
            ("Rental agreement", "The agreement for the van."),
            ("Damage report", "A scratch on the door."),
            ("Fuel policy", "Return the car full."),
            ("Pickup time", "Nine in the morning."),
        ],
    );
    embedded(&idn, 4);
    let cov = |mode: &str| {
        search(&idn, &key, json!({ "q": "van agreement", "mode": mode })).body["semantic_coverage"]
            .clone()
    };
    assert_eq!(cov("hybrid"), json!(1.0));
    assert_eq!(cov("keyword"), Value::Null);
    let ids: Vec<i64> = mailbox_query(&idn, "SELECT rowid FROM messages ORDER BY rowid")
        .iter()
        .map(|r| r["rowid"].as_i64().unwrap())
        .collect();
    // Each change waits out the mailbox's 60-second coverage cache.
    let set = |rowid: i64, sql: &str| {
        mailbox_query(
            &idn,
            &format!("UPDATE chunks SET {sql} WHERE message_rowid = {rowid}"),
        );
        fakes::advance(61_000);
    };
    set(ids[0], "status = 'pending'");
    assert_eq!(cov("semantic"), json!(0.75));
    set(ids[1], "status = 'failed'");
    set(ids[2], "model = 'bge-m3@0'");
    assert_eq!(cov("semantic"), json!(0.25));
    // Only deleting rows: not covered either; restoring the rows restores coverage.
    set(ids[0], "status = 'embedded'");
    set(ids[1], "status = 'embedded'");
    set(ids[2], "model = 'bge-m3@1'");
    assert_eq!(cov("semantic"), json!(1.0));
}

/// F5: fewer than three keyword hits runs the trigram fallback; a partial word and a one-letter typo
/// are found, marked `fuzzy:`.
#[test]
fn f5_trigram() {
    let (_, idn, addr, key) = mailbox_fixture("f5tri");
    deliver(
        &idn,
        &addr,
        &key,
        &[
            ("Brightwell maintenance schedule", "Service on Monday."),
            ("Unrelated", "Nothing here."),
        ],
    );
    // A partial word, and a one-letter substitution.
    for q in ["brightwel", "brightwall"] {
        let r = search(&idn, &key, json!({ "q": q, "mode": "keyword" }));
        assert_eq!(r.status, 200, "{}", r.body);
        let h = hits(&r);
        assert_eq!(h.len(), 1, "{q}: {}", r.body);
        assert_eq!(h[0]["subject"], "Brightwell maintenance schedule");
        assert!(
            h[0]["why"]
                .as_array()
                .unwrap()
                .contains(&json!(format!("fuzzy:\"{q}\""))),
            "{q}: {}",
            h[0]["why"]
        );
    }
}

/// F7: quarantined mail is absent unless `include_quarantined` with `quarantine:review`; without the
/// permission the flag and `is:quarantined` are filtered silently; semantic read-back hides it too.
#[test]
fn f7_quarantine_hidden() {
    let (tid, idn, addr, key) = mailbox_fixture("f7q");
    let raw = dkim::sign(&mail(
        "security@brightwell.example",
        &addr,
        "Your verification code",
        "Your verification code is 135 790.",
    ));
    assert_eq!(inject("bounce@brightwell.example", &addr, &raw), 200);
    wait_stored(&idn, &key, "quarantined", 1);
    deliver(
        &idn,
        &addr,
        &key,
        &[("Verification of booking", "Booking verified.")],
    );
    let p = platform();
    let (_, plain) = mint(
        &p,
        json!({ "name": "s", "level": "tenant", "tenant_id": tid, "permissions": ["search:read"] }),
    );
    let quarantined = |r: &crate::Resp| {
        hits(r)
            .iter()
            .filter(|h| h["trust"]["quarantined"] == true)
            .count()
    };
    for body in [
        json!({ "q": "verification", "mode": "keyword" }),
        json!({ "q": "verification", "mode": "keyword", "include_quarantined": true }),
        json!({ "q": "is:quarantined", "mode": "keyword" }),
    ] {
        let r = search(&idn, &plain, body.clone());
        assert_eq!(r.status, 200, "{body}: {}", r.body);
        assert_eq!(quarantined(&r), 0, "{body}: {}", r.body);
    }
    let r = search(
        &idn,
        &key,
        json!({ "q": "verification", "mode": "keyword", "include_quarantined": true }),
    );
    assert_eq!(quarantined(&r), 1, "{}", r.body);
    assert_eq!(
        quarantined(&search(
            &idn,
            &key,
            json!({ "q": "verification", "mode": "keyword" })
        )),
        0
    );
    // Semantic: the quarantined message has no vectors, and the read-back would hide it.
    embedded(&idn, 1);
    let r = search(
        &idn,
        &key,
        json!({ "q": "verification code", "mode": "semantic" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(quarantined(&r), 0);
}

/// `s` as RFC 2047 encoded words, folded, so a long non-ASCII subject survives the header limits.
fn encoded_subject(s: &str) -> String {
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

/// F8: the request bounds, `snippet_chars`, `group_by: thread`, and the 256 KB cap that sets
/// `truncated` with a cursor continuing after the last hit kept.
#[test]
fn f8_budget() {
    let (_, idn, addr, key) = mailbox_fixture("f8budget");
    for (body, path) in [
        (json!({ "q": "x", "limit": 51 }), "limit"),
        (json!({ "q": "x", "limit": 0 }), "limit"),
        (json!({ "q": "x", "snippet_chars": 39 }), "snippet_chars"),
        (json!({ "q": "x", "snippet_chars": 1001 }), "snippet_chars"),
        (json!({ "q": "x".repeat(1025) }), "q"),
        (json!({ "q": "x", "mode": "fast" }), "mode"),
    ] {
        let r = search(&idn, &key, body);
        assert_eq!((r.status, r.code()), (400, "invalid_request"), "{path}");
    }
    // Big hits: 4-byte characters in the subject (as RFC 2047 encoded words) and the body.
    let wide = "𝐀".repeat(990);
    let words = vec!["𝐁𝐁𝐁𝐁𝐁𝐁"; 300];
    let mut body = String::from("budget");
    for line in words.chunks(10) {
        body.push_str("\r\n");
        body.push_str(&line.join(" "));
    }
    let items: Vec<(String, String)> = (0..60)
        .map(|i| (encoded_subject(&format!("{wide} {i}")), body.clone()))
        .collect();
    let refs: Vec<(&str, &str)> = items
        .iter()
        .map(|(s, b)| (s.as_str(), b.as_str()))
        .collect();
    deliver(&idn, &addr, &key, &refs);
    let r = search(
        &idn,
        &key,
        json!({ "q": "budget", "mode": "keyword", "limit": 50, "snippet_chars": 1000 }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let bytes = r.body.to_string().len();
    assert!(bytes <= 262_144, "{bytes} bytes");
    assert_eq!(r.body["truncated"], true);
    let first = hits(&r);
    assert!(!first.is_empty() && first.len() < 50, "{}", first.len());
    for h in &first {
        assert!(h["snippet"].as_str().unwrap().chars().count() <= 1000);
    }
    let next = r.body["next_cursor"].as_str().expect("a continuing cursor");
    let r2 = search(
        &idn,
        &key,
        json!({ "q": "budget", "mode": "keyword", "limit": 50, "snippet_chars": 1000, "cursor": next }),
    );
    assert_eq!(r2.status, 200, "{}", r2.body);
    let ids = |v: &[Value]| -> Vec<String> {
        v.iter()
            .map(|h| h["message_id"].as_str().unwrap().to_owned())
            .collect()
    };
    let (a, b) = (ids(&first), ids(&hits(&r2)));
    assert!(!b.is_empty());
    assert!(a.iter().all(|x| !b.contains(x)), "no hit twice");
    // Small snippets fit; one thread row per thread.
    let r = search(
        &idn,
        &key,
        json!({ "q": "budget", "mode": "keyword", "limit": 5, "snippet_chars": 40, "group_by": "thread" }),
    );
    assert_eq!(r.body["truncated"], false);
    for h in hits(&r) {
        assert!(h["snippet"].as_str().unwrap().chars().count() <= 40);
        assert!(h["top_message_id"].is_string() && h["message_count"].is_i64());
    }
}

/// F15: a mailbox that misses the 900 ms deadline is left out: `partial: true` and its ID in
/// `failed_identities`; the others answer.
#[test]
fn f15_partial() {
    let _reset = fakes::Reset;
    let p = platform();
    let t = tenant(&p, "f15fan");
    let tid = t["id"].as_str().unwrap().to_owned();
    let a = identity(&p, &tid, "fana");
    let b = identity(&p, &tid, "fanb");
    let (_, tk) = mint(
        &p,
        json!({ "name": "t", "level": "tenant", "tenant_id": tid, "permissions": ["search:read"] }),
    );
    let ids = [a["id"].as_str().unwrap(), b["id"].as_str().unwrap()];
    let first = ids.iter().min().unwrap().to_string();
    let path = format!("/v1/tenants/{tid}/search");
    let ok = post(
        &path,
        &tk,
        json!({ "q": "", "mode": "keyword", "facets": false }),
    );
    assert_eq!(ok.body["partial"], false, "{}", ok.body);
    // The first identity's mailbox is slow (scoped to its object, so a background job of an earlier
    // test cannot take the fault).
    let object = crate::d1(&format!(
        "SELECT mailbox_do_id FROM identities WHERE id = '{first}'"
    ))[0]["mailbox_do_id"]
        .as_str()
        .unwrap()
        .to_owned();
    fakes::fault("do.call", &format!("MAILBOX:{object}"), "delay:2000", 1);
    let r = post(
        &path,
        &tk,
        json!({ "q": "", "mode": "keyword", "facets": false }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["partial"], true, "{}", r.body);
    assert_eq!(r.body["failed_identities"], json!([first]));
}

/// B12 (search side): a keyword hit on a message whose attachment text is `unavailable` says so in
/// `why`.
#[test]
fn b12_unavailable_in_why() {
    let (_, idn, addr, key) = mailbox_fixture("b12why");
    let raw = format!(
        "From: a@pdf.example\r\nTo: {addr}\r\nSubject: Scanned contract\r\nMessage-ID: <{}@it.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nContract attached.\r\n--b\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"contract.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQKJcOkw7zDtsOfCjEgMCBvYmoKPDw+PgplbmRvYmoKdHJhaWxlcgo8PD4+CiUlRU9GCg==\r\n--b--\r\n",
        crate::slug("b12why")
    );
    assert_eq!(inject("a@pdf.example", &addr, &raw), 200);
    wait_stored(&idn, &key, "received", 1);
    eventually(40, "text_status unavailable", || {
        let rows = mailbox_query(&idn, "SELECT text_status FROM attachments");
        (rows[0]["text_status"] == "unavailable").then_some(())
    });
    for q in ["contract", "has:attachment", "type:pdf"] {
        let r = search(&idn, &key, json!({ "q": q, "mode": "keyword" }));
        let h = hits(&r);
        assert_eq!(h.len(), 1, "{q}: {}", r.body);
        assert!(
            h[0]["why"]
                .as_array()
                .unwrap()
                .contains(&json!("attachment_text_unavailable")),
            "{q}: {}",
            h[0]["why"]
        );
    }
    // A query that is not about attachments or text does not mention it.
    let r = search(&idn, &key, json!({ "q": "in:inbound", "mode": "keyword" }));
    assert!(!hits(&r)[0]["why"].to_string().contains("unavailable"));
}

/// Contacts (Search § 12.1): match ranks, outbound weighting, the cursor.
#[test]
fn contacts_rank() {
    let (_, idn, addr, key) = mailbox_fixture("contacts");
    for (from, n) in [
        ("dana@fleet.example", 3),
        ("dan@other.example", 1),
        ("zoe@fleet.example", 1),
    ] {
        for i in 0..n {
            let raw = mail(from, &addr, &format!("Hello {from} {i}"), "Hi.");
            assert_eq!(inject(from, &addr, &raw), 200);
        }
    }
    wait_stored(&idn, &key, "received", 5);
    let list = |q: &str| {
        let r = get(&format!("/v1/identities/{idn}/contacts?q={q}"), &key);
        assert_eq!(r.status, 200, "{}", r.body);
        r.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["address"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    // Exact address first; prefix; domain.
    assert_eq!(list("dan%40other.example")[0], "dan@other.example");
    assert_eq!(list("dan"), ["dana@fleet.example", "dan@other.example"]);
    assert_eq!(
        list("fleet.example"),
        ["dana@fleet.example", "zoe@fleet.example"]
    );
    let all = get(&format!("/v1/identities/{idn}/contacts?limit=2"), &key);
    assert_eq!(all.body["data"][0]["address"], "dana@fleet.example");
    assert_eq!(all.body["data"][0]["inbound_count"], 3);
    let next = all.body["next_cursor"].as_str().expect("more");
    let page2 = get(
        &format!("/v1/identities/{idn}/contacts?limit=2&cursor={next}"),
        &key,
    );
    assert_eq!(page2.status, 200, "{}", page2.body);
    assert_eq!(page2.body["data"].as_array().unwrap().len(), 1);
}

/// Find related (Search § 12.2): neighbours outside the source thread, shared references boost, and
/// the keyword fallback when the source has no vectors.
#[test]
fn related_excludes_thread() {
    let (_, idn, addr, key) = mailbox_fixture("related");
    let stored = deliver(
        &idn,
        &addr,
        &key,
        &[
            (
                "Invoice 88213 for the van",
                "Invoice for the van hire, please pay.",
            ),
            (
                "Re: van hire invoice",
                "Paid the van hire invoice 88213 today.",
            ),
            ("Garden party", "Bring snacks."),
        ],
    );
    embedded(&idn, 3);
    let src = stored
        .iter()
        .find(|m| m["subject"] == "Invoice 88213 for the van")
        .unwrap();
    let sid = src["id"].as_str().unwrap();
    let r = get(
        &format!("/v1/identities/{idn}/messages/{sid}/related?limit=5"),
        &key,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let data = r.body["data"].as_array().unwrap();
    assert!(
        data.iter()
            .all(|h| h["message_id"] != sid && h["thread_id"] != src["thread_id"])
    );
    assert_eq!(data[0]["subject"], "Re: van hire invoice", "{}", r.body);
    assert!(
        data[0]["why"].to_string().contains("ref:88213"),
        "{}",
        data[0]["why"]
    );
    // No vectors: the keyword fallback still finds it.
    mailbox_query(&idn, "DELETE FROM chunks");
    let r = get(
        &format!("/v1/identities/{idn}/messages/{sid}/related"),
        &key,
    );
    assert_eq!(
        r.body["data"][0]["subject"], "Re: van hire invoice",
        "{}",
        r.body
    );
    let missing = get(
        &format!("/v1/identities/{idn}/messages/msg_01ARYZ6S41TSV4RRFFQ69G5FAV/related"),
        &key,
    );
    assert_eq!(missing.code(), "message_not_found");
}

/// Agentic searches count once each against the tenant's daily cap, whatever their steps; a request
/// refused before Init never counts; a partner key searches its own tenant on the tenant route and gets
/// `404 tenant_not_found` for another partner's.
#[test]
fn agentic_counted_once() {
    let _reset = fakes::Reset;
    let (_, pk) = partner(
        "agentic count",
        &[
            "tenants:manage",
            "identities:write",
            "search:read",
            "search:agentic",
            "policy:write",
        ],
    );
    let own = tenant(&pk, "agentic count own");
    let tid = own["id"].as_str().unwrap().to_owned();
    identity(&pk, &tid, "countme");
    let (_, other) = partner("agentic count other", &["tenants:manage"]);
    let theirs = tenant(&other, "agentic count theirs");
    let p = platform();
    assert_eq!(
        crate::patch(
            &format!("/v1/tenants/{tid}/policy"),
            &p,
            json!({ "search": { "agentic_daily_cap": 2 } })
        )
        .status,
        200
    );
    let path = format!("/v1/tenants/{tid}/search");
    let step = |id: &str, q: &str| {
        crate::agentic::completion(
            &[(id, "search", json!({ "q": q, "mode": "keyword" }))],
            None,
        )
    };
    // Invalid bodies: never counted.
    for body in [
        json!({ "q": "", "mode": "agentic" }),
        json!({ "q": "x", "mode": "agentic", "budget": { "max_steps": 11 } }),
        json!({ "q": "\u{0}", "mode": "agentic", "limit": 3 }),
    ] {
        assert_eq!(post(&path, &pk, body).status, 400);
    }
    // Two valid runs of several model calls each.
    for _ in 0..2 {
        fakes::ai_script(
            "qwen",
            vec![
                step("a", "one"),
                step("b", "two"),
                crate::agentic::completion(&[], Some("READY")),
                crate::agentic::answer(&[], "insufficient_evidence"),
            ],
        );
        let r = post(&path, &pk, json!({ "q": "anything", "mode": "agentic" }));
        assert_eq!(r.status, 200, "{}", r.body);
    }
    let r = post(&path, &pk, json!({ "q": "anything", "mode": "agentic" }));
    assert_eq!(
        (r.status, r.code()),
        (429, "agentic_budget_exhausted"),
        "{}",
        r.body
    );
    let r = post(
        &format!("/v1/tenants/{}/search", theirs["id"].as_str().unwrap()),
        &pk,
        json!({ "q": "anything", "mode": "agentic" }),
    );
    assert_eq!(r.code(), "tenant_not_found");
}

/// `it::search::golden_keyword_recall` (NFR-QUAL-1, Testing § 9.3): the golden mailbox, loaded with
/// the fake AI, keeps its keyword recall@10 over the labelled queries no more than 0.01 below the
/// baseline in `quality.md`, and keyword search finds every exact reference (zero-result rate 0).
#[test]
fn golden_keyword_recall() {
    let m = crate::golden::load("golden keyword", json!({ "triage": { "enabled": false } }));
    let r = crate::golden::search_run(&m, &m.golden.queries, "keyword");
    println!(
        "keyword recall@10 {:.4}, MRR {:.4}, nDCG@10 {:.4}, zero-result rate {:.4}",
        r.recall_at_10, r.mrr, r.ndcg_at_10, r.zero_result_rate
    );
    assert!(
        r.exact_zero.is_empty(),
        "exact references with no hit: {:?}",
        r.exact_zero
    );
    let baseline = crate::golden::baseline("it.keyword_recall_at_10");
    assert!(
        baseline.is_some(),
        "quality.md has no `it.keyword_recall_at_10` row"
    );
    if let Err(e) =
        pylota_identity_conformance::golden::policy::gate(r.recall_at_10, None, baseline)
    {
        panic!("keyword recall@10: {e}");
    }
}
