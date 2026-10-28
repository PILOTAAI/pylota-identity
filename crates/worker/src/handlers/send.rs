//! Sending ([Outbound › Handler]): `send`, `reply`, `reply_all` and `forward` (with `?dry_run=true`),
//! and `cancel` and `resolve`. The handler authenticates, rate-limits, checks the `Idempotency-Key`,
//! fingerprints the body and loads the D1 context; the mailbox does the rest, idempotency first.
//!
//! [Outbound › Handler]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::policy::{SendOp, send_fingerprint};
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{Clock as _, ControlDb as _, Ids as _, ObjectClient as _, Platform};
use serde_json::{Value, json};

use super::identity_in_scope;
use super::messages::review;
use crate::ctx::{Ctx, Reply};
use crate::db::identities as idb;
use crate::errors::ApiError;
use crate::mailbox::MailboxRequest;
use crate::mailbox::submit::SubmitInput;
use crate::quota::{self, QuotaRequest};
use crate::rpc::RpcResult;

/// The largest send body (7 MiB).
const MAX_SEND_BODY: usize = 7 * 1024 * 1024;
/// The submit call's deadline.
const SUBMIT_DEADLINE_MS: i64 = 30_000;

/// The fields of a reply or reply-all body.
const REPLY_FIELDS: &[&str] = &[
    "text",
    "html",
    "attachments",
    "kind",
    "unsubscribe",
    "consent",
    "labels",
    "headers",
    "calendar",
    "metadata",
];
const FORWARD_FIELDS: &[&str] = &[
    "to",
    "cc",
    "bcc",
    "text",
    "html",
    "include_attachments",
    "attachments",
    "labels",
    "headers",
    "metadata",
];
const SEND_FIELDS: &[&str] = &[
    "to",
    "cc",
    "bcc",
    "subject",
    "text",
    "html",
    "attachments",
    "kind",
    "unsubscribe",
    "consent",
    "thread_id",
    "from_address",
    "labels",
    "headers",
    "calendar",
    "metadata",
];

/// One mailbox call with the identity's scope; an error answer becomes the same API error.
pub async fn call<P: Platform>(
    c: &Ctx<P>,
    idn: &idb::Row,
    req: MailboxRequest,
    deadline_ms: i64,
) -> Result<Value, ApiError> {
    let mut env = crate::rpc::envelope_for(
        c.p.as_ref(),
        Some(&idn.tenant_id),
        Some(&idn.id),
        &c.request_id,
        Some(&c.key().key_id),
        req,
    );
    env.deadline_ms = c.p.clock().now_ms() + deadline_ms;
    match c
        .p
        .objects()
        .call::<_, Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
        .await?
    {
        RpcResult::Ok(v) => Ok(v),
        RpcResult::Err(e) => {
            let mut err = ApiError::new(e.error.code, e.error.message);
            if let Some(d) = e.error.details {
                err = err.details(serde_json::to_value(d).unwrap_or(Value::Null));
            }
            Err(err)
        }
    }
}

fn body<P: Platform>(c: &Ctx<P>, allowed: &[&str]) -> Result<Value, ApiError> {
    let raw = c.body.as_deref().unwrap_or(b"{}");
    if raw.len() > MAX_SEND_BODY {
        return Err(ApiError::new(
            ErrorCode::PayloadTooLarge,
            "The body is over 7 MiB.",
        ));
    }
    let v: Value = serde_json::from_slice(raw)
        .map_err(|e| ApiError::invalid("", format!("The body is not valid JSON: {e}")))?;
    let o = v
        .as_object()
        .ok_or_else(|| ApiError::invalid("", "The body must be a JSON object."))?;
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(ApiError::invalid(k, "Unknown field."));
    }
    for f in ["to", "cc", "bcc", "attachments", "labels"] {
        if !matches!(o.get(f), None | Some(Value::Array(_)) | Some(Value::Null)) {
            return Err(ApiError::invalid(f, "An array."));
        }
    }
    for f in [
        "subject",
        "text",
        "html",
        "kind",
        "thread_id",
        "from_address",
    ] {
        if !matches!(o.get(f), None | Some(Value::String(_)) | Some(Value::Null)) {
            return Err(ApiError::invalid(f, "A string."));
        }
    }
    if let Some(k) = o.get("kind").and_then(Value::as_str)
        && !matches!(k, "transactional" | "marketing" | "auto_reply")
    {
        return Err(ApiError::invalid(
            "kind",
            "transactional, marketing or auto_reply.",
        ));
    }
    if o.get("calendar").is_some_and(|c| !c.is_null()) {
        return Err(ApiError::new(
            ErrorCode::TransportUnavailable,
            "Calendar sends are not available yet.",
        )
        .details(json!({ "reason": "calendar_needs_raw_mime" })));
    }
    Ok(v)
}

