//! `idvy setup` and `idvy domains subscribe` ([CLI and setup §6, §18.4]): every Cloudflare resource is
//! looked up before it is created, so a second run creates nothing and reports `exists` for all
//! (FR-OPS-1).
//!
//! [CLI and setup §6, §18.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#6-setup

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use crate::bundle::{migrate, release, render};
use crate::cli::SetupArgs;
use crate::cloudflare::{self, Client};
use crate::http::{Api, Idem, seg, ulid};
use crate::wrangler::Wrangler;
use crate::{CliError, Ctx, Mode, Output, VERSION, config, resolve};

/// One line of setup's report.
#[derive(Clone, Debug)]
pub struct Step {
    pub step: &'static str,
    pub status: &'static str,
    pub resource: String,
    pub id: String,
}

/// The steps so far, printed as they happen in human mode.
#[derive(Default)]
pub struct Steps(pub Vec<Step>);

impl Steps {
    pub fn add(
        &mut self,
        ctx: &Ctx,
        step: &'static str,
        status: &'static str,
        resource: impl Into<String>,
        id: impl Into<String>,
    ) {
        let s = Step {
            step,
            status,
            resource: resource.into(),
            id: id.into(),
        };
        if ctx.mode == Mode::Human {
            ctx.note(&format!(
                "{:<8} {:<26} {} {}",
                s.status, s.step, s.resource, s.id
            ));
        }
        self.0.push(s);
    }

    pub fn json(&self) -> Value {
        Value::Array(
            self.0
                .iter()
                .map(|s| json!({ "step": s.step, "status": s.status, "resource": s.resource, "id": s.id }))
                .collect(),
        )
    }
}

/// 32 bytes from the OS CSPRNG, base64.
pub fn secret32() -> Result<(String, [u8; 32]), CliError> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| CliError::internal(format!("no random source: {e}")))?;
    Ok((STANDARD.encode(b), b))
}

/// `n` lower-case base32 characters.
fn token(n: usize) -> Result<String, CliError> {
    const A: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).map_err(|e| CliError::internal(format!("no random source: {e}")))?;
    Ok(b.iter().map(|x| A[(*x & 31) as usize] as char).collect())
}

