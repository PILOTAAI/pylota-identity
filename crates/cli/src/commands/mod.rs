//! One module per group of commands. Each command is `async fn(&Ctx, args) -> Result<Output, CliError>`;
//! [`crate::output::finish`] renders the result and maps an error to its exit code.

pub mod admin;
pub mod deploy;
pub mod doctor;
pub mod domains;
pub mod identities;
pub mod mail;
pub mod ops;
pub mod opsctl;
pub mod privacy;
pub mod profile;
pub mod search;
pub mod secrets;
pub mod setup;
pub mod webhooks;

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::cli::{Command, Paging};
use crate::{CliError, Ctx, Output};

pub async fn dispatch(ctx: &Ctx, cmd: Command) -> Result<Output, CliError> {
    match cmd {
        Command::Setup(a) => setup::run(ctx, a).await,
        Command::Deploy(a) => deploy::deploy(ctx, a).await,
        Command::Upgrade(a) => deploy::upgrade(ctx, a).await,
        Command::Doctor(a) => doctor::run(ctx, a).await,
        Command::Wrangler(a) => deploy::wrangler(ctx, a).await,
        Command::Login(a) => profile::login(ctx, a).await,
        Command::Config(c) => profile::config(ctx, c).await,
        Command::Mcp(c) => profile::mcp(ctx, c).await,
        Command::Tenants(c) => admin::tenants(ctx, c).await,
        Command::Partners(c) => admin::partners(ctx, c).await,
        Command::Keys(c) => admin::keys(ctx, c).await,
        Command::Secrets(c) => secrets::secrets(ctx, c).await,
        Command::Dlq(c) => ops::dlq(ctx, c).await,
        Command::Ops(c) => opsctl::ops(ctx, c).await,
        Command::Jobs(c) => ops::jobs(ctx, c).await,
        Command::Audit(a) => admin::audit(ctx, a).await,
        Command::Identities(c) => identities::identities(ctx, c).await,
        Command::Addresses(c) => identities::addresses(ctx, c).await,
        Command::Domains(c) => domains::run(ctx, c).await,
        Command::Send(a) => mail::send(ctx, a).await,
        Command::Reply(a) => mail::reply(ctx, a, "reply").await,
        Command::ReplyAll(a) => mail::reply(ctx, a, "reply-all").await,
        Command::Forward(a) => mail::forward(ctx, a).await,
        Command::Cancel(a) => mail::cancel(ctx, a).await,
        Command::Resolve(a) => mail::resolve(ctx, a).await,
        Command::Threads(c) => mail::threads(ctx, c).await,
        Command::Messages(c) => mail::messages(ctx, c).await,
        Command::Wait(a) => mail::wait(ctx, a).await,
        Command::Triage(c) => mail::triage(ctx, c).await,
        Command::Quarantine(c) => mail::quarantine(ctx, c).await,
        Command::Search(a) => search::search(ctx, a).await,
        Command::Ask(a) => search::ask(ctx, a).await,
        Command::Webhooks(c) => webhooks::run(ctx, c).await,
        Command::Suppressions(c) => privacy::suppressions(ctx, c).await,
        Command::Lists(c) => privacy::lists(ctx, c).await,
        Command::Erasure(c) => privacy::erasure(ctx, c).await,
        Command::Export(c) => privacy::export(ctx, c).await,
    }
}

/// A JSON object built from the flags that were given.
#[derive(Default)]
pub struct Obj(pub Map<String, Value>);

impl Obj {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `k` when `v` is given.
    pub fn opt<T: Into<Value>>(mut self, k: &str, v: Option<T>) -> Self {
        if let Some(v) = v {
            self.0.insert(k.to_owned(), v.into());
        }
        self
    }

    pub fn set<T: Into<Value>>(mut self, k: &str, v: T) -> Self {
        self.0.insert(k.to_owned(), v.into());
        self
    }

    /// Sets `k` when the list is not empty.
    pub fn list(mut self, k: &str, v: &[String]) -> Self {
        if !v.is_empty() {
            self.0.insert(k.to_owned(), Value::from(v.to_vec()));
        }
        self
    }

