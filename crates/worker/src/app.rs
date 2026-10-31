//! The request pipeline: route (deny by default), authenticate, rate-limit, freeze and read-only, the
//! route's permissions and levels, idempotency, the handler, and the response with its global headers.

use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::policy::request_fingerprint;
use pylota_identity_platform::app::{HttpRequestIn, HttpResponseOut};
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{Clock as _, ControlDb as _, Platform};
use serde_json::{Value, json};

use crate::auth::{self, Level};
use crate::ctx::{Ctx, Reply};
use crate::db::idempotency as idb;
use crate::errors::{ApiError, envelope};
use crate::router::{self, Handler, Idempotency, Scope};
use crate::{handlers, ratelimit, request_id};

/// One request to run as an authenticated key: what `serve_as` needs of an HTTP request.
pub(crate) struct Call {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

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

/// The operations that return mail content (message text, raw MIME, attachments, snippets, answers).
const MAIL_READS: &[&str] = &[
    "listThreads",
    "getThread",
    "listMessages",
    "getMessage",
    "getMessageRaw",
    "getAttachment",
    "getAttachmentText",
    "waitForMessage",
    "listQuarantine",
    "searchIdentity",
    "searchTenant",
    "findRelatedMessages",
];

impl<P: Platform> App<P> {
    pub fn new(p: P) -> Self {
        Self { p: Rc::new(p) }
    }

