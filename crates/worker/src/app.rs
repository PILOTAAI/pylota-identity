//! The request pipeline: route (deny by default), authenticate, rate-limit, freeze and read-only, the
//! route's permissions and levels, idempotency, the handler, and the response with its global headers.

use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::policy::request_fingerprint;
use pylota_identity_platform::app::{HttpRequestIn, HttpResponseOut};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{Clock as _, ControlDb as _, Platform};
use serde_json::{Value, json};

use crate::auth::{self, Level};
use crate::ctx::{Ctx, Reply};
use crate::db::idempotency as idb;
use crate::errors::{ApiError, envelope};
use crate::router::{self, Idempotency, Scope};
use crate::{handlers, ratelimit, request_id};

/// The Worker logic, generic over the platform so native tests run it on `platform::fakes`.
pub struct App<P: Platform> {
    pub p: Rc<P>,
}

/// `(path, query)` of a request URL.
fn split_url(url: &str) -> (String, Vec<(String, String)>) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path_and_query = rest.find('/').map_or("/", |i| &rest[i..]);
    let path_and_query = path_and_query.split('#').next().unwrap_or("/");
    let (path, query) = path_and_query
        .split_once('?')
        .unwrap_or((path_and_query, ""));
    let decode = |s: &str| -> String {
        let s = s.replace('+', " ");
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && let Some(v) = s
                    .get(i + 1..i + 3)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    let params = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();
    (path.to_owned(), params)
}

/// The one-time secret is never stored: a replay gets `"secret_replayed": false` instead.
fn storable(body: &Value) -> Value {
    let mut b = body.clone();
    if let Some(o) = b.as_object_mut()
        && o.remove("secret").is_some()
    {
        o.insert("secret_replayed".into(), Value::Bool(false));
    }
    b
}

impl<P: Platform> App<P> {
    pub fn new(p: P) -> Self {
        Self { p: Rc::new(p) }
    }