/// A deployment name: 3–40 lower-case letters, digits and hyphens, starting with `pylota-identity`.
fn valid_name(n: &str) -> bool {
    (3..=40).contains(&n.len())
        && n.starts_with("pylota-identity")
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// An R2 bucket name: 3–63 of `[a-z0-9-]`, not starting or ending with `-`.
fn valid_bucket(n: &str) -> bool {
    (3..=63).contains(&n.len())
        && !n.starts_with('-')
        && !n.ends_with('-')
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The required generated secrets besides the pepper.
pub const GENERATED: &[&str] = &["PM_MASTER_KEY", "PM_HASH_KEY"];

/// The queues, dead-letter queues first.
pub fn queue_names(prefix: &str) -> Vec<String> {
    let base = [
        "inbound",
        "outbound",
        "delivery-events",
        "webhooks",
        "index",
        "screen",
    ];
    base.iter()
        .map(|q| format!("{prefix}{q}-dlq"))
        .chain(base.iter().map(|q| format!("{prefix}{q}")))
        .collect()
}

/// The Vectorize metadata indexes ([Data model]).
pub const METADATA: &[(&str, &str)] = &[
    ("identity_id", "string"),
    ("thread_id", "string"),
    ("sent_at", "number"),
    ("sender_domain", "string"),
    ("direction", "string"),
    ("has_attachment", "boolean"),
    ("verdict", "string"),
    ("kind", "string"),
];

/// The two lifecycle rules setup owns.
fn lifecycle_rules() -> [Value; 2] {
    [
        json!({ "id": "pm-inbound-staging", "enabled": true, "conditions": { "prefix": "inbound-staging/" },
                "deleteObjectsTransition": { "condition": { "type": "Age", "maxAge": 1_296_000 } } }),
        json!({ "id": "pm-abort-multipart", "enabled": true, "conditions": { "prefix": "" },
                "abortMultipartUploadsTransition": { "condition": { "type": "Age", "maxAge": 86_400 } } }),
    ]
}

/// The existing rules with setup's two added or replaced; `None` when nothing changes.
pub fn merge_lifecycle(existing: &[Value]) -> Option<Vec<Value>> {
    let ours = lifecycle_rules();
    let ids: BTreeSet<&str> = ours.iter().filter_map(|r| r["id"].as_str()).collect();
    let mut rules: Vec<Value> = existing
        .iter()
        .filter(|r| !r["id"].as_str().is_some_and(|i| ids.contains(i)))
        .cloned()
        .collect();
    let same = ours.iter().all(|o| existing.contains(o));
    rules.extend(ours);
    (!same).then_some(rules)
}

/// A Cloudflare DNS record as a stored record ([CLI and setup §6.3] step 18).
fn normalise(r: &Value, mail_domain: &str) -> Value {
    let rtype = r["type"].as_str().unwrap_or("").to_owned();
    let name = r["name"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let value = r["content"]
        .as_str()
        .or_else(|| r["value"].as_str())
        .unwrap_or("")
        .to_owned();
    let purpose = match rtype.as_str() {
        "MX" if name == mail_domain => "mx",
        "MX" => "return_path",
        "TXT" if value.starts_with("v=spf1") => "spf",
        "TXT" if name.starts_with("_dmarc.") => "dmarc",
        "TXT" if name.contains("._domainkey.") => "dkim",
        "CNAME" if name.contains("._domainkey.") => "dkim",
        _ => "other",
    };
    let mut v = json!({ "type": rtype, "name": name, "value": value, "purpose": purpose,
                        "required": purpose != "dmarc", "source": "setup" });
    if let Some(p) = r["priority"].as_u64() {
        v["priority"] = p.into();
    }
    v
}

/// What setup needs from the deployment directory.
pub struct Deployment<'a> {
    pub cf: Client<'a>,
    pub dir: PathBuf,
    pub rendered: Option<String>,
    pub name: String,
}

impl<'a> Deployment<'a> {
    /// The deployment in `dir` with the operator's Cloudflare credentials (exit 3 without them).
    pub fn open(ctx: &'a Ctx, dir: &str) -> Result<Self, CliError> {
        let dir = crate::commands::path(ctx, dir);
        let rendered = std::fs::read_to_string(dir.join("wrangler.toml")).ok();
        let (token, account) = cloudflare::credentials(ctx, rendered.as_deref())?;
        let name = rendered
            .as_deref()
            .and_then(|r| render::var(r, "PM_DEPLOYMENT_NAME"))
            .or_else(|| {
                config::resolve(ctx)
                    .ok()
                    .and_then(|r| r.name.map(|n| n.value))
            })
            .unwrap_or_else(|| "pylota-identity".to_owned());
        Ok(Self {
            cf: Client::new(ctx, token, account),
            dir,
            rendered,
            name,
        })
    }

    /// The D1 database ID: the rendered file's, else the one named after the deployment.
    pub async fn d1(&self) -> Result<String, CliError> {
        if let Some(id) = self.rendered.as_deref().and_then(render::d1_id) {
            return Ok(id);
        }
        self.cf
            .d1_find(&self.name)
            .await?
            .and_then(|d| d["uuid"].as_str().map(str::to_owned))
            .ok_or_else(|| {
                cloudflare::unexpected("D1 database", &format!("no database named {}", self.name))
            })
    }

    pub fn token(&self) -> &str {
        self.cf.token()
    }
}

/// Polls `GET {base}/health` until it answers 200 with `version`, at most `secs` (exit 13 after).
pub async fn wait_health(
    ctx: &Ctx,
    base: &str,
    version: &str,
    secs: u64,
) -> Result<Value, CliError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if let Ok(r) = crate::http::get_public(ctx, base, "/health").await {
            let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            if v["version"] == version {
                return Ok(v);
            }
        }
        if std::time::Instant::now() > deadline {
            return Err(CliError::timeout(format!(
                "{base}/health did not report version {version} within {secs} s."
            ))
            .with_fix(
                "The Custom Domain's certificate can take minutes; run the same command again.",
            ));
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// The version `/health` reports, if it answers.
pub async fn health_version(ctx: &Ctx, base: &str) -> Option<String> {
    let r = crate::http::get_public(ctx, base, "/health").await.ok()?;
    let v: Value = serde_json::from_slice(&r.body).ok()?;
    v["version"].as_str().map(str::to_owned)
}

/// Prompts for a value, or exits 2 naming the flag in a non-interactive run.
fn ask(ctx: &Ctx, value: Option<String>, question: &str, flag: &str) -> Result<String, CliError> {
    match value {
        Some(v) => Ok(v),
        None if ctx.interactive => {
            let a = ctx.prompt(question)?;
            if a.trim().is_empty() {
                Err(CliError::usage(format!("Pass {flag}.")))
            } else {
                Ok(a.trim().to_owned())
            }
        }
        None => Err(CliError::usage(format!("Pass {flag}."))),
    }
}

pub async fn run(ctx: &Ctx, a: SetupArgs) -> Result<Output, CliError> {
    // ---- Preflight (CLI and setup § 6.2) ---------------------------------------------------------
    let d = Deployment::open(ctx, &a.dir.dir)?;
    let norm = |v: &str, flag: &str| {
        pylota_identity_core::address::normalise_domain(v)
            .ok_or_else(|| CliError::usage(format!("{flag} {v} is not a valid host name.")))
    };
    let api_host = norm(&a.domain, "--domain")?;
    let mail_domain = norm(
        &ask(
            ctx,
            a.mail_domain.clone(),
            "Platform mail domain (a zone apex): ",
            "--mail-domain",
        )?,
        "--mail-domain",
    )?;
    if api_host == mail_domain {
        return Err(CliError::usage("--domain and --mail-domain must differ."));
    }
    let console_host = match &a.console_host {
        Some(c) => Some(norm(c, "--console-host")?),
        None => None,
    };
    let owner_email = if a.no_console {
        a.owner_email.clone()
    } else {
        Some(ask(
            ctx,
            a.owner_email.clone(),
            "Console owner's email: ",
            "--owner-email",
        )?)
    };
    if a.daily_send_quota == Some(0) {
        return Err(CliError::usage("--daily-send-quota is a positive integer."));
    }
    if !valid_name(&a.name) {
        return Err(CliError::usage(
            "--name is 3–40 lower-case letters, digits and hyphens, starting with pylota-identity.",
        ));
    }
    if let Some(b) = &a.backup_bucket
        && (!valid_bucket(b) || *b == format!("{}-blobs", a.name))
    {
        return Err(CliError::usage(format!(
            "--backup-bucket {b} is not a valid R2 bucket name."
        )));
    }
    if let Some(r) = &d.rendered {
        for (key, now) in [
            ("PM_PLATFORM_DOMAIN", &mail_domain),
            ("PM_API_HOST", &api_host),
            ("PM_JURISDICTION", &a.jurisdiction),
        ] {
            if let Some(was) = render::var(r, key).filter(|w| !w.starts_with('{'))
                && &was != now
            {
                {
                    return Err(CliError::usage(format!(
                        "{key} is {was} in {}; it cannot change on a re-run.",
                        d.dir.join("wrangler.toml").display()
                    )));
                }
            }
        }
    }
    let cf = &d.cf;
    let jur = a.jurisdiction.as_str();
    let mut steps = Steps::default();

    // Step 1: the bundle, then Wrangler from its lockfile.
    let had_bundle = d
        .dir
        .join(".bundle")
        .join(a.version.as_deref().unwrap_or(VERSION))
        .join(".verified")
        .exists();
    let bundle = if a.from_source {
        release::from_source(ctx, &crate::commands::path(ctx, &a.source_dir))?
    } else {
        release::fetch(ctx, &d.dir, a.version.as_deref().unwrap_or(VERSION)).await?
    };
    steps.add(
        ctx,
        "bundle",
        if had_bundle { "exists" } else { "created" },
        format!("v{}", bundle.version),
        bundle.key_id.clone().unwrap_or_default(),
    );
    let wrangler = Wrangler::install(ctx, &d.dir, &bundle.dir, d.token(), &cf.account)?;

    // Zones: the mail domain must be an apex; the API host's zone is the longest parent in the account.
    let mail_zone = match cf.zone(&mail_domain).await? {
        Some(z) => z,
        None => {
            let parent = cf.zone_of(&mail_domain).await?;
            return Err(CliError::usage(match parent {
                Some(p) => format!(
                    "{mail_domain} is not a zone apex in this account (its zone is {}).",
                    p["name"].as_str().unwrap_or("?")
                ),
                None => format!("{mail_domain} is not a zone in this account."),
            }));
        }
    };
    let zone_id = mail_zone["id"].as_str().unwrap_or("").to_owned();
    if cf.zone_of(&api_host).await?.is_none() {
        return Err(CliError::usage(format!(
            "{api_host} is in no zone of this account."
        )));
    }
    if let Some(c) = &console_host
        && cf.zone_of(c).await?.is_none()
    {
        return Err(CliError::usage(format!(
            "{c} is in no zone of this account."
        )));
    }
    // Existing mail (H5).
    let expected_mx: BTreeSet<String> = cf
        .routing_dns(&zone_id)
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r["type"] == "MX")
        .filter_map(|r| {
            r["content"]
                .as_str()
                .map(|c| c.trim_end_matches('.').to_ascii_lowercase())
        })
        .collect();
    let foreign_mx: Vec<Value> = cf
        .dns_records(&zone_id, "MX", &mail_domain)
        .await?
        .into_iter()
        .filter(|r| {
            !r["content"].as_str().is_some_and(|c| {
                expected_mx.contains(&c.trim_end_matches('.').to_ascii_lowercase())
            })
        })
        .collect();
    if !foreign_mx.is_empty() {
        if !a.replace_mx {
            return Err(CliError::Prerequisite(crate::error::Local {
                message: format!("{mail_domain} already has MX records of another mail provider."),
                fix: Some("This domain already receives mail elsewhere; use a dedicated domain, or pass --replace-mx to stop that mail".into()),
                details: Some(json!({ "reason": "existing_mx", "records": foreign_mx.iter().map(|r| r["content"].clone()).collect::<Vec<_>>() })),
            }));
        }
        ctx.confirm(&format!(
            "Delete {} MX record(s) at {mail_domain}? Mail to its current provider stops.",
            foreign_mx.len()
        ))?;
    }

    // Step 2: D1.
    let db = match cf.d1_find(&a.name).await? {
        Some(db) => {
            let got = db["jurisdiction"].as_str().unwrap_or("default");
            if got != jur && !(jur == "default" && got.is_empty()) {
                return Err(cloudflare::unexpected(
                    "D1 database",
                    &format!(
                        "the database {} is in jurisdiction {got}, not {jur}; it cannot be moved",
                        a.name
                    ),
                ));
            }
            steps.add(
                ctx,
                "d1",
                "exists",
                a.name.clone(),
                db["uuid"].as_str().unwrap_or(""),
            );
            db
        }
        None => {
            let db = cf.d1_create(&a.name, jur).await?;
            steps.add(
                ctx,
                "d1",
                "created",
                a.name.clone(),
                db["uuid"].as_str().unwrap_or(""),
            );
            db
        }
    };
    let d1 = db["uuid"].as_str().unwrap_or("").to_owned();

    // Step 3: R2 buckets.
    let blobs = format!("{}-blobs", a.name);
    let mut buckets = vec![blobs.clone()];
    buckets.extend(a.backup_bucket.clone());
    for b in &buckets {
        if cf.r2_bucket(b, jur).await?.is_some() {
            steps.add(ctx, "r2", "exists", b.clone(), jur);
        } else {
            cf.r2_create(b, jur).await?;
            steps.add(ctx, "r2", "created", b.clone(), jur);
        }
    }
    // Step 4: lifecycle rules, merged.
    match merge_lifecycle(&cf.r2_lifecycle(&blobs, jur).await?) {
        Some(rules) => {
            cf.r2_put_lifecycle(&blobs, jur, &rules).await?;
            steps.add(
                ctx,
                "r2_lifecycle",
                "updated",
                blobs.clone(),
                "pm-inbound-staging,pm-abort-multipart",
            );
        }
        None => steps.add(
            ctx,
            "r2_lifecycle",
            "exists",
            blobs.clone(),
            "pm-inbound-staging,pm-abort-multipart",
        ),
    }
    // Step 5: queues.
    let prefix = render::prefix(&a.name);
    let have: BTreeSet<String> = cf
        .queues()
        .await?
        .iter()
        .filter_map(|q| q["queue_name"].as_str().map(str::to_owned))
        .collect();
    for q in queue_names(&prefix) {
        if have.contains(&q) {
            steps.add(ctx, "queue", "exists", q, "");
        } else {
            let r = cf.queue_create(&q).await?;
            steps.add(
                ctx,
                "queue",
                "created",
                q,
                r["queue_id"].as_str().unwrap_or(""),
            );
        }
    }
    // Step 6: Vectorize and its metadata indexes, before any vector is written.
    let index = format!("{prefix}mail-chunks");
    match cf.vectorize_index(&index).await? {
        Some(i) => {
            let dims = i["config"]["dimensions"].as_u64().unwrap_or(0);
            let metric = i["config"]["metric"].as_str().unwrap_or("");
            if dims != 1024 || metric != "cosine" {
                return Err(cloudflare::unexpected(
                    "Vectorize index",
                    &format!(
                        "{index} has {dims} dimensions and metric {metric}; setup needs 1024 and cosine"
                    ),
                ));
            }
            steps.add(ctx, "vectorize", "exists", index.clone(), "");
        }
        None => {
            cf.vectorize_create(
                &index,
                "pylota-identity generation=1 embed_model=@cf/baai/bge-m3",
                1024,
            )
            .await?;
            steps.add(ctx, "vectorize", "created", index.clone(), "");
        }
    }
    let have_meta: BTreeSet<String> = cf
        .vectorize_metadata(&index)
        .await?
        .iter()
        .filter_map(|m| m["propertyName"].as_str().map(str::to_owned))
        .collect();
    let missing: Vec<&(&str, &str)> = METADATA
        .iter()
        .filter(|(p, _)| !have_meta.contains(*p))
        .collect();
    for (p, k) in &missing {
        cf.vectorize_metadata_create(&index, p, k).await?;
    }
    steps.add(
        ctx,
        "vectorize_metadata",
        if missing.is_empty() {
            "exists"
        } else {
            "created"
        },
        index.clone(),
        format!("{} indexes", METADATA.len()),
    );

    // Step 7: Email Routing.
    let routing = cf.routing(&zone_id).await?;
    if routing["enabled"] == true {
        steps.add(
            ctx,
            "email_routing",
            "exists",
            mail_domain.clone(),
            zone_id.clone(),
        );
    } else {
        for r in &foreign_mx {
            if let Some(id) = r["id"].as_str() {
                cf.delete("existing MX", &format!("/zones/{zone_id}/dns_records/{id}"))
                    .await?;
            }
        }
        cf.post(
            "Email Routing",
            &format!("/zones/{zone_id}/email/routing/dns"),
            &json!({ "name": mail_domain }),
        )
        .await?;
        steps.add(
            ctx,
            "email_routing",
            "created",
            mail_domain.clone(),
            zone_id.clone(),
        );
    }
    if routing["support_subaddress"] != true {
        cf.patch(
            "Email Routing",
            &format!("/zones/{zone_id}/email/routing"),
            &json!({ "support_subaddress": true }),
        )
        .await?;
        steps.add(ctx, "subaddressing", "updated", mail_domain.clone(), "");
    }
    // Step 8: the ownership record.
    let own_name = format!("_pylota-identity.{mail_domain}");
    let own = cf.dns_records(&zone_id, "TXT", &own_name).await?;
    let existing_token = own.iter().find_map(|r| {
        r["content"]
            .as_str()
            .map(|c| c.trim_matches('"'))
            .and_then(|c| c.strip_prefix("pm-verify="))
            .map(|t| (t.to_owned(), r["id"].as_str().unwrap_or("").to_owned()))
    });
    let (ownership_token, ownership_id, own_created) = match existing_token {
        Some((t, id)) => {
            steps.add(
                ctx,
                "ownership_record",
                "exists",
                own_name.clone(),
                id.clone(),
            );
            (t, id, false)
        }
        None => {
            let t = token(16)?;
            let r = cf
                .post("ownership record", &format!("/zones/{zone_id}/dns_records"),
                      &json!({ "type": "TXT", "name": own_name, "content": format!("pm-verify={t}"), "ttl": 1 }))
                .await?;
            let id = r["id"].as_str().unwrap_or("").to_owned();
            steps.add(
                ctx,
                "ownership_record",
                "created",
                own_name.clone(),
                id.clone(),
            );
            (t, id, true)
        }
    };
    // Step 9: Email Sending.
    let subs = cf.sending_subdomains(&zone_id).await?;
    let (sending, sending_created) =
        match subs.into_iter().find(|s| s["name"] == mail_domain.as_str()) {
            Some(s) => (s, false),
            None => (
                cf.post(
                    "Email Sending",
                    &format!("/zones/{zone_id}/email/sending/subdomains"),
                    &json!({ "name": mail_domain }),
                )
                .await?,
                true,
            ),
        };
    let tag = sending["tag"]
        .as_str()
        .or_else(|| sending["id"].as_str())
        .unwrap_or("")
        .to_owned();
    steps.add(
        ctx,
        "email_sending",
        if sending_created { "created" } else { "exists" },
        mail_domain.clone(),
        tag.clone(),
    );
    if sending["drop_suppressed_recipients"] != false || sending["preview_enabled"] != false {
        cf.patch(
            "Email Sending",
            &format!("/zones/{zone_id}/email/sending/subdomains/{tag}"),
            &json!({ "drop_suppressed_recipients": false, "preview_enabled": false }),
        )
        .await?;
        steps.add(
            ctx,
            "email_sending_settings",
            "updated",
            mail_domain.clone(),
            tag.clone(),
        );
    }

    // Step 10: rate-limit namespace IDs.
    let mut ids = d
        .rendered
        .as_deref()
        .map(render::rate_limit_ids)
        .unwrap_or_default();
    let missing: Vec<&str> = render::RATE_LIMITS
        .iter()
        .copied()
        .filter(|n| !ids.contains_key(*n))
        .collect();
    if missing.is_empty() {
        steps.add(ctx, "rate_limits", "exists", "11 namespaces", "");
    } else {
        let mut used: BTreeSet<u32> = ids.values().copied().collect();
        for s in cf.scripts().await? {
            let Some(name) = s["id"].as_str() else {
                continue;
            };
            if let Ok(set) = cf.script_settings(name).await {
                for b in set["bindings"].as_array().into_iter().flatten() {
                    if b["type"] == "ratelimit"
                        && let Some(n) = b["namespace_id"]
                            .as_str()
                            .and_then(|n| n.parse().ok())
                            .or_else(|| b["namespace_id"].as_u64().map(|n| n as u32))
                    {
                        used.insert(n);
                    }
                }
            }
        }
        let mut next = 1001u32;
        for n in &missing {
            while used.contains(&next) {
                next += 1;
            }
            ids.insert((*n).to_owned(), next);
            used.insert(next);
        }
        steps.add(
            ctx,
            "rate_limits",
            "created",
            format!("{} namespaces", missing.len()),
            "",
        );
    }

    // Step 11: render.
    let mut vars = BTreeMap::new();
    if let Some(q) = a.daily_send_quota {
        vars.insert("PM_DAILY_SEND_QUOTA".to_owned(), q.to_string());
    }
    if let Some(b) = &a.backup_bucket {
        vars.insert("PM_BACKUP_BUCKET".to_owned(), b.clone());
    }
    if a.no_console {
        vars.insert("PM_CONSOLE".to_owned(), "off".to_owned());
    }
    let inputs = render::Inputs {
        name: a.name.clone(),
        api_host: api_host.clone(),
        console_host: console_host.clone(),
        platform_domain: mail_domain.clone(),
        jurisdiction: a.jurisdiction.clone(),
        account_id: cf.account.clone(),
        d1_id: d1.clone(),
        rate_limits: ids,
        vars,
    };
    let template = std::fs::read_to_string(bundle.dir.join("deploy/wrangler.toml.tmpl"))
        .map_err(|e| CliError::verification(format!("the bundle's template: {e}")))?;
    let rendered = render::render(&template, d.rendered.as_deref(), &inputs)?;
    let file = d.dir.join("wrangler.toml");
    let previous = d.rendered.clone();
    let unchanged = previous.as_deref().map(render::strip_marker) == Some(rendered.clone());
    let to_write = if unchanged {
        previous.clone().unwrap_or_default()
    } else {
        rendered.clone()
    };
    write_rendered(&file, &to_write)?;
    steps.add(
        ctx,
        "render",
        if unchanged {
            "exists"
        } else if previous.is_some() {
            "updated"
        } else {
            "created"
        },
        file.display().to_string(),
        "",
    );

    // Step 12: D1 migrations.
    let applied = migrate::apply(ctx, cf, &d1, &bundle.dir.join("migrations/d1")).await?;
    steps.add(
        ctx,
        "d1_migrations",
        if applied.is_empty() {
            "exists"
        } else {
            "created"
        },
        format!("{} applied", applied.len()),
        "",
    );

    // Step 13: the first deploy, skipped when nothing changed.
    let base = ctx.base_for(&api_host);
    let live = health_version(ctx, &base).await;
    let deploy_needed = !(render::deployed(&to_write, &bundle.version)
        && live.as_deref() == Some(bundle.version.as_str()));
    let config_path = file.display().to_string();
    if deploy_needed {
        let msg = format!("idvy setup v{}", bundle.version);
        wrangler.ok(
            "deploy",
            &["deploy", "--config", &config_path, "--message", &msg],
            None,
        )?;
        steps.add(
            ctx,
            "deploy",
            "created",
            a.name.clone(),
            format!("v{}", bundle.version),
        );
    } else {
        steps.add(
            ctx,
            "deploy",
            "exists",
            a.name.clone(),
            format!("v{}", bundle.version),
        );
    }

    // Step 14: secrets, and the pepper decision of § 6.5.
    let names: BTreeSet<String> = cf
        .secret_names(&a.name)
        .await?
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut printed = serde_json::Map::new();
    for s in GENERATED {
        if names.contains(*s) {
            steps.add(ctx, "secret", "exists", *s, "");
        } else {
            let (v, _) = secret32()?;
            wrangler.secret_put(&a.name, s, &v)?;
            if a.print_secrets {
                printed.insert((*s).to_owned(), v.into());
            }
            steps.add(ctx, "secret", "created", *s, "");
        }
    }
    let profile_name = ctx
        .global
        .profile
        .clone()
        .unwrap_or_else(|| "default".to_owned());
    let saved = config::load(ctx)?.profiles.get(&profile_name).cloned();
    let profile_works = match (saved.as_ref().and_then(|p| p.key.clone()), a.rotate_pepper) {
        (Some(k), false) => match Api::new(ctx, &base, Some(&k)) {
            Ok(api) => api
                .get(ctx, "/v1/me", &[])
                .await
                .ok()
                .is_some_and(|m| m["level"] == "platform"),
            Err(_) => false,
        },
        _ => false,
    };
    let keys = cf
        .d1_query(&d1, "SELECT COUNT(*) AS n FROM api_keys", &[])
        .await?;
    let key_rows = keys.first().and_then(|r| r["n"].as_u64()).unwrap_or(0);
    let has_pepper = names.contains("PM_KEY_PEPPER");
    let pepper: Option<[u8; 32]> = if profile_works {
        None
    } else if !has_pepper || key_rows == 0 || a.rotate_pepper {
        if has_pepper && key_rows > 0 {
            let typed = if ctx.interactive {
                ctx.prompt(&format!(
                    "--rotate-pepper stops every API key at once. Type {mail_domain} to confirm: "
                ))?
            } else {
                String::new()
            };
            if typed.trim() != mail_domain {
                return Err(CliError::usage(
                    "--rotate-pepper needs the platform domain typed to confirm (it is never answered by --yes).",
                ));
            }
        }
        let (v, raw) = secret32()?;
        wrangler.secret_put(&a.name, "PM_KEY_PEPPER", &v)?;
        if a.print_secrets {
            printed.insert("PM_KEY_PEPPER".into(), v.into());
        }
        steps.add(
            ctx,
            "secret",
            if has_pepper { "updated" } else { "created" },
            "PM_KEY_PEPPER",
            "",
        );
        Some(raw)
    } else {
        steps.add(ctx, "secret", "exists", "PM_KEY_PEPPER", "");
        None
    };

    // Step 15: health.
    wait_health(ctx, &base, &bundle.version, 300).await?;
    steps.add(
        ctx,
        "health",
        "exists",
        base.clone(),
        format!("v{}", bundle.version),
    );

    // Step 16: the delivery-event subscription.
    let sub_name = format!("{} {mail_domain}", a.name);
    let sub = ensure_subscription(
        cf,
        &wrangler,
        &sub_name,
        &format!("{prefix}delivery-events"),
        &zone_id,
        &mail_domain,
        &mut steps,
        ctx,
    )
    .await?;

    // Step 17: the catch-all to the Worker.
    let catch_all = cf.catch_all(&zone_id).await?;
    let ok = catch_all["enabled"] == true
        && catch_all["actions"].as_array().is_some_and(|a2| {
            a2.len() == 1 && a2[0]["type"] == "worker" && a2[0]["value"] == json!([a.name])
        });
    if ok {
        steps.add(
            ctx,
            "catch_all",
            "exists",
            mail_domain.clone(),
            a.name.clone(),
        );
    } else {
        cf.put("catch-all", &format!("/zones/{zone_id}/email/routing/rules/catch_all"),
               &json!({ "actions": [{ "type": "worker", "value": [a.name] }], "matchers": [{ "type": "all" }],
                        "enabled": true, "name": a.name }))
            .await?;
        steps.add(
            ctx,
            "catch_all",
            "updated",
            mail_domain.clone(),
            a.name.clone(),
        );
    }

    // Step 18: the records, read back.
    let mut records: Vec<Value> = cf
        .routing_dns(&zone_id)
        .await?
        .iter()
        .map(|r| normalise(r, &mail_domain))
        .collect();
    records.extend(
        cf.sending_dns(&zone_id, &tag)
            .await
            .unwrap_or_default()
            .iter()
            .map(|r| normalise(r, &mail_domain)),
    );
    records.push(
        json!({ "type": "TXT", "name": own_name, "value": format!("pm-verify={ownership_token}"),
                         "purpose": "ownership", "required": true, "source": "worker" }),
    );
    steps.add(
        ctx,
        "records",
        "exists",
        mail_domain.clone(),
        format!("{} records", records.len()),
    );

    // Step 19: the bootstrap key.
    let key = match (profile_works, pepper) {
        (true, _) => {
            steps.add(
                ctx,
                "bootstrap_key",
                "exists",
                format!("profile {profile_name}"),
                "",
            );
            saved
                .as_ref()
                .and_then(|p| p.key.clone())
                .unwrap_or_default()
        }
        (false, Some(p)) => {
            let k = bootstrap_key(ctx, cf, &d1, &p).await?;
            steps.add(
                ctx,
                "bootstrap_key",
                "created",
                format!("profile {profile_name}"),
                k.1.clone(),
            );
            k.0
        }
        (false, None) => {
            return Err(CliError::config("Keys exist; this profile has no working platform key.")
                .with_fix("Use idvy login with an existing platform key, or idvy setup --rotate-pepper (invalidates every key)."));
        }
    };
    let name_for_profile = a.name.clone();
    let account = cf.account.clone();
    config::update_profile(ctx, &profile_name, |p| {
        p.url = Some(base.clone());
        p.key = Some(key.clone());
        p.key_env = None;
        p.key_command = None;
        p.account_id = Some(account);
        p.name = Some(name_for_profile);
    })?;

    // Step 20: the platform domain row, read first; the objects recorded by the run that made them stay.
    let zone_full = cf.get("zones", &format!("/zones/{zone_id}"), &[]).await?;
    let now = ctx.now_ms();
    let records_json = Value::Array(records).to_string();
    let ns_json = zone_full["name_servers"].to_string();
    let existing = cf
        .d1_query(
            &d1,
            "SELECT zone_id, ownership_token, event_subscription_id, records_json, expected_ns_json, provider_objects_json \
             FROM domains WHERE name = ?1 AND state <> 'removed'",
            &[json!(mail_domain)],
        )
        .await?;
    let provider = match existing.first() {
        Some(row) => row["provider_objects_json"]
            .as_str()
            .unwrap_or("{}")
            .to_owned(),
        None => json!({
            "routing": { "zone_id": zone_id, "created": true },
            "catch_all": { "zone_id": zone_id, "created": true },
            "sending": { "tag": tag, "created": sending_created },
            "ownership_record": { "id": ownership_id, "created": own_created },
            "event_subscription": { "id": sub, "created": true },
        })
        .to_string(),
    };
    let same = existing.first().is_some_and(|row| {
        row["zone_id"] == zone_id.as_str()
            && row["ownership_token"] == ownership_token.as_str()
            && row["event_subscription_id"] == sub.as_str()
            && row["records_json"] == records_json.as_str()
            && row["expected_ns_json"] == ns_json.as_str()
    });
    if same {
        steps.add(ctx, "platform_domain", "exists", mail_domain.clone(), "");
    } else {
        cf.d1_query(
            &d1,
            "INSERT INTO domains (id, tenant_id, name, kind, method, inbound, zone_id, is_apex, routing_mode, \
             transport, reply_token, receiving, sending, state, state_changed_at, ownership_token, event_subscription_id, \
             records_json, expected_ns_json, provider_objects_json, monitor_do_id, created_at, updated_at) \
             VALUES (?1, NULL, ?2, 'platform', 'platform', 'routing', ?3, 1, 'catch_all', 'cloudflare', 'subaddress', 1, 1, \
             'pending', ?4, ?5, ?6, ?7, ?8, ?9, '', ?4, ?4) \
             ON CONFLICT(name) WHERE state <> 'removed' DO UPDATE SET zone_id = excluded.zone_id, \
             ownership_token = excluded.ownership_token, event_subscription_id = excluded.event_subscription_id, \
             records_json = excluded.records_json, expected_ns_json = excluded.expected_ns_json, \
             provider_objects_json = excluded.provider_objects_json, updated_at = excluded.updated_at",
            &[
                json!(format!("dom_{}", ulid(now))),
                json!(mail_domain),
                json!(zone_id),
                json!(now),
                json!(ownership_token),
                json!(sub),
                json!(records_json),
                json!(ns_json),
                json!(provider),
            ],
        )
        .await?;
        steps.add(
            ctx,
            "platform_domain",
            if existing.is_empty() {
                "created"
            } else {
                "updated"
            },
            mail_domain.clone(),
            "",
        );
    }

    // Step 21: the default tenant.
    let api = Api::new(ctx, &base, Some(&key))?;
    let tenants = api
        .list(ctx, "/v1/tenants", &[("limit", "100".into())], true)
        .await?;
    let tenant_id = match tenants["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["address_suffix"] == "")
    {
        Some(t) => {
            let id = t["id"].as_str().unwrap_or("").to_owned();
            steps.add(ctx, "default_tenant", "exists", "default", id.clone());
            id
        }
        None => {
            let mut body = json!({ "slug": "default", "name": a.tenant_name, "mode": "live", "address_suffix": "" });
            if let (Some(e), false) = (&owner_email, a.no_console) {
                body["owner"] = json!({ "email": e, "name": a.owner_name });
            }
            let t = api
                .call(
                    ctx,
                    "POST",
                    "/v1/tenants",
                    &[],
                    Some(&body),
                    Idem::Key("idvy-setup-default-tenant".into()),
                )
                .await?;
            let id = t["id"].as_str().unwrap_or("").to_owned();
            steps.add(ctx, "default_tenant", "created", "default", id.clone());
            id
        }
    };

    // Step 22: the system identity.
    let system = cf
        .d1_query(&d1, "SELECT id FROM identities WHERE is_system = 1", &[])
        .await?;
    if let Some(s) = system.first() {
        steps.add(
            ctx,
            "system_identity",
            "exists",
            format!("postmaster@{mail_domain}"),
            s["id"].as_str().unwrap_or(""),
        );
    } else {
        let idn = format!("idn_{}", ulid(now));
        let owner = owner_email
            .clone()
            .unwrap_or_else(|| format!("postmaster@{mail_domain}"));
        cf.d1_query(
            &d1,
            "INSERT INTO identities (id, tenant_id, username, display_name, owner_name, owner_email, status, \
             send_policy_json, mailbox_do_id, is_system, created_at, updated_at) \
             VALUES (?1, ?2, 'postmaster', 'Pylota Identity', 'Operator', ?3, 'active', '{\"daily_cap\":50000}', '', 1, ?4, ?4)",
            &[json!(idn), json!(tenant_id), json!(owner), json!(now)],
        )
        .await?;
        let dom = cf
            .d1_query(
                &d1,
                "SELECT id FROM domains WHERE name = ?1 AND state <> 'removed'",
                &[json!(mail_domain)],
            )
            .await?;
        let dom_id = dom
            .first()
            .and_then(|r| r["id"].as_str())
            .unwrap_or("")
            .to_owned();
        cf.d1_query(
            &d1,
            "INSERT INTO addresses (id, address, local_part, domain_id, tenant_id, identity_id, role, status, created_at, updated_at) \
             VALUES (?1, ?2, 'postmaster', ?3, ?4, ?5, 'primary', 'active', ?6, ?6)",
            &[json!(format!("adr_{}", ulid(now))), json!(format!("postmaster@{mail_domain}")), json!(dom_id), json!(tenant_id), json!(idn), json!(now)],
        )
        .await?;
        steps.add(
            ctx,
            "system_identity",
            "created",
            format!("postmaster@{mail_domain}"),
            idn,
        );
    }

    // Step 23: the mail test, and PM_TRUSTED_AUTHSERV_ID.
    let mut warnings: Vec<String> = Vec::new();
    match crate::commands::doctor::mail_test(ctx, &api, &mail_domain).await {
        Ok(Some(authserv)) => {
            let current = render::var(&to_write, "PM_TRUSTED_AUTHSERV_ID").unwrap_or_default();
            if current == authserv {
                steps.add(
                    ctx,
                    "mail_test",
                    "exists",
                    "PM_TRUSTED_AUTHSERV_ID",
                    authserv,
                );
            } else {
                let mut inputs2 = inputs.clone();
                inputs2
                    .vars
                    .insert("PM_TRUSTED_AUTHSERV_ID".into(), authserv.clone());
                inputs2.rate_limits = render::rate_limit_ids(&to_write);
                let again = render::render(&template, Some(&to_write), &inputs2)?;
                write_rendered(&file, &again)?;
                let msg = format!("idvy setup v{} (PM_TRUSTED_AUTHSERV_ID)", bundle.version);
                wrangler.ok(
                    "deploy",
                    &["deploy", "--config", &config_path, "--message", &msg],
                    None,
                )?;
                write_rendered(&file, &render::mark_deployed(&again, &bundle.version))?;
                steps.add(
                    ctx,
                    "mail_test",
                    "updated",
                    "PM_TRUSTED_AUTHSERV_ID",
                    authserv,
                );
            }
        }
        Ok(None) | Err(_) => {
            warnings.push("The mail test did not pass; PM_TRUSTED_AUTHSERV_ID stays empty until a re-run sets it.".into());
            steps.add(ctx, "mail_test", "skipped", "PM_TRUSTED_AUTHSERV_ID", "");
        }
    }
    if deploy_needed {
        let current = std::fs::read_to_string(&file).unwrap_or_default();
        if !render::deployed(&current, &bundle.version) {
            write_rendered(&file, &render::mark_deployed(&current, &bundle.version))?;
        }
    }

    // Step 24: the summary.
    let perms = crate::commands::admin::platform_permissions().join(",");
    let mut doc = json!({
        "version": bundle.version,
        "api_url": base,
        "platform_domain": mail_domain,
        "tenant_id": tenant_id,
        "steps": steps.json(),
        "warnings": warnings,
    });
    if a.print_secrets && !printed.is_empty() {
        doc["secrets"] = Value::Object(printed.clone());
    }
    let mut human = format!(
        "Pylota Identity {} is running at {base}\nPlatform domain: {mail_domain} (Email Routing catch-all → {}, Email Sending onboarded)\nDefault tenant: {tenant_id} (addresses look like name@{mail_domain})\n",
        bundle.version, a.name
    );
    if let (Some(e), false) = (&owner_email, a.no_console) {
        human.push_str(&format!("Console owner: {e} (sign-in link sent)\n"));
    }
    human.push_str(&format!(
        "Profile \"{profile_name}\" holds a bootstrap key that expires in 24 hours.\nNext: idvy keys create --level platform --name first-key --permissions {perms}\n      idvy doctor --mail-test\n"
    ));
    for (k, v) in &printed {
        human.push_str(&format!("{k}={}\n", v.as_str().unwrap_or("")));
    }
    let mut out = Output::object(doc)
        .human(human)
        .quiet(format!("{tenant_id}\n"));
    for w in warnings {
        out = out.note(format!("warning: {w}"));
    }
    Ok(out)
}

/// Writes the rendered file atomically, mode `0644` (it holds no secrets).
pub fn write_rendered(file: &Path, text: &str) -> Result<(), CliError> {
    let io = |e: std::io::Error| CliError::internal(format!("{}: {e}", file.display()));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = file.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)).map_err(io)?;
    }
    std::fs::rename(&tmp, file).map_err(io)
}