    /// Sets `k` when the flag is set.
    pub fn flag(mut self, k: &str, on: bool) -> Self {
        if on {
            self.0.insert(k.to_owned(), Value::Bool(true));
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn value(self) -> Value {
        Value::Object(self.0)
    }
}

/// `key=value` pairs as an object.
pub fn metadata(items: &[String]) -> Result<Option<Value>, CliError> {
    if items.is_empty() {
        return Ok(None);
    }
    let mut m = Map::new();
    for i in items {
        let (k, v) = i
            .split_once('=')
            .ok_or_else(|| CliError::usage(format!("--metadata takes key=value, not {i:?}.")))?;
        m.insert(k.to_owned(), Value::String(v.to_owned()));
    }
    Ok(Some(Value::Object(m)))
}

/// A comma-separated list.
pub fn csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Repeated flags that may also hold commas.
pub fn flat(items: &[String]) -> Vec<String> {
    items.iter().flat_map(|i| split_recipients(i)).collect()
}

/// Splits `a@x, "Name, Jr" <b@y>` on the commas outside quotes and angle brackets.
pub fn split_recipients(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut angle) = (false, false);
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' if !quoted && !angle => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_owned());
                }
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_owned());
    }
    out
}

/// A path relative to the working directory.
pub fn path(ctx: &Ctx, p: &str) -> PathBuf {
    let p = PathBuf::from(p);
    if p.is_absolute() { p } else { ctx.cwd.join(p) }
}

pub fn read_file(ctx: &Ctx, p: &str) -> Result<Vec<u8>, CliError> {
    std::fs::read(path(ctx, p)).map_err(|e| CliError::usage(format!("{p}: {e}")))
}

pub fn read_text(ctx: &Ctx, p: &str) -> Result<String, CliError> {
    String::from_utf8(read_file(ctx, p)?).map_err(|_| CliError::usage(format!("{p} is not UTF-8.")))
}

/// A JSON object from a flag or a file.
pub fn json_arg(
    ctx: &Ctx,
    inline: Option<&str>,
    file: Option<&str>,
) -> Result<Option<Value>, CliError> {
    let text = match (inline, file) {
        (Some(s), _) => s.to_owned(),
        (None, Some(f)) => read_text(ctx, f)?,
        (None, None) => return Ok(None),
    };
    let v: Value =
        serde_json::from_str(&text).map_err(|e| CliError::usage(format!("not JSON: {e}")))?;
    if !v.is_object() {
        return Err(CliError::usage("The policy must be a JSON object."));
    }
    Ok(Some(v))
}

/// A time flag: RFC 3339, or a date meaning midnight UTC.
pub fn time(s: &str) -> Result<String, CliError> {
    if pylota_identity_core::days::parse_rfc3339(s).is_some() {
        return Ok(s.to_owned());
    }
    let full = format!("{s}T00:00:00Z");
    if s.len() == 10 && pylota_identity_core::days::parse_rfc3339(&full).is_some() {
        return Ok(full);
    }
    Err(CliError::usage(format!(
        "{s} is neither an RFC 3339 time nor a date (2026-10-09)."
    )))
}

/// The query of a list page.
pub fn paging(p: &Paging, q: &mut Vec<(&'static str, String)>) {
    if let Some(l) = p.limit {
        q.push(("limit", l.to_string()));
    }
    if let Some(c) = &p.cursor {
        q.push(("cursor", c.clone()));
    }
}

/// Adds `(k, v)` when `v` is given.
pub fn q_opt(q: &mut Vec<(&'static str, String)>, k: &'static str, v: Option<impl ToString>) {
    if let Some(v) = v {
        q.push((k, v.to_string()));
    }
}

/// Writes bytes to `--out` (mode `0600`), or to stdout when it is not a terminal; a terminal is refused,
/// so binary or hostile bytes never reach it.
pub fn write_bytes(ctx: &Ctx, out: Option<&str>, bytes: &[u8]) -> Result<Output, CliError> {
    match out {
        Some(p) => {
            crate::config::write_private(&path(ctx, p), bytes)
                .map_err(|e| CliError::usage(format!("{p}: {e}")))?;
            Ok(
                Output::object(serde_json::json!({ "written": p, "bytes": bytes.len() }))
                    .human(format!("Wrote {} bytes to {p}\n", bytes.len())),
            )
        }
        None if ctx.stdout_tty => Err(CliError::usage(
            "Refusing to write raw bytes to a terminal; pass --out <file> or redirect stdout.",
        )),
        None => {
            ctx.print(&String::from_utf8_lossy(bytes));
            Ok(Output {
                doc: Value::Null,
                human: Some(String::new()),
                quiet: Some(String::new()),
                ..Output::default()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn recipients_split_outside_quotes() {
        assert_eq!(
            super::split_recipients(r#"a@x.example, "Patel, Sam" <sam@y.example>,b@z.example"#),
            vec![
                "a@x.example",
                r#""Patel, Sam" <sam@y.example>"#,
                "b@z.example"
            ]
        );
    }

    #[test]
    fn times() {
        assert_eq!(super::time("2026-10-09").unwrap(), "2026-10-09T00:00:00Z");
        assert!(super::time("2026-10-09T10:00:00Z").is_ok());
        assert!(super::time("yesterday").is_err());
    }
}
