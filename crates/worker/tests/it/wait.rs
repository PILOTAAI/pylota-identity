//! `wait` (E4) and its registration (E5) in workerd ([Inbound › The `wait` handler]).
//!
//! [Inbound › The `wait` handler]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{dkim, fakes, get, inject, mail, mailbox_fixture, wait_stored};

fn otp(from: &str, to: &str, code: &str) -> String {
    mail(
        from,
        to,
        "Your verification code",
        &format!("Your verification code is {code}. It expires in 10 minutes."),
    )
}

/// Starts `GET …/wait?{query}` on another thread.
fn wait_async(idn: &str, key: &str, query: &str) -> std::thread::JoinHandle<(Value, Instant)> {
    let (idn, key, query) = (idn.to_owned(), key.to_owned(), query.to_owned());
    std::thread::spawn(move || {
        let r = get(&format!("/v1/identities/{idn}/wait?{query}"), &key);
        assert_eq!(r.status, 200, "{}", r.body);
        (r.body, Instant::now())
    })
}

#[test]
fn e4_code_released_on_pass() {
    let (_, idn, addr, key) = mailbox_fixture("e4pass");
    let w = wait_async(
        &idn,
        &key,
        "from=@brightwell.example&kind=verification&timeout=30",
    );
    std::thread::sleep(Duration::from_secs(2));
    let raw = dkim::sign(&otp(
        "Brightwell <security@brightwell.example>",
        &addr,
        "481 207",
    ));
    assert_eq!(inject("bounce@brightwell.example", &addr, &raw), 200);
    let (body, _) = w.join().unwrap();
    assert_eq!(body["timed_out"], false, "{body}");
    assert_eq!(body["message"]["trust"]["verdict"], "pass", "{body}");
    assert_eq!(body["verification"]["code"], "481 207", "{body}");
    assert_eq!(body["verification"]["sender_domain"], "brightwell.example");
}

#[test]
fn e4_code_withheld_unauthenticated() {
    let (_, idn, addr, key) = mailbox_fixture("e4unauth");
    // Unsigned mail claiming the expected domain: returned, the code withheld.
    let w = wait_async(&idn, &key, "from=@brightwell.example&kind=any&timeout=30");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        inject(
            "x@brightwell.example",
            &addr,
            &otp("security@brightwell.example", &addr, "222 333")
        ),
        200
    );
    let (body, _) = w.join().unwrap();
    assert_eq!(body["timed_out"], false, "{body}");
    // Unsigned mail claiming brightwell.example is not authenticated and is held; this key's
    // `quarantine:review` lets `wait` return it.
    assert_ne!(body["message"]["trust"]["verdict"], "pass", "{body}");
    assert_eq!(body["message"]["status"], "quarantined");
    assert_eq!(body["verification"], Value::Null);
    // Signed mail, but the wait names no expected sender: nothing to release it for.
    let raw = dkim::sign(&otp("security@brightwell.example", &addr, "444 555"));
    assert_eq!(inject("bounce@brightwell.example", &addr, &raw), 200);
    wait_stored(&idn, &key, "received", 1);
    let r = get(
        &format!(
            "/v1/identities/{idn}/wait?subject_contains=verification&since=2000-01-01T00:00:00Z&timeout=5"
        ),
        &key,
    );
    assert_eq!(r.body["timed_out"], false);
    assert_eq!(r.body["verification"], Value::Null);
}

#[test]
fn e4_timeout() {
    let (_, idn, _, key) = mailbox_fixture("e4timeout");
    let start = Instant::now();
    let r = get(
        &format!("/v1/identities/{idn}/wait?from=@nobody.example&timeout=2"),
        &key,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        (r.body["timed_out"].as_bool(), r.body["message"].is_null()),
        (Some(true), true)
    );
    assert!(
        start.elapsed() >= Duration::from_millis(1500),
        "{:?} {}",
        start.elapsed(),
        r.body
    );
    assert_eq!(
        get(&format!("/v1/identities/{idn}/wait?timeout=61"), &key).status,
        400
    );
}

