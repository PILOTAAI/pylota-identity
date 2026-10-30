//! `PM_EVAL_REST` (Testing § 9.2): with `PM_ENV = "local"`, every Workers AI and Vectorize call goes to
//! the Cloudflare REST API with `PM_CF_API_TOKEN`, so an evaluation under `wrangler dev --local` needs
//! no remote binding. The endpoints and bodies are the ones the `cloudflare` 5.2.0 SDK bundled in the
//! pinned Wrangler (4.139.0) sends: `ai/run/{model}` with the model input as the body;
//! `vectorize/v2/indexes/{index}/query` with the query options plus `vector` (or `vectorId`),
//! `upsert` as NDJSON (`application/x-ndjson`), `delete_by_ids` and `get_by_ids` with `{ "ids": […] }`,
//! and `info`. Every answer is the `{ success, errors, result }` envelope.

use serde_json::{Value, json};

use super::net::CfHttp;
use crate::http::{HttpClient as _, HttpRequest};
use crate::{PResult, PlatformError, PlatformErrorKind};

const API: &str = "https://api.cloudflare.com/client/v4";

/// The account, token and index names the REST calls use.
#[derive(Clone, Debug)]
pub struct Rest {
    pub account_id: String,
    pub token: String,
    /// The `VECTORS` index (`PM_VECTORS_INDEX`).
    pub index: String,
    /// The `VECTORS_NEXT` index (`PM_VECTORS_NEXT_INDEX`), during a re-embed.
    pub next_index: Option<String>,
}

impl Rest {
    async fn call(
        &self,
        binding: &'static str,
        method: &'static str,
        path: &str,
        body: Option<(Vec<u8>, &str)>,
    ) -> PResult<Value> {
        let mut req = HttpRequest::get(format!("{API}/accounts/{}{path}", self.account_id));
        req.method = method;
        req.timeout_ms = 120_000;
        req.max_body_bytes = 32 << 20;
        req.headers
            .push(("authorization".into(), format!("Bearer {}", self.token)));
        if let Some((bytes, kind)) = body {
            req.headers.push(("content-type".into(), kind.to_owned()));
            req.body = Some(bytes);
        }
        let r = CfHttp
            .send(req)
            .await
            .map_err(|e| PlatformError::unavailable(binding, format!("REST: {e:?}")))?;
        let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
        if r.status == 429 {
            return Err(PlatformError::unavailable(binding, "REST answered 429"));
        }
        if r.status >= 300 || v["success"] == Value::Bool(false) {
            let errors = v["errors"].to_string();
            let kind = if r.status >= 500 {
                PlatformErrorKind::Unavailable
            } else {
                PlatformErrorKind::Upstream
            };
            return Err(PlatformError::new(
                kind,
                binding,
                format!("REST answered {}: {errors}", r.status),
            ));
        }
        Ok(v["result"].clone())
    }

    fn json(v: &Value) -> Option<(Vec<u8>, &'static str)> {
        Some((v.to_string().into_bytes(), "application/json"))
    }

    pub async fn ai_run(&self, model: &str, input: &Value) -> PResult<Value> {
        self.call("AI", "POST", &format!("/ai/run/{model}"), Self::json(input))
            .await
    }

    fn index(&self, binding: &'static str) -> PResult<&str> {
        match binding {
            "VECTORS_NEXT" => self
                .next_index
                .as_deref()
                .ok_or_else(|| PlatformError::invalid(binding, "PM_VECTORS_NEXT_INDEX is not set")),
            _ => Ok(&self.index),
        }
    }

    /// `op` under `vectorize/v2/indexes/{index}/`.
    pub async fn vectorize(
        &self,
        binding: &'static str,
        method: &'static str,
        op: &str,
        body: Option<&Value>,
    ) -> PResult<Value> {
        let path = format!("/vectorize/v2/indexes/{}/{op}", self.index(binding)?);
        self.call(binding, method, &path, body.and_then(Self::json))
            .await
    }

    /// `upsert` takes NDJSON: one vector object per line.
    pub async fn vectorize_upsert(&self, binding: &'static str, lines: &[Value]) -> PResult<Value> {
        let mut body = String::new();
        for l in lines {
            body.push_str(&l.to_string());
            body.push('\n');
        }
        let path = format!("/vectorize/v2/indexes/{}/upsert", self.index(binding)?);
        self.call(
            binding,
            "POST",
            &path,
            Some((body.into_bytes(), "application/x-ndjson")),
        )
        .await
    }
}

/// `{ "ids": […] }`.
pub fn ids_body(ids: &[String]) -> Value {
    json!({ "ids": ids })
}
