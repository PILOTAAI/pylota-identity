//! `idvy ops` ([CLI and setup §20]): the operator's status, the switches, the restore freeze and the
//! restore runbook's steps (Observability § 9, Restore from PITR).
//!
//! [CLI and setup §20]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#20-ops

use std::path::{Path, PathBuf};
use std::time::Duration;

use pylota_identity_core::days::{parse_rfc3339, rfc3339};
use pylota_identity_core::schema::Schema;
use pylota_identity_core::sqldump::{self, Excludes};
use serde_json::{Value, json};

use super::setup::{Deployment, write_rendered};
use crate::bundle::{release, render};
use crate::cli::{OpsCmd, RestoreCmd};
use crate::http::{Idem, seg};
use crate::wrangler::Wrangler;
use crate::{CliError, Ctx, Output, VERSION};

fn precondition(m: impl Into<String>) -> CliError {
    CliError::Precondition(crate::error::Local {
        message: m.into(),
        fix: None,
        details: None,
    })
}

pub async fn ops(ctx: &Ctx, c: OpsCmd) -> Result<Output, CliError> {
    match c {
        OpsCmd::Status => status(ctx).await,
        OpsCmd::Switch {
            name,
            state,
            reason,
        } => {
            let body = json!({ "name": name, "state": state,
                "reason": reason.unwrap_or_else(|| "idvy ops switch".to_owned()) });
            let r = ctx
                .api()?
                .call(
                    ctx,
                    "PUT",
                    "/v1/platform/switches",
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?;
            Ok(Output::object(r))
        }
        OpsCmd::Freeze { dir } => freeze(ctx, &dir.dir, true).await,
        OpsCmd::Unfreeze { dir } => freeze(ctx, &dir.dir, false).await,
        OpsCmd::Restore(r) => restore(ctx, r).await,
    }
}

async fn status(ctx: &Ctx) -> Result<Output, CliError> {
    let s = ctx.api()?.get(ctx, "/v1/platform/status", &[]).await?;
    let mut h = String::new();
    h.push_str(&format!(
        "Frozen: {}{}\n",
        s["frozen"],
        s["frozen_since"]
            .as_str()
            .map(|t| format!(" (since {t})"))
            .unwrap_or_default()
    ));
    if let Some(sw) = s["switches"].as_object() {
        let list: Vec<String> = sw
            .iter()
            .map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or("?")))
            .collect();
        h.push_str(&format!("Switches: {}\n", list.join(", ")));
    }
    h.push_str(&format!(
        "Alert email: {}\n",
        s["alert_email"].as_str().unwrap_or("?")
    ));
    let firing = s["firing"].as_array().cloned().unwrap_or_default();
    h.push_str(&format!("Firing: {}\n", firing.len()));
    for f in &firing {
        h.push_str(&format!(
            "  {} {} since {}\n",
            f["severity"].as_str().unwrap_or(""),
            f["alert"].as_str().unwrap_or(""),
            f["fired_at"].as_str().unwrap_or("")
        ));
    }
    let mk = &s["master_key"];
    h.push_str(&format!(
        "Master key: slot {}, {} values to re-seal\n",
        mk["active_slot"].as_str().unwrap_or("?"),
        mk["remaining"]
    ));
    let cap = &s["capacity"];
    h.push_str(&format!(
        "Capacity: D1 {} bytes, {} vectors, {} tenants\n",
        cap["d1_bytes"], cap["vectors"], cap["tenants"]
    ));
    Ok(Output::object(s).human(h))
}

