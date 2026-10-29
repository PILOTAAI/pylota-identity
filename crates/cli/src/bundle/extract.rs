//! Safe extraction of a release tarball ([CLI and setup §8.3]): regular files and directories only, every
//! path relative and inside the target, at most 1,000 entries and 128 MiB unpacked, and the expected
//! files present with `VERSION` equal to the requested version.
//!
//! [CLI and setup §8.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#83-extraction

use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use crate::CliError;

pub const MAX_ENTRIES: usize = 1000;
pub const MAX_BYTES: u64 = 128 * 1024 * 1024;

/// The files every bundle carries.
pub const EXPECTED: &[&str] = &[
    "build/index.js",
    "build/index_bg.wasm",
    "build/worker/shim.mjs",
    "deploy/wrangler.toml.tmpl",
    "wrangler/package.json",
    "wrangler/package-lock.json",
    "VERSION",
];

fn refuse(why: String) -> CliError {
    CliError::verification(format!("the release tarball is unsafe: {why}"))
}

/// A tar path as a relative path inside the target, or a refusal.
fn safe_path(p: &Path) -> Result<PathBuf, CliError> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => {
                return Err(refuse(format!(
                    "the path {} leaves the bundle",
                    p.display()
                )));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(refuse("an empty path".into()));
    }
    Ok(out)
}

/// Extracts `tar_gz` into `target`, which must be empty or absent.
pub fn extract(tar_gz: &[u8], target: &Path, version: &str) -> Result<(), CliError> {
    let io = |e: std::io::Error| CliError::internal(format!("{}: {e}", target.display()));
    std::fs::create_dir_all(target).map_err(io)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tar_gz));
    let mut entries = 0usize;
    let mut total = 0u64;
    for entry in archive.entries().map_err(|e| refuse(e.to_string()))? {
        let mut entry = entry.map_err(|e| refuse(e.to_string()))?;
        entries += 1;
        if entries > MAX_ENTRIES {
            return Err(refuse(format!("more than {MAX_ENTRIES} entries")));
        }
        let path = entry
            .path()
            .map_err(|e| refuse(e.to_string()))?
            .into_owned();
        let rel = safe_path(&path)?;
        let dest = target.join(&rel);
        match entry.header().entry_type() {
            tar::EntryType::Directory => std::fs::create_dir_all(&dest).map_err(io)?,
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                let size = entry.header().size().map_err(|e| refuse(e.to_string()))?;
                total += size;
                if total > MAX_BYTES {
                    return Err(refuse("more than 128 MiB unpacked".into()));
                }
                let mut bytes = Vec::with_capacity(size as usize);
                entry
                    .by_ref()
                    .take(MAX_BYTES)
                    .read_to_end(&mut bytes)
                    .map_err(|e| refuse(e.to_string()))?;
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                std::fs::write(&dest, &bytes).map_err(io)?;
            }
            other => {
                return Err(refuse(format!(
                    "{} is a {other:?}, not a file",
                    path.display()
                )));
            }
        }
    }
    for f in EXPECTED {
        if !target.join(f).is_file() {
            return Err(CliError::verification(format!(
                "the release bundle has no {f}"
            )));
        }
    }
    let has_migrations = std::fs::read_dir(target.join("migrations/d1"))
        .map(|d| {
            d.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "sql"))
        })
        .unwrap_or(false);
    if !has_migrations {
        return Err(CliError::verification(
            "the release bundle has no migrations/d1/*.sql",
        ));
    }
    let v = std::fs::read_to_string(target.join("VERSION")).map_err(io)?;
    if v.trim() != version {
        return Err(CliError::verification(format!(
            "the bundle's VERSION is {}, not {version}",
            v.trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn paths_stay_inside() {
        assert!(super::safe_path(Path::new("build/index.js")).is_ok());
        assert!(super::safe_path(Path::new("../x")).is_err());
        assert!(super::safe_path(Path::new("/etc/passwd")).is_err());
        assert!(super::safe_path(Path::new("a/../../b")).is_err());
    }
}
