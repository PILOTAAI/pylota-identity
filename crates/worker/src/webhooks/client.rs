//! One delivery attempt ([Webhooks › Delivering an attempt], steps 4 and 5's outcome): the endpoint's
//! secrets unsealed (cached per isolate for 60 seconds), the body signed with both secrets during a
//! rotation overlap, and a guarded `POST`.
//!
//! [Webhooks › Delivering an attempt]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use std::cell::RefCell;
use std::collections::HashMap;

use pylota_identity_core::crypto;
use pylota_identity_platform::http::HttpError;
use pylota_identity_platform::{Clock as _, Platform};
use serde::Deserialize;

use super::sign;
use crate::net::{self, GuardError};

/// The endpoint columns an attempt needs.
#[derive(Clone, Debug, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub url: String,
    pub secret_enc: String,
    pub prev_secret_enc: Option<String>,
    pub prev_secret_expires_at: Option<i64>,
    pub headers_sealed: Option<String>,
}

/// How an attempt ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub succeeded: bool,
    pub http_status: Option<u16>,
    /// The machine code of a failure.
    pub error: Option<&'static str>,
    pub duration_ms: i64,
}

impl Outcome {
    fn failed(error: &'static str, http_status: Option<u16>, duration_ms: i64) -> Self {
        Self {
            succeeded: false,
            http_status,
            error: Some(error),
            duration_ms,
        }
    }
}

const CACHE_MS: i64 = 60_000;

/// (endpoint, column, envelope) → (plaintext, expiry).
type SecretCache = HashMap<(String, &'static str, String), (Vec<u8>, i64)>;

thread_local! {
    static SECRETS: RefCell<SecretCache> = RefCell::new(HashMap::new());
}

/// The associated data of a sealed endpoint column.
pub fn aad(column: &str, endpoint_id: &str) -> String {
    crypto::aad("webhook_endpoints", column, endpoint_id)
}

/// Seals `plain` into `column` of endpoint `id` under the active master key.
pub fn seal<P: Platform>(p: &P, column: &str, id: &str, plain: &[u8]) -> String {
    let nonce: [u8; 12] = pylota_identity_platform::Rng::bytes(p.rng());
    crypto::seal(
        p.secrets().active_master_key(),
        nonce,
        &aad(column, id),
        plain,
    )
}

/// Opens `column` of endpoint `id` with whichever master-key slot matches, cached for 60 seconds.
pub fn open<P: Platform>(p: &P, column: &'static str, id: &str, envelope: &str) -> Option<Vec<u8>> {
    let now = p.clock().now_ms();
    let key = (id.to_owned(), column, envelope.to_owned());
    if let Some(v) = SECRETS.with(|s| {
        s.borrow()
            .get(&key)
            .filter(|(_, exp)| *exp > now)
            .map(|(v, _)| v.clone())
    }) {
        return Some(v);
    }
    let s = p.secrets();
    let slots: Vec<&[u8; 32]> = [s.master_key_a.as_ref(), s.master_key_b.as_ref()]
        .into_iter()
        .flatten()
        .collect();
    let plain = crypto::open(&slots, &aad(column, id), envelope).ok()?;
    SECRETS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 1000 {
            c.clear();
        }
        c.insert(key, (plain.clone(), now + CACHE_MS));
    });
    Some(plain)
}

/// The custom delivery headers of an endpoint (`headers_sealed`, a JSON map).
fn custom_headers<P: Platform>(p: &P, e: &Endpoint) -> Option<Vec<(String, String)>> {
    let Some(env) = &e.headers_sealed else {
        return Some(Vec::new());
    };
    let plain = open(p, "headers_sealed", &e.id, env)?;
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(&plain).ok()?;
    Some(
        map.into_iter()
            .filter_map(|(k, v)| v.as_str().map(|v| (k, v.to_owned())))
            .collect(),
    )
}

/// One signed `POST` of `body` (the stored envelope bytes) for `event_id`.
pub async fn attempt<P: Platform>(p: &P, e: &Endpoint, event_id: &str, body: &[u8]) -> Outcome {
    let start = p.clock().now_ms();
    let elapsed = |p: &P| (p.clock().now_ms() - start).max(0);
    let Some(current) = open(p, "secret_enc", &e.id, &e.secret_enc) else {
        crate::log::error(
            "webhook_secret_unavailable",
            &[("endpoint", e.id.clone().into())],
        );
        return Outcome::failed("secret_unavailable", None, 0);
    };
    let previous = match (&e.prev_secret_enc, e.prev_secret_expires_at) {
        (Some(env), Some(until)) if until > start => open(p, "prev_secret_enc", &e.id, env),
        _ => None,
    };
    let Some(custom) = custom_headers(p, e) else {
        crate::log::error(
            "webhook_secret_unavailable",
            &[("endpoint", e.id.clone().into())],
        );
        return Outcome::failed("secret_unavailable", None, 0);
    };
    let ts = start / 1000;
    let content = sign::signed_content(event_id, ts, body);
    let signature = sign::signature_header(&current, previous.as_deref(), &content);
    let headers = sign::headers(&custom, event_id, ts, &signature, crate::BUILD_VERSION);
    match net::post(p, &e.url, headers, body.to_vec()).await {
        Ok(r) => {
            let ms = elapsed(p);
            match r.status {
                200..=299 => Outcome {
                    succeeded: true,
                    http_status: Some(r.status),
                    error: None,
                    duration_ms: ms,
                },
                300..=399 => Outcome::failed("redirect", Some(r.status), ms),
                410 => Outcome::failed("status_410", Some(r.status), ms),
                400..=499 => Outcome::failed("status_4xx", Some(r.status), ms),
                _ => Outcome::failed("status_5xx", Some(r.status), ms),
            }
        }
        Err(e) => {
            let ms = elapsed(p);
            let code = match e {
                GuardError::InvalidUrl(_) => "invalid_url",
                GuardError::Blocked(_) => {
                    crate::metrics::count(p, "webhook_ssrf_blocked_total", None, 1);
                    "ssrf_blocked"
                }
                GuardError::Dns | GuardError::Http(HttpError::Dns) => "dns",
                GuardError::Http(HttpError::Timeout) => "timeout",
                GuardError::Http(HttpError::Tls) => "tls",
                GuardError::Http(HttpError::Connect | HttpError::Other(_)) => "connect",
            };
            Outcome::failed(code, None, ms)
        }
    }
}