#[test]
fn e4_registration_refresh() {
    let _reset = fakes::Reset;
    let (_, idn, addr, key) = mailbox_fixture("e4refresh");
    // A 12-second wait re-registers at 10 s; the registration counts for 30 minutes after it ends.
    let r = get(
        &format!("/v1/identities/{idn}/wait?from=@refresh.example&timeout=12"),
        &key,
    );
    assert_eq!(r.body["timed_out"], true);
    fakes::advance(20 * 60_000);
    assert_eq!(
        inject(
            "a@refresh.example",
            &addr,
            &otp("noreply@refresh.example", &addr, "123 456")
        ),
        200
    );
    let m = wait_stored(&idn, &key, "received", 1);
    assert_eq!(m[0]["subject"], "Your verification code");
    // An hour later the same mail is unsolicited again (E5).
    fakes::advance(60 * 60_000);
    assert_eq!(
        inject(
            "a@refresh.example",
            &addr,
            &otp("noreply@refresh.example", &addr, "654 321")
        ),
        200
    );
    wait_stored(&idn, &key, "quarantined", 1);
}

#[test]
fn e4_filters_and_since() {
    let (_, idn, addr, key) = mailbox_fixture("e4filters");
    assert_eq!(
        get(
            &format!("/v1/identities/{idn}/wait?kind=verification&timeout=1"),
            &key
        )
        .status,
        400
    );
    assert_eq!(
        inject(
            "a@f.example",
            &addr,
            &mail("a@f.example", &addr, "Invoice 77", "invoice")
        ),
        200
    );
    assert_eq!(
        inject(
            "b@f.example",
            &addr,
            &mail("b@f.example", &addr, "Hello there", "hi")
        ),
        200
    );
    let stored = wait_stored(&idn, &key, "received", 2);
    let since = "since=2000-01-01T00:00:00Z";
    let r = get(
        &format!("/v1/identities/{idn}/wait?{since}&subject_contains=INVOICE&timeout=3"),
        &key,
    );
    assert_eq!(r.body["message"]["subject"], "Invoice 77");
    let r = get(
        &format!("/v1/identities/{idn}/wait?{since}&from=b@f.example&timeout=3"),
        &key,
    );
    assert_eq!(r.body["message"]["subject"], "Hello there");
    let thr = stored
        .iter()
        .find(|m| m["subject"] == "Hello there")
        .unwrap()["thread_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = get(
        &format!("/v1/identities/{idn}/wait?{since}&thread_id={thr}&timeout=3"),
        &key,
    );
    assert_eq!(r.body["message"]["subject"], "Hello there");
    // since = now: neither old message matches.
    let r = get(&format!("/v1/identities/{idn}/wait?timeout=2"), &key);
    assert_eq!(r.body["timed_out"], true);
    // kind=reply: neither message joined a thread holding outbound mail.
    let r = get(
        &format!("/v1/identities/{idn}/wait?{since}&kind=reply&timeout=2"),
        &key,
    );
    assert_eq!(r.body["timed_out"], true);
}

#[test]
fn e4_retry_after_release() {
    let _reset = fakes::Reset;
    let (_, idn, addr, key) = mailbox_fixture("e4retry");
    let q = "from=@brightwell.example&kind=verification&since=2000-01-01T00:00:00Z&timeout=20";
    let w = wait_async(&idn, &key, q);
    std::thread::sleep(Duration::from_secs(2));
    let raw = dkim::sign(&otp("security@brightwell.example", &addr, "777 888"));
    assert_eq!(inject("bounce@brightwell.example", &addr, &raw), 200);
    let (first, _) = w.join().unwrap();
    assert_eq!(first["verification"]["code"], "777 888", "{first}");
    // A client retrying within the hour gets the same value.
    let again = get(&format!("/v1/identities/{idn}/wait?{q}"), &key);
    assert_eq!(again.body["verification"]["code"], "777 888");
    // More than an hour after the first release, none.
    fakes::advance(61 * 60_000);
    let late = get(&format!("/v1/identities/{idn}/wait?{q}"), &key);
    assert_eq!(late.body["verification"], Value::Null, "{}", late.body);
}