/// The bootstrap key ([CLI and setup §6.5]): a platform key with every permission a platform key may
/// hold, expiring in 24 hours, inserted with its audit row. Returns the key and its ID.
///
/// [CLI and setup §6.5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#65-the-bootstrap-key
async fn bootstrap_key(
    ctx: &Ctx,
    cf: &Client<'_>,
    d1: &str,
    pepper: &[u8; 32],
) -> Result<(String, String), CliError> {
    use pylota_identity_core::keys;
    let mut lookup = [0u8; 8];
    let mut secret = [0u8; 32];
    getrandom::fill(&mut lookup).map_err(|e| CliError::internal(e.to_string()))?;
    getrandom::fill(&mut secret).map_err(|e| CliError::internal(e.to_string()))?;
    let token = keys::generate(keys::Mode::Live, lookup, secret);
    let parts =
        keys::parse(&token).ok_or_else(|| CliError::internal("generated an invalid key"))?;
    let now = ctx.now_ms();
    let id = format!("key_{}", ulid(now));
    let perms =
        serde_json::to_string(&crate::commands::admin::platform_permissions()).unwrap_or_default();
    cf.d1_query(
        d1,
        "INSERT INTO api_keys (id, lookup, hash, name, level, tenant_id, identity_id, mode, permissions_json, \
         created_by_key_id, created_by_level, expires_at, created_at) \
         VALUES (?1, ?2, ?3, 'setup-bootstrap', 'platform', NULL, NULL, 'live', ?4, NULL, 'setup', ?5, ?6)",
        &[json!(id), json!(parts.lookup), json!(keys::hash(pepper, &token)), json!(perms), json!(now + 86_400_000), json!(now)],
    )
    .await?;
    cf.d1_query(
        d1,
        "INSERT INTO audit_log (id, tenant_id, actor_key_id, action, target_type, target_id, details_json, request_id, created_at) \
         VALUES (?1, NULL, NULL, 'key.create', 'api_key', ?2, '{\"via\":\"idvy setup\"}', NULL, ?3)",
        &[json!(format!("aud_{}", ulid(now))), json!(id), json!(now)],
    )
    .await?;
    Ok((token, id))
}

