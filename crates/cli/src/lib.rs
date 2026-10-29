//! `idvy`, the Pylota Identity command-line tool ([CLI and setup], build plan M16): the library behind
//! the binary. [`run`] takes the arguments and an [`Env`] (variables, standard streams, the HTTP client
//! and the outside services), so the tests drive the whole CLI in-process against the workerd harness
//! and recorded fakes.
//!
//! [CLI and setup]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md
#![forbid(unsafe_code)]

pub mod bundle;
pub mod cli;
pub mod cloudflare;
pub mod commands;
pub mod config;
pub mod error;
pub mod http;
pub mod output;
pub mod resolve;
pub mod sse;
pub mod wrangler;

use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser as _;

pub use error::CliError;
pub use output::{Mode, Output};

/// The CLI's version, which is also the Worker release it deploys.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs child processes (`node`, `npm`, Wrangler, `key_command`). The binary runs them for real; the
/// tests record the calls and answer from a script.
pub trait Runner: Send + Sync {
    fn run(&self, cmd: &Cmd) -> Result<Proc, String>;
}

/// One child process.
#[derive(Clone, Debug, Default)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    /// Variables added to the child's environment (the Cloudflare token reaches Wrangler only here).
    pub env: Vec<(String, String)>,
    /// Bytes written to its stdin, then closed; `None` closes stdin at once.
    pub stdin: Option<Vec<u8>>,
    pub cwd: Option<PathBuf>,
    pub timeout_s: Option<u64>,
}

/// What a child process did.
#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Everything the CLI reads from or writes to the outside.
pub struct Env {
    /// The process environment.
    pub vars: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub stdin: Box<dyn Read + Send>,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    pub out: Box<dyn Write + Send>,
    pub err: Box<dyn Write + Send>,
    /// The HTTP client for the API, GitHub and Cloudflare; `None` builds the default one.
    pub http: Option<reqwest::Client>,
    /// `https://api.cloudflare.com/client/v4`, or a recorded fake.
    pub cloudflare_api: String,
    /// `https://api.github.com`, or a recorded fake.
    pub github_api: String,
    pub runner: Arc<dyn Runner>,
    /// A fixed clock (Unix milliseconds) for tests.
    pub now_ms: Option<i64>,
    /// The minisign keys release signatures must verify with; `None` is the compiled-in pair.
    pub release_keys: Option<Vec<String>>,
    /// Base URLs for hosts the CLI would reach as `https://{host}` (the deployment's own host in tests).
    pub hosts: BTreeMap<String, String>,
}

impl Env {
    /// The process's own environment, streams and services.
    pub fn system(runner: Arc<dyn Runner>) -> Self {
        use std::io::IsTerminal as _;
        Self {
            vars: std::env::vars().collect(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            stdin: Box::new(std::io::stdin()),
            stdin_tty: std::io::stdin().is_terminal(),
            stdout_tty: std::io::stdout().is_terminal(),
            stderr_tty: std::io::stderr().is_terminal(),
            out: Box::new(std::io::stdout()),
            err: Box::new(std::io::stderr()),
            http: None,
            cloudflare_api: cloudflare::API.to_owned(),
            github_api: bundle::release::GITHUB_API.to_owned(),
            runner,
            now_ms: None,
            release_keys: None,
            hosts: BTreeMap::new(),
        }
    }
}

/// One invocation: the parsed global flags, the environment and what was resolved so far.
pub struct Ctx {
    pub global: cli::Global,
    pub mode: Mode,
    /// Prompts are allowed: stdin and stderr are terminals and neither `--json` nor `--yes` was given.
    pub interactive: bool,
    pub vars: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub stdin: RefCell<Box<dyn Read + Send>>,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    out: RefCell<Box<dyn Write + Send>>,
    err: RefCell<Box<dyn Write + Send>>,
    pub http: reqwest::Client,
    pub cloudflare_api: String,
    pub github_api: String,
    pub runner: Arc<dyn Runner>,
    pub now_ms: Option<i64>,
    pub release_keys: Option<Vec<String>>,
    pub hosts: BTreeMap<String, String>,
    api: OnceCell<http::Api>,
    pub(crate) me: RefCell<Option<serde_json::Value>>,
}

impl Ctx {
    /// Writes to stdout.
    pub fn print(&self, s: &str) {
        let mut o = self.out.borrow_mut();
        let _ = o.write_all(s.as_bytes());
        let _ = o.flush();
    }

    /// Writes one line to stderr.
    pub fn note(&self, s: &str) {
        let mut e = self.err.borrow_mut();
        let _ = writeln!(e, "{s}");
        let _ = e.flush();
    }

    /// A variable of the environment, when set and not empty.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// `https://{host}`, or the base URL the environment maps the host to.
    pub fn base_for(&self, host: &str) -> String {
        self.hosts
            .get(host)
            .cloned()
            .unwrap_or_else(|| format!("https://{host}"))
    }