/// `send`, `reply`, `reply_all`, `forward`.
pub async fn send<P: Platform>(c: &Ctx<P>, op: SendOp) -> Result<Reply, ApiError> {
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    crate::ratelimit::check(c.p.as_ref(), RlBucket::Send, &idn.id).await?;
    let dry_run = c.query("dry_run") == Some("true");
    let key = c.header("idempotency-key");
    let key_hash = match key {
        None if dry_run => None,
        None => {
            return Err(ApiError::new(
                ErrorCode::IdempotencyKeyRequired,
                "A send needs an Idempotency-Key.",
            ));
        }
        Some(k) => {
            if k.is_empty() || k.len() > 255 || !k.bytes().all(|b| (0x20..0x7f).contains(&b)) {
                return Err(ApiError::new(
                    ErrorCode::InvalidIdempotencyKey,
                    "An Idempotency-Key is 1 to 255 printable ASCII characters.",
                ));
            }
            Some(pylota_identity_core::ses::sha256_hex(k.as_bytes()))
        }
    };
    let allowed = match op {
        SendOp::Send => SEND_FIELDS,
        SendOp::Forward => FORWARD_FIELDS,
        _ => REPLY_FIELDS,
    };
    let b = body(c, allowed)?;
    let target = match op {
        SendOp::Send => None,
        _ => Some(c.route.param("message_id").to_owned()),
    };
    if matches!(op, SendOp::Send | SendOp::Forward)
        && !b["to"].as_array().is_some_and(|a| !a.is_empty())
    {
        return Err(ApiError::invalid("to", "At least one recipient."));
    }
    if op == SendOp::Send && !b["subject"].is_string() {
        return Err(ApiError::invalid("subject", "A subject is required."));
    }
    let fp_target = match &target {
        Some(m) => format!("{}/{m}", idn.id),
        None => idn.id.clone(),
    };
    let ctx = crate::send::context::load(c.p.as_ref(), &idn, &t).await?;
    let inp = SubmitInput {
        operation: op.as_str().to_owned(),
        target,
        key_hash,
        fingerprint: send_fingerprint(op, &fp_target, &b),
        dry_run,
        review: review(c),
        body: b,
        ctx,
        request_id: c.request_id.clone(),
    };
    let out = call(
        c,
        &idn,
        MailboxRequest::Submit(Box::new(inp)),
        SUBMIT_DEADLINE_MS,
    )
    .await?;
    if let Some(d) = out.get("dry_run") {
        return Ok(Reply::json(200, d.clone()));
    }
    let mut reply = Reply::json(202, out["message"].clone());
    if out["replayed"] == true {
        reply
            .headers
            .push(("idempotent-replayed".into(), "true".into()));
    }
    Ok(reply)
}

/// `POST …/messages/{message_id}/cancel`.
pub async fn cancel<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let msg = c.route.param("message_id").to_owned();
    let out = call(
        c,
        &idn,
        MailboxRequest::Cancel {
            message_id: msg.clone(),
        },
        10_000,
    )
    .await?;
    if let Some(day) = out["day"].as_str() {
        let _ = quota::call(
            c.p.as_ref(),
            &t.quota_do_id,
            &t.id,
            &c.request_id,
            QuotaRequest::Release {
                identity_id: idn.id.clone(),
                day: day.to_owned(),
                tenant_counted: out["tenant_counted"].as_bool().unwrap_or(true),
            },
        )
        .await;
    }
    let _ = quota::call(
        c.p.as_ref(),
        &t.quota_do_id,
        &t.id,
        &c.request_id,
        QuotaRequest::Settle {
            feature: quota::Feature::Sends,
            r#ref: msg.clone(),
            consume: 0,
            keep: 0,
        },
    )
    .await;
    message(c, &idn, &msg).await
}

/// `POST …/messages/{message_id}/resolve` (FR-DLV-7).
pub async fn resolve<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let b = c.json()?;
    super::messages::only_fields(&b, &["outcome"])?;
    let outcome = match b["outcome"].as_str() {
        Some(o @ ("sent" | "not_sent")) => o.to_owned(),
        _ => return Err(ApiError::invalid("outcome", "sent or not_sent.")),
    };
    let msg = c.route.param("message_id").to_owned();
    let out = call(
        c,
        &idn,
        MailboxRequest::Resolve {
            message_id: msg.clone(),
            outcome: outcome.clone(),
        },
        10_000,
    )
    .await?;
    let consume = out["consume"].as_u64().unwrap_or(0) as u32;
    if consume > 0 {
        let _ = quota::call(
            c.p.as_ref(),
            &t.quota_do_id,
            &t.id,
            &c.request_id,
            QuotaRequest::Settle {
                feature: quota::Feature::Sends,
                r#ref: msg.clone(),
                consume,
                keep: 0,
            },
        )
        .await;
    }
    c.p.db()
        .execute(crate::db::audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            Some(&t.id),
            Some(&c.key().key_id),
            "message.resolve",
            "message",
            &msg,
            &json!({ "outcome": outcome }),
            &c.request_id,
            c.now,
        ))
        .await?;
    message(c, &idn, &msg).await
}

async fn message<P: Platform>(c: &Ctx<P>, idn: &idb::Row, msg: &str) -> Result<Reply, ApiError> {
    let v = super::messages::mailbox(
        c,
        idn,
        crate::mailbox::read::ReadRequest::GetMessage {
            message_id: msg.to_owned(),
            review: true,
            include: crate::mailbox::read::Include::default(),
        },
    )
    .await?;
    Ok(Reply::json(200, v))
}