/// Finds the subscription named `name`, or creates it with Wrangler and reads its ID back.
#[allow(clippy::too_many_arguments)]
async fn ensure_subscription(
    cf: &Client<'_>,
    wrangler: &Wrangler<'_>,
    name: &str,
    queue: &str,
    zone_id: &str,
    domain: &str,
    steps: &mut Steps,
    ctx: &Ctx,
) -> Result<String, CliError> {
    let find = |subs: &[Value]| {
        subs.iter()
            .find(|s| {
                s["name"] == name
                    || (s["source"]["zone_id"] == zone_id && s["source"]["domain"] == domain)
            })
            .and_then(|s| s["id"].as_str().map(str::to_owned))
    };
    if let Some(id) = find(&cf.event_subscriptions().await?) {
        steps.add(ctx, "event_subscription", "exists", name, id.clone());
        return Ok(id);
    }
    wrangler.ok(
        "event subscription",
        &[
            "queues", "subscription", "create", queue, "--source", "email.sending", "--events",
            "message.delivered,message.deferred,message.bounced,message.failed,message.rejected,message.complained",
            "--zone-id", zone_id, "--domain", domain, "--name", name,
        ],
        None,
    )?;
    let id = find(&cf.event_subscriptions().await?)
        .ok_or_else(|| cloudflare::unexpected("event subscription", "created, but not listed"))?;
    steps.add(ctx, "event_subscription", "created", name, id.clone());
    Ok(id)
}

