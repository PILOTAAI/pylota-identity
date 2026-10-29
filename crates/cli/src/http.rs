//! API calls through the SDK ([CLI and setup §4]): the CLI's user agent, idempotency keys, retries and
//! `--all` paging. Bodies come back unchanged, so `--json` prints exactly what the API sent.
//!
//! [CLI and setup §4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#4-http-behaviour-against-the-api

use std::time::{Duration, Instant};

use pylota_identity::{Body, Client, IdempotencyKey, Req, Response};
use serde_json::{Value, json};

use crate::{CliError, Ctx, Mode, VERSION};

pub fn user_agent() -> String {
    format!("idvy/{VERSION} (+https://github.com/PILOTAAI/pylota-identity)")
}

/// The default HTTP client: connect timeout 10 s, `HTTPS_PROXY` honoured, at most 5 redirects.
pub fn default_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())
}

/// How a call carries an `Idempotency-Key`.
#[derive(Clone, Debug)]
pub enum Idem {
    /// No key (`GET`, `PATCH`, `DELETE`, and the endpoints that store no result).
    None,
    /// A key generated for this invocation (every other `POST`).
    Generated,
    /// The caller's key, or one generated for a send and printed.
    Key(String),
}

/// A send's key: `--idempotency-key`, else `idvy-<ulid>`, printed to stderr in human mode so the person
/// can rerun with it.
pub fn send_key(ctx: &Ctx, given: Option<&str>) -> Result<Idem, CliError> {
    match given {
        Some(k) => {
            IdempotencyKey::new(k).map_err(|e| CliError::usage(e.to_string()))?;
            Ok(Idem::Key(k.to_owned()))
        }
        None => {
            let k = format!("idvy-{}", ulid(ctx.now_ms()));
            if ctx.mode == Mode::Human {
                ctx.note(&format!("Idempotency-Key: {k}"));
            }
            Ok(Idem::Key(k))
        }
    }
}

/// A ULID from the OS's random source.
pub fn ulid(ms: i64) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut r = [0u8; 10];
    let _ = getrandom::fill(&mut r);
    let mut v: u128 = (ms as u128 & 0xFFFF_FFFF_FFFF) << 80;
    for (i, b) in r.iter().enumerate() {
        v |= u128::from(*b) << (8 * (9 - i));
    }
    (0..26)
        .rev()
        .map(|i| ALPHABET[((v >> (i * 5)) & 31) as usize] as char)
        .collect()
}

/// The API at one URL with one key (or none, for unauthenticated routes).
pub struct Api {
    client: Client,
    url: String,
    verbose: bool,
}

/// One path segment, encoded.
pub fn seg(s: &str) -> String {
    pylota_identity::segment(s)
}

impl Api {
    pub fn new(ctx: &Ctx, url: &str, key: Option<&str>) -> Result<Self, CliError> {
        let mut b = Client::builder()
            .base_url(url)
            .user_agent(user_agent())
            .http_client(ctx.http.clone());
        if let Some(k) = key {
            b = b.api_key(k);
        }
        Ok(Self {
            client: b.build().map_err(CliError::from)?,
            url: url.to_owned(),
            verbose: ctx.global.verbose,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Sends one request and returns the raw answer.
    pub async fn send(
        &self,
        ctx: &Ctx,
        method: &'static str,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        idem: Idem,
    ) -> Result<Response, CliError> {
        let mut req = Req::new(method, path.to_owned(), true);
        for (k, v) in query {
            req.query(k, v);
        }
        if let Some(b) = body {
            req.body = Body::json(b)?;
        }
        match idem {
            Idem::None => {}
            Idem::Generated => req.idempotency(None, true),
            Idem::Key(k) => {
                let k = IdempotencyKey::new(&k).map_err(|e| CliError::usage(e.to_string()))?;
                req.idempotency(Some(&k), false);
            }
        }
        let start = Instant::now();
        let r = self.client.send(req).await;
        if self.verbose {
            let status = match &r {
                Ok(resp) => resp.status.to_string(),
                Err(e) => e
                    .status()
                    .map_or_else(|| "error".to_owned(), |s| s.to_string()),
            };
            ctx.note(&format!(
                "{method} {path} {status} {} ms",
                start.elapsed().as_millis()
            ));
        }
        Ok(r?)
    }

    /// One call whose answer is JSON (an empty `204` is `{}`).
    pub async fn call(
        &self,
        ctx: &Ctx,
        method: &'static str,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        idem: Idem,
    ) -> Result<Value, CliError> {
        let r = self.send(ctx, method, path, query, body, idem).await?;
        if r.body.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_slice(&r.body)
            .map_err(|e| CliError::internal(format!("unexpected response from {path}: {e}")))
    }

    pub async fn get(
        &self,
        ctx: &Ctx,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, CliError> {
        self.call(ctx, "GET", path, query, None, Idem::None).await
    }

    /// A list: one page, or with `all` every page into one document with `"next_cursor": null`
    /// (at most `limit` items, default 10,000). `410 cursor_expired` ends it with that error; the
    /// items so far are not printed.
    pub async fn list(
        &self,
        ctx: &Ctx,
        path: &str,
        query: &[(&str, String)],
        all: bool,
    ) -> Result<Value, CliError> {
        let first = self.get(ctx, path, query).await?;
        if !all {
            return Ok(first);
        }
        let cap: usize = query
            .iter()
            .find(|(k, _)| *k == "limit")
            .and_then(|(_, v)| v.parse().ok())
            .map_or(10_000, |l: usize| l.max(10_000));
        let mut doc = first;
        let mut items = doc["data"].as_array().cloned().unwrap_or_default();
        while let Some(c) = doc["next_cursor"].as_str().map(str::to_owned) {
            if items.len() >= cap {
                break;
            }
            let mut q: Vec<(&str, String)> = query
                .iter()
                .filter(|(k, _)| *k != "cursor")
                .cloned()
                .collect();
            q.push(("cursor", c));
            doc = self.get(ctx, path, &q).await?;
            items.extend(doc["data"].as_array().cloned().unwrap_or_default());
        }
        items.truncate(cap);
        let mut out = doc;
        out["data"] = Value::Array(items);
        out["next_cursor"] = Value::Null;
        if out.get("has_more").is_some() {
            out["has_more"] = Value::Bool(false);
        }
        Ok(out)
    }
}

/// An API call without a key (`GET /health`, signed links).
pub async fn get_public(ctx: &Ctx, url: &str, path: &str) -> Result<Response, CliError> {
    let api = Api::new(ctx, url, None)?;
    let mut req = Req::new("GET", path.to_owned(), false);
    req.idempotency(None, false);
    Ok(api.client.send(req).await?)
}
