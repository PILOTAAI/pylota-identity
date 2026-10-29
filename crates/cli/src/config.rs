//! The config file, profiles and precedence ([CLI and setup §2]): flags over the environment over the
//! profile; the file is refused when anyone else can read it, and written atomically with mode `0600`.
//!
//! [CLI and setup §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#2-configuration-and-credentials

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{CliError, Cmd, Ctx};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// The deployment name `setup --name` chose (CLI and setup § 6.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Where a setting came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Flag,
    Environment,
    Profile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Setting {
    pub value: String,
    pub source: Source,
}

/// The settings of one invocation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Resolved {
    /// The profile read, when one exists.
    pub profile: Option<String>,
    pub url: Option<Setting>,
    pub key: Option<Setting>,
    pub identity: Option<Setting>,
    pub tenant: Option<Setting>,
    pub account_id: Option<Setting>,
    pub name: Option<Setting>,
}

/// The config file's path.
pub fn path(ctx: &Ctx) -> Result<PathBuf, CliError> {
    if let (true, Some(a)) = (cfg!(windows), ctx.var("APPDATA")) {
        return Ok(Path::new(a).join("pylota-identity").join("config.toml"));
    }
    let base = match (ctx.var("XDG_CONFIG_HOME"), ctx.var("HOME")) {
        (Some(x), _) => PathBuf::from(x),
        (None, Some(h)) => Path::new(h).join(".config"),
        (None, None) => return Err(CliError::config("Neither XDG_CONFIG_HOME nor HOME is set.")),
    };
    Ok(base.join("pylota-identity").join("config.toml"))
}

/// The config file, or an empty one when it does not exist.
pub fn load(ctx: &Ctx) -> Result<ConfigFile, CliError> {
    let p = path(ctx)?;
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ConfigFile::default()),
        Err(e) => return Err(CliError::config(format!("{}: {e}", p.display()))),
    };
    check_permissions(&p)?;
    let file: ConfigFile =
        toml::from_str(&text).map_err(|e| CliError::config(format!("{}: {e}", p.display())))?;
    for (name, prof) in &file.profiles {
        if !valid_profile_name(name) {
            return Err(CliError::config(format!(
                "{}: invalid profile name {name:?}",
                p.display()
            )));
        }
        let sources = [&prof.key, &prof.key_env, &prof.key_command]
            .iter()
            .filter(|s| s.is_some())
            .count();
        if sources > 1 {
            return Err(CliError::config(format!(
                "{}: profile {name} sets more than one of key, key_env and key_command",
                p.display()
            ))
            .with_fix("Keep one of them."));
        }
    }
    Ok(file)
}

