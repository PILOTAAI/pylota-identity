//! `suppressions`, `lists`, `erasure` and `export` ([CLI and setup §19]).
//!
//! [CLI and setup §19]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#19-command-to-endpoint-map

use std::time::Duration;

use serde_json::{Value, json};

use super::{Obj, paging, q_opt};
use crate::cli::{ErasureCmd, ExportCmd, ListsCmd, SuppressionsCmd};
use crate::http::{Idem, seg};
use crate::output::{Col, col};
use crate::{CliError, Ctx, Output, resolve};

const SUPPRESSION_COLS: &[Col] = &[
    col("ADDRESS", "address_hint", 36),
    col("REASON", "reason", 12),
    col("CREATED", "created_at", 20),
    col("EXPIRES", "expires_at", 20),
];

const LIST_COLS: &[Col] = &[
    col("ENTRY", "entry", 36),
    col("SCOPE", "scope", 9),
    col("NOTE", "note", 30),
    col("CREATED", "created_at", 20),
];

const ERASURE_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("SCOPE", "scope", 12),
    col("STATUS", "status", 20),
    col("CREATED", "created_at", 20),
    col("COMPLETED", "completed_at", 20),
];

pub async fn suppressions(ctx: &Ctx, c: SuppressionsCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let base = format!(
        "/v1/tenants/{}/suppressions",
        seg(&resolve::tenant(ctx).await?)
    );
    match c {
        SuppressionsCmd::List {
            address,
            reason,
            paging: p,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "address", address);
            q_opt(&mut q, "reason", reason);
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, &base, &q, p.all).await?,
                SUPPRESSION_COLS,
            ))
        }
        SuppressionsCmd::Add {
            address,
            reason,
            note,
        } => {
            let body = Obj::new()
                .set("address", address)
                .set("reason", reason)
                .opt("note", note)
                .value();
            Ok(Output::object(
                api.call(ctx, "POST", &base, &[], Some(&body), Idem::Generated)
                    .await?,
            ))
        }
        SuppressionsCmd::Remove {
            address,
            confirm_complaint_removal,
            reason,
        } => {
            let body = Obj::new()
                .flag("confirm_complaint_removal", confirm_complaint_removal)
                .opt("reason", reason)
                .value();
            Ok(Output::object(
                api.call(
                    ctx,
                    "DELETE",
                    &format!("{base}/{}", seg(&address)),
                    &[],
                    Some(&body),
                    Idem::None,
                )
                .await?,
            ))
        }
    }
}

/// The list's base path: the identity's with `--identity`, else the tenant's.
async fn list_base(ctx: &Ctx, direction: &str, kind: &str) -> Result<String, CliError> {
    if ctx.global.identity.is_some() {
        let i = resolve::identity(ctx).await?;
        Ok(format!(
            "/v1/identities/{}/lists/{direction}/{kind}",
            seg(&i)
        ))
    } else {
        let t = resolve::tenant(ctx).await?;
        Ok(format!("/v1/tenants/{}/lists/{direction}/{kind}", seg(&t)))
    }
}

pub async fn lists(ctx: &Ctx, c: ListsCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        ListsCmd::List {
            direction,
            kind,
            paging: p,
        } => {
            let base = list_base(ctx, &direction, &kind).await?;
            let mut q = Vec::new();
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, &base, &q, p.all).await?,
                LIST_COLS,
            ))
        }
        ListsCmd::Add {
            direction,
            kind,
            entry,
        } => {
            let base = list_base(ctx, &direction, &kind).await?;
            Ok(Output::object(
                api.call(
                    ctx,
                    "PUT",
                    &format!("{base}/{}", seg(&entry)),
                    &[],
                    Some(&json!({})),
                    Idem::None,
                )
                .await?,
            ))
        }
        ListsCmd::Remove {
            direction,
            kind,
            entry,
        } => {
            let base = list_base(ctx, &direction, &kind).await?;
            Ok(Output::object(
                api.call(
                    ctx,
                    "DELETE",
                    &format!("{base}/{}", seg(&entry)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?,
            ))
        }
    }
}