/// `[vars]` of a rendered file with `key` set to `value`, or removed.
pub fn set_var(rendered: &str, key: &str, value: Option<&str>) -> String {
    let mut out = Vec::new();
    let mut in_vars = false;
    let mut done = false;
    for line in rendered.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            if in_vars && !done {
                if let Some(v) = value {
                    out.push(format!("{key} = \"{v}\""));
                }
                done = true;
            }
            in_vars = t.starts_with("[vars]");
        }
        if in_vars
            && t.split(['=', ' ']).next() == Some(key)
            && t[key.len()..].trim_start().starts_with('=')
        {
            if let Some(v) = value {
                out.push(format!("{key} = \"{v}\""));
            }
            done = true;
            continue;
        }
        out.push(line.to_owned());
    }
    if in_vars
        && !done
        && let Some(v) = value
    {
        out.push(format!("{key} = \"{v}\""));
    }
    let mut s = out.join("\n");
    if rendered.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// The pinned Wrangler of the deployed bundle.
async fn wrangler_of<'a>(ctx: &'a Ctx, d: &Deployment<'a>) -> Result<Wrangler<'a>, CliError> {
    let version = d
        .rendered
        .as_deref()
        .and_then(|r| {
            r.lines()
                .find_map(|l| l.strip_prefix(render::MARKER))
                .and_then(|v| v.split(' ').next())
                .map(|v| v.trim_start_matches('v').to_owned())
        })
        .unwrap_or_else(|| VERSION.to_owned());
    let bundle = release::fetch(ctx, &d.dir, &version).await?;
    Wrangler::install(ctx, &d.dir, &bundle.dir, d.token(), &d.cf.account)
}

/// Whether `GET /health` reports `frozen: true`.
async fn frozen(ctx: &Ctx, base: &str) -> Option<bool> {
    let r = crate::http::get_public(ctx, base, "/health").await.ok()?;
    let v: Value = serde_json::from_slice(&r.body).ok()?;
    Some(v["frozen"] == true)
}

fn api_base(ctx: &Ctx, d: &Deployment<'_>) -> Result<String, CliError> {
    let host = d
        .rendered
        .as_deref()
        .and_then(|r| render::var(r, "PM_API_HOST"))
        .ok_or_else(|| {
            CliError::config("The rendered wrangler.toml has no PM_API_HOST.")
                .with_fix("idvy setup")
        })?;
    Ok(ctx.base_for(&host))
}

/// `ops freeze` / `ops unfreeze`: the variable change, the code deploy of the deployed bundle, then
/// `/health` until it reports the new state (at most 120 seconds).
async fn freeze(ctx: &Ctx, dir: &str, on: bool) -> Result<Output, CliError> {
    let d = Deployment::open(ctx, dir)?;
    let file = d.dir.join("wrangler.toml");
    let rendered = d.rendered.clone().ok_or_else(|| {
        CliError::config(format!("{} does not exist.", file.display())).with_fix("idvy setup")
    })?;
    let since = rfc3339(ctx.now_ms());
    let next = if on {
        set_var(
            &set_var(&rendered, "PM_FREEZE", Some("on")),
            "PM_FREEZE_SINCE",
            Some(&since),
        )
    } else {
        set_var(
            &set_var(&rendered, "PM_FREEZE", Some("off")),
            "PM_FREEZE_SINCE",
            None,
        )
    };
    write_rendered(&file, &next)?;
    let wrangler = wrangler_of(ctx, &d).await?;
    let msg = if on {
        "idvy ops freeze"
    } else {
        "idvy ops unfreeze"
    };
    wrangler.ok(
        "deploy",
        &[
            "deploy",
            "--config",
            &file.display().to_string(),
            "--message",
            msg,
        ],
        None,
    )?;
    let base = api_base(ctx, &d)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    let interval = Duration::from_millis(ctx.poll_ms.unwrap_or(2_000));
    while frozen(ctx, &base).await != Some(on) {
        if std::time::Instant::now() > deadline {
            return Err(CliError::timeout(format!(
                "{base}/health did not report frozen: {on} within 120 s."
            ))
            .with_fix(if on {
                "idvy ops freeze"
            } else {
                "idvy ops unfreeze"
            }));
        }
        tokio::time::sleep(interval).await;
    }
    Ok(
        Output::object(json!({ "frozen": on, "frozen_since": on.then_some(since.clone()) })).human(
            if on {
                format!("Frozen since {since}: only platform keys and /health answer.\n")
            } else {
                "Unfrozen: queued work drains now.\n".to_owned()
            },
        ),
    )
}

