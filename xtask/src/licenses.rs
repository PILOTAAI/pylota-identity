//! `cargo xtask gen-licenses [--check]` (build plan M19): the `THIRD_PARTY_LICENSES` file of each
//! artefact, listing every third-party crate in its build graph with its version, licence and source.
//! The list is the component list of the artefact's SBOM, read from the same place (`cargo metadata`
//! resolve graph, normal dependencies only; the Worker's on `wasm32-unknown-unknown`). The files are
//! committed, so `cargo package` puts them in each crate and `--check` fails on a stale one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// The Worker bundle's file.
pub const WORKER_FILE: &str = "crates/worker/THIRD_PARTY_LICENSES";
/// The crates published to crates.io (Rust workspace § 9, `release crates`).
pub const PUBLISHED: &[&str] = &[
    "pylota-identity-api-types",
    "pylota-identity",
    "pylota-identity-cli",
];

/// `(root package, platform filter, output)`.
const ARTEFACTS: &[(&str, Option<&str>, &str)] = &[
    (
        "pylota-identity-worker",
        Some("wasm32-unknown-unknown"),
        WORKER_FILE,
    ),
    (
        "pylota-identity-cli",
        None,
        "crates/cli/THIRD_PARTY_LICENSES",
    ),
    ("pylota-identity", None, "crates/sdk/THIRD_PARTY_LICENSES"),
    (
        "pylota-identity-api-types",
        None,
        "crates/api-types/THIRD_PARTY_LICENSES",
    ),
];

fn metadata(root: &Path, platform: Option<&str>) -> Result<Value, String> {
    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.args(["metadata", "--format-version", "1", "--locked", "--offline"]);
    if let Some(p) = platform {
        cmd.args(["--filter-platform", p]);
    }
    let out = cmd
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

fn first_party(name: &str) -> bool {
    name.starts_with("pylota-identity") || name == "xtask"
}

/// The file for one artefact.
pub fn render(meta: &Value, root_name: &str) -> Result<String, String> {
    let packages: BTreeMap<&str, &Value> = meta["packages"]
        .as_array()
        .ok_or("no packages")?
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let nodes: BTreeMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .ok_or("no resolve graph")?
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();
    let start = packages
        .iter()
        .find(|(_, p)| p["name"] == root_name)
        .map(|(id, _)| *id)
        .ok_or_else(|| format!("{root_name} is not in the workspace"))?;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut todo = vec![start];
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        for d in nodes
            .get(id)
            .and_then(|n| n["deps"].as_array())
            .into_iter()
            .flatten()
        {
            let normal = d["dep_kinds"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k["kind"].is_null()));
            if normal && let Some(pkg) = d["pkg"].as_str() {
                todo.push(pkg);
            }
        }
    }
    let mut rows: BTreeSet<(String, String, String, String)> = BTreeSet::new();
    for id in seen {
        let p = packages[id];
        let name = p["name"].as_str().unwrap_or("");
        if first_party(name) {
            continue;
        }
        rows.insert((
            name.to_owned(),
            p["version"].as_str().unwrap_or("").to_owned(),
            p["license"]
                .as_str()
                .unwrap_or("see its licence file")
                .to_owned(),
            p["repository"].as_str().unwrap_or("").to_owned(),
        ));
    }
    let mut out = format!(
        "Third-party software in {root_name}\n\nWritten by `cargo xtask gen-licenses` from the Cargo dependency graph, the component list of\nthe artefact's SBOM. Each crate is used under the licence named here; its licence text is in its\nsource at the version named.\n\n"
    );
    for (name, version, license, repo) in rows {
        out.push_str(&format!("{name} {version}\n  licence: {license}\n"));
        if !repo.is_empty() {
            out.push_str(&format!("  source: {repo}\n"));
        }
    }
    Ok(out)
}

fn expected(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut cache: BTreeMap<Option<&str>, Value> = BTreeMap::new();
    let mut out = Vec::new();
    for (name, platform, file) in ARTEFACTS {
        if !cache.contains_key(platform) {
            cache.insert(*platform, metadata(root, *platform)?);
        }
        out.push(((*file).to_owned(), render(&cache[platform], name)?));
    }
    Ok(out)
}

pub fn run(root: &Path, check_only: bool) -> Result<(), String> {
    if check_only {
        return check(root);
    }
    for (file, text) in expected(root)? {
        std::fs::write(root.join(&file), text).map_err(|e| format!("{file}: {e}"))?;
        println!("wrote {file}");
    }
    Ok(())
}

/// Fails when a committed file differs from what the graph gives.
pub fn check(root: &Path) -> Result<(), String> {
    for (file, text) in expected(root)? {
        let have = std::fs::read_to_string(root.join(&file)).unwrap_or_default();
        if have != text {
            return Err(format!("{file} is stale: run cargo xtask gen-licenses"));
        }
    }
    Ok(())
}
