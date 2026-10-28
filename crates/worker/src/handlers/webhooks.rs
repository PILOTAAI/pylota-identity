//! Webhook endpoints ([Webhooks], [REST API › Webhooks]): create (platform, partner and tenant scope),
//! list, get, update, delete, rotate the secret, test, the delivery log and replay. Mail events need
//! `messages:read` (and `quarantine:review`) besides `webhooks:manage` (J29).
//!
//! [Webhooks]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md
//! [REST API › Webhooks]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::EventType;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::days::parse_rfc3339;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform, Rng as _};
use serde_json::{Value, json};

use super::{Count, tenant_in_scope};
use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::webhooks::{self as wdb, Row};
use crate::db::{audit, identities as idb};
use crate::errors::ApiError;
use crate::webhooks::client;
use crate::webhooks::envelope::envelope;
use crate::webhooks::matching::required_permissions;
use crate::webhooks::replay::{self, Selection};

/// Endpoints per tenant, per partner, and platform endpoints.
pub const MAX_ENDPOINTS: i64 = 20;
const MAX_HEADERS: usize = 10;
const MAX_HEADER_BYTES: usize = 4096;

fn not_found() -> ApiError {
    ApiError::not_found(ErrorCode::WebhookNotFound)
}

/// The endpoint if the key can see it: a platform key every one; a partner key its partner's and its
/// tenants'; a tenant or identity key its tenant's.
async fn in_scope<P: Platform>(c: &Ctx<P>) -> Result<Row, ApiError> {
    let row: Row =
        c.p.db()
            .first(wdb::get(c.route.param("webhook_id")))
            .await?
            .ok_or_else(not_found)?;
    let key = c.key();
    let visible = match key.level {
        Level::Platform => true,
        Level::Partner => {
            key.partner_id.is_some() && row.owner_partner_id.as_deref() == key.partner_id.as_deref()
        }
        Level::Tenant | Level::Identity => {
            row.tenant_id.is_some() && row.tenant_id == key.tenant_id
        }
    };
    if visible { Ok(row) } else { Err(not_found()) }
}

/// J29: the permissions mail event types need beyond `webhooks:manage`.
fn check_mail_permissions<P: Platform>(c: &Ctx<P>, types: &[String]) -> Result<(), ApiError> {
    let missing: Vec<&str> = required_permissions(types)
        .into_iter()
        .filter(|p| !c.key().has(p))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(ApiError::permission_denied(&missing))
    }
}

fn events_field(v: &Value) -> Result<Vec<String>, ApiError> {
    let bad = || ApiError::invalid("events", "A list of event types, or [\"*\"].");
    let list = v.as_array().filter(|a| !a.is_empty()).ok_or_else(bad)?;
    let mut out: Vec<String> = Vec::new();
    for t in list {
        let t = t.as_str().ok_or_else(bad)?;
        if t != "*" && serde_json::from_value::<EventType>(json!(t)).is_err() {
            return Err(ApiError::invalid(
                "events",
                format!("Unknown event type {t}."),
            ));
        }
        if out.iter().any(|x| x == t) {
            return Err(ApiError::invalid("events", "Event types are unique."));
        }
        out.push(t.to_owned());
    }
    Ok(out)
}

async fn identity_ids_field<P: Platform>(
    c: &Ctx<P>,
    v: &Value,
    tenant: Option<&str>,
) -> Result<Option<Vec<String>>, ApiError> {
    match v {
        Value::Null => Ok(None),
        Value::Array(a) => {
            let mut out: Vec<String> = Vec::new();
            for id in a {
                let id = id
                    .as_str()
                    .filter(|s| s.starts_with("idn_"))
                    .ok_or_else(|| ApiError::invalid("identity_ids", "Identity IDs."))?;
                // On a tenant endpoint the identities must be the tenant's.
                if let Some(t) = tenant {
                    let row: Option<idb::Row> = c.p.db().first(idb::get(id)).await?;
                    if row.is_none_or(|r| r.tenant_id != t) {
                        return Err(ApiError::invalid(
                            "identity_ids",
                            format!("{id} is not an identity of this tenant."),
                        ));
                    }
                }
                if !out.iter().any(|x| x == id) {
                    out.push(id.to_owned());
                }
            }
            Ok(Some(out))
        }
        _ => Err(ApiError::invalid(
            "identity_ids",
            "A list of identity IDs, or null.",
        )),
    }
}

