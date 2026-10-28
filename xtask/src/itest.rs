//! `cargo xtask itest` (Testing § 6.1): the Worker under `wrangler dev --local` with the test hooks, a
//! fresh local D1 with `migrations/d1`, a throwaway TLS certificate, the bootstrap seed `idvy setup`
//! writes, and the `it` suite against it. The test process hosts the fake server on `127.0.0.1:8798`.
//!
//! Until the CLI milestone adds `deploy/wrangler/package-lock.json`, the Wrangler run is the one pinned
//! by `site/package-lock.json` (4.139.0, Rust workspace § 8).

use std::io::Read as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pylota_identity_core::keys;

const PORT: u16 = 8799;
const FAKES: &str = "http://127.0.0.1:8798";

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| format!("/dev/urandom: {e}"))?;
    Ok(b)
}

fn wrangler(root: &Path) -> Result<PathBuf, String> {
    let w = root.join("site/node_modules/.bin/wrangler");
    if w.exists() {
        Ok(w)
    } else {
        Err("wrangler not installed: run `npm ci` in site/".into())
    }
}

/// The production template with local resources and the itest variables.
pub fn render(template: &str, root: &Path, token: &str) -> String {
    let mut out = String::new();
    let mut skip_table = false;
    for line in template.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            // Workers AI and Vectorize have no local mode: the M5 routes use neither.
            skip_table = matches!(trimmed, "[ai]" | "[[vectorize]]");
        }
        // Local workerd has no jurisdictions (`newUniqueId({ jurisdiction: "eu" })` throws), so the run
        // is a `default` deployment, written as setup writes one.
        if skip_table || trimmed.starts_with("routes") || trimmed.starts_with("jurisdiction") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    let main = root.join("crates/worker/build/index.js");
    let migrations = root.join("migrations/d1");
    let mut s = out
        .replace(
            "\"build/index.js\"",
            &format!("{:?}", main.display().to_string()),
        )
        .replace(
            "\"migrations/d1\"",
            &format!("{:?}", migrations.display().to_string()),
        )
        .replace("{agents.example}", "agents.example")
        .replace("{mail.example.com}", &format!("localhost:{PORT}"))
        .replace("{d1-id}", "00000000-0000-4000-8000-000000000000")
        .replace("{account-id}", "00000000000000000000000000000000")
        .replace(
            "PM_JURISDICTION        = \"eu\"",
            "PM_JURISDICTION        = \"default\"",
        )
        .replace(
            "PM_ENV                 = \"production\"",
            "PM_ENV                 = \"local\"",
        );
    for n in 1001..=1011 {
        s = s.replace(&format!("\"{{{n}}}\""), &format!("\"{n}\""));
    }
    s = s.replace(
        &format!("PM_CONSOLE_HOST        = \"localhost:{PORT}\""),
        &format!("PM_CONSOLE_HOST        = \"console.localhost:{PORT}\""),
    );
    s = s.replace(
        "PM_SIGNUP              = \"closed\"",
        "PM_SIGNUP              = \"open\"",
    );
    s = s.replace(
        "PM_WEB_BOT_AUTH        = \"off\"",
        "PM_WEB_BOT_AUTH        = \"on\"",
    );
    s = s.replace(
        "[vars]\n",
        &format!(
            "[vars]\nPM_TERMS_URL = \"https://agents.example/terms\"\nPM_PRIVACY_URL = \"https://agents.example/privacy\"\nPM_DPA_URL = \"https://agents.example/dpa\"\nPM_TERMS_VERSION = \"itest-1\"\nPM_ITEST_FAKES_URL = \"{FAKES}\"\nPM_ITEST_TOKEN = \"{token}\"\nPM_SECURITY_CONTACT = \"security@agents.example\"\nPM_QUARANTINE_KEY_RELEASE = \"off\"\nPM_SES_REGION = \"eu-west-2\"\nPM_SES_SNS_TOPIC_ARN = \"arn:aws:sns:eu-west-2:111122223333:pylota-identity-ses\"\n"
        ),
    );
    s
}

