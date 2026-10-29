//! The HTTP side of the SDK: the builder and client, one request (`Req`) and its answer, idempotency
//! keys, the retry policy and the pager of list operations ([Rust workspace § 11]).
//!
//! [Rust workspace § 11]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

use std::marker::PhantomData;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The client of one deployment, holding one API key.
#[derive(Clone, Debug)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    /// `None` for a client of the routes that need no key (`/health`, `/v1/plans`).
    key: Option<String>,
    user_agent: String,
    retry: Option<RetryPolicy>,
}

#[derive(Clone, Debug, Default)]
pub struct ClientBuilder {
    base_url: Option<String>,
    api_key: Option<String>,
    user_agent: Option<String>,
    retry: Option<RetryPolicy>,
    http: Option<reqwest::Client>,
}

impl ClientBuilder {
    /// Uses this `reqwest` client (its own TLS roots, proxy or resolver) instead of the default one.
    /// The SDK's per-request timeouts still apply.
    pub fn http_client(mut self, http: reqwest::Client) -> Self {
        self.http = Some(http);
        self
    }

    /// The deployment root, for example `https://mail.example.com`.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// The API key (`pmk_live_…` or `pmk_test_…`).
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Replaces the `User-Agent` (`pylota-identity-rs/{version}` by default).
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Turns on retries (off by default).
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = Some(policy);
        self
    }

    pub fn build(self) -> Result<Client, Error> {
        let base = self
            .base_url
            .ok_or_else(|| Error::Config("base_url is required".into()))?;
        let base = base.trim_end_matches('/').to_owned();
        let local = ["http://localhost", "http://127.0.0.1"].iter().any(|p| {
            base == *p || base.starts_with(&format!("{p}:")) || base.starts_with(&format!("{p}/"))
        });
        if !base.starts_with("https://") && !local {
            return Err(Error::Config(
                "base_url must be https:// (http:// only for localhost)".into(),
            ));
        }
        let key = self.api_key.filter(|k| !k.is_empty());
        let http = match self.http {
            Some(h) => h,
            None => reqwest::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| Error::Config(e.to_string()))?,
        };
        Ok(Client {
            http,
            base,
            key,
            user_agent: self
                .user_agent
                .unwrap_or_else(|| format!("pylota-identity-rs/{}", env!("CARGO_PKG_VERSION"))),
            retry: self.retry,
        })
    }
}

