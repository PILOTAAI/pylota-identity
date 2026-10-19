//! `cargo xtask check-layering`: the dependency rules of Rust workspace § 2.

use std::path::Path;

/// One broken rule, printed as is.
pub type Violation = String;

pub fn run(root: &Path) -> Result<(), String> {
    let mut violations = check_manifest(root)?;
    violations.extend(check_wasm_graph(root)?);
    if violations.is_empty() {
        println!("check-layering: ok (rules 1 to 4 of Rust workspace § 2)");
        Ok(())
    } else {
        Err(format!(
            "check-layering failed:\n  {}",
            violations.join("\n  ")
        ))
    }
}

/// Rule 1: only `pylota-mail-platform` may depend on these directly.
const WORKER_FAMILY: &[&str] = &[
    "worker",
    "worker-sys",
    "worker-macros",
    "wasm-bindgen",
    "js-sys",
    "web-sys",
];

/// Rule 2 and the other "Must not depend on" cells of the § 2 table, for normal and build
/// dependencies. Rule 1 already covers the `worker` family.
const FORBIDDEN: &[(&str, &[&str])] = &[
    (
        "pylota-mail-core",
        &["pylota-mail-platform", "reqwest", "tokio"],
    ),
    (
        "pylota-mail-platform",
        &["pylota-mail-core", "tokio", "reqwest"],
    ),
    ("pylota-mail-worker", &["tokio", "reqwest"]),
    ("pylota-mail", &["pylota-mail-platform", "pylota-mail-core"]),
    ("pylota-mail-cli", &["pylota-mail-platform"]),
];

/// `api-types` may depend on these and nothing else.
const API_TYPES_ALLOWED: &[&str] = &["serde", "serde_json", "utoipa"];

/// Rules 1, 2 and 4, read from `cargo metadata --no-deps` and the root `Cargo.toml` of the workspace
/// at `root`. Needs no network and no lockfile.
pub fn check_manifest(root: &Path) -> Result<Vec<Violation>, String> {
    let manifest = root.join("Cargo.toml");
    let output = crate::cargo()
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("cargo metadata output: {e}"))?;

    let mut violations = Vec::new();
    for package in metadata["packages"].as_array().into_iter().flatten() {
        let name = package["name"].as_str().unwrap_or_default();
        for dep in package["dependencies"].as_array().into_iter().flatten() {
            let dep_name = dep["name"].as_str().unwrap_or_default();
            let kind = dep["kind"].as_str().unwrap_or("normal");
            let shipped = kind == "normal" || kind == "build";
            if shipped && name != "pylota-mail-platform" && WORKER_FAMILY.contains(&dep_name) {
                violations.push(format!(
                    "rule 1: {name} depends on `{dep_name}` ({kind}); only pylota-mail-platform may"
                ));
            }
            if shipped {
                let banned = FORBIDDEN.iter().find(|(p, _)| *p == name).map(|(_, b)| *b);
                if banned.is_some_and(|b| b.contains(&dep_name)) {
                    violations.push(format!(
                        "rule 2: {name} must not depend on `{dep_name}` ({kind})"
                    ));
                }
                if name == "pylota-mail-api-types" && !API_TYPES_ALLOWED.contains(&dep_name) {
                    violations.push(format!(
                        "rule 2: pylota-mail-api-types may depend only on serde, serde_json and utoipa, not `{dep_name}`"
                    ));
                }
            }
            if dep.get("path").is_none() {
                let req = dep["req"].as_str().unwrap_or_default();
                if !is_exact_pin(req) {
                    violations.push(format!("rule 4: {name} depends on `{dep_name}` with `{req}`, not an exact =x.y.z pin"));
                }
            }
        }
    }

    let text =
        std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    violations.extend(workspace_pin_violations(&text)?);
    Ok(violations)
}

/// Rule 4: every entry in `[workspace.dependencies]` is an exact `=x.y.z` pin.
pub fn workspace_pin_violations(cargo_toml: &str) -> Result<Vec<Violation>, String> {
    let doc: toml::Table = cargo_toml
        .parse()
        .map_err(|e| format!("root Cargo.toml: {e}"))?;
    let mut violations = Vec::new();
    let deps = doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table());
    for (name, spec) in deps.into_iter().flatten() {
        let version = match spec {
            toml::Value::String(v) => Some(v.as_str()),
            toml::Value::Table(t) => t.get("version").and_then(|v| v.as_str()),
            _ => None,
        };
        match version {
            Some(v) if is_exact_pin(v) => {}
            Some(v) => violations.push(format!(
                "rule 4: [workspace.dependencies] {name} = `{v}` is not an exact =x.y.z pin"
            )),
            None => violations.push(format!(
                "rule 4: [workspace.dependencies] {name} has no version"
            )),
        }
    }
    Ok(violations)
}

