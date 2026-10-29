//! Rendering `<dir>/wrangler.toml` from the bundle's template ([CLI and setup §7]): this run's values
//! first, then the existing file's `[vars]` (operator edits are kept) and `[observability.traces]`,
//! then the template's defaults. A placeholder left over, invalid TOML or an invalid `PM_DOCS_URL` is
//! refused (exit 3).
//!
//! [CLI and setup §7]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#7-rendering-wranglertoml

use std::collections::BTreeMap;

use crate::CliError;

/// The rate-limit bindings, in the template's order.
pub const RATE_LIMITS: &[&str] = &[
    "RL_API",
    "RL_SEARCH",
    "RL_AGENTIC",
    "RL_SEND",
    "RL_SIGNIN",
    "RL_SIGN",
    "RL_PARTNER",
    "RL_TENANT",
    "RL_PARTNER_API",
    "RL_AI",
    "RL_EMBED",
];

/// What this run knows.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub name: String,
    pub api_host: String,
    pub console_host: Option<String>,
    pub platform_domain: String,
    pub jurisdiction: String,
    pub account_id: String,
    pub d1_id: String,
    pub rate_limits: BTreeMap<String, u32>,
    /// `[vars]` this run sets (`PM_DAILY_SEND_QUOTA`, `PM_BACKUP_BUCKET`, `PM_CONSOLE`, …).
    pub vars: BTreeMap<String, String>,
}

/// The queue and index prefix of a deployment name: `pm-` for the default, else `pm-{suffix}-`.
pub fn prefix(name: &str) -> String {
    match name.strip_prefix("pylota-identity-") {
        Some(s) => format!("pm-{s}-"),
        None => "pm-".to_owned(),
    }
}

/// A `KEY = "value"` of the `[vars]` table.
pub fn var(rendered: &str, key: &str) -> Option<String> {
    let doc: toml::Table = toml::from_str(&strip_marker(rendered)).ok()?;
    doc.get("vars")?.get(key)?.as_str().map(str::to_owned)
}

/// The rate-limit namespace IDs of a rendered file.
pub fn rate_limit_ids(rendered: &str) -> BTreeMap<String, u32> {
    let Ok(doc) = toml::from_str::<toml::Table>(&strip_marker(rendered)) else {
        return BTreeMap::new();
    };
    doc.get("ratelimits")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let id = r.get("namespace_id")?.as_str()?.parse().ok()?;
            Some((r.get("name")?.as_str()?.to_owned(), id))
        })
        .collect()
}

/// The `database_id` of the `DB` binding.
pub fn d1_id(rendered: &str) -> Option<String> {
    let doc: toml::Table = toml::from_str(&strip_marker(rendered)).ok()?;
    doc.get("d1_databases")?
        .as_array()?
        .iter()
        .find(|d| d.get("binding").and_then(|b| b.as_str()) == Some("DB"))?
        .get("database_id")?
        .as_str()
        .map(str::to_owned)
}

/// The deploy marker `# idvy: deployed v{version} sha256={hash}`, the file's last line.
pub const MARKER: &str = "# idvy: deployed ";

pub fn strip_marker(rendered: &str) -> String {
    rendered
        .lines()
        .filter(|l| !l.starts_with(MARKER))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// The file with the marker of a successful deploy of `version`.
pub fn mark_deployed(rendered: &str, version: &str) -> String {
    let body = strip_marker(rendered);
    let hash = super::verify::sha256_hex(body.as_bytes());
    format!("{body}{MARKER}v{version} sha256={hash}\n")
}

/// Whether the file carries a valid marker for `version`.
pub fn deployed(rendered: &str, version: &str) -> bool {
    let body = strip_marker(rendered);
    let hash = super::verify::sha256_hex(body.as_bytes());
    rendered
        .lines()
        .any(|l| l == format!("{MARKER}v{version} sha256={hash}"))
}

/// The `[vars]` lines of a file, as `KEY -> raw value text`.
fn vars_of(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_vars = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_vars = t == "[vars]";
            continue;
        }
        if !in_vars || t.starts_with('#') || t.is_empty() {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            let v = v.trim();
            let v = match v.find(" #") {
                Some(i) if v.starts_with('"') && v[..i].trim_end().ends_with('"') => {
                    v[..i].trim_end()
                }
                _ => v,
            };
            out.push((k.trim().to_owned(), v.to_owned()));
        }
    }
    out
}

fn quote(v: &str) -> String {
    toml::Value::String(v.to_owned()).to_string()
}