    pub async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        let request_id = request_id::new(self.p.as_ref());
        let (path, query) = split_url(&req.url);
        let path_for_headers = path.clone();
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
        let mut stream = None;
        let (status, mut headers, body) = match result {
            Ok(reply) => {
                stream = reply.stream;
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
        let console = path_for_headers == "/console" || path_for_headers.starts_with("/console/");
        if console {
            // `same-origin`, so a form's `Origin` is not `null` (Console › CSRF).
            headers.push(("referrer-policy".into(), "same-origin".into()));
            if !headers.iter().any(|(n, _)| n == "content-security-policy") {
                headers.push(("content-security-policy".into(), crate::console::CSP.into()));
            }
            if !headers.iter().any(|(n, _)| n == "cache-control") {
                headers.push(("cache-control".into(), "no-store".into()));
            }
        } else {
            headers.push(("referrer-policy".into(), "no-referrer".into()));
        }
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
        match stream {
            Some(body) => HttpResponseOut::Stream {
                status,
                headers,
                body,
            },
            None => HttpResponseOut::Http {
                status,
                headers,
                body,
            },
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
        // Console paths answer only on the console host and API paths only on the API host when the
        // two differ (Cloud sign-up § 2); no cookie is read or set on the API host.
        let host = req.header("host").unwrap_or("");
        let split = cfg.console_host != cfg.api_host;
        if path == "/console" || path.starts_with("/console/") {
            if split && !host.eq_ignore_ascii_case(&cfg.console_host) {
                return Err(ApiError::new(
                    ErrorCode::RouteNotFound,
                    "No route matches this path.",
                ));
            }
            return Ok(crate::console::serve(self, req, request_id, &path, query).await);
        }
        if split && host.eq_ignore_ascii_case(&cfg.console_host) {
            return Err(ApiError::new(
                ErrorCode::RouteNotFound,
                "No route matches this path.",
            ));
        }
        #[cfg(feature = "itest-hooks")]
        if path.starts_with("/__test/") {
            return crate::hooks::serve(&self.p, req, request_id, &path).await;
        }
        if path == "/mcp" {
            *authenticated = true;
            return Ok(crate::mcp::serve(self, req, request_id).await);
        }
        let route = router::find(&req.method, &path)?;
        if route.spec.scope == Scope::Public {
            let c = Ctx {
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
            return handlers::dispatch(&c).await;
        }

        let key = auth::authenticate(&self.p, req.header("authorization")).await?;
        *authenticated = true;
        // The break-glass key (Observability § 5.7, J36): any use writes its audit row and pages at
        // once, from the authentication path itself.
        if cfg.break_glass_key_id.as_deref() == Some(key.key_id.as_str()) {
            let ip = req.header("cf-connecting-ip").unwrap_or("");
            let hash = pylota_identity_core::address::address_hash(&cfg.secrets.hash_key, ip);
            let ip_hash = hash.get(..16).unwrap_or("");
            let now = self.p.clock().now_ms();
            let _ = self
                .p
                .db()
                .execute(crate::db::audit::insert(
                    &pylota_identity_platform::Ids::new_id(
                        self.p.ids(),
                        pylota_identity_platform::ids::IdPrefix::Audit,
                    ),
                    None,
                    Some(&key.key_id),
                    "key.break_glass_used",
                    "api_key",
                    &key.key_id,
                    &json!({ "method": req.method, "ip_hash": ip_hash }),
                    request_id,
                    now,
                ))
                .await;
            crate::ops::alerts::report(
                self.p.as_ref(),
                "break_glass_key_used",
                None,
                Some(pylota_identity_core::slo::Severity::Page),
                &json!({ "key_id": key.key_id }),
                request_id,
            )
            .await;
        }
        *rl_limit = Some(ratelimit::limit(RlBucket::Api));
        self.charge(&key).await?;
        self.serve_as(
            key,
            Call {
                method: req.method.clone(),
                path,
                query,
                headers: req.headers.clone(),
                body: req.body.as_ref().map(|b| b.to_vec()),
            },
            request_id,
        )
        .await
    }

    /// The request-level rate limits: `RL_API` per key, then the tenant's or the partner's bucket.
    pub(crate) async fn charge(&self, key: &auth::KeyScope) -> Result<(), ApiError> {
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
        Ok(())
    }

    /// Everything after authentication and the request-level rate limits: freeze and read-only, the
    /// route's levels and permissions, idempotency and the handler. MCP tools call it with the key
    /// their request authenticated, so `RL_API` is charged once per MCP request (MCP § 4).
    pub(crate) async fn serve_as(
        &self,
        key: auth::KeyScope,
        call: Call,
        request_id: &str,
    ) -> Result<Reply, ApiError> {
        let cfg = self.p.config();
        let route = router::find(&call.method, &call.path)?;
        let spec = route.spec;
        let mut c = Ctx {
            p: self.p.clone(),
            request_id: request_id.to_owned(),
            now: self.p.clock().now_ms(),
            method: call.method,
            path: call.path,
            query: call.query,
            headers: call.headers,
            body: call.body,
            route,
            key: None,
        };
        if key.level != Level::Platform {
            if cfg.frozen {
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "The deployment is frozen for a restore.",
                )
                .details(json!({ "reason": "frozen" }))
                .header("Retry-After", "300"));
            }
            if c.method != "GET"
                && c.method != "HEAD"
                && crate::ops::switches::state(self.p.as_ref(), "read_only").await == "on"
            {
                return Err(
                    ApiError::new(ErrorCode::Unavailable, "The deployment is read-only.")
                        .details(json!({ "reason": "read_only" })),
                );
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
        // Mail reads by platform and partner keys are break-glass access (Security § 3.6): one
        // `mail.read` row before the handler runs; a row that cannot be written refuses the read.
        if key.level >= Level::Partner && MAIL_READS.contains(&spec.operation_id) {
            let (target_type, target_id) = c
                .route
                .params
                .iter()
                .find(|(n, _)| n.ends_with("_id"))
                .map_or(("route", String::new()), |(n, v)| {
                    (n.trim_end_matches("_id"), v.clone())
                });
            let tenant = (target_type == "tenant").then(|| target_id.clone());
            let detail: serde_json::Map<String, Value> = c
                .route
                .params
                .iter()
                .map(|(n, v)| (n.to_string(), Value::from(v.as_str())))
                .collect();
            let row = crate::db::audit::insert(
                &pylota_identity_platform::Ids::new_id(
                    self.p.ids(),
                    pylota_identity_platform::ids::IdPrefix::Audit,
                ),
                tenant.as_deref(),
                Some(&key.key_id),
                "mail.read",
                target_type,
                &target_id,
                &json!({ "route": spec.pattern, "operation": spec.operation_id, "params": detail }),
                request_id,
                c.now,
            );
            if self.p.db().execute(row).await.is_err() {
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "The read could not be audited.",
                )
                .details(json!({ "reason": "audit_unavailable" })));
            }
        }
        c.key = Some(key);

        // The send routes keep their ledger in the mailbox (Outbound › Reservation inside the mailbox).
        if matches!(
            spec.handler,
            Handler::Send | Handler::Reply | Handler::ReplyAll | Handler::Forward
        ) {
            return handlers::dispatch(&c).await;
        }
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
