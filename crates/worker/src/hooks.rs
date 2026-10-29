//! The test hooks of `cargo xtask itest` (Testing § 6.2): compiled only with `itest-hooks`, honoured
//! only with `PM_ENV = local`, and refused without `x-pm-test-token: <PM_ITEST_TOKEN>`.

use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::app::HttpRequestIn;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{Clock as _, ControlDb as _, ObjectClient as _, Platform};
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
        ("POST", "/__test/bulk-seed") => {
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            let row: crate::db::identities::Row = p
                .db()
                .first(crate::db::identities::get(
                    body["identity_id"].as_str().unwrap_or(""),
                ))
                .await?
                .ok_or_else(|| ApiError::not_found(ErrorCode::IdentityNotFound))?;
            let start = body["start"].as_u64().unwrap_or(0) as u32;
            let count = body["count"].as_u64().unwrap_or(0).min(50_000) as u32;
            let seed = body["seed"].as_u64().unwrap_or(1);
            let mut rows = Vec::new();
            let mut at = start;
            while at < start + count {
                let n = (start + count - at).min(500);
                let env = crate::rpc::envelope_for(
                    p.as_ref(),
                    Some(&row.tenant_id),
                    Some(&row.id),
                    _request_id,
                    None,
                    crate::mailbox::MailboxRequest::TestBulkSeed {
                        start: at,
                        count: n,
                        seed,
                    },
                );
                match p
                    .objects()
                    .call::<_, Value>(DoClass::Mailbox, &row.mailbox_do_id, &env)
                    .await?
                {
                    crate::rpc::RpcResult::Ok(Value::Array(r)) => rows.extend(r),
                    other => {
                        return Ok(Reply::json(
                            500,
                            serde_json::to_value(other).unwrap_or(Value::Null),
                        ));
                    }
                }
                at += n;
            }
            Ok(Reply::json(200, json!({ "rows": rows })))
        }
        ("POST", "/__test/mailbox-schema" | "/__test/alarm" | "/__test/mailbox-query") => {
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
            let req = match path {
                "/__test/alarm" => crate::mailbox::MailboxRequest::TestAlarm,
                "/__test/mailbox-query" => crate::mailbox::MailboxRequest::TestQuery {
                    sql: body["sql"].as_str().unwrap_or("").to_owned(),
                },
                _ => crate::mailbox::MailboxRequest::TestSchemaBack,
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
        ("POST", "/__test/domain-alarm") => {
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            let id = body["domain_id"].as_str().unwrap_or("").to_owned();
            let row: crate::db::domains::Row = p
                .db()
                .first(crate::db::domains::get(&id))
                .await?
                .ok_or_else(|| ApiError::not_found(ErrorCode::DomainNotFound))?;
            let out = crate::domains::monitor::call(
                p.as_ref(),
                &row.monitor_do_id,
                crate::domains::monitor::DomainRequest::TestAlarm { domain_id: id },
            )
            .await?;
            Ok(Reply::json(200, out))
        }
        ("POST", "/__test/job-alarm") => {
            // Runs one alarm of a job's runner now; answers its meta, steps and next alarm.
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            #[derive(serde::Deserialize)]
            struct J {
                tenant_id: Option<String>,
                runner_do_id: String,
            }
            let j: J = p
                .db()
                .first(pylota_identity_platform::d1::Stmt::new(
                    "SELECT tenant_id, runner_do_id FROM jobs WHERE id = ?1",
                    vec![body["job_id"].as_str().unwrap_or("").into()],
                ))
                .await?
                .ok_or_else(|| ApiError::invalid("job_id", "No such job."))?;
            let env = crate::rpc::envelope_for(
                p.as_ref(),
                Some(j.tenant_id.as_deref().unwrap_or("")),
                None,
                "req_test",
                None,
                crate::jobs::JobRequest::TestAlarm,
            );
            let out = p
                .objects()
                .call::<_, Value>(DoClass::Job, &j.runner_do_id, &env)
                .await?;
            Ok(Reply::json(
                200,
                serde_json::to_value(out).unwrap_or(Value::Null),
            ))
        }
        ("POST", "/__test/queue-send") => {
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            let (q, _) = pylota_identity_platform::queues::QueueName::from_queue_name(
                body["queue"].as_str().unwrap_or(""),
            )
            .ok_or_else(|| ApiError::invalid("queue", "Unknown queue."))?;
            pylota_identity_platform::QueueProducer::send(p.queues(), q, &body["body"], 0).await?;
            Ok(Reply::json(200, json!({})))
        }
        ("POST", "/__test/inbound") => {
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            let raw =
                pylota_identity_platform::base64::decode(body["raw_base64"].as_str().unwrap_or(""))
                    .ok_or_else(|| ApiError::invalid("raw_base64", "Base64 expected."))?;
            // The header names and values workerd's `message.headers` would give.
            let head_end = raw
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .unwrap_or(raw.len());
            let mut headers: Vec<(String, String)> = Vec::new();
            for line in String::from_utf8_lossy(&raw[..head_end]).split("\r\n") {
                if line.starts_with([' ', '\t']) {
                    if let Some(last) = headers.last_mut() {
                        last.1.push_str(line.trim());
                    }
                } else if let Some((k, v)) = line.split_once(':') {
                    headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
                }
            }
            let msg = pylota_identity_platform::app::EmailIn {
                from: body["mail_from"].as_str().unwrap_or("").to_owned(),
                to: body["rcpt_to"].as_str().unwrap_or("").to_owned(),
                headers,
                raw_size: raw.len() as u64,
                raw: pylota_identity_platform::buffer::JsBuffer::from_bytes(&raw),
            };
            let out = match crate::email::handle(p.as_ref(), &msg).await {
                pylota_identity_platform::app::EmailOutcome::Accept => {
                    json!({ "outcome": "accepted" })
                }
                pylota_identity_platform::app::EmailOutcome::Reject { smtp } => {
                    json!({ "outcome": "rejected", "smtp": smtp })
                }
                pylota_identity_platform::app::EmailOutcome::TempFail => {
                    json!({ "outcome": "tempfail" })
                }
            };
            Ok(Reply::json(200, out))
        }
        ("POST", "/__test/ses-acquire" | "/__test/ses-reset") => {
            // One `Acquire` answered as is (no wait), or the bucket emptied.
            use crate::domains::ses_control::{SesCaller, SesControlRequest, object_id};
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .unwrap_or_default();
            let id = object_id(p.as_ref())
                .await
                .map_err(|_| ApiError::internal())?;
            let caller: SesCaller =
                serde_json::from_value(body["caller"].clone()).unwrap_or(SesCaller::DomainCheck);
            let now = p.clock().now_ms();
            let rq = if path == "/__test/ses-reset" {
                SesControlRequest::Reset
            } else {
                SesControlRequest::Acquire {
                    caller,
                    deadline_ms: now + caller.wait_ms(),
                }
            };
            let env = crate::rpc::envelope_for(p.as_ref(), None, None, "req_test", None, rq);
            let out = p
                .objects()
                .call::<_, Value>(DoClass::SesControl, &id, &env)
                .await?;
            Ok(Reply::json(
                200,
                serde_json::to_value(out).unwrap_or(Value::Null),
            ))
        }
        ("POST", "/__test/security-contact") => {
            // `{"value": "a@b"}` overrides `PM_SECURITY_CONTACT`, `{"value": null}` unsets it,
            // `{}` restores the configured value.
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .unwrap_or_default();
            crate::inbound::role_mail::override_contact(
                body.get("value").map(|v| v.as_str().map(str::to_owned)),
            );
            Ok(Reply::json(200, json!({})))
        }
        ("POST", "/__test/d1") => {
            // One statement on D1 with JSON parameters; the SQL is leaked into a static string,
            // which only the test build does.
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            let sql: &'static str = Box::leak(
                body["sql"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned()
                    .into_boxed_str(),
            );
            let params: Vec<pylota_identity_platform::d1::SqlValue> = body["params"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|v| match v {
                    Value::Null => pylota_identity_platform::d1::SqlValue::Null,
                    Value::Number(n) => n.as_i64().unwrap_or(0).into(),
                    Value::String(s) => s.into(),
                    other => other.to_string().into(),
                })
                .collect();
            let rows: Vec<Value> = p
                .db()
                .query(pylota_identity_platform::d1::Stmt::new(sql, params))
                .await?;
            Ok(Reply::json(200, json!({ "rows": rows })))
        }
        ("POST", "/__test/r2") => {
            use pylota_identity_platform::BlobStore as _;
            let body: Value = req
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
                .ok_or_else(|| ApiError::invalid("", "JSON body expected."))?;
            let key = body["key"].as_str().unwrap_or("").to_owned();
            match body["op"].as_str() {
                Some("put") => {
                    let bytes = pylota_identity_platform::base64::decode(
                        body["body_base64"].as_str().unwrap_or(""),
                    )
                    .unwrap_or_default();
                    p.blobs()
                        .put(
                            &key,
                            pylota_identity_platform::r2::BlobBody::Bytes(bytes),
                            &Default::default(),
                        )
                        .await?;
                    Ok(Reply::json(200, json!({})))
                }
                Some("list") => {
                    let page = p.blobs().list(&key, None, 1000).await?;
                    Ok(Reply::json(
                        200,
                        json!({ "keys": page.keys.iter().map(|(k, _)| k).collect::<Vec<_>>() }),
                    ))
                }
                _ => Ok(Reply::json(
                    200,
                    json!({ "size": p.blobs().head(&key).await? }),
                )),
            }
        }
        _ => Err(not_found()),
    }
}