/// Renders the file.
pub fn render(template: &str, existing: Option<&str>, inp: &Inputs) -> Result<String, CliError> {
    let mut lines: Vec<String> = template.lines().map(str::to_owned).collect();
    let console = inp
        .console_host
        .clone()
        .unwrap_or_else(|| inp.api_host.clone());
    let prefix = prefix(&inp.name);
    let mut current_rl: Option<String> = None;
    let mut out: Vec<String> = Vec::new();
    for line in lines.drain(..) {
        let t = line.trim_start();
        let mut l = line.clone();
        if t.starts_with("PM_CONSOLE_HOST") {
            l = l.replace("{mail.example.com}", &console);
        }
        if t.starts_with("name = \"RL_") {
            current_rl = t.split('"').nth(1).map(str::to_owned);
        }
        if t.starts_with("namespace_id")
            && let Some(rl) = current_rl.take()
        {
            let id = inp
                .rate_limits
                .get(&rl)
                .ok_or_else(|| CliError::config(format!("No rate-limit namespace ID for {rl}.")))?;
            l = format!("namespace_id = \"{id}\"");
        }
        if t.starts_with("jurisdiction") && inp.jurisdiction == "default" {
            continue;
        }
        if t.starts_with("PM_JURISDICTION") {
            l = format!("PM_JURISDICTION        = {}", quote(&inp.jurisdiction));
        }
        if t.starts_with("jurisdiction") {
            l = format!("jurisdiction = {}", quote(&inp.jurisdiction));
        }
        l = l
            .replace("{agents.example}", &inp.platform_domain)
            .replace("{mail.example.com}", &inp.api_host)
            .replace("{PM_API_HOST}", &inp.api_host)
            .replace("{account-id}", &inp.account_id)
            .replace("{d1-id}", &inp.d1_id);
        if inp.name != "pylota-identity" {
            l = l
                .replace(
                    "\"pylota-identity-blobs\"",
                    &format!("\"{}-blobs\"", inp.name),
                )
                .replace("\"pylota-identity\"", &quote(&inp.name))
                .replace("\"pm-", &format!("\"{prefix}"));
        }
        out.push(l);
        if t.starts_with("routes = ") && console != inp.api_host {
            out.pop();
            out.push(format!(
                "routes = [ {{ pattern = {}, custom_domain = true }}, {{ pattern = {}, custom_domain = true }} ]",
                quote(&inp.api_host),
                quote(&console)
            ));
        }
    }
    let mut text = out.join("\n") + "\n";

    // [vars]: this run's values, then the existing file's (operator edits), then the template's.
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    if let Some(e) = existing {
        for (k, v) in vars_of(&strip_marker(e)) {
            vars.insert(k, v);
        }
    }
    for (k, v) in &inp.vars {
        vars.insert(k.clone(), quote(v));
    }
    vars.insert("PM_DEPLOYMENT_NAME".into(), quote(&inp.name));
    for (k, v) in [
        ("PM_PLATFORM_DOMAIN", &inp.platform_domain),
        ("PM_API_HOST", &inp.api_host),
        ("PM_JURISDICTION", &inp.jurisdiction),
        ("PM_CF_ACCOUNT_ID", &inp.account_id),
        ("PM_CONSOLE_HOST", &console),
    ] {
        vars.insert(k.to_owned(), quote(v));
    }
    text = set_vars(&text, &vars);
    if let Some(bucket) = inp.vars.get("PM_BACKUP_BUCKET") {
        let jur = if inp.jurisdiction == "default" {
            String::new()
        } else {
            format!("jurisdiction = {}\n", quote(&inp.jurisdiction))
        };
        text = text.replacen(
            "\n[[durable_objects.bindings]]",
            &format!(
                "\n[[r2_buckets]]\nbinding      = \"BACKUP\"\nbucket_name  = {}\n{jur}\n[[durable_objects.bindings]]",
                quote(bucket)
            ),
            1,
        );
    }
    if let Some(e) = existing {
        text = keep_traces(&text, &strip_marker(e));
    }
    validate(&text)?;
    Ok(text)
}

/// Replaces the template's `[vars]` values with `vars`, and appends the keys it lacks.
fn set_vars(text: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = Vec::new();
    let mut in_vars = false;
    let mut seen = std::collections::BTreeSet::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim();
        if t.starts_with('[') {
            in_vars = t == "[vars]";
        }
        if in_vars
            && !t.starts_with('#')
            && !t.starts_with('[')
            && let Some((k, _)) = t.split_once('=')
            && let Some(v) = vars.get(k.trim())
        {
            let k = k.trim().to_owned();
            out.push(format!("{k:<22} = {v}"));
            seen.insert(k);
            continue;
        }
        out.push(line.to_owned());
        let next_is_section = lines.peek().is_some_and(|n| n.trim().starts_with('['));
        if in_vars && (next_is_section || lines.peek().is_none()) {
            while out.last().is_some_and(|l: &String| l.trim().is_empty()) {
                out.pop();
            }
            for (k, v) in vars {
                if !seen.contains(k) {
                    out.push(format!("{k:<22} = {v}"));
                }
            }
            out.push(String::new());
            in_vars = false;
        }
    }
    out.join("\n") + "\n"
}

/// Keeps the existing `[observability.traces]` table.
fn keep_traces(text: &str, existing: &str) -> String {
    let section = |s: &str| -> Option<String> {
        let start = s.find("[observability.traces]")?;
        let rest = &s[start..];
        let end = rest[1..].find("\n[").map_or(rest.len(), |i| i + 1);
        Some(rest[..end].trim_end().to_owned())
    };
    match (section(text), section(existing)) {
        (Some(new), Some(old)) => text.replacen(&new, &old, 1),
        _ => text.to_owned(),
    }
}