fn url_field<P: Platform>(c: &Ctx<P>, v: &Value) -> Result<String, ApiError> {
    let url = v
        .as_str()
        .ok_or_else(|| ApiError::invalid("url", "An https URL."))?;
    crate::net::check_url(c.p.as_ref(), url).map_err(|r| ApiError::invalid("url", r.message()))?;
    Ok(url.to_owned())
}

fn description_field(v: &Value) -> Result<Option<String>, ApiError> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.chars().count() <= 1000 => Ok(Some(s.clone())),
        _ => Err(ApiError::invalid(
            "description",
            "At most 1,000 characters.",
        )),
    }
}

/// Custom delivery headers (Parity additions § 8.2): names, values and limits.
fn headers_field(v: &Value) -> Result<Option<serde_json::Map<String, Value>>, ApiError> {
    let Some(map) = (match v {
        Value::Null => return Ok(None),
        Value::Object(m) => Some(m),
        _ => None,
    }) else {
        return Err(ApiError::invalid(
            "headers",
            "A map of header names to values.",
        ));
    };
    let refused = |name: &str| {
        let n = name.to_ascii_lowercase();
        const EXACT: &[&str] = &[
            "accept-charset",
            "accept-encoding",
            "access-control-request-headers",
            "access-control-request-method",
            "connection",
            "content-length",
            "cookie",
            "cookie2",
            "date",
            "dnt",
            "expect",
            "host",
            "keep-alive",
            "origin",
            "referer",
            "set-cookie",
            "te",
            "trailer",
            "transfer-encoding",
            "upgrade",
            "via",
            "x-http-method",
            "x-http-method-override",
            "x-method-override",
            "content-type",
            "content-encoding",
            "user-agent",
            "forwarded",
            "x-real-ip",
            "true-client-ip",
        ];
        let cf_access = n == "cf-access-client-id" || n == "cf-access-client-secret";
        EXACT.contains(&n.as_str())
            || ["proxy-", "sec-", "webhook-", "x-forwarded-", "x-pylota-"]
                .iter()
                .any(|p| n.starts_with(p))
            || (n.starts_with("cf-") && !cf_access)
    };
    let mut total = 0;
    let mut seen: Vec<String> = Vec::new();
    for (name, value) in map {
        let valid_name = !name.is_empty()
            && name.len() <= 64
            && name.as_bytes()[0].is_ascii_alphanumeric()
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !valid_name || refused(name) {
            return Err(
                ApiError::new(ErrorCode::HeaderNotAllowed, "This header cannot be set.")
                    .details(json!({ "reason": "webhook_header", "name": name })),
            );
        }
        let lower = name.to_ascii_lowercase();
        if seen.contains(&lower) {
            return Err(ApiError::invalid(
                &format!("headers.{name}"),
                "A name appears twice.",
            ));
        }
        seen.push(lower);
        let value = value.as_str().unwrap_or("");
        let b = value.as_bytes();
        if b.is_empty()
            || b.len() > 1024
            || !b.iter().all(|c| (0x20..=0x7e).contains(c))
            || b[0] == b' '
            || b[b.len() - 1] == b' '
        {
            return Err(ApiError::invalid(
                &format!("headers.{name}"),
                "1 to 1,024 printable bytes.",
            ));
        }
        total += b.len();
    }
    if map.len() > MAX_HEADERS {
        return Err(
            ApiError::new(ErrorCode::WebhookHeaderLimitReached, "At most 10 headers.")
                .details(json!({ "limit": "count", "reason": "count", "max": MAX_HEADERS })),
        );
    }
    if total > MAX_HEADER_BYTES {
        return Err(ApiError::new(
            ErrorCode::WebhookHeaderLimitReached,
            "At most 4,096 bytes of values.",
        )
        .details(json!({ "limit": "bytes", "reason": "bytes", "max": MAX_HEADER_BYTES })));
    }
    Ok(Some(map.clone()))
}

fn only(body: &Value, allowed: &[&str]) -> Result<(), ApiError> {
    super::messages::only_fields(body, allowed)
}

fn audit_stmt<P: Platform>(
    c: &Ctx<P>,
    tenant: Option<&str>,
    action: &str,
    id: &str,
    details: Value,
) -> Stmt {
    let s = audit::insert(
        &c.p.ids().new_id(IdPrefix::Audit),
        tenant,
        Some(&c.key().key_id),
        action,
        "webhook",
        id,
        &details,
        &c.request_id,
        c.now,
    );
    if tenant.is_some() {
        audit::when_tenant(s)
    } else {
        s
    }
}