/// The itest build's queue consumer: each message is posted to `{fakes}/queues/{queue}` and acked.
pub async fn capture_queue<P: Platform>(
    p: &P,
    queue: pylota_identity_platform::queues::QueueName,
    dead_letter: bool,
    batch: Vec<pylota_identity_platform::queues::Incoming<Value>>,
) {
    use pylota_identity_platform::HttpClient as _;
    use pylota_identity_platform::http::HttpRequest;
    let Some(fakes) = p.config().itest_fakes_url.clone() else {
        return;
    };
    for m in &batch {
        let dlq = if dead_letter { "-dlq" } else { "" };
        let mut req = HttpRequest::get(format!("{fakes}/queues/{}{dlq}", queue.queue_name()));
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

/// The itest build's queue tap: each message body is posted to `{fakes}/queues/{name}` before the
/// consumer handles it, so the suite sees what went through a queue.
pub async fn tap_queue<P: Platform>(
    p: &P,
    name: &str,
    batch: &[pylota_identity_platform::queues::Incoming<Value>],
) {
    use pylota_identity_platform::HttpClient as _;
    use pylota_identity_platform::http::HttpRequest;
    let Some(fakes) = p.config().itest_fakes_url.clone() else {
        return;
    };
    for m in batch {
        let mut req = HttpRequest::get(format!("{fakes}/queues/{name}"));
        req.method = "POST";
        req.timeout_ms = 2_000;
        req.headers
            .push(("content-type".into(), "application/json".into()));
        req.body = Some(m.body.to_string().into_bytes());
        let _ = p.http().send(req).await;
    }
}