/// Polls an erasure until it ends, at most ten minutes (exit 13 after that).
async fn wait_erasure(ctx: &Ctx, id: &str) -> Result<Value, CliError> {
    let api = ctx.api()?;
    for _ in 0..300 {
        let e = api
            .get(ctx, &format!("/v1/erasure-requests/{}", seg(id)), &[])
            .await?;
        if matches!(
            e["status"].as_str(),
            Some("completed" | "completed_with_holds" | "failed")
        ) {
            return Ok(e);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err(
        CliError::timeout(format!("{id} did not finish within ten minutes."))
            .with_fix(format!("idvy erasure get {id}")),
    )
}

pub async fn erasure(ctx: &Ctx, c: ErasureCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        ErasureCmd::Create {
            scope,
            reason,
            message,
            thread,
            address,
            wait,
        } => {
            let tenant = resolve::tenant(ctx).await?;
            let mut body = Obj::new()
                .set("tenant_id", tenant)
                .set("scope", scope.clone())
                .set("reason", reason);
            let need = |v: Option<String>, flag: &str| {
                v.ok_or_else(|| CliError::usage(format!("--scope {scope} needs {flag}.")))
            };
            match scope.as_str() {
                "message" => {
                    body = body
                        .set("identity_id", resolve::identity(ctx).await?)
                        .set("message_id", need(message, "--message")?);
                }
                "thread" => {
                    body = body
                        .set("identity_id", resolve::identity(ctx).await?)
                        .set("thread_id", need(thread, "--thread")?);
                }
                "counterparty" => {
                    body = body.set("counterparty_address", need(address, "--address")?)
                }
                "identity" => body = body.set("identity_id", resolve::identity(ctx).await?),
                _ => {}
            }
            let e = api
                .call(
                    ctx,
                    "POST",
                    "/v1/erasure-requests",
                    &[],
                    Some(&body.value()),
                    Idem::Generated,
                )
                .await?;
            if wait {
                let id = e["id"].as_str().unwrap_or("").to_owned();
                return Ok(Output::object(wait_erasure(ctx, &id).await?));
            }
            Ok(Output::object(e))
        }
        ErasureCmd::Get { erasure } => Ok(Output::object(
            api.get(ctx, &format!("/v1/erasure-requests/{}", seg(&erasure)), &[])
                .await?,
        )),
        ErasureCmd::List { status, paging: p } => {
            let mut q = vec![("tenant_id", resolve::tenant(ctx).await?)];
            q_opt(&mut q, "status", status);
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, "/v1/erasure-requests", &q, p.all).await?,
                ERASURE_COLS,
            ))
        }
        ErasureCmd::Retry { erasure, address } => {
            let e = api
                .get(ctx, &format!("/v1/erasure-requests/{}", seg(&erasure)), &[])
                .await?;
            let scope = e["scope"].as_str().unwrap_or("").to_owned();
            let id = e["id"].as_str().unwrap_or(&erasure).to_owned();
            let mut body = Obj::new()
                .set("tenant_id", e["tenant_id"].clone())
                .set("scope", scope.clone())
                .set("reason", format!("retry:{id}"));
            match scope.as_str() {
                "message" => {
                    body = body
                        .set("identity_id", e["identity_id"].clone())
                        .set("message_id", e["target_id"].clone())
                }
                "thread" => {
                    body = body
                        .set("identity_id", e["identity_id"].clone())
                        .set("thread_id", e["target_id"].clone())
                }
                "counterparty" => {
                    let a = address.ok_or_else(|| {
                        CliError::usage(
                            "A counterparty erasure stores only the address's hash: pass --address.",
                        )
                    })?;
                    body = body.set("counterparty_address", a)
                }
                "identity" => body = body.set("identity_id", e["identity_id"].clone()),
                _ => {}
            }
            Ok(Output::object(
                api.call(
                    ctx,
                    "POST",
                    "/v1/erasure-requests",
                    &[],
                    Some(&body.value()),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        ErasureCmd::Reapply { erasure } => Ok(Output::object(
            api.call(
                ctx,
                "POST",
                &format!("/v1/platform/erasure-requests/{}/reapply", seg(&erasure)),
                &[],
                None,
                Idem::Generated,
            )
            .await?,
        )),
    }
}

pub async fn export(ctx: &Ctx, c: ExportCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        ExportCmd::Create { scope, address } => {
            let mut body = Obj::new()
                .set("tenant_id", resolve::tenant(ctx).await?)
                .set("scope", scope.clone());
            body = if scope == "counterparty" {
                body.set(
                    "counterparty_address",
                    address
                        .ok_or_else(|| CliError::usage("--scope counterparty needs --address."))?,
                )
            } else {
                body.set("identity_id", resolve::identity(ctx).await?)
            };
            Ok(Output::object(
                api.call(
                    ctx,
                    "POST",
                    "/v1/exports",
                    &[],
                    Some(&body.value()),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        ExportCmd::Get { export, download } => {
            let e = api
                .get(ctx, &format!("/v1/exports/{}", seg(&export)), &[])
                .await?;
            let Some(file) = download else {
                return Ok(Output::object(e));
            };
            let url = e["download_url"].as_str().ok_or_else(|| {
                CliError::usage(format!("{export} is not ready (status {}).", e["status"]))
            })?;
            let resp = ctx
                .http
                .get(url)
                .header("user-agent", crate::http::user_agent())
                .send()
                .await
                .map_err(|e| CliError::Network {
                    message: e.to_string(),
                    idempotency_key: None,
                })?;
            let status = resp.status().as_u16();
            let bytes = resp.bytes().await.map_err(|e| CliError::Network {
                message: e.to_string(),
                idempotency_key: None,
            })?;
            if !(200..300).contains(&status) {
                return Err(pylota_identity::Error::from_response(status, &bytes).into());
            }
            crate::config::write_private(&super::path(ctx, &file), &bytes)
                .map_err(|e| CliError::usage(format!("{file}: {e}")))?;
            Ok(Output::object(e).note(format!("Saved {} bytes to {file}", bytes.len())))
        }
        ExportCmd::Delete { export } => Ok(Output::object(
            api.call(
                ctx,
                "DELETE",
                &format!("/v1/exports/{}", seg(&export)),
                &[],
                None,
                Idem::None,
            )
            .await?,
        )),
    }
}