/// A new secret: its `whsec_` form and its sealed bytes.
fn new_secret<P: Platform>(c: &Ctx<P>, id: &str) -> (String, String) {
    let bytes: [u8; 32] = c.p.rng().bytes();
    (
        crate::webhooks::sign::secret_string(&bytes),
        client::seal(c.p.as_ref(), "secret_enc", id, &bytes),
    )
}

/// `POST /v1/webhooks` (platform or partner) and `POST /v1/tenants/{tenant_id}/webhooks`.
pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    let (tenant, partner) = if c.route.params.iter().any(|(n, _)| *n == "tenant_id") {
        let t = tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
        (Some(t.id), None)
    } else {
        match key.level {
            Level::Platform => (None, None),
            Level::Partner => (None, key.partner_id.clone()),
            _ => return Err(ApiError::scope_denied(None)),
        }
    };
    let body = c.json()?;
    only(
        &body,
        &["url", "events", "identity_ids", "description", "headers"],
    )?;
    let url = url_field(c, &body["url"])?;
    let events = events_field(&body["events"])?;
    check_mail_permissions(c, &events)?;
    let identity_ids = identity_ids_field(c, &body["identity_ids"], tenant.as_deref()).await?;
    let description = description_field(&body["description"])?;
    let headers = headers_field(&body["headers"])?;
    let n: Option<Count> =
        c.p.db()
            .first(wdb::count(tenant.as_deref(), partner.as_deref()))
            .await?;
    if n.map_or(0, |n| n.n) >= MAX_ENDPOINTS {
        return Err(ApiError::new(
            ErrorCode::WebhookLimitReached,
            "This scope has 20 endpoints already.",
        )
        .details(json!({ "limit": MAX_ENDPOINTS })));
    }
    let id = c.p.ids().new_id(IdPrefix::Webhook);
    let (secret, secret_enc) = new_secret(c, &id);
    let (headers_sealed, header_names) = match &headers {
        Some(m) if !m.is_empty() => (
            Some(client::seal(
                c.p.as_ref(),
                "headers_sealed",
                &id,
                Value::Object(m.clone()).to_string().as_bytes(),
            )),
            m.keys().cloned().collect::<Vec<_>>(),
        ),
        _ => (None, Vec::new()),
    };
    let insert = Stmt::new(
        "INSERT INTO webhook_endpoints (id, tenant_id, partner_id, url, description, event_types_json, identity_ids_json, enabled, disabled_reason, created_by_key_id, created_by_user_id, secret_enc, headers_sealed, header_names_json, consecutive_failures, created_at, updated_at) \
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, NULL, ?8, ?9, ?10, ?11, ?12, 0, ?13, ?13 \
         WHERE (SELECT COUNT(*) FROM webhook_endpoints WHERE CASE WHEN ?2 IS NOT NULL THEN tenant_id = ?2 WHEN ?3 IS NOT NULL THEN tenant_id IS NULL AND partner_id = ?3 ELSE tenant_id IS NULL AND partner_id IS NULL END) < ?14",
        vec![
            id.clone().into(),
            tenant.clone().into(),
            partner.clone().into(),
            url.clone().into(),
            description.into(),
            serde_json::to_string(&events).unwrap_or_default().into(),
            identity_ids
                .map(|i| serde_json::to_string(&i).unwrap_or_default())
                .into(),
            key.key_id.clone().into(),
            key.created_by_user_id.clone().into(),
            secret_enc.into(),
            headers_sealed.into(),
            serde_json::to_string(&header_names)
                .unwrap_or_default()
                .into(),
            c.now.into(),
            MAX_ENDPOINTS.into(),
        ],
    );
    let host = crate::net::check_url(c.p.as_ref(), &url)
        .map(|t| t.host)
        .unwrap_or_default();
    let out =
        c.p.db()
            .batch(vec![
                insert,
                audit_stmt(
                    c,
                    tenant.as_deref(),
                    "webhook.create",
                    &id,
                    json!({ "host": host, "events": events }),
                ),
            ])
            .await?;
    if out.first().is_none_or(|m| m.changes == 0) {
        return Err(ApiError::new(
            ErrorCode::WebhookLimitReached,
            "This scope has 20 endpoints already.",
        )
        .details(json!({ "limit": MAX_ENDPOINTS })));
    }
    let row: Row =
        c.p.db()
            .first(wdb::get(&id))
            .await?
            .ok_or_else(ApiError::internal)?;
    let mut v = row.json();
    v["secret"] = json!(secret);
    Ok(Reply::json(201, v))
}