/// `idvy domains subscribe` ([CLI and setup §18.4]): a `manual` Cloudflare-transport domain gets its
/// Email Sending event subscription through Wrangler with the local token, recorded through the D1
/// query API. Any other domain is a no-op.
///
/// [CLI and setup §18.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#184-domains-subscribe
pub async fn domains_subscribe(ctx: &Ctx, domain: &str, dir: &str) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let id = resolve::domain_arg(ctx, domain).await?;
    let path = format!("/v1/domains/{}", seg(&id));
    let dom = api.get(ctx, &path, &[]).await?;
    if dom["transport"] != "cloudflare"
        || dom["sending"] != true
        || dom["delivery_events"] != "manual"
    {
        let human = format!(
            "Nothing to do: {} has transport {}, sending {} and delivery_events {}.\n",
            dom["name"].as_str().unwrap_or(domain),
            dom["transport"],
            dom["sending"],
            dom["delivery_events"]
        );
        return Ok(Output::object(dom).human(human));
    }
    let d = Deployment::open(ctx, dir)?;
    let cf = &d.cf;
    let name = dom["name"].as_str().unwrap_or("").to_owned();
    let zone = cf.zone_of(&name).await?.ok_or_else(|| {
        cloudflare::unexpected("zones", &format!("no zone for {name} in this account"))
    })?;
    let zone_id = zone["id"].as_str().unwrap_or("").to_owned();
    let version = d
        .rendered
        .as_deref()
        .and_then(|r| {
            r.lines().find_map(|l| {
                l.strip_prefix(render::MARKER)
                    .and_then(|v| v.split(' ').next())
                    .map(|v| v.trim_start_matches('v').to_owned())
            })
        })
        .unwrap_or_else(|| VERSION.to_owned());
    let bundle = release::fetch(ctx, &d.dir, &version).await?;
    let wrangler = Wrangler::install(ctx, &d.dir, &bundle.dir, d.token(), &cf.account)?;
    let mut steps = Steps::default();
    let prefix = render::prefix(&d.name);
    let sub = ensure_subscription(
        cf,
        &wrangler,
        &format!("{} {name}", d.name),
        &format!("{prefix}delivery-events"),
        &zone_id,
        &name,
        &mut steps,
        ctx,
    )
    .await?;
    let d1 = d.d1().await?;
    cf.d1_query(
        &d1,
        "UPDATE domains SET event_subscription_id = ?1 WHERE id = ?2 AND event_subscription_id IS NULL",
        &[json!(sub), json!(id)],
    )
    .await?;
    let after = api.get(ctx, &path, &[]).await?;
    Ok(Output::object(after))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// `cli::setup::lifecycle_merge`: other rules are kept; setup's two are added or replaced; nothing
    /// is written when they are already identical.
    #[test]
    fn lifecycle_merge() {
        let other =
            json!({ "id": "operator-rule", "enabled": true, "conditions": { "prefix": "x/" } });
        let stale = json!({ "id": "pm-inbound-staging", "enabled": false });
        let merged = super::merge_lifecycle(&[other.clone(), stale]).unwrap();
        assert_eq!(merged.len(), 3);
        assert!(merged.contains(&other));
        assert!(
            merged
                .iter()
                .any(|r| r["id"] == "pm-inbound-staging" && r["enabled"] == true)
        );
        assert!(super::merge_lifecycle(&merged).is_none());
    }

    #[test]
    fn names_and_queues() {
        assert!(
            super::valid_name("pylota-identity") && super::valid_name("pylota-identity-staging")
        );
        assert!(!super::valid_name("mail") && !super::valid_name("pylota-identity_x"));
        let q = super::queue_names("pm-");
        assert_eq!(q.len(), 12);
        assert!(q[..6].iter().all(|n| n.ends_with("-dlq")));
    }
}
