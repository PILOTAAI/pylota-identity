//! Wrangler, installed from the verified bundle's lockfile ([CLI and setup §5.1], J34): never fetched by
//! name, `npm ci --ignore-scripts` into `<dir>/.wrangler/{version}/`, reused while `.lock-sha256` matches,
//! and run by its absolute path after a `--version` check. The Cloudflare token reaches it through the
//! child's environment only.
//!
//! [CLI and setup §5.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#51-wrangler

use std::path::{Path, PathBuf};

use crate::bundle::verify::sha256_hex;
use crate::{CliError, Cmd, Ctx, Proc, VERSION};

/// The Wrangler version the bundle's lockfile pins.
pub const WRANGLER_VERSION: &str = "4.139.0";

pub struct Wrangler<'a> {
    ctx: &'a Ctx,
    bin: PathBuf,
    env: Vec<(String, String)>,
    cwd: PathBuf,
}

fn run(ctx: &Ctx, cmd: &Cmd) -> Result<Proc, CliError> {
    ctx.runner.run(cmd).map_err(CliError::prerequisite)
}

/// Node.js 22 or later and npm.
fn check_node(ctx: &Ctx) -> Result<(), CliError> {
    let node = run(
        ctx,
        &Cmd {
            program: "node".into(),
            args: vec!["--version".into()],
            ..Cmd::default()
        },
    )
    .map_err(|_| {
        CliError::prerequisite("Node.js is not installed.").with_fix("Install Node.js 22 or later.")
    })?;
    let v = String::from_utf8_lossy(&node.stdout);
    let major: u32 = v
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    if node.code != 0 || major < 22 {
        return Err(
            CliError::prerequisite(format!("Node.js {} is older than 22.", v.trim()))
                .with_fix("Install Node.js 22 or later."),
        );
    }
    let npm = run(
        ctx,
        &Cmd {
            program: "npm".into(),
            args: vec!["--version".into()],
            ..Cmd::default()
        },
    )?;
    if npm.code != 0 {
        return Err(CliError::prerequisite("npm does not answer."));
    }
    Ok(())
}

impl<'a> Wrangler<'a> {
    /// Installs (or reuses) Wrangler from `bundle/wrangler/package-lock.json`.
    pub fn install(
        ctx: &'a Ctx,
        dir: &Path,
        bundle: &Path,
        token: &str,
        account: &str,
    ) -> Result<Self, CliError> {
        check_node(ctx)?;
        let lock = std::fs::read(bundle.join("wrangler/package-lock.json")).map_err(|e| {
            CliError::verification(format!("the bundle's wrangler/package-lock.json: {e}"))
        })?;
        let pkg = std::fs::read(bundle.join("wrangler/package.json")).map_err(|e| {
            CliError::verification(format!("the bundle's wrangler/package.json: {e}"))
        })?;
        let home = dir.join(".wrangler").join(VERSION);
        let hash = sha256_hex(&lock);
        let marker = home.join(".lock-sha256");
        let io = |e: std::io::Error| CliError::internal(format!("{}: {e}", home.display()));
        if std::fs::read_to_string(&marker).ok().as_deref() != Some(hash.as_str()) {
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).map_err(io)?;
            std::fs::write(home.join("package.json"), &pkg).map_err(io)?;
            std::fs::write(home.join("package-lock.json"), &lock).map_err(io)?;
            let p = run(
                ctx,
                &Cmd {
                    program: "npm".into(),
                    args: ["ci", "--ignore-scripts", "--no-audit", "--no-fund"]
                        .map(String::from)
                        .to_vec(),
                    cwd: Some(home.clone()),
                    ..Cmd::default()
                },
            )?;
            if p.code != 0 {
                return Err(CliError::prerequisite(format!(
                    "npm ci failed: {}",
                    String::from_utf8_lossy(&p.stderr)
                        .lines()
                        .last()
                        .unwrap_or("")
                )));
            }
            std::fs::write(&marker, &hash).map_err(io)?;
        }
        let bin = home.join("node_modules/.bin").join(if cfg!(windows) {
            "wrangler.cmd"
        } else {
            "wrangler"
        });
        let w = Self {
            ctx,
            bin,
            env: vec![
                ("CLOUDFLARE_API_TOKEN".into(), token.to_owned()),
                ("CLOUDFLARE_ACCOUNT_ID".into(), account.to_owned()),
                ("WRANGLER_SEND_METRICS".into(), "false".into()),
            ],
            cwd: bundle.to_owned(),
        };
        let v = w.run(&["--version"], None)?;
        let out = String::from_utf8_lossy(&v.stdout);
        if v.code != 0 || !out.contains(WRANGLER_VERSION) {
            return Err(CliError::prerequisite(format!(
                "wrangler --version printed {:?}, not {WRANGLER_VERSION}",
                out.trim()
            )));
        }
        Ok(w)
    }

    /// Runs Wrangler with `args` in the bundle directory; `stdin` is written then closed.
    pub fn run(&self, args: &[&str], stdin: Option<Vec<u8>>) -> Result<Proc, CliError> {
        run(
            self.ctx,
            &Cmd {
                program: self.bin.display().to_string(),
                args: args.iter().map(|s| (*s).to_owned()).collect(),
                env: self.env.clone(),
                stdin: Some(stdin.unwrap_or_default()),
                cwd: Some(self.cwd.clone()),
                timeout_s: Some(600),
            },
        )
    }

    /// Runs Wrangler and fails (exit 10) unless it succeeds, with its last 40 lines.
    pub fn ok(
        &self,
        step: &str,
        args: &[&str],
        stdin: Option<Vec<u8>>,
    ) -> Result<String, CliError> {
        let p = self.run(args, stdin)?;
        let out = String::from_utf8_lossy(&p.stdout).into_owned();
        if p.code != 0 {
            let err = String::from_utf8_lossy(&p.stderr);
            let tail: Vec<&str> = err.lines().chain(out.lines()).collect();
            let tail = tail[tail.len().saturating_sub(40)..].join("\n");
            return Err(CliError::Cloudflare {
                step: step.to_owned(),
                status: None,
                errors: vec![crate::error::CfError {
                    code: p.code.into(),
                    message: tail,
                }],
                fix: None,
            });
        }
        Ok(out)
    }

    /// `wrangler secret put NAME --name WORKER` with the value on stdin only.
    pub fn secret_put(&self, worker: &str, name: &str, value: &str) -> Result<(), CliError> {
        self.ok(
            &format!("secret {name}"),
            &["secret", "put", name, "--name", worker],
            Some(value.as_bytes().to_vec()),
        )?;
        Ok(())
    }
}
