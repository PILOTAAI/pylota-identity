//! Outbound HTTP: the runtime's `fetch` with a deadline (an `AbortController` raced against a timer)
//! and a cap on the body read, after which the stream is cancelled. Every outbound URL a tenant can
//! influence passes the SSRF rules before it gets here (Security § 9).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redirect {
    Manual,
    Follow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: u32,
    pub redirect: Redirect,
    pub max_body_bytes: usize,
}

impl HttpRequest {
    /// A `GET` with the defaults: 10 seconds, no redirects, 1 MiB.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET",
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout_ms: 10_000,
            redirect: Redirect::Manual,
            max_body_bytes: 1 << 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The body was longer than `max_body_bytes` and was cut there.
    pub body_truncated: bool,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    Timeout,
    Dns,
    Tls,
    Connect,
    Other(String),
}

pub trait HttpClient {
    async fn send(&self, req: HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// Appends `chunk` to `body` up to `max` bytes; `true` once the cap is reached and the rest dropped.
pub fn take_capped(body: &mut Vec<u8>, chunk: &[u8], max: usize) -> bool {
    let room = max.saturating_sub(body.len());
    if chunk.len() > room {
        body.extend_from_slice(&chunk[..room]);
        true
    } else {
        body.extend_from_slice(chunk);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::{FakeHttp, ScriptedResponse, block_on};

    /// A slow server times out at the deadline; bodies are cut at `max_body_bytes`.
    #[test]
    fn timeout_and_body_cap() {
        let http = FakeHttp::new();
        http.script(
            "https://slow.example/",
            ScriptedResponse::ok(b"late".to_vec()).after_ms(3_001),
        );
        http.script(
            "https://big.example/",
            ScriptedResponse::ok(vec![b'x'; 10_000]).chunked(4_096),
        );
        let mut slow = HttpRequest::get("https://slow.example/");
        slow.timeout_ms = 3_000;
        assert_eq!(block_on(http.send(slow.clone())), Err(HttpError::Timeout));
        slow.timeout_ms = 3_001;
        assert_eq!(block_on(http.send(slow)).unwrap().body, b"late");

        let mut big = HttpRequest::get("https://big.example/");
        big.max_body_bytes = 5_000;
        let r = block_on(http.send(big.clone())).unwrap();
        assert_eq!((r.body.len(), r.body_truncated), (5_000, true));
        big.max_body_bytes = 10_000;
        let r = block_on(http.send(big)).unwrap();
        assert_eq!((r.body.len(), r.body_truncated), (10_000, false));
        assert_eq!(http.requests().len(), 4);
    }

    #[test]
    fn capping() {
        let mut body = Vec::new();
        assert!(!take_capped(&mut body, b"abc", 5));
        assert!(take_capped(&mut body, b"def", 5));
        assert_eq!(body, b"abcde");
        assert!(take_capped(&mut body, b"g", 5));
    }
}
