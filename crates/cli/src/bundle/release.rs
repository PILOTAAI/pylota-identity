//! The release bundle ([CLI and setup §8.1–§8.3, §8.8]): looked up on GitHub, downloaded with size caps,
//! verified (signature, checksums) and extracted to `<dir>/.bundle/{version}/`; or, with
//! `--from-source`, built from a checkout.
//!
//! [CLI and setup §8.1–§8.3, §8.8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#81-bundle-download

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{extract, verify};
use crate::{CliError, Cmd, Ctx, VERSION};

pub const GITHUB_API: &str = "https://api.github.com";
const REPO: &str = "PILOTAAI/pylota-identity";

/// A verified, extracted bundle.
#[derive(Clone, Debug)]
pub struct Bundle {
    pub dir: PathBuf,
    pub version: String,
    /// The minisign key that signed it; `None` for `--from-source`.
    pub key_id: Option<String>,
    pub tarball_sha256: Option<String>,
}

/// `1.2.3` as numbers, for "newer than".
fn semver(v: &str) -> Vec<u64> {
    v.split(['.', '-'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

pub fn newer(a: &str, b: &str) -> bool {
    semver(a) > semver(b)
}

/// Downloads `url` with a size cap: HTTPS only (or a loopback host, for the recorded fakes).
async fn download(ctx: &Ctx, url: &str, cap: usize) -> Result<Vec<u8>, CliError> {
    let loopback = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
    if !url.starts_with("https://") && !loopback {
        return Err(CliError::verification(format!("{url} is not an https URL")));
    }
    let net = |e: reqwest::Error| CliError::Network {
        message: e.to_string(),
        idempotency_key: None,
    };
    let mut resp = ctx
        .http
        .get(url)
        .header("user-agent", crate::http::user_agent())
        .header("accept", "application/octet-stream")
        .send()
        .await
        .map_err(net)?;
    if !resp.status().is_success() {
        return Err(CliError::Network {
            message: format!("{url}: HTTP {}", resp.status().as_u16()),
            idempotency_key: None,
        });
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(net)? {
        out.extend_from_slice(&chunk);
        if out.len() > cap {
            return Err(CliError::verification(format!(
                "{url} is larger than {cap} bytes"
            )));
        }
    }
    Ok(out)
}

/// The bundle of `version` in `<dir>/.bundle/`, reused when an earlier run verified it.
pub async fn fetch(ctx: &Ctx, dir: &Path, version: &str) -> Result<Bundle, CliError> {
    if newer(version, VERSION) {
        return Err(CliError::usage(format!(
            "idvy {VERSION} cannot deploy {version}; install idvy {version} first."
        )));
    }
    let final_dir = dir.join(".bundle").join(version);
    let marker = final_dir.join(".verified");
    if let Ok(m) = std::fs::read_to_string(&marker) {
        let v: Value = serde_json::from_str(&m).unwrap_or(Value::Null);
        return Ok(Bundle {
            dir: final_dir,
            version: version.to_owned(),
            key_id: v["key_id"].as_str().map(str::to_owned),
            tarball_sha256: v["sha256"].as_str().map(str::to_owned),
        });
    }
    let url = format!("{}/repos/{REPO}/releases/tags/v{version}", ctx.github_api);
    let resp = ctx
        .http
        .get(&url)
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2026-03-10")
        .header("user-agent", crate::http::user_agent())
        .send()
        .await
        .map_err(|e| CliError::Network {
            message: e.to_string(),
            idempotency_key: None,
        })?;
    if resp.status().as_u16() == 404 {
        return Err(CliError::usage(format!("There is no release v{version}.")));
    }
    let release: Value = resp.json().await.map_err(|e| CliError::Network {
        message: e.to_string(),
        idempotency_key: None,
    })?;
    let tarball = format!("pylota-identity-worker-{version}.tar.gz");
    let asset = |name: &str| -> Result<String, CliError> {
        release["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["name"] == name)
            .and_then(|a| a["browser_download_url"].as_str().map(str::to_owned))
            .ok_or_else(|| CliError::verification(format!("release v{version} has no {name}")))
    };
    let partial = dir.join(".bundle").join(format!("{version}.partial"));
    let _ = std::fs::remove_dir_all(&partial);
    let result = async {
        let sums_bytes = download(ctx, &asset("SHA256SUMS")?, 64 * 1024).await?;
        let sig = download(ctx, &asset("SHA256SUMS.sig")?, 4 * 1024).await?;
        let keys: Vec<String> = ctx.release_keys.clone().unwrap_or_else(|| {
            verify::RELEASE_KEYS
                .iter()
                .map(|k| (*k).to_owned())
                .collect()
        });
        let key_id =
            verify::signature(&sums_bytes, &String::from_utf8_lossy(&sig), version, &keys)?;
        let listed = verify::sums(&sums_bytes)?;
        let tar_bytes = download(ctx, &asset(&tarball)?, 64 * 1024 * 1024).await?;
        let sha = verify::checksum(&listed, &tarball, &tar_bytes)?;
        extract::extract(&tar_bytes, &partial, version)?;
        Ok::<_, CliError>((key_id, sha))
    }
    .await;
    let (key_id, sha) = match result {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&partial);
            return Err(e);
        }
    };
    let io = |e: std::io::Error| CliError::internal(format!("{}: {e}", final_dir.display()));
    let _ = std::fs::remove_dir_all(&final_dir);
    std::fs::rename(&partial, &final_dir).map_err(io)?;
    std::fs::write(
        &marker,
        serde_json::json!({ "key_id": key_id, "sha256": sha }).to_string(),
    )
    .map_err(io)?;
    Ok(Bundle {
        dir: final_dir,
        version: version.to_owned(),
        key_id: Some(key_id),
        tarball_sha256: Some(sha),
    })
}

/// `--from-source`: `worker-build --release` in the checkout's `crates/worker`, and the checkout's own
/// migrations, template and Wrangler lockfile. Nothing is signed; the commit is printed with a warning.
pub fn from_source(ctx: &Ctx, source: &Path) -> Result<Bundle, CliError> {
    let worker = source.join("crates/worker");
    if !worker.is_dir() {
        return Err(CliError::usage(format!(
            "{} is not a checkout of pylota-identity.",
            source.display()
        )));
    }
    let run = |program: &str, args: &[&str], cwd: &Path| -> Result<String, CliError> {
        let p = ctx
            .runner
            .run(&Cmd {
                program: program.into(),
                args: args.iter().map(|s| (*s).to_owned()).collect(),
                cwd: Some(cwd.to_owned()),
                ..Cmd::default()
            })
            .map_err(CliError::prerequisite)?;
        if p.code != 0 {
            return Err(CliError::prerequisite(format!(
                "{program} {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&p.stderr)
                    .lines()
                    .last()
                    .unwrap_or("")
            )));
        }
        Ok(String::from_utf8_lossy(&p.stdout).trim().to_owned())
    };
    let targets = run("rustup", &["target", "list", "--installed"], source)?;
    if !targets
        .lines()
        .any(|t| t.trim() == "wasm32-unknown-unknown")
    {
        return Err(
            CliError::prerequisite("The wasm32-unknown-unknown target is not installed.")
                .with_fix("rustup target add wasm32-unknown-unknown"),
        );
    }
    if !run("worker-build", &["--version"], source).is_ok_and(|v| v.contains("0.8.7")) {
        run(
            "cargo",
            &["install", "worker-build", "--version", "0.8.7", "--locked"],
            source,
        )?;
    }
    run("worker-build", &["--release"], &worker)?;
    let commit = run("git", &["rev-parse", "HEAD"], source).unwrap_or_default();
    ctx.note(&format!(
        "warning: deploying an unverified build of commit {commit} from {}",
        source.display()
    ));
    let dir = source.join("target/idvy-bundle");
    let io = |e: std::io::Error| CliError::internal(format!("{}: {e}", dir.display()));
    let _ = std::fs::remove_dir_all(&dir);
    for (from, to) in [
        (worker.join("build"), dir.join("build")),
        (source.join("migrations/d1"), dir.join("migrations/d1")),
        (source.join("deploy/wrangler"), dir.join("wrangler")),
    ] {
        copy_dir(&from, &to).map_err(io)?;
    }
    std::fs::create_dir_all(dir.join("deploy")).map_err(io)?;
    std::fs::copy(
        source.join("deploy/wrangler.toml.tmpl"),
        dir.join("deploy/wrangler.toml.tmpl"),
    )
    .map_err(io)?;
    std::fs::write(dir.join("VERSION"), VERSION).map_err(io)?;
    Ok(Bundle {
        dir,
        version: VERSION.to_owned(),
        key_id: None,
        tarball_sha256: None,
    })
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            std::fs::copy(e.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions() {
        assert!(super::newer("1.2.0", "1.1.9"));
        assert!(!super::newer("1.0.0", "1.0.0"));
        assert!(super::newer("0.10.0", "0.9.0"));
    }
}
