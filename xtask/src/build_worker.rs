//! `cargo xtask build-worker`: `worker-build --release` in `crates/worker` with the build-time
//! values, then the size budget (Rust workspace § 9, NFR-SEC-2).

use crate::size::{self, FileSize, Verdict};
use std::path::Path;
use std::process::Command;

/// The `worker-build` that matches `worker` 0.8.7 (ADR 0001).
pub const WORKER_BUILD_VERSION: &str = "0.8.7";

pub fn run(root: &Path) -> Result<(), String> {
    check_worker_build()?;
    let crate_dir = root.join("crates/worker");

    let version = match std::env::var("PM_BUILD_VERSION") {
        Ok(v) => v,
        Err(_) => workspace_version(root)?,
    };
    let commit = std::env::var("PM_BUILD_COMMIT").unwrap_or_else(|_| git_commit(root));
    let date = match std::env::var("PM_BUILD_DATE") {
        Ok(d) => d,
        Err(_) => utc_date(build_epoch()?),
    };
    println!("build-worker: version {version}, commit {commit}, date {date}");

    let status = Command::new("worker-build")
        .args(["--release", "--locked"])
        .current_dir(&crate_dir)
        .env("PM_BUILD_VERSION", &version)
        .env("PM_BUILD_COMMIT", &commit)
        .env("PM_BUILD_DATE", &date)
        .status()
        .map_err(|e| format!("cannot run worker-build: {e}"))?;
    if !status.success() {
        return Err(format!("worker-build --release failed ({status})"));
    }

    let mut files = Vec::new();
    for name in ["build/index.js", "build/index_bg.wasm"] {
        let path = crate_dir.join(name);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push(FileSize::measure(name, &bytes));
    }
    print!("{}", size::table(&files));
    match size::size_budget(&files) {
        Verdict::Within => {
            println!("size budget: ok (NFR-SEC-2)");
            Ok(())
        }
        Verdict::Warn => {
            println!(
                "warning: the compressed bundle is over 8 MiB; see Rust workspace § 4 (opt-level)"
            );
            Ok(())
        }
        Verdict::Over(why) => Err(format!("size budget: {why}")),
    }
}

fn check_worker_build() -> Result<(), String> {
    let hint = format!(
        "install it with `cargo install worker-build --version {WORKER_BUILD_VERSION} --locked`"
    );
    let output = Command::new("worker-build")
        .arg("--version")
        .output()
        .map_err(|e| format!("worker-build not found ({e}); {hint}"))?;
    let found = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if found != WORKER_BUILD_VERSION {
        return Err(format!(
            "worker-build {found} found, {WORKER_BUILD_VERSION} required; {hint}"
        ));
    }
    Ok(())
}

/// `[workspace.package] version` of the root `Cargo.toml`.
pub fn workspace_version(root: &Path) -> Result<String, String> {
    let text =
        std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    let doc: toml::Table = text.parse().map_err(|e| format!("Cargo.toml: {e}"))?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml has no [workspace.package] version".to_owned())
}

fn git_commit(root: &Path) -> String {
    Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// `SOURCE_DATE_EPOCH` when set (a release sets it to the tagged commit's time), otherwise now.
fn build_epoch() -> Result<i64, String> {
    match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(s) => s
            .trim()
            .parse()
            .map_err(|_| format!("SOURCE_DATE_EPOCH is not an integer: {s}")),
        Err(_) => Ok(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default()),
    }
}

/// The UTC calendar date of a Unix time, `YYYY-MM-DD` (days-to-civil, proleptic Gregorian).
pub fn utc_date(epoch_secs: i64) -> String {
    let z = epoch_secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PM_BUILD_DATE` comes from `SOURCE_DATE_EPOCH` as a UTC date.
    #[test]
    fn build_date_from_epoch() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
        assert_eq!(utc_date(1_792_454_399), "2026-10-19");
        assert_eq!(utc_date(-1), "1969-12-31");
    }

    /// The version comes from `[workspace.package]`, the one place it is set.
    #[test]
    fn version_from_workspace() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert_eq!(workspace_version(root).unwrap(), env!("CARGO_PKG_VERSION"));
    }
}
