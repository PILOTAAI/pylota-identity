//! The Cloudflare API client ([CLI and setup §5]): Cloudflare's envelope, pagination, retries of `429`
//! and `5xx`, and "find, then create" helpers, so `setup` is safe to run again. Bodies are never logged;
//! `--verbose` logs method, path, status and duration.
//!
//! [CLI and setup §5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#5-the-cloudflare-client

pub mod api;

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::error::CfError;
use crate::{CliError, Ctx, config};

pub const API: &str = "https://api.cloudflare.com/client/v4";

/// The operator's token and account.
pub struct Client<'a> {
    ctx: &'a Ctx,
    token: String,
    pub account: String,
}

/// `CLOUDFLARE_API_TOKEN` (environment only) and the account ID, from `--account-id`,
/// `CLOUDFLARE_ACCOUNT_ID`, the profile, else `PM_CF_ACCOUNT_ID` of the rendered file (exit 3 without).
pub fn credentials(ctx: &Ctx, rendered: Option<&str>) -> Result<(String, String), CliError> {
    let token = ctx
        .var("CLOUDFLARE_API_TOKEN")
        .map(str::to_owned)
        .ok_or_else(|| {
            CliError::config("CLOUDFLARE_API_TOKEN is not set.")
                .with_fix("Export a Cloudflare API token (see Deploy to Cloudflare › Create a Cloudflare API token).")
        })?;
    let account = config::resolve(ctx)?
        .account_id
        .map(|s| s.value)
        .or_else(|| rendered.and_then(|r| crate::bundle::render::var(r, "PM_CF_ACCOUNT_ID")))
        .filter(|a| !a.is_empty() && !a.starts_with('{'))
        .ok_or_else(|| {
            CliError::config("No Cloudflare account ID.")
                .with_fix("Pass --account-id or set CLOUDFLARE_ACCOUNT_ID.")
        })?;
    Ok((token, account))
}

impl<'a> Client<'a> {
    pub fn new(ctx: &'a Ctx, token: String, account: String) -> Self {
        Self {
            ctx,
            token,
            account,
        }
    }