const LIST_FILTERS: &[&str] = &[];

async fn render_list<P: Platform>(
    c: &Ctx<P>,
    tenant: Option<&str>,
    partner: Option<&str>,
) -> Result<Reply, ApiError> {
    let limit = c.limit()?;
    let after = c.after(LIST_FILTERS).await?;
    let rows: Vec<Row> =
        c.p.db()
            .query(wdb::list(tenant, partner, after, limit))
            .await?;
    c.page(
        rows,
        limit,
        LIST_FILTERS,
        |r| (r.created_at, r.id.clone()),
        Row::json,
    )
    .await
}

/// `GET /v1/webhooks`: the key's own scope.
pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    match key.level {
        Level::Platform => render_list(c, None, None).await,
        Level::Partner => render_list(c, None, key.partner_id.as_deref()).await,
        Level::Tenant | Level::Identity => render_list(c, key.tenant_id.as_deref(), None).await,
    }
}

/// `GET /v1/tenants/{tenant_id}/webhooks`.
pub async fn list_tenant<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), false).await?;
    render_list(c, Some(&t.id), None).await
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    Ok(Reply::json(200, in_scope(c).await?.json()))
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split(['/', '?', ':']).next())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub async fn update<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c).await?;
    let body = c.json()?;
    only(
        &body,
        &["url", "events", "identity_ids", "description", "enabled"],
    )?;
    let obj = body.as_object().cloned().unwrap_or_default();
    let url = match obj.get("url") {
        Some(v) => Some(url_field(c, v)?),
        None => None,
    };
    let events = match obj.get("events") {
        Some(v) => Some(events_field(v)?),
        None => None,
    };
    let url_changed = url.as_deref().is_some_and(|u| u != row.url);
    if url_changed || events.is_some() {
        check_mail_permissions(c, events.as_deref().unwrap_or(&row.event_types()))?;
    }
    let identity_ids = match obj.get("identity_ids") {
        Some(v) => Some(identity_ids_field(c, v, row.tenant_id.as_deref()).await?),
        None => None,
    };
    let description = match obj.get("description") {
        Some(v) => Some(description_field(v)?),
        None => None,
    };
    let enabled = match obj.get("enabled") {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(ApiError::invalid("enabled", "true or false.")),
    };
    if enabled == Some(true)
        && !url_changed
        && row.disabled_reason.as_deref() == Some("url_changed")
    {
        return Err(ApiError::invalid(
            "enabled",
            "Send a test delivery to confirm the new URL.",
        ));
    }
    let key = c.key().clone();
    let mut details = json!({});
    let mut u = wdb::Update {
        url: url.clone(),
        event_types_json: events
            .as_ref()
            .map(|e| serde_json::to_string(e).unwrap_or_default()),
        identity_ids_json: identity_ids
            .map(|i| i.map(|i| serde_json::to_string(&i).unwrap_or_default())),
        description,
        enabled: None,
        disabled_reason: None,
        reenabled_by: None,
        clear_headers: false,
    };
    if let Some(new_url) = url.as_deref().filter(|_| url_changed) {
        u.enabled = Some(false);
        u.disabled_reason = Some(Some("url_changed".into()));
        details["from_host"] = json!(host_of(&row.url));
        details["to_host"] = json!(host_of(new_url));
        u.clear_headers = host_of(new_url) != host_of(&row.url);
    } else {
        match enabled {
            Some(true) if row.enabled == 0 => {
                u.enabled = Some(true);
                u.disabled_reason = Some(None);
                u.reenabled_by = Some((key.key_id.clone(), key.created_by_user_id.clone()));
                details["enabled"] = json!(true);
            }
            Some(false) if row.enabled == 1 => {
                u.enabled = Some(false);
                u.disabled_reason = Some(Some("manual".into()));
                details["enabled"] = json!(false);
            }
            _ => {}
        }
    }
    let update = wdb::update(&row.id, u, c.now);
    c.p.db()
        .batch(vec![
            update,
            audit_stmt(
                c,
                row.tenant_id.as_deref(),
                "webhook.update",
                &row.id,
                details,
            ),
        ])
        .await?;
    let row: Row =
        c.p.db()
            .first(wdb::get(&row.id))
            .await?
            .ok_or_else(not_found)?;
    Ok(Reply::json(200, row.json()))
}