/// `^[a-z0-9][a-z0-9_-]{0,31}$`.
pub fn valid_profile_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// Refuses a file group or others can access, or one another user owns (Unix only).
#[cfg(unix)]
fn check_permissions(p: &Path) -> Result<(), CliError> {
    use std::os::unix::fs::MetadataExt as _;
    let m = std::fs::metadata(p).map_err(|e| CliError::config(format!("{}: {e}", p.display())))?;
    let insecure = || {
        CliError::config(format!(
            "{} can be read by other users or belongs to another user.",
            p.display()
        ))
        .with_fix(format!("chmod 600 {}", p.display()))
    };
    if m.mode() & 0o077 != 0 {
        return Err(insecure());
    }
    if m.uid() != current_uid()? {
        return Err(insecure());
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_permissions(_: &Path) -> Result<(), CliError> {
    Ok(())
}

/// The effective user ID: the owner of a file this process creates (no `unsafe` needed).
#[cfg(unix)]
fn current_uid() -> Result<u32, CliError> {
    use std::os::unix::fs::MetadataExt as _;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let probe = std::env::temp_dir().join(format!(".idvy-uid-{}-{nanos}", std::process::id()));
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| CliError::config(format!("cannot check the config file's owner: {e}")))?;
    let uid = f.metadata().map(|m| m.uid());
    drop(f);
    let _ = std::fs::remove_file(&probe);
    uid.map_err(|e| CliError::config(format!("cannot check the config file's owner: {e}")))
}

/// Writes the file atomically: a `0600` temporary file in the same `0700` directory, then a rename.
pub fn save(ctx: &Ctx, file: &ConfigFile) -> Result<PathBuf, CliError> {
    let p = path(ctx)?;
    let dir = p.parent().unwrap_or(Path::new("."));
    let io = |e: std::io::Error| CliError::config(format!("{}: {e}", p.display()));
    std::fs::create_dir_all(dir).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    let text = toml::to_string(file).map_err(|e| CliError::internal(e.to_string()))?;
    let tmp = dir.join(format!(".config.toml.{}.tmp", std::process::id()));
    write_private(&tmp, text.as_bytes()).map_err(io)?;
    std::fs::rename(&tmp, &p).map_err(io)?;
    Ok(p)
}

/// Creates or replaces a file readable only by its owner.
pub fn write_private(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let _ = std::fs::remove_file(p);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        o.mode(0o600);
    }
    let mut f = o.open(p)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// The profile name: `--profile`, else `PYLOTA_IDENTITY_PROFILE`, else `default_profile`, else `default`.
/// The flag and the variable must name an existing profile (exit 3).
fn profile_name(ctx: &Ctx, file: &ConfigFile) -> Result<(String, bool), CliError> {
    if let Some(p) = ctx.global.profile.as_deref() {
        return Ok((p.to_owned(), true));
    }
    if let Some(p) = ctx.var("PYLOTA_IDENTITY_PROFILE") {
        return Ok((p.to_owned(), true));
    }
    Ok((
        file.default_profile
            .clone()
            .unwrap_or_else(|| "default".to_owned()),
        false,
    ))
}

/// Resolves the settings of this invocation ([CLI and setup §2.2]).
///
/// [CLI and setup §2.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#22-precedence
pub fn resolve(ctx: &Ctx) -> Result<Resolved, CliError> {
    let file = load(ctx)?;
    let (name, named) = profile_name(ctx, &file)?;
    let prof = match file.profiles.get(&name) {
        Some(p) => Some(p.clone()),
        None if named => {
            return Err(CliError::config(format!("No profile named {name}."))
                .with_fix("idvy login --profile <name>"));
        }
        None => None,
    };
    let pick = |flag: Option<&str>, env: Option<&str>, prof: Option<&String>| -> Option<Setting> {
        if let Some(v) = flag {
            return Some(Setting {
                value: v.to_owned(),
                source: Source::Flag,
            });
        }
        if let Some(v) = env.and_then(|e| ctx.var(e)) {
            return Some(Setting {
                value: v.to_owned(),
                source: Source::Environment,
            });
        }
        prof.map(|v| Setting {
            value: v.clone(),
            source: Source::Profile,
        })
    };
    let p = prof.clone().unwrap_or_default();
    let mut url = pick(
        ctx.global.url.as_deref(),
        Some("PYLOTA_IDENTITY_URL"),
        p.url.as_ref(),
    );
    if let Some(u) = &mut url {
        u.value = normalise_url(&u.value)?;
    }
    let key = match pick(ctx.global.key.as_deref(), Some("PYLOTA_IDENTITY_KEY"), None) {
        Some(k) => Some(k),
        None => profile_key(ctx, &p)?.map(|v| Setting {
            value: v,
            source: Source::Profile,
        }),
    };
    if let Some(k) = &key {
        check_key(&k.value)?;
    }
    Ok(Resolved {
        profile: prof.map(|_| name),
        url,
        key,
        identity: pick(ctx.global.identity.as_deref(), None, p.identity.as_ref()),
        tenant: pick(ctx.global.tenant.as_deref(), None, p.tenant.as_ref()),
        account_id: pick(
            ctx.global.account_id.as_deref(),
            Some("CLOUDFLARE_ACCOUNT_ID"),
            p.account_id.as_ref(),
        ),
        name: p.name.map(|v| Setting {
            value: v,
            source: Source::Profile,
        }),
    })
}

/// A profile's key: `key`, the variable `key_env` names, or the output of `key_command`.
fn profile_key(ctx: &Ctx, p: &Profile) -> Result<Option<String>, CliError> {
    if let Some(k) = &p.key {
        return Ok(Some(k.clone()));
    }
    if let Some(var) = &p.key_env {
        return ctx
            .var(var)
            .map(|v| Some(v.to_owned()))
            .ok_or_else(|| CliError::config(format!("key_env names {var}, which is not set.")));
    }
    if let Some(cmd) = &p.key_command {
        let (program, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };
        let out = ctx
            .runner
            .run(&Cmd {
                program: program.into(),
                args: vec![flag.into(), cmd.clone()],
                stdin: Some(Vec::new()),
                timeout_s: Some(10),
                ..Cmd::default()
            })
            .map_err(|e| CliError::config(format!("key_command `{cmd}` failed: {e}")))?;
        if !out.stderr.is_empty() {
            ctx.note(String::from_utf8_lossy(&out.stderr).trim_end());
        }
        if out.code != 0 {
            return Err(CliError::config(format!(
                "key_command `{cmd}` exited with {}.",
                out.code
            )));
        }
        let key = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !valid_key(&key) {
            return Err(CliError::config(format!(
                "key_command `{cmd}` did not print an API key."
            )));
        }
        return Ok(Some(key));
    }
    Ok(None)
}

/// `^pmk_(live|test)_[0-9a-hjkmnp-tv-z]{12}_[0-9a-hjkmnp-tv-z]{52}$`.
pub fn valid_key(k: &str) -> bool {
    pylota_identity_core::keys::parse(k).is_some()
}

fn check_key(k: &str) -> Result<(), CliError> {
    if valid_key(k) {
        Ok(())
    } else {
        Err(CliError::config(
            "The API key is not a pmk_live_… or pmk_test_… key.",
        ))
    }
}

/// `https://` (or `http://` for a loopback host), without a trailing `/` or `/v1`.
pub fn normalise_url(u: &str) -> Result<String, CliError> {
    let mut s = u.trim().to_owned();
    while s.ends_with('/') {
        s.pop();
    }
    if let Some(stripped) = s.strip_suffix("/v1") {
        s = stripped.to_owned();
    }
    let (scheme, rest) = s
        .split_once("://")
        .ok_or_else(|| CliError::config(format!("{u} is not a URL.")))?;
    let host = rest.split('/').next().unwrap_or("");
    let host_only = if let Some(h) = host.strip_prefix('[') {
        format!("[{}", h.split(']').next().unwrap_or("")) + "]"
    } else {
        host.split(':').next().unwrap_or("").to_owned()
    };
    let loopback = matches!(host_only.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    match scheme {
        "https" => {}
        "http" if loopback => {}
        _ => {
            return Err(CliError::config(format!(
                "{u}: the API URL must use https://."
            )));
        }
    }
    if host_only.is_empty() || rest.contains('/') {
        return Err(CliError::config(format!(
            "{u}: the API URL is a host, without a path."
        )));
    }
    Ok(s)
}

/// Writes `key` (or a key source), `url` and other fields into the named profile.
pub fn update_profile(
    ctx: &Ctx,
    name: &str,
    f: impl FnOnce(&mut Profile),
) -> Result<PathBuf, CliError> {
    if !valid_profile_name(name) {
        return Err(CliError::usage(format!("Invalid profile name {name:?}.")));
    }
    let mut file = load(ctx)?;
    let p = file.profiles.entry(name.to_owned()).or_default();
    f(p);
    if [&p.key, &p.key_env, &p.key_command]
        .iter()
        .filter(|s| s.is_some())
        .count()
        > 1
    {
        return Err(CliError::config(
            "A profile holds one of key, key_env and key_command.",
        ));
    }
    save(ctx, &file)
}

/// The key as `config show` prints it: the prefix and lookup, never the secret.
pub fn redact_key(k: &str) -> String {
    match k.rsplit_once('_') {
        Some((head, _)) => format!("{head}…"),
        None => "…".to_owned(),
    }
}