fn validate(text: &str) -> Result<(), CliError> {
    for (i, line) in text.lines().enumerate() {
        let code = line.split(" #").next().unwrap_or("");
        if code.trim_start().starts_with('#') {
            continue;
        }
        if let Some(s) = code.find("\"{") {
            let rest = &code[s + 2..];
            if !rest.starts_with('}') {
                return Err(CliError::config(format!(
                    "wrangler.toml line {}: a placeholder is left: {}",
                    i + 1,
                    line.trim()
                )));
            }
        }
    }
    let doc: toml::Table = toml::from_str(text).map_err(|e| {
        CliError::config(format!("the rendered wrangler.toml is not valid TOML: {e}"))
    })?;
    if let Some(url) = doc
        .get("vars")
        .and_then(|v| v.get("PM_DOCS_URL"))
        .and_then(|v| v.as_str())
    {
        let ok = url.starts_with("https://") && !url.contains(['?', '#']) && url.len() <= 200;
        if !ok {
            return Err(CliError::config(format!(
                "PM_DOCS_URL {url:?} must be an https URL without a query or fragment, at most 200 bytes."
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            name: "pylota-identity".into(),
            api_host: "mail.example.com".into(),
            console_host: None,
            platform_domain: "agents.example".into(),
            jurisdiction: "eu".into(),
            account_id: "acc".into(),
            d1_id: "d1".into(),
            rate_limits: RATE_LIMITS
                .iter()
                .enumerate()
                .map(|(i, n)| ((*n).to_owned(), 1001 + i as u32))
                .collect(),
            vars: BTreeMap::new(),
        }
    }

    fn template() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/wrangler.toml.tmpl"
        ))
        .unwrap()
    }

    /// `cli::render::preserves_vars`: an operator's `[vars]` edit and `[observability.traces]` survive a
    /// re-render; the owned sections and this run's values come back.
    #[test]
    fn preserves_vars() {
        let first = render(&template(), None, &inputs()).unwrap();
        assert_eq!(
            var(&first, "PM_API_HOST").as_deref(),
            Some("mail.example.com")
        );
        assert_eq!(
            var(&first, "PM_CONSOLE_HOST").as_deref(),
            Some("mail.example.com")
        );
        assert_eq!(rate_limit_ids(&first)["RL_EMBED"], 1011);
        assert_eq!(d1_id(&first).as_deref(), Some("d1"));
        let edited = first
            .replace(
                "PM_TRUSTED_AUTHSERV_ID = \"\"",
                "PM_TRUSTED_AUTHSERV_ID = \"mx.cloudflare.net\"",
            )
            .replace(
                "enabled = false                                # staging",
                "enabled = true # staging",
            )
            .replace("binding = \"AI\"", "binding = \"AI_EDITED\"");
        let again = render(&template(), Some(&edited), &inputs()).unwrap();
        assert_eq!(
            var(&again, "PM_TRUSTED_AUTHSERV_ID").as_deref(),
            Some("mx.cloudflare.net")
        );
        assert!(again.contains("enabled = true # staging"));
        assert!(again.contains("binding = \"AI\""));
        assert_eq!(render(&template(), Some(&again), &inputs()).unwrap(), again);
    }

    #[test]
    fn jurisdiction_names_and_refusals() {
        let mut i = inputs();
        i.jurisdiction = "default".into();
        i.name = "pylota-identity-staging".into();
        i.console_host = Some("console.example.com".into());
        i.vars.insert("PM_BACKUP_BUCKET".into(), "backup".into());
        let r = render(&template(), None, &i).unwrap();
        assert!(!r.lines().any(|l| l.trim_start().starts_with("jurisdiction")));
        assert!(
            r.contains("\"pm-staging-inbound\"") && r.contains("\"pylota-identity-staging-blobs\"")
        );
        assert!(r.contains("console.example.com\", custom_domain = true }"));
        assert!(r.contains("binding      = \"BACKUP\""));
        let mut bad = inputs();
        bad.vars
            .insert("PM_DOCS_URL".into(), "http://docs.example.com".into());
        assert!(matches!(
            render(&template(), None, &bad),
            Err(CliError::Config(_))
        ));
        bad.vars.clear();
        bad.rate_limits.remove("RL_AI");
        assert!(matches!(
            render(&template(), None, &bad),
            Err(CliError::Config(_))
        ));
    }

    #[test]
    fn marker() {
        let r = render(&template(), None, &inputs()).unwrap();
        let m = mark_deployed(&r, "1.0.0");
        assert!(deployed(&m, "1.0.0") && !deployed(&m, "1.0.1") && !deployed(&r, "1.0.0"));
        assert!(!deployed(
            &m.replace(
                "PM_LOG_LEVEL           = \"info\"",
                "PM_LOG_LEVEL = \"debug\""
            ),
            "1.0.0"
        ));
    }
}