    /// One call: the envelope's `result`, with `result_info` kept in `info` when there is one.
    pub async fn call(
        &self,
        step: &str,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        headers: &[(&str, &str)],
    ) -> Result<(Value, Value), CliError> {
        let url = format!("{}{path}", self.ctx.cloudflare_api);
        let mut attempt = 0u32;
        loop {
            let mut r = self
                .ctx
                .http
                .request(method.clone(), &url)
                .bearer_auth(&self.token)
                .timeout(Duration::from_secs(60))
                .query(query);
            for (k, v) in headers {
                r = r.header(*k, *v);
            }
            if let Some(b) = body {
                r = r.json(b);
            }
            let start = Instant::now();
            let resp = r.send().await;
            let (status, retry_after, bytes) = match resp {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let ra = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok());
                    (status, ra, resp.bytes().await.unwrap_or_default().to_vec())
                }
                Err(e) => {
                    return Err(CliError::Cloudflare {
                        step: step.to_owned(),
                        status: None,
                        errors: vec![CfError {
                            code: 0,
                            message: e.to_string(),
                        }],
                        fix: None,
                    });
                }
            };
            if self.ctx.global.verbose {
                self.ctx.note(&format!(
                    "cloudflare {method} {path} {status} {} ms",
                    start.elapsed().as_millis()
                ));
            }
            if (status == 429 || status >= 500) && attempt < 5 {
                let wait = retry_after.unwrap_or(1 << attempt).min(60);
                tokio::time::sleep(Duration::from_secs(wait)).await;
                attempt += 1;
                continue;
            }
            let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            if (200..300).contains(&status) && v["success"] != false {
                return Ok((v["result"].clone(), v["result_info"].clone()));
            }
            let errors: Vec<CfError> = v["errors"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| CfError {
                    code: e["code"].as_i64().unwrap_or(0),
                    message: e["message"].as_str().unwrap_or("").to_owned(),
                })
                .collect();
            let fix = known_fix(status, &errors);
            return Err(CliError::Cloudflare {
                step: step.to_owned(),
                status: Some(status),
                errors,
                fix,
            });
        }
    }

    pub async fn get(
        &self,
        step: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, CliError> {
        Ok(self
            .call(step, reqwest::Method::GET, path, query, None, &[])
            .await?
            .0)
    }

    pub async fn post(&self, step: &str, path: &str, body: &Value) -> Result<Value, CliError> {
        Ok(self
            .call(step, reqwest::Method::POST, path, &[], Some(body), &[])
            .await?
            .0)
    }

    pub async fn put(&self, step: &str, path: &str, body: &Value) -> Result<Value, CliError> {
        Ok(self
            .call(step, reqwest::Method::PUT, path, &[], Some(body), &[])
            .await?
            .0)
    }

    pub async fn patch(&self, step: &str, path: &str, body: &Value) -> Result<Value, CliError> {
        Ok(self
            .call(step, reqwest::Method::PATCH, path, &[], Some(body), &[])
            .await?
            .0)
    }

    pub async fn delete(&self, step: &str, path: &str) -> Result<Value, CliError> {
        Ok(self
            .call(step, reqwest::Method::DELETE, path, &[], None, &[])
            .await?
            .0)
    }

    /// Every item of a list: `result_info` pages (`page`, `per_page`, `total_pages`) or `cursor`.
    pub async fn list(
        &self,
        step: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Vec<Value>, CliError> {
        let mut out = Vec::new();
        let mut page = 1u32;
        let mut cursor: Option<String> = None;
        for _ in 0..1000 {
            let mut q: Vec<(&str, String)> = query.to_vec();
            match &cursor {
                Some(c) => q.push(("cursor", c.clone())),
                None => {
                    q.push(("page", page.to_string()));
                    q.push(("per_page", "50".to_owned()));
                }
            }
            let (result, info) = self
                .call(step, reqwest::Method::GET, path, &q, None, &[])
                .await?;
            let items = result.as_array().cloned().unwrap_or_default();
            let n = items.len();
            out.extend(items);
            if let Some(c) = info["cursor"].as_str().filter(|c| !c.is_empty()) {
                cursor = Some(c.to_owned());
                continue;
            }
            let total = info["total_pages"].as_u64().unwrap_or(0);
            if total > u64::from(page) || (total == 0 && n == 50) {
                page += 1;
                continue;
            }
            break;
        }
        Ok(out)
    }

    /// The token, for Wrangler's environment.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// `/accounts/{account}` + `rest`.
    pub fn acct(&self, rest: &str) -> String {
        format!("/accounts/{}{rest}", self.account)
    }
}

/// A fix for the Cloudflare errors people meet most.
fn known_fix(status: u16, errors: &[CfError]) -> Option<String> {
    let codes: Vec<i64> = errors.iter().map(|e| e.code).collect();
    if status == 403 || codes.contains(&10000) || codes.contains(&9109) {
        return Some("The token lacks a permission this step needs; see Deploy to Cloudflare › Create a Cloudflare API token (the first setup needs the first-run token with Workers Admin).".into());
    }
    if codes.contains(&1001) || codes.contains(&7003) {
        return Some("Check the account ID and that the zone is in that account.".into());
    }
    if codes.contains(&10006) || codes.contains(&10004) {
        return Some(
            "A resource of that name exists in another jurisdiction; it is reported, not reused."
                .into(),
        );
    }
    None
}

/// A one-off error for a step whose answer did not have the expected shape.
pub fn unexpected(step: &str, what: &str) -> CliError {
    CliError::Cloudflare {
        step: step.to_owned(),
        status: None,
        errors: vec![CfError {
            code: 0,
            message: what.to_owned(),
        }],
        fix: None,
    }
}

/// The D1 query API's one statement, `?` parameters as strings.
pub fn d1_body(sql: &str, params: &[Value]) -> Value {
    let params: Vec<Value> = params
        .iter()
        .map(|p| match p {
            Value::String(_) | Value::Null => p.clone(),
            other => Value::String(other.to_string()),
        })
        .collect();
    json!({ "sql": sql, "params": params })
}
