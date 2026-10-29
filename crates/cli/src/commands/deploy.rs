//! `idvy deploy`, `idvy upgrade` and `idvy wrangler` ([CLI and setup §8, §9, §5.1]): verify the bundle,
//! render, apply D1 migrations, deploy the code, wait for `/health`. A redeploy that changes nothing calls
//! no Wrangler command.
//!
//! [CLI and setup §8, §9, §5.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#8-deploy

use serde_json::{Value, json};

use crate::bundle::{migrate, release, render};
use crate::cli::{DeployArgs, DoctorArgs, UpgradeArgs, WranglerArgs};
use crate::commands::setup::{Deployment, health_version, wait_health, write_rendered};
use crate::wrangler::Wrangler;
use crate::{CliError, Ctx, Output, VERSION};

/// The render inputs a deployed file already holds.
fn inputs_of(rendered: &str, account: &str) -> Result<render::Inputs, CliError> {
    let need = |k: &str| {
        render::var(rendered, k)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                CliError::config(format!("wrangler.toml has no {k}.")).with_fix("idvy setup")
            })
    };
    let api_host = need("PM_API_HOST")?;
    let console = render::var(rendered, "PM_CONSOLE_HOST").filter(|c| *c != api_host);
    Ok(render::Inputs {
        name: render::var(rendered, "PM_DEPLOYMENT_NAME")
            .unwrap_or_else(|| "pylota-identity".into()),
        api_host,
        console_host: console,
        platform_domain: need("PM_PLATFORM_DOMAIN")?,
        jurisdiction: need("PM_JURISDICTION")?,
        account_id: account.to_owned(),
        d1_id: render::d1_id(rendered).ok_or_else(|| {
            CliError::config("wrangler.toml has no D1 database ID.").with_fix("idvy setup")
        })?,
        rate_limits: render::rate_limit_ids(rendered),
        vars: Default::default(),
    })
}

pub async fn deploy(ctx: &Ctx, a: DeployArgs) -> Result<Output, CliError> {
    let d = Deployment::open(ctx, &a.dir.dir)?;
    let file = d.dir.join("wrangler.toml");
    let existing = d.rendered.clone().ok_or_else(|| {
        CliError::config(format!("{} does not exist.", file.display())).with_fix("idvy setup")
    })?;
    let bundle = if a.from_source {
        release::from_source(ctx, &crate::commands::path(ctx, &a.source_dir))?
    } else {
        release::fetch(ctx, &d.dir, a.version.as_deref().unwrap_or(VERSION)).await?
    };
    let verified = format!(
        "Verified pylota-identity-worker-{}.tar.gz (minisign key {}, sha256 {})",
        bundle.version,
        bundle.key_id.as_deref().unwrap_or("-"),
        bundle
            .tarball_sha256
            .as_deref()
            .map(|s| &s[..12.min(s.len())])
            .unwrap_or("-")
    );
    let inputs = inputs_of(&existing, &d.cf.account)?;
    let template = std::fs::read_to_string(bundle.dir.join("deploy/wrangler.toml.tmpl"))
        .map_err(|e| CliError::verification(format!("the bundle's template: {e}")))?;
    let rendered = render::render(&template, Some(&existing), &inputs)?;
    let unchanged = render::strip_marker(&existing) == rendered;
    let index = index_generation(&rendered)?;

    // Migrations come before code; a deploy with nothing pending and nothing changed stops here.
    let applied =
        migrate::apply(ctx, &d.cf, &inputs.d1_id, &bundle.dir.join("migrations/d1")).await?;
    let base = ctx.base_for(&inputs.api_host);
    let live = health_version(ctx, &base).await;
    let noop = unchanged
        && applied.is_empty()
        && render::deployed(&existing, &bundle.version)
        && live.as_deref() == Some(bundle.version.as_str());
    let doc = |id: Value| {
        json!({
            "version": bundle.version, "worker_version_id": id, "migrations_applied": applied,
            "gradual": Value::Null, "index_generation": index,
        })
    };
    if noop && !a.force {
        return Ok(Output::object(doc(Value::Null)).human(format!(
            "{verified}\nRendered {} (no changes)\nD1 migrations: 0 pending\nNothing to deploy: v{} is running at {base}\n",
            file.display(),
            bundle.version
        )));
    }
    if !unchanged {
        write_rendered(&file, &rendered)?;
    }
    let wrangler = Wrangler::install(ctx, &d.dir, &bundle.dir, d.token(), &d.cf.account)?;
    let msg = format!("idvy deploy v{}", bundle.version);
    let out = wrangler.ok(
        "deploy",
        &[
            "deploy",
            "--config",
            &file.display().to_string(),
            "--message",
            &msg,
        ],
        None,
    )?;
    let version_id = out
        .lines()
        .find_map(|l| l.split("Version ID:").nth(1))
        .map(|v| v.trim().to_owned());
    wait_health(ctx, &base, &bundle.version, 300).await?;
    write_rendered(&file, &render::mark_deployed(&rendered, &bundle.version))?;
    Ok(Output::object(doc(version_id.clone().map_or(Value::Null, Value::from))).human(format!(
        "{verified}\nRendered {} ({})\nD1 migrations: {} applied\nDeployed v{} (version {}) to {base}\nHealth: ok\n",
        file.display(),
        if unchanged { "no changes" } else { "updated" },
        applied.len(),
        bundle.version,
        version_id.as_deref().unwrap_or("-"),
    )))
}