fn database_name(rendered: &str) -> Option<String> {
    let doc: toml::Table = toml::from_str(&render::strip_marker(rendered)).ok()?;
    doc.get("d1_databases")?
        .as_array()?
        .first()?
        .get("database_name")?
        .as_str()
        .map(str::to_owned)
}

/// A private working directory (0700) for one restore.
fn work_dir(base: &Path, now: i64) -> Result<PathBuf, CliError> {
    let stamp = rfc3339(now).replace([':', '-'], "");
    let dir = base.join(format!("restore-{stamp}"));
    let io = |e: std::io::Error| CliError::internal(format!("{}: {e}", dir.display()));
    std::fs::create_dir_all(&dir).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    Ok(dir)
}

fn write_private(path: &Path, text: &str) -> Result<(), CliError> {
    let io = |e: std::io::Error| CliError::internal(format!("{}: {e}", path.display()));
    std::fs::write(path, text).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(io)?;
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, CliError> {
    std::fs::read_to_string(path)
        .map_err(|e| CliError::internal(format!("{}: {e}", path.display())))
}

async fn restore(ctx: &Ctx, r: RestoreCmd) -> Result<Output, CliError> {
    match r {
        RestoreCmd::D1 {
            at,
            exclude,
            exclude_row,
            replay_only,
            dir,
        } => restore_d1(ctx, at, &exclude, &exclude_row, replay_only, &dir.dir).await,
        RestoreCmd::Mailbox {
            identity,
            at,
            bookmark,
        } => {
            let body = match (at, bookmark) {
                (Some(t), None) => json!({ "at": t }),
                (None, Some(b)) => json!({ "bookmark": b }),
                _ => return Err(CliError::usage("Pass exactly one of --at and --bookmark.")),
            };
            let id = crate::resolve::identity_arg(ctx, &identity).await?;
            let v = ctx
                .api()?
                .call(
                    ctx,
                    "POST",
                    &format!("/v1/platform/identities/{}/restore", seg(&id)),
                    &[],
                    Some(&body),
                    Idem::None,
                )
                .await?;
            let undo = v["undo_bookmark"].as_str().unwrap_or("").to_owned();
            ctx.note(&format!("Undo bookmark: {undo}"));
            Ok(Output::object(v))
        }
        RestoreCmd::Reconcile { identities, after } => reconcile(ctx, &identities, &after).await,
        RestoreCmd::Cleanup { dir } => {
            let base = crate::commands::path(ctx, &dir.dir);
            let mut removed = Vec::new();
            for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with("restore-") && e.path().is_dir() {
                    std::fs::remove_dir_all(e.path())
                        .map_err(|err| CliError::internal(format!("{name}: {err}")))?;
                    removed.push(name);
                }
            }
            Ok(Output::object(json!({ "removed": removed })))
        }
    }
}

