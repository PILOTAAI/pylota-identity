//! `login`, `config show|set` and `mcp config` ([CLI and setup §14, §15]).
//!
//! [CLI and setup §14, §15]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#14-login-and-config

use serde_json::{Value, json};

use crate::cli::{ConfigCmd, LoginArgs, McpCmd};
use crate::config;
use crate::http::Api;
use crate::{CliError, Ctx, Output};

/// `idvy login`: checks the key with `GET /v1/me` and saves it in the profile named by `--profile`,
/// default `default` (never the one `PYLOTA_IDENTITY_PROFILE` or `default_profile` names).
pub async fn login(ctx: &Ctx, a: LoginArgs) -> Result<Output, CliError> {
    let profile = ctx
        .global
        .profile
        .clone()
        .unwrap_or_else(|| "default".to_owned());
    let url = match ctx
        .global
        .url
        .clone()
        .or_else(|| ctx.var("PYLOTA_IDENTITY_URL").map(str::to_owned))
    {
        Some(u) => u,
        None if ctx.interactive => ctx.prompt("API URL (https://…): ")?,
        None => return Err(CliError::usage("Pass --url.")),
    };
    let url = config::normalise_url(&url)?;
    let key = if let Some(var) = &a.key_env {
        ctx.var(var)
            .map(str::to_owned)
            .ok_or_else(|| CliError::config(format!("{var} is not set.")))?
    } else if a.key_command.is_some() {
        String::new()
    } else if let Some(k) = ctx
        .global
        .key
        .clone()
        .or_else(|| ctx.var("PYLOTA_IDENTITY_KEY").map(str::to_owned))
    {
        k
    } else if ctx.interactive {
        hidden_prompt(ctx, "API key: ")?
    } else {
        return Err(CliError::usage(
            "Put the key in PYLOTA_IDENTITY_KEY, or pass --key-env or --key-command.",
        ));
    };
    let key = if let Some(cmd) = &a.key_command {
        let out = ctx
            .runner
            .run(&crate::Cmd {
                program: if cfg!(windows) {
                    "cmd".into()
                } else {
                    "sh".into()
                },
                args: vec![
                    if cfg!(windows) {
                        "/C".into()
                    } else {
                        "-c".into()
                    },
                    cmd.clone(),
                ],
                stdin: Some(Vec::new()),
                timeout_s: Some(10),
                ..crate::Cmd::default()
            })
            .map_err(|e| CliError::config(format!("key_command `{cmd}` failed: {e}")))?;
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    } else {
        key
    };
    if !config::valid_key(&key) {
        return Err(CliError::config(
            "That is not a pmk_live_… or pmk_test_… key.",
        ));
    }
    let me = Api::new(ctx, &url, Some(&key))?
        .get(ctx, "/v1/me", &[])
        .await?;
    let path = config::update_profile(ctx, &profile, |p| {
        p.url = Some(url.clone());
        p.key = None;
        p.key_env = None;
        p.key_command = None;
        if let Some(v) = &a.key_env {
            p.key_env = Some(v.clone());
        } else if let Some(c) = &a.key_command {
            p.key_command = Some(c.clone());
        } else {
            p.key = Some(key.clone());
        }
    })?;
    let mut out = Output::object(json!({ "profile": profile, "url": url, "key": me.clone() }))
        .human(format!(
            "Saved profile {profile} in {}\nlevel: {}  tenant: {}  identity: {}\npermissions: {}\n",
            path.display(),
            me["level"].as_str().unwrap_or("?"),
            me["tenant_id"].as_str().unwrap_or("-"),
            me["identity_id"].as_str().unwrap_or("-"),
            me["permissions"]
                .as_array()
                .map(|p| p
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(","))
                .unwrap_or_default(),
        ));
    if ctx.var("PYLOTA_IDENTITY_KEY").is_some_and(|k| k != key) {
        out = out.note("warning: PYLOTA_IDENTITY_KEY is set and differs from the saved key; it keeps overriding the profile until you unset it.");
    }
    Ok(out)
}

/// Reads a line without echo, through `stty` on Unix.
fn hidden_prompt(ctx: &Ctx, question: &str) -> Result<String, CliError> {
    let stty = |arg: &str| {
        let _ = std::process::Command::new("stty")
            .arg(arg)
            .stdin(std::process::Stdio::inherit())
            .status();
    };
    if cfg!(unix) {
        stty("-echo");
    }
    let line = ctx.prompt(question);
    if cfg!(unix) {
        stty("echo");
        ctx.note("");
    }
    line
}

