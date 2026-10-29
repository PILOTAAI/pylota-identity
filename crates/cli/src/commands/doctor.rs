//! `idvy doctor` ([CLI and setup §10], [Observability §7.2]): every check is `pass`, `warn`, `fail` or
//! `skip`, each failing one with a fix; a Cloudflare error inside a check is that check's `fail`, so
//! doctor never exits 10. Exit 12 with a failure, 0 otherwise.
//!
//! [CLI and setup §10]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#10-doctor
//! [Observability §7.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/observability.md#72-idvy-doctor

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::{Value, json};

use crate::bundle::render;
use crate::cli::DoctorArgs;
use crate::cloudflare::Client;
use crate::commands::setup::{Deployment, GENERATED, METADATA, health_version, queue_names};
use crate::http::{Api, Idem, seg, ulid};
use crate::{CliError, Ctx, Output, VERSION, config};

#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub name: &'static str,
    pub status: &'static str,
    pub detail: String,
    pub fix: Option<String>,
}

fn pass(name: &'static str, detail: impl Into<String>) -> Check {
    Check {
        name,
        status: "pass",
        detail: detail.into(),
        fix: None,
    }
}

fn warn(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Check {
    Check {
        name,
        status: "warn",
        detail: detail.into(),
        fix: Some(fix.into()),
    }
}

fn fail(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Check {
    Check {
        name,
        status: "fail",
        detail: detail.into(),
        fix: Some(fix.into()),
    }
}

fn skip(name: &'static str, detail: impl Into<String>) -> Check {
    Check {
        name,
        status: "skip",
        detail: detail.into(),
        fix: None,
    }
}

/// A failed Cloudflare call as the check's failure.
fn from_err(name: &'static str, e: &CliError, fix: &str) -> Check {
    let detail = match e {
        CliError::Cloudflare {
            step,
            status,
            errors,
            ..
        } => format!(
            "{step}: HTTP {} {}",
            status.map_or_else(|| "-".to_owned(), |s| s.to_string()),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        other => other.envelope()["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
    };
    fail(name, detail, fix)
}

/// Every check, in the order of Observability § 7.2.
pub const CHECKS: &[&str] = &[
    "dns.platform",
    "routing.catch_all",
    "sending.domains",
    "sending.event_subscriptions",
    "bindings",
    "secrets",
    "observability",
    "worker.version",
    "health",
    "agent_signup",
    "alerts",
    "dlq",
    "quota",
    "web_bot_auth",
    "ses",
    "domains.orphans",
    "routing.leftover_subdomains",
    "cloudflare.zones",
    "security_txt",
    "mail_test",
    "phone",
    "vault",
];

/// What the checks share.
struct Env<'a> {
    ctx: &'a Ctx,
    rendered: String,
    cf: Option<Deployment<'a>>,
    cf_error: Option<CliError>,
    d1: Option<String>,
    base: String,
    mail_domain: String,
    name: String,
}

impl Env<'_> {
    fn var(&self, k: &str) -> Option<String> {
        render::var(&self.rendered, k).filter(|v| !v.is_empty())
    }

    /// The Cloudflare client, or the check's failure when there are no credentials.
    fn cf(&self, name: &'static str) -> Result<&Client<'_>, Check> {
        match (&self.cf, &self.cf_error) {
            (Some(d), _) => Ok(&d.cf),
            (None, Some(e)) => Err(from_err(
                name,
                e,
                "Export CLOUDFLARE_API_TOKEN and pass --account-id.",
            )),
            (None, None) => Err(fail(
                name,
                "no Cloudflare credentials",
                "Export CLOUDFLARE_API_TOKEN.",
            )),
        }
    }

    async fn d1(&self, name: &'static str, sql: &str) -> Result<Vec<Value>, Check> {
        let cf = self.cf(name)?;
        let db = self
            .d1
            .as_deref()
            .ok_or_else(|| fail(name, "no D1 database ID in wrangler.toml", "idvy setup"))?;
        cf.d1_query(db, sql, &[]).await.map_err(|e| {
            from_err(
                name,
                &e,
                "Check the D1 database and the token's D1 permission.",
            )
        })
    }

    async fn zone_id(&self, name: &'static str) -> Result<String, Check> {
        let cf = self.cf(name)?;
        match cf.zone(&self.mail_domain).await {
            Ok(Some(z)) => Ok(z["id"].as_str().unwrap_or("").to_owned()),
            Ok(None) => Err(fail(
                name,
                format!("{} is not a zone in this account", self.mail_domain),
                "idvy setup",
            )),
            Err(e) => Err(from_err(
                name,
                &e,
                "Check the account ID and the token's Zone permissions.",
            )),
        }
    }
}

pub async fn run(ctx: &Ctx, a: DoctorArgs) -> Result<Output, CliError> {
    let dir = crate::commands::path(ctx, &a.dir.dir);
    let rendered = std::fs::read_to_string(dir.join("wrangler.toml")).map_err(|_| {
        CliError::config(format!("{} has no wrangler.toml.", dir.display())).with_fix("idvy setup")
    })?;
    let (cf, cf_error) = match Deployment::open(ctx, &a.dir.dir) {
        Ok(d) => (Some(d), None),
        Err(e) => (None, Some(e)),
    };
    let api_host = render::var(&rendered, "PM_API_HOST").unwrap_or_default();
    let env = Env {
        ctx,
        d1: render::d1_id(&rendered),
        base: ctx.base_for(&api_host),
        mail_domain: render::var(&rendered, "PM_PLATFORM_DOMAIN").unwrap_or_default(),
        name: render::var(&rendered, "PM_DEPLOYMENT_NAME")
            .unwrap_or_else(|| "pylota-identity".into()),
        rendered,
        cf,
        cf_error,
    };
    for c in &a.checks {
        if !CHECKS.contains(&c.as_str()) {
            return Err(CliError::usage(format!(
                "Unknown check {c}; the checks are {}.",
                CHECKS.join(", ")
            )));
        }
    }
    let wanted = |n: &str| {
        if a.checks.is_empty() {
            n != "mail_test" || a.mail_test
        } else {
            a.checks.iter().any(|c| c == n)
        }
    };
    let mut checks = Vec::new();
    for name in CHECKS.iter().copied().filter(|n| wanted(n)) {
        let limit = if name == "mail_test" { 150 } else { 20 };
        let c = match tokio::time::timeout(Duration::from_secs(limit), one(&env, name)).await {
            Ok(c) => c,
            Err(_) => fail(
                name,
                "timeout",
                "Run the check again; a slow provider API answers later.",
            ),
        };
        checks.push(c);
    }
    Ok(report(checks))
}

/// The report: one line per check for people, `{ checks, summary }` for JSON, exit 12 with a failure.
pub fn report(checks: Vec<Check>) -> Output {
    let count = |s: &str| checks.iter().filter(|c| c.status == s).count();
    let (p, w, f, s) = (count("pass"), count("warn"), count("fail"), count("skip"));
    let mut human = String::new();
    for c in &checks {
        human.push_str(&format!(
            "{:<5} {:<28} {}\n",
            c.status,
            c.name,
            crate::output::terminal_safe(&c.detail)
        ));
        if let Some(fix) = &c.fix {
            human.push_str(&format!("      fix: {fix}\n"));
        }
    }
    human.push_str(&format!(
        "{p} passed, {w} warning(s), {f} failed, {s} skipped\n"
    ));
    let doc = json!({
        "checks": checks.iter().map(|c| json!({ "name": c.name, "status": c.status, "detail": c.detail, "fix": c.fix })).collect::<Vec<_>>(),
        "summary": { "pass": p, "warn": w, "fail": f, "skip": s },
    });
    Output {
        exit: if f > 0 { 12 } else { 0 },
        ..Output::object(doc).human(human).quiet(format!("{f}\n"))
    }
}

async fn one(env: &Env<'_>, name: &'static str) -> Check {
    match name {
        "dns.platform" => dns_platform(env).await,
        "routing.catch_all" => catch_all(env).await,
        "sending.domains" => sending_domains(env).await,
        "sending.event_subscriptions" => event_subscriptions(env).await,
        "bindings" => bindings(env).await,
        "secrets" => secrets(env).await,
        "observability" => observability(env),
        "worker.version" => match health_version(env.ctx, &env.base).await {
            Some(v) if v == VERSION => pass(name, format!("v{v}")),
            Some(v) => fail(name, format!("the Worker runs v{v}; this idvy is v{VERSION}"), "idvy upgrade"),
            None => fail(name, format!("{}/health does not answer", env.base), "idvy deploy"),
        },
        "health" => health(env).await,
        "agent_signup" => match env.var("PM_AGENT_SIGNUP").as_deref() {
            Some("on") => skip(name, "needs GET /v1/platform/status, which this deployment does not serve yet"),
            _ => skip(name, "PM_AGENT_SIGNUP is off"),
        },
        "alerts" => skip(name, "needs GET /v1/platform/status, which this deployment does not serve yet"),
        "dlq" => match env
            .d1(name, "SELECT queue, COUNT(*) AS n, MIN(first_seen_at) AS oldest FROM dlq_items WHERE redriven_at IS NULL GROUP BY queue")
            .await
        {
            Ok(rows) if rows.is_empty() => pass(name, "no open dead-letter items"),
            Ok(rows) => fail(
                name,
                rows.iter().map(|r| format!("{} {}", r["queue"].as_str().unwrap_or("?"), r["n"])).collect::<Vec<_>>().join(", "),
                "idvy dlq list (GET /v1/platform/dlq), then redrive or discard each item",
            ),
            Err(c) => c,
        },
        "quota" => quota(env).await,
        "web_bot_auth" => web_bot_auth(env).await,
        "ses" => match env.var("PM_SES_REGION") {
            None => skip(name, "PM_SES_REGION is not set"),
            Some(r) if env.ctx.var("AWS_ACCESS_KEY_ID").is_none() && env.ctx.var("AWS_PROFILE").is_none() => {
                skip(name, format!("PM_SES_REGION is {r}; no local AWS credentials"))
            }
            Some(r) => skip(name, format!("PM_SES_REGION is {r}; the SES account check comes with idvy setup ses")),
        },
        "domains.orphans" => orphans(env).await,
        "routing.leftover_subdomains" => leftover_subdomains(env).await,
        "cloudflare.zones" => match env.cf(name) {
            Ok(cf) => match cf.list("zones", "/zones", &[("account.id", cf.account.clone())]).await {
                Ok(z) if z.len() > 1000 => warn(name, format!("{} zones", z.len()), "Ask Cloudflare to confirm the account's zone limit."),
                Ok(z) => pass(name, format!("{} zones", z.len())),
                Err(e) => from_err(name, &e, "Check the token's Zone · Read permission."),
            },
            Err(c) => c,
        },
        "security_txt" => security_txt(env).await,
        "mail_test" => {
            let key = config::resolve(env.ctx).ok().and_then(|r| r.key.map(|k| k.value));
            let Some(key) = key else {
                return fail(name, "no API key", "Run with the bootstrap key or an identity key of idvy-doctor.");
            };
            let api = match Api::new(env.ctx, &env.base, Some(&key)) {
                Ok(a) => a,
                Err(e) => return from_err(name, &e, "Check the API URL."),
            };
            match mail_test(env.ctx, &api, &env.mail_domain).await {
                Ok(Some(authserv)) => pass(name, format!("delivered with verdict pass; authserv-id {authserv}")),
                Ok(None) => fail(name, "the test message did not arrive with verdict pass within 120 s", "Check dns.platform and routing.catch_all, then run again."),
                Err(e) => from_err(name, &e, "Check the key's permissions (messages:send, search:read, messages:read, erasure:manage)."),
            }
        }
        "phone" => match env.var("PM_PHONE").as_deref() {
            None | Some("off") => skip(name, "PM_PHONE is off"),
            Some(_) => skip(name, "the phone checks come with idvy setup phone"),
        },
        "vault" => match env.var("PM_VAULT").as_deref() {
            Some("on") => skip(name, "the vault checks come with the vault"),
            _ => skip(name, "PM_VAULT is off"),
        },
        _ => skip(name, "unknown"),
    }
}

/// A record value as DNS returns it: no quotes, no trailing dot, lower case.
fn canon(s: &str) -> String {
    s.split("\" \"")
        .collect::<String>()
        .trim_matches('"')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

/// The answers of one DNS-over-HTTPS query (JSON API).
async fn doh(ctx: &Ctx, resolver: &str, name: &str, rtype: &str) -> Result<Vec<String>, String> {
    let url = match resolver
        .strip_prefix("https://")
        .and_then(|r| r.split_once('/'))
    {
        Some((host, path)) => format!("{}/{path}", ctx.base_for(host)),
        None => resolver.to_owned(),
    };
    let r = ctx
        .http
        .get(&url)
        .query(&[("name", name), ("type", rtype)])
        .header("accept", "application/dns-json")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    Ok(v["Answer"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["data"].as_str().map(canon))
        .collect())
}

async fn dns_platform(env: &Env<'_>) -> Check {
    let name = "dns.platform";
    let cf = match env.cf(name) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let zone = match env.zone_id(name).await {
        Ok(z) => z,
        Err(c) => return c,
    };
    let mut expected: Vec<Value> = match cf.routing_dns(&zone).await {
        Ok(r) => r,
        Err(e) => return from_err(name, &e, "idvy setup (step 7)"),
    };
    if let Ok(subs) = cf.sending_subdomains(&zone).await
        && let Some(tag) = subs
            .iter()
            .find(|s| s["name"] == env.mail_domain.as_str())
            .and_then(|s| s["tag"].as_str())
    {
        expected.extend(cf.sending_dns(&zone, tag).await.unwrap_or_default());
    }
    let resolvers: Vec<String> = env
        .var("PM_DOH_RESOLVERS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .collect();
    let mut missing = Vec::new();
    for r in &expected {
        let (rtype, rname) = (
            r["type"].as_str().unwrap_or(""),
            r["name"].as_str().unwrap_or(""),
        );
        let want = canon(r["content"].as_str().unwrap_or(""));
        for res in &resolvers {
            let got = doh(env.ctx, res, rname, rtype).await.unwrap_or_default();
            if !got
                .iter()
                .any(|g| g == &want || g.ends_with(&format!(" {want}")))
            {
                missing.push(format!(
                    "{rtype} {rname} {} (on {res})",
                    r["content"].as_str().unwrap_or("")
                ));
                break;
            }
        }
    }
    if missing.is_empty() {
        pass(
            name,
            format!(
                "{} records match on {} resolvers",
                expected.len(),
                resolvers.len()
            ),
        )
    } else {
        fail(
            name,
            format!("missing or different: {}", missing.join("; ")),
            format!("Add the record(s): {}", missing.join("; ")),
        )
    }
}

async fn catch_all(env: &Env<'_>) -> Check {
    let name = "routing.catch_all";
    let cf = match env.cf(name) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let zone = match env.zone_id(name).await {
        Ok(z) => z,
        Err(c) => return c,
    };
    match cf.catch_all(&zone).await {
        Ok(r)
            if r["enabled"] == true
                && r["actions"].as_array().is_some_and(|a| {
                    a.len() == 1 && a[0]["type"] == "worker" && a[0]["value"] == json!([env.name])
                }) =>
        {
            pass(name, format!("catch-all → {}", env.name))
        }
        Ok(r) => fail(
            name,
            if r["enabled"] == true {
                "the catch-all does not target the Worker".to_owned()
            } else {
                "the catch-all is disabled".to_owned()
            },
            format!(
                "idvy setup (step 17), or PUT /zones/{zone}/email/routing/rules/catch_all with a worker action to {}",
                env.name
            ),
        ),
        Err(e) => from_err(name, &e, "idvy setup (step 17)"),
    }
}

async fn sending_domains(env: &Env<'_>) -> Check {
    let name = "sending.domains";
    let cf = match env.cf(name) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let rows = match env
        .d1(name, "SELECT name, zone_id FROM domains WHERE sending = 1 AND transport = 'cloudflare' AND state <> 'removed'")
        .await
    {
        Ok(r) => r,
        Err(c) => return c,
    };
    let mut problems = Vec::new();
    for r in &rows {
        let (dname, zone) = (
            r["name"].as_str().unwrap_or(""),
            r["zone_id"].as_str().unwrap_or(""),
        );
        match cf.sending_subdomains(zone).await {
            Ok(subs) => match subs.iter().find(|s| s["name"] == dname) {
                None => problems.push(format!("{dname} is not onboarded")),
                Some(s) if s["preview_enabled"] == true => {
                    problems.push(format!("{dname} has preview_enabled"))
                }
                _ => {}
            },
            Err(e) => return from_err(name, &e, "Check the token's Email Sending permission."),
        }
    }
    if problems.is_empty() {
        pass(name, format!("{} sending domain(s) onboarded", rows.len()))
    } else {
        fail(
            name,
            problems.join("; "),
            "Onboard the domain (idvy setup step 9), or PATCH …/email/sending/subdomains/{tag} {\"preview_enabled\": false}",
        )
    }
}

async fn event_subscriptions(env: &Env<'_>) -> Check {
    let name = "sending.event_subscriptions";
    let cf = match env.cf(name) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let rows = match env
        .d1(name, "SELECT name, event_subscription_id FROM domains WHERE sending = 1 AND transport = 'cloudflare' AND state <> 'removed'")
        .await
    {
        Ok(r) => r,
        Err(c) => return c,
    };
    let manual: Vec<&str> = rows
        .iter()
        .filter(|r| r["event_subscription_id"].is_null())
        .filter_map(|r| r["name"].as_str())
        .collect();
    let used: BTreeSet<&str> = rows
        .iter()
        .filter_map(|r| r["event_subscription_id"].as_str())
        .collect();
    let subs = match cf.event_subscriptions().await {
        Ok(s) => s,
        Err(e) => return from_err(name, &e, "Check the token's Queues permission."),
    };
    let ours = format!("{} ", env.name);
    let leftover: Vec<String> = subs
        .iter()
        .filter(|s| s["name"].as_str().is_some_and(|n| n.starts_with(&ours)))
        .filter_map(|s| s["id"].as_str())
        .filter(|id| !used.contains(id))
        .map(str::to_owned)
        .collect();
    if let Some(d) = manual.first() {
        return fail(
            name,
            format!("no event subscription for {}", manual.join(", ")),
            format!("idvy domains subscribe {d}"),
        );
    }
    if let Some(id) = leftover.first() {
        return fail(
            name,
            format!("left-over subscription(s): {}", leftover.join(", ")),
            format!("idvy wrangler queues subscription delete {id}"),
        );
    }
    pass(name, format!("{} subscription(s)", used.len()))
}

async fn bindings(env: &Env<'_>) -> Check {
    let name = "bindings";
    let cf = match env.cf(name) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let jur = env.var("PM_JURISDICTION").unwrap_or_else(|| "eu".into());
    let prefix = render::prefix(&env.name);
    let mut problems = Vec::new();
    match cf.d1_find(&env.name).await {
        Ok(Some(db)) if db["uuid"].as_str() != env.d1.as_deref() => {
            problems.push("the D1 database ID differs from wrangler.toml".to_owned())
        }
        Ok(Some(db)) if jur == "eu" && db["jurisdiction"].as_str().unwrap_or("") != "eu" => {
            problems.push("D1 is not in the eu jurisdiction".into())
        }
        Ok(Some(_)) => {}
        Ok(None) => problems.push(format!("no D1 database {}", env.name)),
        Err(e) => return from_err(name, &e, "Check the token's D1 permission."),
    }
    let blobs = format!("{}-blobs", env.name);
    let r2_jur = if jur == "default" {
        "default"
    } else {
        jur.as_str()
    };
    match cf.r2_bucket(&blobs, r2_jur).await {
        Ok(Some(_)) => {}
        Ok(None) => problems.push(format!("no R2 bucket {blobs} in {r2_jur}")),
        Err(e) => return from_err(name, &e, "Check the token's R2 permission."),
    }
    match cf.queues().await {
        Ok(qs) => {
            let have: BTreeSet<String> = qs
                .iter()
                .filter_map(|q| q["queue_name"].as_str().map(str::to_owned))
                .collect();
            for q in queue_names(&prefix) {
                if !have.contains(&q) {
                    problems.push(format!("no queue {q}"));
                }
            }
        }
        Err(e) => return from_err(name, &e, "Check the token's Queues permission."),
    }
    let index = format!("{prefix}mail-chunks");
    match cf.vectorize_index(&index).await {
        Ok(Some(i)) => {
            let model = i["description"]
                .as_str()
                .and_then(|d| d.split("embed_model=").nth(1))
                .unwrap_or("");
            let dims = i["config"]["dimensions"].as_u64().unwrap_or(0);
            if i["config"]["metric"] != "cosine" {
                problems.push(format!("{index} is not cosine"));
            }
            if model == "@cf/baai/bge-m3" && dims != 1024 {
                problems.push(format!(
                    "{index} has {dims} dimensions, not the 1024 of {model}"
                ));
            }
            match cf.vectorize_metadata(&index).await {
                Ok(m) => {
                    let have: BTreeSet<&str> = m
                        .iter()
                        .filter_map(|x| x["propertyName"].as_str())
                        .collect();
                    let lacking: Vec<&str> = METADATA
                        .iter()
                        .map(|(p, _)| *p)
                        .filter(|p| !have.contains(p))
                        .collect();
                    if !lacking.is_empty() {
                        problems.push(format!(
                            "{index} lacks metadata indexes {}",
                            lacking.join(",")
                        ));
                    }
                }
                Err(e) => return from_err(name, &e, "Check the token's Vectorize permission."),
            }
        }
        Ok(None) => problems.push(format!("no Vectorize index {index}")),
        Err(e) => return from_err(name, &e, "Check the token's Vectorize permission."),
    }
    if !env.rendered.contains("binding = \"METRICS\"") {
        problems.push("the METRICS binding is missing".into());
    }
    if problems.is_empty() {
        pass(name, "D1, R2, queues, Vectorize and METRICS present")
    } else {
        fail(
            name,
            problems.join("; "),
            "idvy setup creates what is missing",
        )
    }
}

async fn secrets(env: &Env<'_>) -> Check {
    let name = "secrets";
    let cf = match env.cf(name) {
        Ok(c) => c,
        Err(c) => return c,
    };
    match cf.secret_names(&env.name).await {
        Ok(Some(names)) => {
            let mut required: Vec<&str> = GENERATED.to_vec();
            required.push("PM_KEY_PEPPER");
            let lacking: Vec<&str> = required
                .into_iter()
                .filter(|s| !names.iter().any(|n| n == s))
                .collect();
            if lacking.is_empty() {
                pass(name, format!("{} secrets set", names.len()))
            } else {
                fail(
                    name,
                    format!("missing: {}", lacking.join(", ")),
                    "idvy setup generates and uploads the missing secrets",
                )
            }
        }
        Ok(None) => fail(name, format!("no Worker {}", env.name), "idvy setup"),
        Err(e) => from_err(name, &e, "Check the token's Workers permission."),
    }
}

fn observability(env: &Env<'_>) -> Check {
    let name = "observability";
    let doc: toml::Table = match toml::from_str(&render::strip_marker(&env.rendered)) {
        Ok(d) => d,
        Err(e) => return fail(name, e.to_string(), "idvy deploy re-renders wrangler.toml"),
    };
    let obs = doc.get("observability");
    let invocation = obs
        .and_then(|o| o.get("logs"))
        .and_then(|l| l.get("invocation_logs"))
        .and_then(|v| v.as_bool());
    let traces = obs
        .and_then(|o| o.get("traces"))
        .and_then(|t| t.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let production = env.var("PM_ENV").as_deref().unwrap_or("production") == "production";
    if invocation != Some(false) {
        return fail(
            name,
            "invocation_logs is not false",
            "[observability.logs]\ninvocation_logs = false",
        );
    }
    if traces && production {
        return fail(
            name,
            "traces are enabled in production",
            "[observability.traces]\nenabled = false",
        );
    }
    pass(name, "invocation logs off")
}

async fn health(env: &Env<'_>) -> Check {
    let name = "health";
    match crate::http::get_public(env.ctx, &env.base, "/health").await {
        Ok(r) => {
            let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            if v["frozen"] == true {
                warn(
                    name,
                    "the deployment is frozen",
                    "idvy ops unfreeze once the restore is done",
                )
            } else if let Some(d) = v["degraded"].as_array().filter(|d| !d.is_empty()) {
                warn(
                    name,
                    format!(
                        "degraded: {}",
                        d.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    "Set the missing variable or secret (Observability § 7.1).",
                )
            } else {
                pass(name, "200")
            }
        }
        Err(e) => from_err(
            name,
            &e,
            "See Observability § 7.1: set the missing variable or secret, then idvy deploy.",
        ),
    }
}

async fn quota(env: &Env<'_>) -> Check {
    let name = "quota";
    let cf = match env.cf(name) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let sql = "SELECT SUM(_sample_interval) AS n FROM pylota_identity_metrics WHERE blob1 = 'provider_quota_errors_total' AND timestamp > NOW() - INTERVAL '1' DAY";
    let r = env
        .ctx
        .http
        .post(format!(
            "{}{}",
            env.ctx.cloudflare_api,
            cf.acct("/analytics_engine/sql")
        ))
        .bearer_auth(cf.token())
        .body(sql)
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let errors = match r {
        Ok(r) if r.status().as_u16() == 403 || r.status().as_u16() == 401 => {
            return warn(
                name,
                "the token cannot read Analytics Engine",
                "Add Account Analytics · Read to the token (Deploy to Cloudflare › step 2).",
            );
        }
        Ok(r) if r.status().is_success() => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            v["data"][0]["n"]
                .as_f64()
                .or_else(|| v["data"][0]["n"].as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(0.0)
        }
        Ok(r) => {
            return fail(
                name,
                format!("Analytics Engine SQL API: HTTP {}", r.status().as_u16()),
                "Run the check again.",
            );
        }
        Err(e) => return fail(name, e.to_string(), "Run the check again."),
    };
    if errors > 0.0 {
        return fail(
            name,
            format!("{errors} provider quota errors in the last 24 hours"),
            "See Observability › Quota exhausted.",
        );
    }
    if env.var("PM_DAILY_SEND_QUOTA").is_none() {
        return warn(
            name,
            "PM_DAILY_SEND_QUOTA is not set",
            "idvy setup --daily-send-quota <n> (or set it under [vars] and idvy deploy)",
        );
    }
    pass(name, "no provider quota errors in the last 24 hours")
}

async fn web_bot_auth(env: &Env<'_>) -> Check {
    let name = "web_bot_auth";
    if env.var("PM_WEB_BOT_AUTH").as_deref() != Some("on") {
        return skip(name, "PM_WEB_BOT_AUTH is off");
    }
    let r = env
        .ctx
        .http
        .get(format!(
            "{}/.well-known/http-message-signatures-directory",
            env.base
        ))
        .send()
        .await;
    let fix = "idvy keys rotate web_bot_auth; Deploy › Signed HTTP requests";
    match r {
        Ok(r) if r.status().as_u16() == 200 => {
            let ct = r
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let v: Value = r.json().await.unwrap_or(Value::Null);
            let n = v["keys"].as_array().map_or(0, Vec::len);
            if !ct.starts_with("application/http-message-signatures-directory+json") {
                fail(name, format!("content type {ct}"), fix)
            } else if !(1..=3).contains(&n) {
                fail(name, format!("{n} keys listed"), fix)
            } else {
                pass(name, format!("{n} key(s) listed"))
            }
        }
        Ok(r) => fail(name, format!("HTTP {}", r.status().as_u16()), fix),
        Err(e) => fail(name, e.to_string(), fix),
    }
}

async fn orphans(env: &Env<'_>) -> Check {
    let name = "domains.orphans";
    let now = env.ctx.now_ms();
    let claims = match env
        .d1(
            name,
            &format!(
                "SELECT zone_name FROM zone_claims WHERE state = 'pending' AND created_at < {}",
                now - 3_600_000
            ),
        )
        .await
    {
        Ok(r) => r,
        Err(c) => return c,
    };
    let journal = match env
        .d1(
            name,
            &format!(
                "SELECT name FROM domain_onboarding WHERE created_at < {}",
                now - 7_200_000
            ),
        )
        .await
    {
        Ok(r) => r,
        Err(c) => return c,
    };
    let left: Vec<String> = claims
        .iter()
        .filter_map(|r| r["zone_name"].as_str().map(|z| format!("zone claim {z}")))
        .chain(
            journal
                .iter()
                .filter_map(|r| r["name"].as_str().map(|n| format!("onboarding {n}"))),
        )
        .collect();
    if left.is_empty() {
        pass(name, "no leftovers")
    } else {
        warn(
            name,
            left.join(", "),
            "Delete the zone in the dashboard if no domain uses it (Identities and domains › Cleanup after a failed add).",
        )
    }
}

async fn leftover_subdomains(env: &Env<'_>) -> Check {
    let name = "routing.leftover_subdomains";
    let cf = match env.cf(name) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let removed = match env.d1(name, "SELECT name, zone_id FROM domains WHERE state = 'removed' AND inbound = 'routing' AND is_apex = 0").await {
        Ok(r) => r,
        Err(c) => return c,
    };
    let live = match env
        .d1(name, "SELECT name FROM domains WHERE state <> 'removed'")
        .await
    {
        Ok(r) => r,
        Err(c) => return c,
    };
    let live: BTreeSet<&str> = live.iter().filter_map(|r| r["name"].as_str()).collect();
    let mut left = Vec::new();
    for r in &removed {
        let (n, z) = (
            r["name"].as_str().unwrap_or(""),
            r["zone_id"].as_str().unwrap_or(""),
        );
        if live.contains(n) {
            continue;
        }
        if let Ok(recs) = cf
            .get(
                "Email Routing DNS",
                &format!("/zones/{z}/email/routing/dns"),
                &[("subdomain", n.to_owned())],
            )
            .await
            && recs.as_array().is_some_and(|a| !a.is_empty())
        {
            left.push(n.to_owned());
        }
    }
    if left.is_empty() {
        pass(name, "none")
    } else {
        warn(
            name,
            left.join(", "),
            "Remove them in the dashboard: Email Routing › Settings › Subdomains.",
        )
    }
}

async fn security_txt(env: &Env<'_>) -> Check {
    let name = "security_txt";
    if env.var("PM_SECURITY_CONTACT").is_none() {
        return warn(
            name,
            "PM_SECURITY_CONTACT is not set",
            "Set PM_SECURITY_CONTACT under [vars] and idvy deploy.",
        );
    }
    match crate::http::get_public(env.ctx, &env.base, "/.well-known/security.txt").await {
        Ok(r) => {
            let text = String::from_utf8_lossy(&r.body).into_owned();
            let expires = text
                .lines()
                .find_map(|l| l.strip_prefix("Expires:"))
                .map(str::trim)
                .and_then(pylota_identity_core::days::parse_rfc3339);
            match expires {
                Some(t) if t - env.ctx.now_ms() < 30 * 86_400_000 => {
                    fail(name, "Expires is within 30 days", "idvy upgrade")
                }
                Some(_) => pass(name, "Expires is more than 30 days away"),
                None => fail(name, "no Expires field", "idvy upgrade"),
            }
        }
        Err(e) => from_err(name, &e, "idvy deploy"),
    }
}

/// The mail test ([CLI and setup §10]): the `idvy-doctor` identity sends to its own platform address and
/// waits for it; `Some(authserv-id)` when it arrives with verdict `pass`. Both test messages are erased.
///
/// [CLI and setup §10]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#10-doctor
pub async fn mail_test(
    ctx: &Ctx,
    api: &Api,
    mail_domain: &str,
) -> Result<Option<String>, CliError> {
    let me = api.get(ctx, "/v1/me", &[]).await?;
    let idn = if me["level"] == "identity" {
        let id = me["identity_id"].as_str().unwrap_or("").to_owned();
        let i = api
            .get(ctx, &format!("/v1/identities/{}", seg(&id)), &[])
            .await?;
        if i["username"] != "idvy-doctor" {
            return Err(CliError::usage(
                "The mail test runs with a platform key or the identity key of idvy-doctor.",
            ));
        }
        id
    } else {
        let tenants = api
            .list(ctx, "/v1/tenants", &[("limit", "100".into())], true)
            .await?;
        let t = tenants["data"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["address_suffix"] == "")
            .and_then(|t| t["id"].as_str().map(str::to_owned))
            .ok_or_else(|| CliError::usage("No default tenant."))?;
        let body = json!({
            "username": "idvy-doctor", "display_name": "idvy doctor", "client_id": "idvy:doctor",
            "owner": { "name": "Operator", "email": format!("postmaster@{mail_domain}") },
            "send_policy": { "require_known_recipient": true },
        });
        let i = api
            .call(
                ctx,
                "POST",
                &format!("/v1/tenants/{}/identities", seg(&t)),
                &[],
                Some(&body),
                Idem::Key("idvy-doctor-identity".into()),
            )
            .await?;
        i["id"].as_str().unwrap_or("").to_owned()
    };
    let id = ulid(ctx.now_ms());
    let subject = format!("idvy doctor {id}");
    let to = format!("idvy-doctor@{mail_domain}");
    let sent = api
        .call(
            ctx,
            "POST",
            &format!("/v1/identities/{}/messages", seg(&idn)),
            &[],
            Some(&json!({ "to": [to], "subject": subject, "text": "idvy doctor mail test" })),
            Idem::Key(format!("idvy-doctor-{id}")),
        )
        .await?;
    let mut inbound = Value::Null;
    for _ in 0..2 {
        let w = api
            .get(
                ctx,
                &format!("/v1/identities/{}/wait", seg(&idn)),
                &[
                    ("subject_contains", subject.clone()),
                    ("timeout", "60".into()),
                ],
            )
            .await?;
        if w["timed_out"] != true {
            inbound = w["message"].clone();
            break;
        }
    }
    let mut result = None;
    if let Some(mid) = inbound["id"].as_str() {
        if inbound["trust"]["verdict"] == "pass" {
            let raw = api
                .send(
                    ctx,
                    "GET",
                    &format!("/v1/identities/{}/messages/{}/raw", seg(&idn), seg(mid)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?;
            let text = String::from_utf8_lossy(&raw.body);
            result = text
                .lines()
                .find_map(|l| {
                    l.strip_prefix("Authentication-Results:")
                        .or_else(|| l.strip_prefix("authentication-results:"))
                })
                .and_then(|v| v.split(';').next())
                .map(|s| s.trim().to_owned())
                .or(Some(String::new()));
        }
        let _ = api
            .call(
                ctx,
                "DELETE",
                &format!("/v1/identities/{}/messages/{}", seg(&idn), seg(mid)),
                &[],
                None,
                Idem::None,
            )
            .await;
    }
    if let Some(out) = sent["id"].as_str() {
        let _ = api
            .call(
                ctx,
                "DELETE",
                &format!("/v1/identities/{}/messages/{}", seg(&idn), seg(out)),
                &[],
                None,
                Idem::None,
            )
            .await;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    /// Every failing or warning check carries a fix, and the exit code follows the worst status.
    #[test]
    fn report_exit_codes() {
        let w = super::report(vec![
            super::pass("health", "200"),
            super::warn("quota", "unset", "set it"),
        ]);
        assert_eq!(w.exit, 0);
        let f = super::report(vec![super::fail("secrets", "missing", "idvy setup")]);
        assert_eq!(f.exit, 12);
        assert_eq!(f.doc["summary"]["fail"], 1);
    }
}
