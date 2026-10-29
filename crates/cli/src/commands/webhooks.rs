//! `webhooks` ([CLI and setup §16, §18]): endpoints, deliveries, replay and offline verification.
//!
//! [CLI and setup §16, §18]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#16-webhooks-verify

use pylota_identity::webhooks::{VerifyError, verify_with};
use serde_json::{Value, json};

use super::{Obj, csv, paging, q_opt, read_file, time};
use crate::cli::WebhooksCmd;
use crate::http::{Idem, seg};
use crate::output::{Col, col, pretty};
use crate::{CliError, Ctx, Output, resolve};

const WEBHOOK_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("SCOPE", "scope", 8),
    col("URL", "url", 44),
    col("EVENTS", "events", 40),
    col("ENABLED", "enabled", 7),
];

const DELIVERY_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("TIME", "created_at", 20),
    col("EVENT", "event_type", 24),
    col("ATTEMPT", "attempt", 7),
    col("STATUS", "status", 9),
    col("HTTP", "http_status", 4),
];

/// An endpoint with its new secret on its own line, shown once.
fn with_secret(w: Value) -> Output {
    let secret = w["secret"].as_str().unwrap_or("").to_owned();
    let mut shown = w.clone();
    if let Some(o) = shown.as_object_mut() {
        o.remove("secret");
    }
    Output::object(w)
        .human(format!("{}{secret}\n", pretty(&shown)))
        .quiet(format!("{secret}\n"))
        .note("The signing secret above is shown only once.")
}