pub async fn config(ctx: &Ctx, c: ConfigCmd) -> Result<Output, CliError> {
    match c {
        ConfigCmd::Show => {
            let r = config::resolve(ctx)?;
            let show = |s: &Option<config::Setting>, redact: bool| match s {
                Some(s) => json!({
                    "value": if redact { config::redact_key(&s.value) } else { s.value.clone() },
                    "source": s.source,
                }),
                None => Value::Null,
            };
            let doc = json!({
                "config_file": config::path(ctx)?.display().to_string(),
                "profile": r.profile,
                "url": show(&r.url, false),
                "key": show(&r.key, true),
                "identity": show(&r.identity, false),
                "tenant": show(&r.tenant, false),
                "account_id": show(&r.account_id, false),
            });
            let mut human = String::new();
            for k in ["config_file", "profile"] {
                human.push_str(&format!("{k:<12} {}\n", doc[k].as_str().unwrap_or("-")));
            }
            for k in ["url", "key", "identity", "tenant", "account_id"] {
                let v = &doc[k];
                human.push_str(&format!(
                    "{k:<12} {}{}\n",
                    v["value"].as_str().unwrap_or("-"),
                    v["source"]
                        .as_str()
                        .map(|s| format!("  ({s})"))
                        .unwrap_or_default()
                ));
            }
            Ok(Output::object(doc).human(human))
        }
        ConfigCmd::Set { setting, value } => {
            let file = config::load(ctx)?;
            let name = ctx
                .global
                .profile
                .clone()
                .or_else(|| ctx.var("PYLOTA_IDENTITY_PROFILE").map(str::to_owned))
                .or(file.default_profile.clone())
                .unwrap_or_else(|| "default".to_owned());
            if setting == "default_profile" {
                if !config::valid_profile_name(&value) {
                    return Err(CliError::usage(format!("Invalid profile name {value:?}.")));
                }
                let mut f = file;
                f.default_profile = Some(value.clone());
                config::save(ctx, &f)?;
            } else {
                let value = match setting.as_str() {
                    "url" => config::normalise_url(&value)?,
                    "key" => {
                        return Err(CliError::usage(
                            "A key set this way would land in your shell history.",
                        )
                        .with_fix("idvy login"));
                    }
                    "identity" | "tenant" | "account_id" | "key_env" | "key_command" => value,
                    other => {
                        return Err(CliError::usage(format!(
                            "Unknown setting {other}: url, identity, tenant, account_id, key_env, key_command or default_profile."
                        )));
                    }
                };
                config::update_profile(ctx, &name, |p| {
                    let slot = match setting.as_str() {
                        "url" => &mut p.url,
                        "identity" => &mut p.identity,
                        "tenant" => &mut p.tenant,
                        "account_id" => &mut p.account_id,
                        "key_env" => {
                            p.key = None;
                            p.key_command = None;
                            &mut p.key_env
                        }
                        _ => {
                            p.key = None;
                            p.key_env = None;
                            &mut p.key_command
                        }
                    };
                    *slot = Some(value.clone());
                })?;
            }
            Ok(
                Output::object(json!({ "profile": name, "setting": setting, "set": true }))
                    .human(format!("Set {setting} in profile {name}\n")),
            )
        }
    }
}

/// `idvy mcp config`: the generic client configuration for the profile's URL, never the key, and the
/// tools the key would see. The other client forms are M36's.
pub async fn mcp(ctx: &Ctx, c: McpCmd) -> Result<Output, CliError> {
    let McpCmd::Config { name, .. } = c;
    let api = ctx.api()?;
    let url = format!("{}/mcp", api.url());
    let me = crate::resolve::me(ctx).await?;
    let doc = json!({
        "mcpServers": { name: {
            "url": url,
            "headers": { "Authorization": "Bearer ${PYLOTA_IDENTITY_KEY}" },
        }},
    });
    let tools = me["mcp_tools"].clone();
    let human = format!(
        "{}\nTools for this key: {}\n",
        serde_json::to_string_pretty(&doc).unwrap_or_default(),
        match &tools {
            Value::Array(t) => t
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            _ => "every tool its permissions allow".to_owned(),
        }
    );
    Ok(Output::object(doc).human(human))
}
