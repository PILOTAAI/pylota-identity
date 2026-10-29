//! `idvy dlq` ([CLI and setup §13]) and `idvy jobs` (§19): platform API calls with a platform key
//! holding `platform:ops`; no Cloudflare credentials, no D1 query.
//!
//! [CLI and setup §13]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#13-dlq

use serde_json::{Value, json};

use super::{Obj, paging, q_opt};
use crate::cli::{DlqCmd, JobsCmd};
use crate::http::{Idem, seg};
use crate::output::{Col, col};
use crate::{CliError, Ctx, Output, resolve};

const DLQ_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("QUEUE", "queue", 20),
    col("KIND", "kind", 12),
    col("TENANT", "tenant_id", 30),
    col("FIRST SEEN", "first_seen_at", 20),
    col("REDRIVES", "redrive_count", 8),
];

/// The tenant of `--tenant`, when given (never the profile's or the default tenant).
async fn tenant_flag(ctx: &Ctx) -> Result<Option<String>, CliError> {
    match ctx.global.tenant.clone() {
        Some(t) => Ok(Some(resolve::tenant_arg(ctx, &t).await?)),
        None => Ok(None),
    }
}

pub async fn dlq(ctx: &Ctx, c: DlqCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        DlqCmd::List {
            queue,
            status,
            paging: p,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "queue", queue);
            q_opt(&mut q, "status", status);
            q_opt(&mut q, "tenant_id", tenant_flag(ctx).await?);
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, "/v1/platform/dlq", &q, p.all).await?,
                DLQ_COLS,
            ))
        }
        DlqCmd::Redrive { ids, queue } => {
            let ids = match (ids.is_empty(), queue) {
                (false, None) => ids,
                (true, Some(queue)) => {
                    let mut q = vec![("queue", queue.clone()), ("status", "open".to_owned())];
                    q_opt(&mut q, "tenant_id", tenant_flag(ctx).await?);
                    let all = api.list(ctx, "/v1/platform/dlq", &q, true).await?;
                    let ids: Vec<String> = all["data"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|i| i["id"].as_str().map(str::to_owned))
                        .collect();
                    ctx.note(&format!("{} open items in {queue}.", ids.len()));
                    if !ids.is_empty() {
                        ctx.confirm(&format!("Redrive {} items?", ids.len()))?;
                    }
                    ids
                }
                _ => {
                    return Err(CliError::usage(
                        "Name the items to redrive, or --queue for every open item of a queue.",
                    ));
                }
            };
            let (mut redriven, mut failed, mut exit) = (Vec::new(), Vec::new(), 0);
            let mut human = String::new();
            for id in &ids {
                match api
                    .call(
                        ctx,
                        "POST",
                        &format!("/v1/platform/dlq/{}/redrive", seg(id)),
                        &[],
                        None,
                        Idem::Generated,
                    )
                    .await
                {
                    Ok(item) => {
                        human.push_str(&format!("redriven {id}\n"));
                        redriven.push(item);
                    }
                    Err(e) => {
                        if exit == 0 {
                            exit = e.exit_code();
                        }
                        let code = e.envelope()["error"]["code"].clone();
                        human.push_str(&format!(
                            "failed   {id}: {}\n",
                            code.as_str().unwrap_or("?")
                        ));
                        failed.push(json!({ "id": id, "error": code }));
                    }
                }
            }
            let mut out =
                Output::object(json!({ "redriven": redriven, "failed": failed })).human(human);
            out.exit = exit;
            Ok(out)
        }
    }
}

pub async fn jobs(ctx: &Ctx, c: JobsCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        JobsCmd::Start {
            kind,
            identities,
            after,
            before,
        } => {
            let tenant = tenant_flag(ctx).await?.ok_or_else(|| {
                CliError::usage(
                    "jobs start needs --tenant: a job over the wrong tenant is costly to undo.",
                )
            })?;
            let mut body = Obj::new()
                .set("kind", kind.clone())
                .set("tenant_id", tenant.clone());
            if kind == "event_log_rebuild" {
                if !identities.is_empty() || after.is_some() || before.is_some() {
                    return Err(CliError::usage(
                        "event_log_rebuild takes --tenant only: no --identity, --after or --before.",
                    ));
                }
            } else {
                let mut ids = Vec::new();
                for i in &identities {
                    ids.push(Value::from(resolve::identity_arg(ctx, i).await?));
                }
                body = body
                    .set(
                        "identity_ids",
                        if ids.is_empty() {
                            Value::Null
                        } else {
                            Value::from(ids)
                        },
                    )
                    .set("after", after.map_or(Value::Null, Value::from))
                    .set("before", before.map_or(Value::Null, Value::from));
            }
            Ok(Output::object(
                api.call(
                    ctx,
                    "POST",
                    "/v1/platform/jobs",
                    &[],
                    Some(&body.value()),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        JobsCmd::Get { job } => Ok(Output::object(
            api.get(ctx, &format!("/v1/platform/jobs/{}", seg(&job)), &[])
                .await?,
        )),
    }
}