    pub async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        let request_id = request_id::new(self.p.as_ref());
        let (path, query) = split_url(&req.url);
        let mut authenticated = false;
        let mut rl_limit: Option<u32> = None;
        let result = self
            .serve(
                &req,
                &request_id,
                path,
                query,
                &mut authenticated,
                &mut rl_limit,
            )
            .await;
        let (status, mut headers, body) = match result {
            Ok(reply) => {
                let mut h = reply.headers;
                let body = match (reply.raw, reply.body) {
                    (Some((ct, bytes)), _) => {
                        h.push(("content-type".into(), ct.into()));
                        bytes
                    }
                    (None, Some(v)) => {
                        h.push((
                            "content-type".into(),
                            "application/json; charset=utf-8".into(),
                        ));
                        v.to_string().into_bytes()
                    }
                    (None, None) => Vec::new(),
                };
                (reply.status, h, body)
            }
            Err(e) => {
                let body = envelope(&e, &request_id, &self.p.config().docs_url)
                    .to_string()
                    .into_bytes();
                let mut h = e.headers.clone();
                h.push((
                    "content-type".into(),
                    "application/json; charset=utf-8".into(),
                ));
                (e.code.http_status(), h, body)
            }
        };
        headers.push(("request-id".into(), request_id));
        headers.push(("x-content-type-options".into(), "nosniff".into()));
        headers.push(("referrer-policy".into(), "no-referrer".into()));
        headers.push((
            "strict-transport-security".into(),
            "max-age=31536000".into(),
        ));
        if authenticated {
            headers.push(("cache-control".into(), "no-store".into()));
            if !headers.iter().any(|(n, _)| n == "ratelimit-limit")
                && let Some(l) = rl_limit
            {
                headers.push(("ratelimit-limit".into(), l.to_string()));
            }
        }
        HttpResponseOut::Http {
            status,
            headers,
            body,
        }
    }

    async fn serve(
        &self,
        req: &HttpRequestIn,
        request_id: &str,
        path: String,
        query: Vec<(String, String)>,
        authenticated: &mut bool,
        rl_limit: &mut Option<u32>,
    ) -> Result<Reply, ApiError> {
        let cfg = self.p.config();
        // Console paths answer only on the console host, and the console arrives with M21.
        if path == "/console" || path.starts_with("/console/") {
            return Err(ApiError::new(
                ErrorCode::RouteNotFound,
                "No route matches this path.",
            ));
        }
        if cfg.console_host != cfg.api_host
            && req
                .header("host")
                .is_some_and(|h| h.eq_ignore_ascii_case(&cfg.console_host))
        {
            return Err(ApiError::new(
                ErrorCode::RouteNotFound,
                "No route matches this path.",
            ));
        }
        let route = router::find(&req.method, &path)?;
        let spec = route.spec;
        let mut c = Ctx {
            p: self.p.clone(),
            request_id: request_id.to_owned(),
            now: self.p.clock().now_ms(),
            method: req.method.clone(),
            path,
            query,
            headers: req.headers.clone(),
            body: req.body.as_ref().map(|b| b.to_vec()),
            route,
            key: None,
        };
        if spec.scope == Scope::Public {
            return handlers::dispatch(&c).await;
        }

        let key = auth::authenticate(&self.p, req.header("authorization")).await?;
        *authenticated = true;
        *rl_limit = Some(ratelimit::limit(RlBucket::Api));
        ratelimit::check(self.p.as_ref(), RlBucket::Api, &key.key_id).await?;
        match key.level {
            Level::Tenant | Level::Identity => {
                ratelimit::check(
                    self.p.as_ref(),
                    RlBucket::Tenant,
                    key.tenant_id.as_deref().unwrap_or(""),
                )
                .await?;
            }
            Level::Partner => {
                ratelimit::check(
                    self.p.as_ref(),
                    RlBucket::PartnerApi,
                    key.partner_id.as_deref().unwrap_or(""),
                )
                .await?;
            }
            Level::Platform => {}
        }
        if key.level != Level::Platform {
            if cfg.frozen {
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "The deployment is frozen for a restore.",
                )
                .details(json!({ "reason": "frozen" })));
            }
            if req.method != "GET" && req.method != "HEAD" {
                #[derive(serde::Deserialize)]
                struct S {
                    state: String,
                }
                let ro: Option<S> = self
                    .p
                    .db()
                    .first(Stmt::new(
                        "SELECT state FROM ops_switches WHERE name = 'read_only'",
                        vec![],
                    ))
                    .await?;
                if ro.is_some_and(|s| s.state == "on") {
                    return Err(ApiError::new(
                        ErrorCode::Unavailable,
                        "The deployment is read-only.",
                    )
                    .details(json!({ "reason": "read_only" })));
                }
            }
        }
        let foreign = match spec.scope {
            Scope::TenantPath { param } => {
                key.level >= Level::Partner
                    || key.tenant_id.as_deref() != Some(c.route.param(param))
            }
            _ => false,
        };
        auth::check_route(&key, spec, foreign)?;
        c.key = Some(key);

        let idem = match (spec.idempotency, c.header("idempotency-key")) {
            (Idempotency::None, _) | (Idempotency::Optional, None) => None,
            (Idempotency::Required, None) => {
                return Err(ApiError::new(
                    ErrorCode::IdempotencyKeyRequired,
                    "This route needs an Idempotency-Key.",
                ));
            }
            (_, Some(k)) => {
                if k.is_empty() || k.len() > 255 || !k.bytes().all(|b| (0x20..0x7f).contains(&b)) {
                    return Err(ApiError::invalid(
                        "Idempotency-Key",
                        "1–255 printable ASCII characters.",
                    ));
                }
                Some(k.to_owned())
            }
        };
        let Some(idem_key) = idem else {
            return handlers::dispatch(&c).await;
        };
        self.idempotent(&c, &idem_key).await
    }

    /// Runs the handler once per (scope, key, Idempotency-Key): a replay returns the stored response.
    async fn idempotent(&self, c: &Ctx<P>, idem_key: &str) -> Result<Reply, ApiError> {
        let key = c.key();
        let scope = match key.level {
            Level::Platform => key.tenant_id.clone().unwrap_or_else(|| "platform".into()),
            Level::Partner => format!("partner:{}", key.partner_id.as_deref().unwrap_or("")),
            _ => key.tenant_id.clone().unwrap_or_default(),
        };
        let body = c.json().unwrap_or(Value::Null);
        let fp = request_fingerprint(&c.method, &c.path, &body);
        let db = self.p.db();
        db.execute(idb::expire(&scope, &key.key_id, idem_key, c.now))
            .await?;
        let began = db
            .execute(idb::begin(
                &scope,
                &key.key_id,
                key.tenant_id.as_deref(),
                idem_key,
                &c.method,
                &c.path,
                &fp,
                c.now,
            ))
            .await?;
        if began.changes == 0 {
            let row: Option<idb::Row> = db
                .first(idb::get(&scope, &key.key_id, idem_key, c.now))
                .await?;
            let row = row.ok_or_else(|| {
                ApiError::new(
                    ErrorCode::RequestInProgress,
                    "The first request is still running.",
                )
            })?;
            if row.fingerprint != fp {
                return Err(ApiError::new(
                    ErrorCode::IdempotencyConflict,
                    "This Idempotency-Key was used with a different request.",
                ));
            }
            if row.status != "completed" {
                return Err(ApiError::new(
                    ErrorCode::RequestInProgress,
                    "The first request is still running.",
                ));
            }
            let mut reply = Reply::json(
                row.response_status.unwrap_or(200) as u16,
                row.response_body
                    .as_deref()
                    .and_then(|b| serde_json::from_str(b).ok())
                    .unwrap_or(Value::Null),
            );
            if reply.body.as_ref().is_some_and(Value::is_null) {
                reply.body = None;
            }
            reply
                .headers
                .push(("idempotent-replayed".into(), "true".into()));
            return Ok(reply);
        }
        match handlers::dispatch(c).await {
            Ok(reply) if (200..300).contains(&reply.status) => {
                let stored = reply
                    .body
                    .as_ref()
                    .map(storable)
                    .unwrap_or(Value::Null)
                    .to_string();
                if db
                    .execute(idb::complete(
                        &scope,
                        &key.key_id,
                        idem_key,
                        reply.status,
                        &stored,
                    ))
                    .await
                    .is_err()
                {
                    crate::log::warn("idempotency_store_failed", &[]);
                }
                Ok(reply)
            }
            other => {
                let _ = db
                    .execute(idb::abandon(&scope, &key.key_id, idem_key))
                    .await;
                other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_split_into_path_and_query() {
        let (p, q) =
            split_url("https://api.example.com/v1/tenants?status=active&partner_id=ptn%5F1&x");
        assert_eq!(p, "/v1/tenants");
        assert_eq!(
            q,
            [
                ("status".into(), "active".into()),
                ("partner_id".into(), "ptn_1".into()),
                ("x".into(), String::new())
            ]
        );
        assert_eq!(split_url("https://h").0, "/");
        assert_eq!(
            storable(&json!({ "id": "k", "secret": "pmk" })),
            json!({ "id": "k", "secret_replayed": false })
        );
    }
}