/// An `Idempotency-Key`: 1–255 printable ASCII characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(key: &str) -> Result<Self, Error> {
        if key.is_empty() || key.len() > 255 || !key.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            return Err(Error::Config(
                "an idempotency key is 1–255 printable ASCII characters".into(),
            ));
        }
        Ok(Self(key.to_owned()))
    }

    /// A fresh key, `sdk-` and a ULID-shaped value (time, then randomness from the process's hasher
    /// seeds).
    pub fn generate() -> Self {
        use std::hash::{BuildHasher as _, Hasher as _};
        const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(ms);
        let r1 = h.finish();
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(r1);
        let r2 = h.finish();
        let v: u128 = (ms << 80) | (u128::from(r1 & 0xFFFF) << 64) | u128::from(r2);
        let s: String = (0..26)
            .rev()
            .map(|i| ALPHABET[((v >> (i * 5)) & 31) as usize] as char)
            .collect();
        Self(format!("sdk-{s}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Retries of retryable errors ([Errors › How a client should retry]): at most `max_retries`, with
/// back-off 0.5 s, 1 s, 2 s plus up to 250 ms of jitter, `Retry-After` honoured up to 60 s, always with
/// the same idempotency key.
///
/// [Errors › How a client should retry]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/errors.md
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_retries: u32,
}

impl RetryPolicy {
    pub fn standard() -> Self {
        Self { max_retries: 3 }
    }

    fn delay(&self, attempt: u32, retry_after: Option<u64>) -> Duration {
        if let Some(s) = retry_after {
            return Duration::from_secs(s.min(60));
        }
        let base = 500u64 << attempt.min(2);
        let jitter = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::from(d.subsec_nanos()) % 251);
        Duration::from_millis(base + jitter)
    }
}

/// The body of a request.
#[derive(Clone, Debug, Default)]
pub enum Body {
    #[default]
    None,
    Json(Vec<u8>),
    Bytes(Vec<u8>),
}

impl Body {
    pub fn json<T: Serialize + ?Sized>(t: &T) -> Result<Self, Error> {
        serde_json::to_vec(t)
            .map(Body::Json)
            .map_err(|e| Error::Decode(e.to_string()))
    }
}

/// One request, as a generated method builds it.
#[derive(Clone, Debug)]
pub struct Req {
    method: &'static str,
    path: String,
    auth: bool,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    pub body: Body,
    idempotency_key: Option<String>,
}

/// A query or header value: strings as they are, numbers and booleans as JSON writes them, arrays as
/// one entry per item.
fn values<T: Serialize + ?Sized>(v: &T) -> Vec<String> {
    match serde_json::to_value(v).unwrap_or(Value::Null) {
        Value::Null => Vec::new(),
        Value::String(s) => vec![s],
        Value::Array(a) => a
            .iter()
            .flat_map(|x| match x {
                Value::String(s) => vec![s.clone()],
                Value::Null => Vec::new(),
                other => vec![other.to_string()],
            })
            .collect(),
        other => vec![other.to_string()],
    }
}

impl Req {
    pub fn new(method: &'static str, path: String, auth: bool) -> Self {
        Self {
            method,
            path,
            auth,
            query: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            idempotency_key: None,
        }
    }

    pub fn query<T: Serialize + ?Sized>(&mut self, name: &str, v: &T) {
        for s in values(v) {
            self.query.push((name.to_owned(), s));
        }
    }

    pub fn header<T: Serialize + ?Sized>(&mut self, name: &str, v: &T) {
        if let Some(s) = values(v).into_iter().next() {
            self.headers.push((name.to_owned(), s));
        }
    }

    /// The `Idempotency-Key` of the call: the caller's, else a generated one when `generate`.
    pub fn idempotency(&mut self, key: Option<&IdempotencyKey>, generate: bool) {
        self.idempotency_key = match key {
            Some(k) => Some(k.0.clone()),
            None if generate => Some(IdempotencyKey::generate().0),
            None => None,
        };
    }

    fn url(&self, base: &str) -> String {
        let mut url = format!("{base}{}", self.path);
        for (i, (k, v)) in self.query.iter().enumerate() {
            url.push(if i == 0 { '?' } else { '&' });
            url.push_str(&encode(k));
            url.push('=');
            url.push_str(&encode(v));
        }
        url
    }
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A decode error with the text before where it happened, so a mismatch is easy to place.
fn decode_message(e: &serde_json::Error, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let line: Vec<char> = text
        .lines()
        .nth(e.line().saturating_sub(1))
        .unwrap_or("")
        .chars()
        .collect();
    let at = e.column().min(line.len());
    let near: String = line[at.saturating_sub(80)..at].iter().collect();
    format!("{e} (after …{near})")
}

/// One path segment, encoded.
pub fn segment(s: &str) -> String {
    encode(s)
}

/// A successful answer, before decoding.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// An answer that is not JSON (raw MIME, attachment bytes, a text file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raw {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json<T: DeserializeOwned>(self) -> Result<T, Error> {
        let bytes: &[u8] = if self.body.is_empty() {
            b"null"
        } else {
            &self.body
        };
        serde_json::from_slice(bytes).map_err(|e| Error::Decode(decode_message(&e, bytes)))
    }

    pub fn raw(self) -> Result<Raw, Error> {
        let content_type = self
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.clone());
        Ok(Raw {
            status: self.status,
            content_type,
            body: self.body,
        })
    }

    pub fn unit(self) -> Result<(), Error> {
        Ok(())
    }
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The deployment root this client calls.
    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// Sends one request, with the client's retries.
    pub async fn send(&self, req: Req) -> Result<Response, Error> {
        let mut attempt = 0;
        loop {
            match self.once(&req).await {
                Ok(r) => return Ok(r),
                Err((e, retry_after)) => {
                    let retryable = retryable(&req, &e);
                    match self.retry {
                        Some(p) if retryable && attempt < p.max_retries => {
                            tokio::time::sleep(p.delay(attempt, retry_after)).await;
                            attempt += 1;
                        }
                        _ => return Err(e),
                    }
                }
            }
        }
    }

    async fn once(&self, req: &Req) -> Result<Response, (Error, Option<u64>)> {
        let method = reqwest::Method::from_bytes(req.method.as_bytes())
            .map_err(|e| (Error::Config(e.to_string()), None))?;
        // `GET …/wait` holds the request up to its `timeout`; the client waits 15 s more.
        let timeout = req
            .query
            .iter()
            .find(|(k, _)| k == "timeout")
            .and_then(|(_, v)| v.parse::<u64>().ok())
            .map_or(REQUEST_TIMEOUT, |s| {
                REQUEST_TIMEOUT.max(Duration::from_secs(s + 15))
            });
        let mut r = self
            .http
            .request(method, req.url(&self.base))
            .timeout(timeout)
            .header("user-agent", &self.user_agent);
        if req.auth {
            let Some(key) = &self.key else {
                return Err((Error::Config("this call needs an API key".into()), None));
            };
            r = r.header("authorization", format!("Bearer {key}"));
        }
        for (k, v) in &req.headers {
            r = r.header(k.as_str(), v.as_str());
        }
        if let Some(k) = &req.idempotency_key {
            r = r.header("idempotency-key", k.as_str());
        }
        r = match &req.body {
            Body::None => r,
            Body::Json(b) => r.header("content-type", "application/json").body(b.clone()),
            Body::Bytes(b) => r
                .header("content-type", "application/octet-stream")
                .body(b.clone()),
        };
        let transport = |e: reqwest::Error| Error::Transport {
            message: e.to_string(),
            idempotency_key: req.idempotency_key.clone(),
        };
        let resp = r.send().await.map_err(|e| (transport(e), None))?;
        let status = resp.status().as_u16();
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_str().unwrap_or("").to_owned()))
            .collect();
        let body = resp
            .bytes()
            .await
            .map_err(|e| (transport(e), None))?
            .to_vec();
        if (200..300).contains(&status) {
            return Ok(Response {
                status,
                headers,
                body,
            });
        }
        let retry_after = headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("retry-after"))
            .and_then(|(_, v)| v.parse::<u64>().ok());
        Err((Error::from_response(status, &body), retry_after))
    }
}