    /// Now, in Unix milliseconds.
    pub fn now_ms(&self) -> i64 {
        self.now_ms.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64)
        })
    }

    /// Reads all of stdin.
    pub fn read_stdin(&self) -> Result<Vec<u8>, CliError> {
        let mut buf = Vec::new();
        self.stdin
            .borrow_mut()
            .read_to_end(&mut buf)
            .map_err(|e| CliError::usage(format!("cannot read stdin: {e}")))?;
        Ok(buf)
    }

    /// The API client for the resolved URL and key (exit 3 without them).
    pub fn api(&self) -> Result<&http::Api, CliError> {
        if let Some(a) = self.api.get() {
            return Ok(a);
        }
        let profile = config::resolve(self)?;
        let url = profile.url.ok_or_else(|| {
            CliError::config("No API URL: pass --url, set PYLOTA_IDENTITY_URL or run idvy login.")
        })?;
        let key = profile.key.ok_or_else(|| {
            CliError::config(
                "No API key: set PYLOTA_IDENTITY_KEY, use a profile or run idvy login.",
            )
        })?;
        let api = http::Api::new(self, &url.value, Some(&key.value))?;
        Ok(self.api.get_or_init(|| api))
    }

    /// Asks a yes/no question on stderr; `--yes` answers yes, and a non-interactive run without it is
    /// exit 2 naming `--yes`.
    pub fn confirm(&self, question: &str) -> Result<(), CliError> {
        if self.global.yes {
            return Ok(());
        }
        if !self.interactive {
            return Err(CliError::usage(format!(
                "{question} Pass --yes to confirm."
            )));
        }
        let answer = self.prompt(&format!("{question} [y/N] "))?;
        if matches!(answer.trim(), "y" | "Y" | "yes") {
            Ok(())
        } else {
            Err(CliError::usage("Not confirmed."))
        }
    }

    /// Asks for one line on stderr and reads it from stdin.
    pub fn prompt(&self, question: &str) -> Result<String, CliError> {
        {
            let mut e = self.err.borrow_mut();
            let _ = write!(e, "{question}");
            let _ = e.flush();
        }
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        let mut stdin = self.stdin.borrow_mut();
        while stdin
            .read(&mut byte)
            .map_err(|e| CliError::usage(e.to_string()))?
            == 1
        {
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
        }
        Ok(String::from_utf8_lossy(&line)
            .trim_end_matches('\r')
            .to_owned())
    }
}

/// Runs one invocation and returns its exit code. Nothing in the CLI calls `std::process::exit`.
pub async fn run(args: Vec<String>, env: Env) -> i32 {
    let Env {
        vars,
        cwd,
        stdin,
        stdin_tty,
        stdout_tty,
        stderr_tty,
        mut out,
        mut err,
        http,
        cloudflare_api,
        github_api,
        runner,
        now_ms,
        release_keys,
        hosts,
    } = env;
    let parsed = match cli::Cli::try_parse_from(&args) {
        Ok(p) => p,
        Err(e) => {
            use clap::error::ErrorKind;
            let code = match e.kind() {
                ErrorKind::DisplayHelp
                | ErrorKind::DisplayVersion
                | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => 0,
                _ => 2,
            };
            let text = e.render().to_string();
            let json = args.iter().any(|a| a == "--json");
            if code == 2 && json {
                let doc = CliError::usage(text.trim().to_owned()).envelope();
                let _ = writeln!(out, "{doc}");
            } else if code == 0 {
                let _ = write!(out, "{text}");
            } else {
                let _ = write!(err, "{text}");
            }
            return code;
        }
    };
    let global = parsed.global.clone();
    let mode = if global.json {
        Mode::Json
    } else if global.quiet {
        Mode::Quiet
    } else {
        Mode::Human
    };
    let http = match http {
        Some(h) => h,
        None => match http::default_client() {
            Ok(h) => h,
            Err(e) => {
                let _ = writeln!(err, "error: cli_internal: {e}");
                return 1;
            }
        },
    };
    let ctx = Ctx {
        interactive: stdin_tty && stderr_tty && !global.json && !global.yes,
        global,
        mode,
        vars,
        cwd,
        stdin: RefCell::new(stdin),
        stdin_tty,
        stdout_tty,
        stderr_tty,
        out: RefCell::new(out),
        err: RefCell::new(err),
        http,
        cloudflare_api,
        github_api,
        runner,
        now_ms,
        release_keys,
        hosts,
        api: OnceCell::new(),
        me: RefCell::new(None),
    };
    if ctx.global.key.is_some() && ctx.mode == Mode::Human {
        ctx.note("warning: --key is visible to other users in the process list; prefer PYLOTA_IDENTITY_KEY or a profile.");
    }
    let result = commands::dispatch(&ctx, parsed.command).await;
    output::finish(&ctx, result)
}