pub async fn delete<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c).await?;
    c.p.db()
        .batch(vec![
            Stmt::new(
                "DELETE FROM webhook_deliveries WHERE endpoint_id = ?1",
                vec![row.id.clone().into()],
            ),
            Stmt::new(
                "DELETE FROM webhook_held WHERE endpoint_id = ?1",
                vec![row.id.clone().into()],
            ),
            Stmt::new(
                "DELETE FROM webhook_endpoints WHERE id = ?1",
                vec![row.id.clone().into()],
            ),
            audit_stmt(
                c,
                row.tenant_id.as_deref(),
                "webhook.delete",
                &row.id,
                json!({}),
            ),
        ])
        .await?;
    Ok(Reply::no_content())
}

pub async fn rotate_secret<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c).await?;
    let body = c.json()?;
    only(&body, &["overlap_hours"])?;
    let overlap = match &body["overlap_hours"] {
        Value::Null => 24,
        v => match v.as_i64() {
            Some(h @ 0..=168) => h,
            _ => return Err(ApiError::invalid("overlap_hours", "0 to 168.")),
        },
    };
    let current =
        client::open(c.p.as_ref(), "secret_enc", &row.id, &row.secret_enc).ok_or_else(|| {
            ApiError::new(
                ErrorCode::Unavailable,
                "The current secret cannot be read; retry.",
            )
        })?;
    let (prev, until) = if overlap == 0 {
        (None, None)
    } else {
        (
            Some(client::seal(
                c.p.as_ref(),
                "prev_secret_enc",
                &row.id,
                &current,
            )),
            Some(c.now + overlap * 3_600_000),
        )
    };
    let (secret, secret_enc) = new_secret(c, &row.id);
    c.p.db()
        .batch(vec![
            Stmt::new(
                "UPDATE webhook_endpoints SET secret_enc = ?2, prev_secret_enc = ?3, prev_secret_expires_at = ?4, updated_at = ?5 WHERE id = ?1",
                vec![row.id.clone().into(), secret_enc.into(), prev.into(), until.into(), c.now.into()],
            ),
            audit_stmt(c, row.tenant_id.as_deref(), "webhook.rotate_secret", &row.id, json!({ "overlap_hours": overlap })),
        ])
        .await?;
    let row: Row =
        c.p.db()
            .first(wdb::get(&row.id))
            .await?
            .ok_or_else(not_found)?;
    let mut v = row.json();
    v["secret"] = json!(secret);
    Ok(Reply::json(200, v))
}

/// `POST …/test`: a `webhook.test` platform event, one synchronous attempt, recorded and returned.
pub async fn test<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c).await?;
    let event_id = c.p.ids().new_id(IdPrefix::Event);
    let body = envelope(
        &event_id,
        "webhook.test",
        c.now,
        row.tenant_id.as_deref(),
        None,
        None,
        &json!({ "message": "hello" }),
    );
    c.p.db()
        .execute(Stmt::new(
            "INSERT INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, partner_id, payload_json, occurred_at, fanned_out_at) VALUES (?1, ?2, NULL, 'webhook.test', 'platform', 'platform', ?3, ?4, ?5, ?5)",
            vec![
                event_id.clone().into(),
                row.tenant_id.clone().into(),
                row.owner_partner_id.clone().into(),
                body.clone().into(),
                c.now.into(),
            ],
        ))
        .await?;
    let outcome = client::attempt(c.p.as_ref(), &row.endpoint(), &event_id, body.as_bytes()).await;
    let delivery_id = c.p.ids().new_id(IdPrefix::Delivery);
    let status = if outcome.succeeded {
        "succeeded"
    } else {
        "dead"
    };
    let mut stmts = vec![wdb::record(
        &delivery_id,
        &row.id,
        row.tenant_id.as_deref(),
        &event_id,
        "webhook.test",
        1,
        status,
        outcome.http_status,
        outcome.error,
        outcome.duration_ms,
        None,
        c.now,
    )];
    if outcome.succeeded && row.disabled_reason.as_deref() == Some("url_changed") {
        stmts.push(Stmt::new(
            "UPDATE webhook_endpoints SET enabled = 1, disabled_reason = NULL, consecutive_failures = 0, updated_at = ?2 WHERE id = ?1 AND disabled_reason = 'url_changed'",
            vec![row.id.clone().into(), c.now.into()],
        ));
    }
    c.p.db().batch(stmts).await?;
    let d: wdb::Delivery =
        c.p.db()
            .first(wdb::delivery(&delivery_id))
            .await?
            .ok_or_else(ApiError::internal)?;
    Ok(Reply::json(200, d.json()))
}