/// The index generation state of a rendered file ([CLI and setup §8.7]): this idvy deploys only while
/// the bound index matches `PM_EMBED_MODEL`; starting, finalising or cancelling a re-embed is refused
/// with the reason.
///
/// [CLI and setup §8.7]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#87-index-generation-changes
fn index_generation(rendered: &str) -> Result<&'static str, CliError> {
    let next = rendered.contains("binding    = \"VECTORS_NEXT\"")
        || rendered.contains("binding = \"VECTORS_NEXT\"");
    let model = render::var(rendered, "PM_EMBED_MODEL").unwrap_or_default();
    if next || model != "@cf/baai/bge-m3" {
        return Err(CliError::usage(format!(
            "PM_EMBED_MODEL is {model}{}: changing the embedding model needs a re-embed, which this idvy does not run yet.",
            if next { " with VECTORS_NEXT bound" } else { "" }
        ))
        .with_fix("Set PM_EMBED_MODEL back to @cf/baai/bge-m3 and remove VECTORS_NEXT."));
    }
    Ok("unchanged")
}

/// `idvy upgrade`: the CLI's version decides the Worker version; then `doctor`.
pub async fn upgrade(ctx: &Ctx, a: UpgradeArgs) -> Result<Output, CliError> {
    let latest: Value = ctx
        .http
        .get(format!(
            "{}/repos/PILOTAAI/pylota-identity/releases/latest",
            ctx.github_api
        ))
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2026-03-10")
        .header("user-agent", crate::http::user_agent())
        .send()
        .await
        .map_err(|e| CliError::Network {
            message: e.to_string(),
            idempotency_key: None,
        })?
        .json()
        .await
        .map_err(|e| CliError::Network {
            message: e.to_string(),
            idempotency_key: None,
        })?;
    let tag = latest["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches('v')
        .to_owned();
    if release::newer(&tag, VERSION) {
        return Err(CliError::usage(format!(
            "idvy {VERSION} is older than the latest release {tag}; install the new CLI first (cargo install pylota-identity-cli --locked)."
        )));
    }
    let d = Deployment::open(ctx, &a.dir.dir)?;
    let host = d
        .rendered
        .as_deref()
        .and_then(|r| render::var(r, "PM_API_HOST"))
        .unwrap_or_default();
    match health_version(ctx, &ctx.base_for(&host)).await {
        Some(v) if v == VERSION => {
            return Ok(Output::object(json!({ "version": v, "upgraded": false }))
                .human(format!("up to date (v{v})\n")));
        }
        Some(v) if release::newer(&v, VERSION) => {
            return Err(CliError::usage(format!(
                "The deployment runs v{v}, newer than this idvy; a rollback is idvy deploy --version <v>."
            )));
        }
        _ => {}
    }
    let dir = a.dir.dir.clone();
    let deployed = deploy(
        ctx,
        DeployArgs {
            version: None,
            from_source: false,
            source_dir: ".".into(),
            force: false,
            dir: crate::cli::DirArg { dir: dir.clone() },
        },
    )
    .await?;
    let doctor = crate::commands::doctor::run(
        ctx,
        DoctorArgs {
            mail_test: false,
            checks: vec![],
            dir: crate::cli::DirArg { dir },
        },
    )
    .await?;
    let human = format!(
        "{}{}",
        deployed.human.clone().unwrap_or_default(),
        doctor.human.clone().unwrap_or_default()
    );
    Ok(Output {
        exit: doctor.exit,
        ..Output::object(json!({ "deploy": deployed.doc, "doctor": doctor.doc })).human(human)
    })
}

/// `idvy wrangler <args…>`: the pinned Wrangler, its output and exit code passed through.
pub async fn wrangler(ctx: &Ctx, a: WranglerArgs) -> Result<Output, CliError> {
    let d = Deployment::open(ctx, &a.dir.dir)?;
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
    let w = Wrangler::install(ctx, &d.dir, &bundle.dir, d.token(), &d.cf.account)?;
    let stdin = if ctx.stdin_tty {
        None
    } else {
        Some(ctx.read_stdin()?)
    };
    let args: Vec<&str> = a.args.iter().map(String::as_str).collect();
    let p = w.run(&args, stdin)?;
    ctx.print(&String::from_utf8_lossy(&p.stdout));
    if !p.stderr.is_empty() {
        ctx.note(String::from_utf8_lossy(&p.stderr).trim_end());
    }
    Ok(Output {
        exit: p.code,
        ..Output::default()
    }
    .streamed())
}
