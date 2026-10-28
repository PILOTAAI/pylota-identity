//! The test hooks of `cargo xtask itest` (Testing § 6.2): compiled only with `itest-hooks`, honoured
//! only with `PM_ENV = local`, and refused without `x-pm-test-token: <PM_ITEST_TOKEN>`.

use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::app::HttpRequestIn;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{ControlDb as _, ObjectClient as _, Platform};
use serde_json::{Value, json};

use crate::ctx::Reply;
use crate::errors::ApiError;
use crate::router::{ROUTES, Scope};
use crate::rpc::RpcEnvelope;

fn not_found() -> ApiError {
    ApiError::new(ErrorCode::RouteNotFound, "No route matches this path.")
}

pub async fn serve<P: Platform>(
    p: &Rc<P>,
    req: &HttpRequestIn,
    _request_id: &str,
    path: &str,
) -> Result<Reply, ApiError> {
    let cfg = p.config();
    let token = cfg.itest_token.as_deref().ok_or_else(not_found)?;
    if req.header("x-pm-test-token") != Some(token) {
        return Err(not_found());
    }
    match (req.method.as_str(), path) {
        ("GET", "/__test/routes") => {
            let routes: Vec<Value> = ROUTES
                .iter()
                .map(|r| {
                    json!({
                        "method": r.method.as_str().to_ascii_uppercase(), "pattern": r.pattern, "operation_id": r.operation_id,
                        "permissions": r.permissions, "foreign_permissions": r.foreign_permissions,
                        "scope": match r.scope {
                            Scope::Public => "public", Scope::AnyKey => "any_key", Scope::PlatformOnly => "platform_only",
                            Scope::PlatformOrPartner => "platform_or_partner", Scope::TenantPath { .. } => "tenant_path",
                            Scope::Resource { .. } => "resource", Scope::TenantFilter => "tenant_filter",
                        },
                        "levels": r.levels.iter().map(|l| l.as_str()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            Ok(Reply::json(200, json!({ "routes": routes })))
        }
        ("POST", "/__test/rpc") => {
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            let class = match body["class"].as_str() {
                Some("quota") => DoClass::Quota,
                Some("mailbox") => DoClass::Mailbox,
                _ => return Err(ApiError::invalid("class", "quota or mailbox.")),
            };
            let env: RpcEnvelope<Value> = serde_json::from_value(body["envelope"].clone())
                .map_err(|e| ApiError::invalid("envelope", e.to_string()))?;
            let out = p
                .objects()
                .call::<_, Value>(class, body["object_id"].as_str().unwrap_or(""), &env)
                .await?;
            Ok(Reply::json(
                200,
                serde_json::to_value(out).unwrap_or(Value::Null),
            ))
        }
        ("POST", "/__test/mailbox-schema" | "/__test/alarm") => {
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            let id = body["identity_id"].as_str().unwrap_or("");
            let row: crate::db::identities::Row = p
                .db()
                .first(crate::db::identities::get(id))
                .await?
                .ok_or_else(|| ApiError::not_found(ErrorCode::IdentityNotFound))?;
            let req = if path == "/__test/alarm" {
                crate::mailbox::MailboxRequest::TestAlarm
            } else {
                crate::mailbox::MailboxRequest::TestSchemaBack
            };
            let env = crate::rpc::envelope_for(
                p.as_ref(),
                Some(&row.tenant_id),
                Some(&row.id),
                _request_id,
                None,
                req,
            );
            let out = p
                .objects()
                .call::<_, Value>(DoClass::Mailbox, &row.mailbox_do_id, &env)
                .await?;
            Ok(Reply::json(
                200,
                serde_json::to_value(out).unwrap_or(Value::Null),
            ))
        }
        _ => Err(not_found()),
    }
}

/// The itest build's queue consumer: each message is posted to `{fakes}/queues/{queue}` and acked.
pub async fn capture_queue<P: Platform>(
    p: &P,
    queue: pylota_identity_platform::queues::QueueName,
    batch: Vec<pylota_identity_platform::queues::Incoming<Value>>,
) {
    use pylota_identity_platform::HttpClient as _;
    use pylota_identity_platform::http::HttpRequest;
    let Some(fakes) = p.config().itest_fakes_url.clone() else {
        return;
    };
    for m in &batch {
        let mut req = HttpRequest::get(format!("{fakes}/queues/{}", queue.queue_name()));
        req.method = "POST";
        req.timeout_ms = 2_000;
        req.headers
            .push(("content-type".into(), "application/json".into()));
        req.body = Some(m.body.to_string().into_bytes());
        match p.http().send(req).await {
            Ok(_) => m.ack(),
            Err(_) => m.retry(None),
        }
    }
}