struct Wrangler(Child);

/// Stops wrangler and the workerd processes it started: they share the process group `run` gave it.
impl Drop for Wrangler {
    fn drop(&mut self) {
        let group = format!("-{}", self.0.id());
        let _ = Command::new("kill").args(["-TERM", "--", &group]).status();
        let _ = self.0.wait();
    }
}

fn curl(dir: &Path, args: &[&str]) -> Result<(u16, String), String> {
    let out = Command::new("curl")
        .args(["-sS", "--cacert"])
        .arg(dir.join("tls/ca.pem"))
        .args([
            "--resolve",
            &format!("localhost:{PORT}:127.0.0.1"),
            "-o",
            "-",
            "-w",
            "\n%{http_code}",
        ])
        .args(args)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", "0"));
    Ok((code.trim().parse().unwrap_or(0), body.to_owned()))
}

fn run_ok(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// `PM_PLATFORM_DOMAIN` of the rendered file, and the Cloudflare zone its row names.
const PLATFORM_DOMAIN: &str = "agents.example";
const ZONE: &str = "00000000000000000000000000000001";

/// Every permission a platform key can hold but `tenants:erase` (owner-only): the bootstrap key and the
/// suite's key hold the same set (Testing § 6.1 step 5). `itest::tests::platform_permissions_match_auth`
/// checks it against `worker::auth`.
const PLATFORM_PERMISSIONS: &str = r#"["tenants:manage","identities:read","identities:write","domains:read","domains:write","messages:read","messages:send","messages:write","attachments:read","search:read","search:agentic","quarantine:review","webhooks:read","webhooks:manage","events:read","keys:manage","erasure:manage","suppressions:manage","usage:read","audit:read","policy:write","accounts:request","accounts:approve","phone:read","phone:request","phone:send","phone:call","phone:approve","phone:manage","phone:compliance","vault:read","vault:write","vault:request","vault:use","vault:approve","vault:admin","drafts:read","drafts:write","drafts:approve","members:read","members:manage","partners:manage","platform:ops","metrics:read"]"#;

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let keep = args.iter().any(|a| a == "--keep");
    let dir = root.join("target/itest");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tls")).map_err(|e| e.to_string())?;

    // 1. The Worker with the test hooks.
    crate::build_worker::build(root, &["itest-hooks"])?;

    // 2. The configuration and the secrets.
    let token = hex(&random::<16>()?);
    let pepper = random::<32>()?;
    let template = std::fs::read_to_string(root.join("deploy/wrangler.toml.tmpl"))
        .map_err(|e| e.to_string())?;
    let config = dir.join("wrangler.itest.toml");
    std::fs::write(&config, render(&template, root, &token)).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join(".dev.vars"),
        format!(
            "PM_MASTER_KEY={}\nPM_KEY_PEPPER={}\nPM_HASH_KEY={}\nPM_CF_API_TOKEN=itest-cf-token\nPM_SES_ACCESS_KEY_ID=AKIAITESTITESTITEST\nPM_SES_SECRET_ACCESS_KEY=itest-ses-secret\n",
            STANDARD.encode(random::<32>()?),
            STANDARD.encode(pepper),
            STANDARD.encode(random::<32>()?)
        ),
    )
    .map_err(|e| e.to_string())?;
    let w = wrangler(root)?;
    let state = dir.join("state");

    // 3. Migrations.
    run_ok(
        Command::new(&w)
            .args([
                "d1",
                "migrations",
                "apply",
                "pylota-identity",
                "--local",
                "--persist-to",
            ])
            .arg(&state)
            .arg("--config")
            .arg(&config)
            .env("CI", "1"),
        "wrangler d1 migrations apply",
    )?;

    // 4. TLS and wrangler dev.
    // A throwaway CA and a leaf it signs: rustls refuses a self-signed certificate used as both.
    let ec = [
        "-newkey",
        "ec",
        "-pkeyopt",
        "ec_paramgen_curve:P-256",
        "-nodes",
        "-days",
        "2",
    ];
    run_ok(
        Command::new("openssl")
            .args(["req", "-x509"])
            .args(ec)
            .args([
                "-subj",
                "/CN=pylota-identity itest CA",
                "-addext",
                "basicConstraints=critical,CA:TRUE",
                "-addext",
                "keyUsage=critical,keyCertSign",
                "-keyout",
            ])
            .arg(dir.join("tls/ca-key.pem"))
            .arg("-out")
            .arg(dir.join("tls/ca.pem")),
        "openssl (CA)",
    )?;
    run_ok(
        Command::new("openssl")
            .args(["req", "-x509", "-CA"])
            .arg(dir.join("tls/ca.pem"))
            .arg("-CAkey")
            .arg(dir.join("tls/ca-key.pem"))
            .args(ec)
            .args([
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=DNS:localhost,DNS:console.localhost,IP:127.0.0.1",
                "-addext",
                "basicConstraints=critical,CA:FALSE",
                "-addext",
                "extendedKeyUsage=serverAuth",
                "-keyout",
            ])
            .arg(dir.join("tls/key.pem"))
            .arg("-out")
            .arg(dir.join("tls/cert.pem")),
        "openssl (leaf)",
    )?;
    let log = std::fs::File::create(dir.join("worker.log")).map_err(|e| e.to_string())?;
    let child = Command::new(&w)
        .args([
            "dev",
            "--local",
            "--port",
            &PORT.to_string(),
            "--local-protocol",
            "https",
            "--https-key-path",
        ])
        .arg(dir.join("tls/key.pem"))
        .arg("--https-cert-path")
        .arg(dir.join("tls/cert.pem"))
        .arg("--persist-to")
        .arg(&state)
        .args(["--test-scheduled", "--config"])
        .arg(&config)
        .env("CI", "1")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("wrangler dev: {e}"))?;
    std::fs::write(dir.join("wrangler.pid"), child.id().to_string()).map_err(|e| e.to_string())?;
    let _guard = Wrangler(child);
    let base = format!("https://localhost:{PORT}");
    let start = Instant::now();
    loop {
        if let Ok((200, _)) = curl(&dir, &[&format!("{base}/health")]) {
            break;
        }
        if start.elapsed() > Duration::from_secs(90) {
            return Err(format!(
                "the Worker did not answer /health; see {}",
                dir.join("worker.log").display()
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    // 5. The bootstrap key (setup step 19), then a full platform key and the default tenant through the API.
    let bootstrap = keys::generate(keys::Mode::Live, random()?, random()?);
    let lookup = keys::parse(&bootstrap)
        .map(|p| p.lookup.to_owned())
        .unwrap_or_default();
    let hash = keys::hash(&pepper, &bootstrap);
    let insert = format!(
        "INSERT INTO api_keys (id, lookup, hash, name, level, mode, permissions_json, created_by_level, expires_at, created_at) VALUES ('key_01JBOOTSTRAP0000000000000', '{lookup}', '{hash}', 'bootstrap', 'platform', 'live', '{PLATFORM_PERMISSIONS}', 'setup', strftime('%s','now') * 1000 + 86400000, strftime('%s','now') * 1000)"
    );
    run_ok(
        Command::new(&w)
            .args([
                "d1",
                "execute",
                "pylota-identity",
                "--local",
                "--persist-to",
            ])
            .arg(&state)
            .arg("--config")
            .arg(&config)
            .args(["--command", &insert])
            .env("CI", "1"),
        "seeding the bootstrap key",
    )?;
    let auth = format!("authorization: Bearer {bootstrap}");
    let (code, body) = curl(
        &dir,
        &[
            "-X",
            "POST",
            "-H",
            &auth,
            "-H",
            "content-type: application/json",
            "--data",
            &format!(
                r#"{{"name":"itest","level":"platform","permissions":{PLATFORM_PERMISSIONS}}}"#
            ),
            &format!("{base}/v1/keys"),
        ],
    )?;
    if code != 201 {
        return Err(format!("minting the suite's platform key: {code} {body}"));
    }
    let suite_key = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["secret"].as_str().map(str::to_owned))
        .ok_or("no secret")?;
    let (code, body) = curl(
        &dir,
        &[
            "-X",
            "POST",
            "-H",
            &format!("authorization: Bearer {suite_key}"),
            "-H",
            "content-type: application/json",
            "--data",
            r#"{"slug":"default","name":"Default","mode":"live","address_suffix":""}"#,
            &format!("{base}/v1/tenants"),
        ],
    )?;
    if code != 201 {
        return Err(format!("creating the default tenant: {code} {body}"));
    }
    let default_tenant = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["id"].as_str().map(str::to_owned))
        .ok_or("no tenant id")?;
    // Setup steps 20 and 22: the platform domain row (`healthy` until M13 builds the DomainMonitor)
    // and the system identity with its primary address, in one batch; the cron mints its mailbox.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let id = |prefix| -> Result<String, String> {
        let r: [u8; 16] = random()?;
        Ok(pylota_identity_core::ids::format(
            prefix,
            now,
            u128::from_be_bytes(r),
        ))
    };
    use pylota_identity_core::ids::IdPrefix;
    let (dom, idn, adr) = (
        id(IdPrefix::Domain)?,
        id(IdPrefix::Identity)?,
        id(IdPrefix::Address)?,
    );
    let seed = format!(
        "INSERT INTO domains (id, tenant_id, name, kind, method, zone_id, is_apex, routing_mode, inbound, transport, reply_token, receiving, sending, state, state_changed_at, records_json, monitor_do_id, created_at, updated_at) VALUES ('{dom}', NULL, '{PLATFORM_DOMAIN}', 'platform', 'platform', '{ZONE}', 1, 'catch_all', 'routing', 'cloudflare', 'subaddress', 1, 1, 'healthy', {now}, '[]', '', {now}, {now}); \
         INSERT INTO identities (id, tenant_id, username, display_name, owner_name, owner_email, send_policy_json, mailbox_do_id, is_system, created_at, updated_at) VALUES ('{idn}', '{default_tenant}', 'no-reply', 'Pylota Identity', 'Operator', 'postmaster@{PLATFORM_DOMAIN}', '{{\"daily_cap\":50000,\"require_known_recipient\":false}}', '', 1, {now}, {now}); \
         INSERT INTO addresses (id, address, local_part, domain_id, tenant_id, identity_id, role, status, created_at, updated_at) VALUES ('{adr}', 'no-reply@{PLATFORM_DOMAIN}', 'no-reply', '{dom}', '{default_tenant}', '{idn}', 'primary', 'active', {now}, {now});"
    );
    run_ok(
        Command::new(&w)
            .args([
                "d1",
                "execute",
                "pylota-identity",
                "--local",
                "--persist-to",
            ])
            .arg(&state)
            .arg("--config")
            .arg(&config)
            .args(["--command", &seed])
            .env("CI", "1"),
        "seeding the platform domain and the system identity",
    )?;
    let (code, body) = curl(
        &dir,
        &[&format!("{base}/cdn-cgi/handler/scheduled?cron=*+*+*+*+*")],
    )?;
    if code >= 300 {
        return Err(format!("running the every-minute cron: {code} {body}"));
    }

    // 6. The suite.
    let status = Command::new(env!("CARGO"))
        .args([
            "test",
            "-p",
            "pylota-identity-worker",
            "--features",
            "itest-hooks",
            "--test",
            "it",
            "--",
            "--test-threads=1",
        ])
        .args(args.iter().filter(|a| *a != "--keep"))
        .current_dir(root)
        .env("PM_ITEST_URL", &base)
        .env("PM_ITEST_CA", dir.join("tls/ca.pem"))
        .env("PM_ITEST_KEY", &suite_key)
        .env("PM_ITEST_TOKEN", &token)
        .env("PM_ITEST_PEPPER", hex(&pepper))
        .env("PM_ITEST_STATE", &state)
        .env("PM_ITEST_CONFIG", &config)
        .env("PM_ITEST_WRANGLER", &w)
        .status()
        .map_err(|e| format!("cargo test: {e}"))?;

    // 8. Clean up.
    drop(_guard);
    if !keep {
        let _ = std::fs::remove_dir_all(&state);
    }
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "the it suite failed; the Worker's log is {}",
            dir.join("worker.log").display()
        ))
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rendered file keeps every production binding but the two without a local mode, and the itest
    /// variables Testing § 6.1 lists.
    #[test]
    fn render_keeps_bindings_and_sets_itest_vars() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let tmpl = std::fs::read_to_string(root.join("deploy/wrangler.toml.tmpl")).unwrap();
        let out = render(&tmpl, root, "tok");
        let v: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(v["vars"]["PM_ENV"].as_str(), Some("local"));
        assert_eq!(v["vars"]["PM_JURISDICTION"].as_str(), Some("default"));
        assert_eq!(v["vars"]["PM_QUARANTINE_KEY_RELEASE"].as_str(), Some("off"));
        assert!(v["r2_buckets"][0].get("jurisdiction").is_none());
        assert_eq!(v["vars"]["PM_API_HOST"].as_str(), Some("localhost:8799"));
        assert_eq!(
            v["vars"]["PM_CONSOLE_HOST"].as_str(),
            Some("console.localhost:8799")
        );
        assert_eq!(v["vars"]["PM_ITEST_TOKEN"].as_str(), Some("tok"));
        assert!(v.get("ai").is_none() && v.get("vectorize").is_none() && v.get("routes").is_none());
        assert_eq!(
            v["durable_objects"]["bindings"].as_array().unwrap().len(),
            7
        );
        assert_eq!(v["ratelimits"].as_array().unwrap().len(), 11);
        assert!(!out.contains('{') || !out.contains("{agents.example}"));
    }

    /// The compiled-in limits of `crates/worker/src/ratelimit.rs` equal the template's `[[ratelimits]]`.
    #[test]
    fn ratelimit_limits_match_template() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let tmpl: toml::Value = toml::from_str(
            &std::fs::read_to_string(root.join("deploy/wrangler.toml.tmpl")).unwrap(),
        )
        .unwrap();
        let src = std::fs::read_to_string(root.join("crates/worker/src/ratelimit.rs")).unwrap();
        let mut compiled = Vec::new();
        for line in src.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("(\"RL_") {
                let parts: Vec<String> = rest
                    .trim_end_matches("),")
                    .split(',')
                    .map(|p| p.trim().trim_matches('"').replace('_', ""))
                    .collect();
                compiled.push((
                    format!("RL_{}", rest.split('"').next().unwrap()),
                    parts[1].parse::<i64>().unwrap(),
                    parts[2].parse::<i64>().unwrap(),
                ));
            }
        }
        let mut template: Vec<(String, i64, i64)> = tmpl["ratelimits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["name"].as_str().unwrap().to_owned(),
                    r["simple"]["limit"].as_integer().unwrap(),
                    r["simple"]["period"].as_integer().unwrap(),
                )
            })
            .collect();
        compiled.sort();
        template.sort();
        assert_eq!(compiled, template);
    }

    #[test]
    fn platform_permissions_match_auth() {
        let src =
            std::fs::read_to_string(crate::workspace_root().join("crates/worker/src/auth.rs"))
                .unwrap();
        let list = src
            .split("pub const PERMISSIONS: &[&str] = &[")
            .nth(1)
            .unwrap()
            .split("];")
            .next()
            .unwrap();
        let want: Vec<&str> = list
            .split('"')
            .skip(1)
            .step_by(2)
            .filter(|p| !matches!(*p, "identities:sign" | "tenants:erase"))
            .collect();
        let have: Vec<String> = serde_json::from_str(PLATFORM_PERMISSIONS).unwrap();
        assert_eq!(have, want);
    }
}