/// `ops restore d1` (step 3): export, restore, export, compare, confirm, apply.
async fn restore_d1(
    ctx: &Ctx,
    at: Option<String>,
    exclude: &[String],
    exclude_row: &[String],
    replay_only: Option<String>,
    dir: &str,
) -> Result<Output, CliError> {
    // The excludes are checked before anything is restored.
    let schema = Schema::d1();
    let ex = Excludes::parse(schema, exclude, exclude_row).map_err(CliError::usage)?;
    if replay_only.is_none() && at.as_deref().and_then(parse_rfc3339).is_none() {
        return Err(CliError::usage(
            "--at needs an RFC 3339 time (or --replay-only <dir>).",
        ));
    }
    let d = Deployment::open(ctx, dir)?;
    let rendered = d.rendered.clone().ok_or_else(|| {
        CliError::config("The deployment has no rendered wrangler.toml.").with_fix("idvy setup")
    })?;
    let base = api_base(ctx, &d)?;
    if frozen(ctx, &base).await != Some(true) {
        return Err(precondition("The deployment is not frozen.").with_fix("idvy ops freeze"));
    }
    let db = database_name(&rendered).unwrap_or_else(|| "pylota-identity".to_owned());
    let wrangler = wrangler_of(ctx, &d).await?;
    let export = |out: &Path| -> Result<String, CliError> {
        wrangler.ok(
            "d1 export",
            &[
                "d1",
                "export",
                &db,
                "--remote",
                "--output",
                &out.display().to_string(),
            ],
            None,
        )?;
        read(out)
    };
    let (work, undo) = match &replay_only {
        Some(p) => (crate::commands::path(ctx, p), None),
        None => {
            let work = work_dir(&d.dir, ctx.now_ms())?;
            export(&work.join("before.sql"))?;
            let t = at.clone().unwrap_or_default();
            let info = wrangler.ok(
                "d1 time-travel info",
                &[
                    "d1",
                    "time-travel",
                    "info",
                    &db,
                    &format!("--timestamp={t}"),
                    "--json",
                ],
                None,
            )?;
            let bookmark = json_field(&info, "bookmark")?;
            let restored = wrangler.ok(
                "d1 time-travel restore",
                &[
                    "d1",
                    "time-travel",
                    "restore",
                    &db,
                    &format!("--bookmark={bookmark}"),
                    "--json",
                ],
                None,
            )?;
            let undo = json_field(&restored, "previous_bookmark")?;
            write_private(&work.join("undo-bookmark"), &undo)?;
            ctx.note(&format!("Restored to {t}. Undo bookmark: {undo}"));
            (work, Some(undo))
        }
    };
    let before = sqldump::parse(&read(&work.join("before.sql"))?).map_err(CliError::internal)?;
    let after_sql = export(&work.join("after.sql"))?;
    let after = sqldump::parse(&after_sql).map_err(CliError::internal)?;
    let rp = sqldump::replay(&before, &after, schema, &ex);
    let summary = json!({
        "tables": rp.summary.iter().map(|(t, (i, u, d))| (t.clone(), json!({ "inserted": i, "updated": u, "deleted": d }))).collect::<serde_json::Map<_, _>>(),
        "missing_columns": rp.missing_columns,
        "undo_bookmark": undo,
        "work_dir": work.display().to_string(),
    });
    write_private(&work.join("replay.sql"), &rp.sql)?;
    write_private(&work.join("summary.json"), &summary.to_string())?;
    if !rp.missing_columns.is_empty() {
        return Err(precondition(format!(
            "The restored schema lacks columns the database had: {}.",
            rp.missing_columns.join(", ")
        ))
        .with_fix(format!(
            "Re-apply the migration with idvy deploy, then idvy ops restore d1 --replay-only {}",
            work.display()
        )));
    }
    let mut human = String::new();
    for (t, (i, u, d)) in &rp.summary {
        human.push_str(&format!("{t}: {i} inserted, {u} updated, {d} deleted\n"));
    }
    ctx.note(human.trim_end());
    if !rp.summary.is_empty() {
        ctx.confirm("Apply the replay?")?;
        wrangler.ok(
            "d1 execute",
            &[
                "d1",
                "execute",
                &db,
                "--remote",
                "--file",
                &work.join("replay.sql").display().to_string(),
                "--yes",
            ],
            None,
        )?;
    }
    Ok(Output::object(summary).human(format!("{human}Replay applied.\n")))
}