const DELIVERY_FILTERS: &[&str] = &["status", "event_type", "after"];

pub async fn deliveries<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c).await?;
    let limit = c.limit()?;
    let cursor = c.after(DELIVERY_FILTERS).await?;
    let status = match c.query("status") {
        None => None,
        Some(s @ ("succeeded" | "failed" | "dead")) => Some(s.to_owned()),
        Some(_) => return Err(ApiError::invalid("status", "succeeded, failed or dead.")),
    };
    let after = match c.query("after") {
        None => None,
        Some(a) => {
            Some(parse_rfc3339(a).ok_or_else(|| ApiError::invalid("after", "An RFC 3339 time."))?)
        }
    };
    let rows: Vec<wdb::Delivery> =
        c.p.db()
            .query(wdb::deliveries(
                &row.id,
                status,
                c.query("event_type").map(str::to_owned),
                after,
                cursor,
                limit,
            ))
            .await?;
    c.page(
        rows,
        limit,
        DELIVERY_FILTERS,
        |d| (d.created_at, d.id.clone()),
        wdb::Delivery::json,
    )
    .await
}

pub async fn replay<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let row = in_scope(c).await?;
    check_mail_permissions(c, &row.event_types())?;
    let body = c.json()?;
    only(&body, &["event_ids", "since", "until", "status"])?;
    let sel = if !body["event_ids"].is_null() {
        if !body["since"].is_null() || !body["until"].is_null() || !body["status"].is_null() {
            return Err(ApiError::invalid(
                "event_ids",
                "Either event_ids, or a time range.",
            ));
        }
        let ids: Vec<String> = body["event_ids"]
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= 100)
            .ok_or_else(|| ApiError::invalid("event_ids", "1 to 100 event IDs."))?
            .iter()
            .map(|v| {
                v.as_str()
                    .filter(|s| s.starts_with("evt_"))
                    .map(str::to_owned)
            })
            .collect::<Option<_>>()
            .ok_or_else(|| ApiError::invalid("event_ids", "Event IDs."))?;
        Selection::Ids(ids)
    } else {
        let time = |name: &str| -> Result<Option<i64>, ApiError> {
            match &body[name] {
                Value::Null => Ok(None),
                v => v
                    .as_str()
                    .and_then(parse_rfc3339)
                    .map(Some)
                    .ok_or_else(|| ApiError::invalid(name, "An RFC 3339 time.")),
            }
        };
        let since =
            time("since")?.ok_or_else(|| ApiError::invalid("since", "since is required."))?;
        let status = match &body["status"] {
            Value::Null => None,
            Value::String(s) if matches!(s.as_str(), "succeeded" | "failed" | "dead") => {
                Some(s.clone())
            }
            _ => return Err(ApiError::invalid("status", "succeeded, failed or dead.")),
        };
        Selection::Window {
            since,
            until: time("until")?,
            status,
        }
    };
    // 30 days, or the tenant's `retention.events_days` when shorter.
    let mut window = replay::WINDOW_MS;
    if let Some(t) = &row.tenant_id
        && let Some(t) =
            c.p.db()
                .first::<crate::db::tenants::Row>(crate::db::tenants::get(t))
                .await?
    {
        let eff = crate::policy::effective(c.p.as_ref(), &t);
        if let Some(d) = pylota_identity_core::policy::get_path(&eff, "retention.events_days")
            .and_then(Value::as_i64)
        {
            window = window.min(d * 86_400_000);
        }
    }
    let queued = match replay::replay(c.p.as_ref(), &row.matching(), &sel, window, c.now).await? {
        Ok(n) => n,
        Err(_) => {
            return Err(ApiError::invalid(
                "since",
                "More than 1,000 events match; narrow the window.",
            ));
        }
    };
    c.p.db()
        .execute(audit_stmt(
            c,
            row.tenant_id.as_deref(),
            "webhook.replay",
            &row.id,
            json!({ "queued": queued }),
        ))
        .await?;
    Ok(Reply::json(202, json!({ "queued": queued })))
}