pub async fn run(ctx: &Ctx, c: WebhooksCmd) -> Result<Output, CliError> {
    if let WebhooksCmd::Verify {
        secret_env,
        secret,
        headers,
        id,
        timestamp,
        signature,
        body,
        tolerance,
        now,
    } = c
    {
        return verify(
            ctx,
            secret_env,
            secret,
            headers,
            (id, timestamp, signature),
            body,
            tolerance,
            now,
        );
    }
    let api = ctx.api()?;
    match c {
        WebhooksCmd::Create {
            url,
            events,
            identity_ids,
            description,
            platform,
            partner,
        } => {
            let body = Obj::new()
                .set("url", url)
                .set("events", csv(&events))
                .opt("identity_ids", identity_ids.map(|i| csv(&i)))
                .opt("description", description)
                .value();
            let path = if platform || partner {
                let level = resolve::level(ctx).await?;
                let want = if platform { "platform" } else { "partner" };
                if level != want {
                    return Err(CliError::usage(format!(
                        "--{want} needs a {want} key; this key is a {level} key."
                    )));
                }
                "/v1/webhooks".to_owned()
            } else {
                format!("/v1/tenants/{}/webhooks", seg(&resolve::tenant(ctx).await?))
            };
            Ok(with_secret(
                api.call(ctx, "POST", &path, &[], Some(&body), Idem::Generated)
                    .await?,
            ))
        }
        WebhooksCmd::List { paging: p } => {
            let mut q = Vec::new();
            paging(&p, &mut q);
            let path = if ctx.global.tenant.is_some() {
                format!("/v1/tenants/{}/webhooks", seg(&resolve::tenant(ctx).await?))
            } else {
                "/v1/webhooks".to_owned()
            };
            Ok(Output::list(
                api.list(ctx, &path, &q, p.all).await?,
                WEBHOOK_COLS,
            ))
        }
        WebhooksCmd::Get { webhook } => Ok(Output::object(
            api.get(ctx, &format!("/v1/webhooks/{}", seg(&webhook)), &[])
                .await?,
        )),
        WebhooksCmd::Update {
            webhook,
            url,
            events,
            identity_ids,
            description,
            enable,
            disable,
        } => {
            let mut body = Obj::new()
                .opt("url", url)
                .opt("events", events.map(|e| csv(&e)))
                .opt("identity_ids", identity_ids.map(|i| csv(&i)))
                .opt("description", description);
            if enable || disable {
                body = body.set("enabled", enable);
            }
            if body.is_empty() {
                return Err(CliError::usage("Nothing to change."));
            }
            Ok(Output::object(
                api.call(
                    ctx,
                    "PATCH",
                    &format!("/v1/webhooks/{}", seg(&webhook)),
                    &[],
                    Some(&body.value()),
                    Idem::None,
                )
                .await?,
            ))
        }
        WebhooksCmd::Delete { webhook } => {
            ctx.confirm(&format!("Delete {webhook}?"))?;
            Ok(Output::object(
                api.call(
                    ctx,
                    "DELETE",
                    &format!("/v1/webhooks/{}", seg(&webhook)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?,
            ))
        }
        WebhooksCmd::Rotate {
            webhook,
            overlap_hours,
        } => {
            let body = Obj::new().opt("overlap_hours", overlap_hours).value();
            Ok(with_secret(
                api.call(
                    ctx,
                    "POST",
                    &format!("/v1/webhooks/{}/rotate-secret", seg(&webhook)),
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        WebhooksCmd::Test { webhook } => Ok(Output::object(
            api.call(
                ctx,
                "POST",
                &format!("/v1/webhooks/{}/test", seg(&webhook)),
                &[],
                None,
                Idem::Generated,
            )
            .await?,
        )),
        WebhooksCmd::Deliveries {
            webhook,
            status,
            event_type,
            after,
            paging: p,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "status", status);
            q_opt(&mut q, "event_type", event_type);
            if let Some(t) = after {
                q.push(("after", time(&t)?));
            }
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(
                    ctx,
                    &format!("/v1/webhooks/{}/deliveries", seg(&webhook)),
                    &q,
                    p.all,
                )
                .await?,
                DELIVERY_COLS,
            ))
        }
        WebhooksCmd::Replay {
            webhook,
            event_ids,
            since,
            until,
            status,
        } => {
            let body = match (event_ids.is_empty(), since) {
                (false, None) if until.is_none() && status.is_none() => {
                    json!({ "event_ids": event_ids })
                }
                (true, Some(s)) => Obj::new()
                    .set("since", time(&s)?)
                    .opt("until", until.map(|u| time(&u)).transpose()?)
                    .opt("status", status)
                    .value(),
                _ => {
                    return Err(CliError::usage(
                        "Pass --event-id (repeatable), or --since [--until] [--status].",
                    ));
                }
            };
            Ok(Output::object(
                api.call(
                    ctx,
                    "POST",
                    &format!("/v1/webhooks/{}/replay", seg(&webhook)),
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        WebhooksCmd::Verify { .. } => unreachable!("handled above"),
    }
}

/// `idvy webhooks verify`: offline, with the receiving guide's rules; exit 11 when it does not verify.
#[allow(clippy::too_many_arguments)]
fn verify(
    ctx: &Ctx,
    secret_env: Option<String>,
    secret: Option<String>,
    headers_file: Option<String>,
    flags: (Option<String>, Option<String>, Option<String>),
    body: Option<String>,
    tolerance: i64,
    now: Option<i64>,
) -> Result<Output, CliError> {
    let secret = match (secret_env, secret) {
        (Some(var), _) => ctx
            .var(&var)
            .map(str::to_owned)
            .ok_or_else(|| CliError::config(format!("{var} is not set.")))?,
        (None, Some(s)) => {
            ctx.note("warning: --secret is visible to other users in the process list; prefer --secret-env.");
            s
        }
        (None, None) => return Err(CliError::usage("Pass --secret-env <NAME>.")),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    if let Some(f) = headers_file {
        let text = String::from_utf8_lossy(&read_file(ctx, &f)?).into_owned();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once(':') {
                headers.push((k.trim().to_owned(), v.trim().to_owned()));
            }
        }
    } else {
        let (id, ts, sig) = flags;
        for (k, v) in [
            ("webhook-id", id),
            ("webhook-timestamp", ts),
            ("webhook-signature", sig),
        ] {
            if let Some(v) = v {
                headers.push((k.to_owned(), v));
            }
        }
    }
    let body = match body {
        Some(f) => read_file(ctx, &f)?,
        None => ctx.read_stdin()?,
    };
    let now = now.unwrap_or(ctx.now_ms() / 1000);
    let pairs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    match verify_with(&secret, &pairs, &body, now, tolerance) {
        Ok(()) => {
            let event_id = serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| v["id"].as_str().map(str::to_owned));
            Ok(
                Output::object(json!({ "valid": true, "matched": 1, "event_id": event_id }))
                    .human("valid\n")
                    .quiet("valid\n"),
            )
        }
        Err(e) => {
            let reason = match e {
                VerifyError::MissingHeader(_) => "missing_header",
                VerifyError::BadSecret => "malformed_secret",
                VerifyError::Timestamp => "timestamp_out_of_tolerance",
                VerifyError::Signature => "no_matching_signature",
            };
            Err(CliError::verification(format!("invalid: {reason}"))
                .with_details(json!({ "valid": false, "reason": reason })))
        }
    }
}