fn json_field(out: &str, key: &str) -> Result<String, CliError> {
    let start = out.find('{').unwrap_or(0);
    let v: Value = serde_json::from_str(&out[start..])
        .map_err(|e| CliError::internal(format!("Wrangler's JSON: {e}")))?;
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| CliError::internal(format!("Wrangler's JSON has no {key}")))
}

/// `ops restore reconcile` (step 5): the `restore_reconcile` job, then the re-applied erasures.
async fn reconcile(ctx: &Ctx, identities: &[String], after: &str) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let tenant = match ctx.global.tenant.clone() {
        Some(t) => crate::resolve::tenant_arg(ctx, &t).await?,
        None => return Err(CliError::usage("ops restore reconcile needs --tenant.")),
    };
    let t =
        parse_rfc3339(after).ok_or_else(|| CliError::usage("--after needs an RFC 3339 time."))?;
    let mut ids = Vec::new();
    for i in identities {
        ids.push(crate::resolve::identity_arg(ctx, i).await?);
    }
    let job = api
        .call(
            ctx,
            "POST",
            "/v1/platform/jobs",
            &[],
            Some(&json!({ "kind": "restore_reconcile", "tenant_id": tenant, "identity_ids": ids, "after": after })),
            Idem::Generated,
        )
        .await?;
    let eras = api
        .list(
            ctx,
            "/v1/erasure-requests",
            &[("tenant_id", tenant.clone())],
            true,
        )
        .await?;
    let mut reapplied = Vec::new();
    for e in eras["data"].as_array().into_iter().flatten() {
        let done = e["completed_at"]
            .as_str()
            .and_then(parse_rfc3339)
            .is_some_and(|c| c > t);
        let touches = e["identity_id"]
            .as_str()
            .is_some_and(|i| ids.iter().any(|x| x == i))
            || matches!(e["scope"].as_str(), Some("counterparty" | "tenant"));
        if done && touches {
            let id = e["id"].as_str().unwrap_or("");
            reapplied.push(
                api.call(
                    ctx,
                    "POST",
                    &format!("/v1/platform/erasure-requests/{}/reapply", seg(id)),
                    &[],
                    None,
                    Idem::Generated,
                )
                .await?,
            );
        }
    }
    let job_id = job["id"].as_str().unwrap_or("").to_owned();
    let interval = Duration::from_millis(ctx.poll_ms.unwrap_or(5_000));
    loop {
        let j = api
            .get(ctx, &format!("/v1/platform/jobs/{}", seg(&job_id)), &[])
            .await?;
        let mut pending = !matches!(
            j["status"].as_str(),
            Some("completed" | "failed" | "canceled")
        );
        for r in &reapplied {
            let id = r["id"].as_str().unwrap_or("");
            let e = api
                .get(ctx, &format!("/v1/erasure-requests/{}", seg(id)), &[])
                .await?;
            pending |= matches!(e["status"].as_str(), Some("queued" | "running"));
        }
        if !pending {
            return Ok(Output::object(json!({ "job": j, "reapplied": reapplied })));
        }
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vars_set_and_removed() {
        let t = "name = \"x\"\n[vars]\nPM_ENV = \"production\"\nPM_FREEZE = \"off\"\n[[d1_databases]]\nbinding = \"DB\"\n";
        let on = set_var(t, "PM_FREEZE", Some("on"));
        assert!(on.contains("PM_FREEZE = \"on\"") && !on.contains("\"off\""));
        let since = set_var(&on, "PM_FREEZE_SINCE", Some("2026-10-30T10:00:00Z"));
        let v: toml::Table = toml::from_str(&since).unwrap();
        assert_eq!(
            v["vars"]["PM_FREEZE_SINCE"].as_str(),
            Some("2026-10-30T10:00:00Z")
        );
        assert_eq!(v["d1_databases"][0]["binding"].as_str(), Some("DB"));
        let gone = set_var(&since, "PM_FREEZE_SINCE", None);
        assert!(!gone.contains("PM_FREEZE_SINCE"));
    }
}