/// Whether a failed call may be sent again ([Errors › How a client should retry]): an envelope that
/// says so; a network error or a proxy `502`, `503` or `504` only for a `GET` or a request with an
/// idempotency key, so a keyless `PATCH`, `DELETE` or `PUT` is sent once.
///
/// [Errors › How a client should retry]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/errors.md
fn retryable(req: &Req, e: &Error) -> bool {
    let safe = req.method == "GET" || req.idempotency_key.is_some();
    match e {
        Error::Api(a) => a.retryable,
        Error::Transport { .. } => safe,
        Error::Status { status, .. } => safe && matches!(status, 502..=504),
        _ => false,
    }
}

/// The pages of a list operation, following `next_cursor`; the last item is the error that ended it,
/// such as `410 cursor_expired`.
pub struct Pages<'a, T> {
    client: &'a Client,
    req: Req,
    cursor: Option<String>,
    done: bool,
    _t: PhantomData<T>,
}

impl<'a, T: DeserializeOwned> Pages<'a, T> {
    pub fn new(client: &'a Client, req: Req) -> Self {
        Self {
            client,
            req,
            cursor: None,
            done: false,
            _t: PhantomData,
        }
    }

    /// The next page, or `None` after the last one.
    pub async fn next(&mut self) -> Option<Result<T, Error>> {
        if self.done {
            return None;
        }
        let mut req = self.req.clone();
        if let Some(c) = &self.cursor {
            req.query.retain(|(k, _)| k != "cursor");
            req.query.push(("cursor".into(), c.clone()));
        }
        let page = match self
            .client
            .send(req)
            .await
            .and_then(Response::json::<Value>)
        {
            Ok(p) => p,
            Err(e) => {
                self.done = true;
                return Some(Err(e));
            }
        };
        match page["next_cursor"].as_str() {
            Some(c) => self.cursor = Some(c.to_owned()),
            None => self.done = true,
        }
        Some(serde_json::from_value(page).map_err(|e| Error::Decode(e.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_keys() {
        assert!(IdempotencyKey::new("bk-2291-confirm").is_ok());
        assert!(IdempotencyKey::new("").is_err());
        assert!(IdempotencyKey::new("é").is_err());
        assert!(IdempotencyKey::new(&"x".repeat(256)).is_err());
        let a = IdempotencyKey::generate();
        let b = IdempotencyKey::generate();
        assert!(a.as_str().starts_with("sdk-") && a.as_str().len() == 30);
        assert_ne!(a, b);
    }

    #[test]
    fn builder_rules() {
        let ok = |u: &str| {
            Client::builder()
                .base_url(u)
                .api_key("pmk_test_x")
                .build()
                .is_ok()
        };
        assert!(ok("https://mail.example.com"));
        assert!(ok("http://localhost:8787") && ok("http://127.0.0.1"));
        assert!(!ok("http://mail.example.com") && !ok("http://localhost.example.com"));
        assert!(Client::builder().api_key("pmk_test_x").build().is_err());
    }

    /// A client without a key reaches the routes that need none, and refuses the others before sending.
    #[tokio::test]
    async fn keyless_client() {
        let c = Client::builder()
            .base_url("http://127.0.0.1:9")
            .build()
            .unwrap();
        let e = c
            .send(Req::new("GET", "/v1/me".into(), true))
            .await
            .unwrap_err();
        assert!(matches!(e, Error::Config(_)), "{e:?}");
    }

    #[test]
    fn urls() {
        let mut r = Req::new(
            "GET",
            format!("/v1/identities/{}/threads", segment("idn_1")),
            true,
        );
        r.query("labels", &vec!["a b", "c"]);
        r.query("limit", &10);
        r.query("is_unread", &true);
        assert_eq!(
            r.url("https://h"),
            "https://h/v1/identities/idn_1/threads?labels=a%20b&labels=c&limit=10&is_unread=true"
        );
        assert_eq!(segment("jo@example.net"), "jo%40example.net");
    }

    /// A network error or a proxy `503` is retried for a `GET` or a keyed request, never for a keyless
    /// `PATCH` or `DELETE`; an envelope's `retryable` decides for every method.
    #[test]
    fn retry_rules() {
        let transport = Error::Transport {
            message: "reset".into(),
            idempotency_key: None,
        };
        let proxy = Error::Status {
            status: 503,
            body: String::new(),
        };
        let get = Req::new("GET", "/v1/me".into(), true);
        let patch = Req::new("PATCH", "/v1/identities/idn_1".into(), true);
        let mut post = Req::new("POST", "/v1/identities/idn_1/messages".into(), true);
        post.idempotency(None, true);
        for r in [&get, &post] {
            assert!(retryable(r, &transport) && retryable(r, &proxy));
        }
        assert!(!retryable(&patch, &transport) && !retryable(&patch, &proxy));
        let limited = Error::from_response(
            429,
            br#"{"error":{"code":"rate_limited","message":"x","retryable":true}}"#,
        );
        assert!(retryable(&patch, &limited));
        assert!(!retryable(
            &get,
            &Error::Status {
                status: 404,
                body: String::new()
            }
        ));
    }

    #[test]
    fn backoff() {
        let p = RetryPolicy::standard();
        assert!(
            p.delay(0, None) >= Duration::from_millis(500)
                && p.delay(0, None) <= Duration::from_millis(750)
        );
        assert!(p.delay(2, None) >= Duration::from_millis(2000));
        assert_eq!(p.delay(0, Some(120)), Duration::from_secs(60));
    }
}
