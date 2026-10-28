//! The SES control plane through workerd (Domains on any DNS host § 4.8): the deployment's one token
//! bucket, the request-path deadline, throttling, and the daily identity checks at their offsets.

use std::time::Duration;

use serde_json::{Value, json};

use crate::domains::healthy;
use crate::{cloud, fakes, hook, patch, platform};

fn acquire() -> Value {
    let r = hook(
        "POST",
        "/__test/ses-acquire",
        Some(json!({ "caller": "domain_check" })),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["ok"].clone()
}

fn reset_bucket() {
    let r = hook("POST", "/__test/ses-reset", Some(json!({})));
    assert_eq!(r.status, 200, "{}", r.body);
}

/// The domain's monitor runs once; answers its next daily SES check.
fn ses_check_at(domain_id: &str) -> i64 {
    let r = hook(
        "POST",
        "/__test/domain-alarm",
        Some(json!({ "domain_id": domain_id })),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["ses_check_at"]
        .as_i64()
        .unwrap_or_else(|| panic!("{}", r.body))
}

/// `u64::from_be_bytes(SHA-256(domain_id)[0..8]) % 86_400`, in milliseconds.
fn offset_ms(domain_id: &str) -> i64 {
    let h = pylota_identity_core::ses::sha256_hex(domain_id.as_bytes());
    (u64::from_str_radix(&h[..16], 16).unwrap() % 86_400) as i64 * 1000
}

/// Twenty concurrent `Acquire` calls are granted one second apart; a request-path caller past its
/// 5-second deadline gets `429 upstream_rate_limited`; the daily check falls at the domain's hash
/// offset; a `ThrottlingException` re-acquires after 2 s.
#[test]
fn control_plane_rate() {
    let _reset = fakes::Reset;
    let (_, _, _, _, id) = healthy("ses rate");
    let path = format!("/v1/domains/{id}");

    // Twenty at once: twenty distinct seconds.
    reset_bucket();
    let threads: Vec<_> = (0..20).map(|_| std::thread::spawn(acquire)).collect();
    let mut at: Vec<i64> = threads
        .into_iter()
        .map(|t| t.join().unwrap()["at_ms"].as_i64().expect("granted"))
        .collect();
    at.sort_unstable();
    for w in at.windows(2) {
        assert!(w[1] - w[0] >= 1000, "{at:?}");
    }
    // The queue is now ~20 s ahead: a request-path caller is refused, nothing taken.
    let r = patch(&path, &platform(), json!({ "transport": "ses" }));
    assert_eq!(
        (r.status, r.code()),
        (429, "upstream_rate_limited"),
        "{}",
        r.body
    );
    let retry: i64 = r.header("retry-after").unwrap().parse().unwrap();
    assert!((10..=21).contains(&retry), "{retry}");
    assert_eq!(r.details()["retry_after"], retry);
    reset_bucket();

    // Throttled: the call is made again after 2 seconds.
    let before = cloud::ses_times().len();
    cloud::ses_throttle(1);
    let r = patch(&path, &platform(), json!({ "transport": "ses" }));
    assert_eq!(
        (r.status, r.details()["reason"].as_str()),
        (422, Some("ses_identity_not_verified")),
        "{}",
        r.body
    );
    let times = cloud::ses_times();
    assert_eq!(times.len() - before, 2);
    assert!(times[before + 1] - times[before] >= Duration::from_secs(2));

    // The daily identity check: at the hash offset, then a day later.
    let first = ses_check_at(&id);
    assert_eq!(first.rem_euclid(86_400_000), offset_ms(&id));
    let calls = cloud::ses_times().len();
    fakes::advance(86_400_000 + 60_000);
    let next = ses_check_at(&id);
    assert_eq!(next, first + 86_400_000);
    assert_eq!(cloud::ses_times().len(), calls + 1, "one GetEmailIdentity");
}