/// `=1.2.3`, optionally with a pre-release or build part (`=0.10.0-rc.19`).
fn is_exact_pin(req: &str) -> bool {
    let Some(version) = req.strip_prefix('=') else {
        return false;
    };
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Rule 3, on the normal and build graph of the Worker for wasm32: tokio only directly under
/// `worker` and with no features; no `gethostname` or `hickory-resolver`.
fn check_wasm_graph(root: &Path) -> Result<Vec<Violation>, String> {
    let tree = |extra: &[&str]| -> Result<(bool, String, String), String> {
        let output = crate::cargo()
            .current_dir(root)
            .args([
                "tree",
                "--locked",
                "-p",
                "pylota-mail-worker",
                "--target",
                "wasm32-unknown-unknown",
                "-e",
                "no-dev",
            ])
            .args(extra)
            .output()
            .map_err(|e| format!("cannot run cargo tree: {e}"))?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    };

    let mut violations = Vec::new();
    let (ok, stdout, stderr) = tree(&["-i", "tokio", "--prefix", "depth", "-f", "{p} {f}"])?;
    if ok {
        violations.extend(tokio_violations(&stdout));
    } else if !stderr.contains("did not match any packages") {
        return Err(format!("cargo tree -i tokio failed: {stderr}"));
    }

    let (ok, stdout, stderr) = tree(&["--prefix", "none", "-f", "{p}"])?;
    if !ok {
        return Err(format!("cargo tree failed: {stderr}"));
    }
    violations.extend(banned_violations(&stdout));
    Ok(violations)
}

/// Reads `cargo tree -i tokio --prefix depth -f '{p} {f}'`: every tokio has an empty feature list
/// and `worker` as its only direct dependent.
pub fn tokio_violations(inverted: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    for line in inverted.lines() {
        let depth_len = line.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(depth) = line[..depth_len].parse::<u32>() else {
            continue;
        };
        let tokens: Vec<&str> = line[depth_len..].split_whitespace().collect();
        match (depth, tokens.as_slice()) {
            (0, [_, version, features @ ..]) if !features.is_empty() => violations.push(format!(
                "rule 3: tokio {version} has features `{}` in the Worker's wasm32 graph; it must have none",
                features.join(" ")
            )),
            (1, [dependent, ..]) if *dependent != "worker" => violations.push(format!(
                "rule 3: `{dependent}` depends on tokio in the Worker's wasm32 graph; only `worker` may"
            )),
            _ => {}
        }
    }
    violations
}

/// Reads `cargo tree --prefix none -f '{p}'`: no `gethostname` and no `hickory-resolver`.
pub fn banned_violations(listing: &str) -> Vec<Violation> {
    let mut found: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| ["gethostname", "hickory-resolver"].contains(name))
        .collect();
    found.dedup();
    found
        .into_iter()
        .map(|name| format!("rule 3: `{name}` is in the Worker's wasm32 graph"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// A throwaway workspace outside the repository, removed on drop.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            let dir = std::env::temp_dir()
                .join(format!("pm-xtask-{name}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Fixture(dir)
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn crate_with(&self, dir: &str, name: &str, deps: &str) {
            self.write(
                &format!("{dir}/Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{deps}\n"),
            );
            self.write(&format!("{dir}/src/lib.rs"), "");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A fixture crate depending on `worker` fails the check; `platform` may (AGENTS.md rule).
    #[test]
    fn check_layering_rejects_worker_dep() {
        let fx = Fixture::new("layering");
        fx.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"platform\", \"logic\"]\n\n[workspace.dependencies]\nworker = \"=0.8.7\"\n",
        );
        fx.crate_with(
            "platform",
            "pylota-mail-platform",
            "worker = { workspace = true }",
        );
        fx.crate_with("logic", "fixture-logic", "worker = { workspace = true }");

        let violations = check_manifest(&fx.0).unwrap();

        assert!(
            violations
                .iter()
                .any(|v| v.contains("fixture-logic") && v.contains("`worker`")),
            "a crate other than platform depending on worker must fail: {violations:?}"
        );
        assert!(
            !violations
                .iter()
                .any(|v| v.starts_with("rule 1: pylota-mail-platform ")),
            "platform may depend on worker: {violations:?}"
        );
    }

    /// Rule 4 refuses a caret requirement and a path-only entry in `[workspace.dependencies]`.
    #[test]
    fn check_layering_rejects_inexact_pins() {
        let toml = "[workspace.dependencies]\na = \"=1.2.3\"\nb = \"1.2.3\"\nc = { path = \"c\" }\nd = { version = \"=0.10.0-rc.19\" }\n";
        let v = workspace_pin_violations(toml).unwrap();
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v[0].contains(" b ") && v[1].contains(" c "), "{v:?}");
    }

    /// Rule 3 accepts tokio with no features under `worker` only.
    #[test]
    fn check_layering_rejects_tokio_outside_worker() {
        let good = "0tokio v1.54.0 \n1worker v0.8.7 d1,queue\n2pylota-mail-platform v0.1.0 (/x)\n";
        assert!(tokio_violations(good).is_empty());
        let featured = "0tokio v1.54.0 rt,time\n1worker v0.8.7 d1,queue\n";
        assert_eq!(tokio_violations(featured).len(), 1);
        let other = "0tokio v1.54.0 \n1worker v0.8.7 d1,queue\n1rmcp v3.4.1 default\n";
        assert_eq!(tokio_violations(other).len(), 1);
        assert_eq!(
            banned_violations("serde v1.0.229\ngethostname v1.0.0\n").len(),
            1
        );
    }
}
