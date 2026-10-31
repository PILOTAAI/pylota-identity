//! `cargo xtask release --check-version <tag>` and `release --version <v>` (Rust workspace § 9, § 10):
//! the version rule, and the unsigned release files in `dist/`: the Worker bundle, the CLI archives the
//! CI matrix built, `openapi.json`, the SDK fixtures tarball, the `*-tests-required.json` contracts of
//! the SDK repositories, and `SHA256SUMS`. Every archive carries `LICENSE.md` and a
//! `THIRD_PARTY_LICENSES` file (`gen-licenses`). Nothing here signs: the key never enters CI.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest as _, Sha256};

/// The CLI targets of the release matrix, with their archive's binary name.
pub const CLI_TARGETS: &[(&str, &str)] = &[
    ("aarch64-apple-darwin", "idvy"),
    ("x86_64-apple-darwin", "idvy"),
    ("x86_64-unknown-linux-gnu", "idvy"),
    ("aarch64-unknown-linux-gnu", "idvy"),
    ("x86_64-pc-windows-msvc", "idvy.exe"),
];

/// SemVer with an optional pre-release part (`1.0.0`, `1.0.0-rc.1`).
pub fn semver(v: &str) -> bool {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let nums: Vec<&str> = core.split('.').collect();
    let numeric = |s: &str| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
    };
    nums.len() == 3
        && nums.iter().all(|n| numeric(n))
        && pre.is_none_or(|p| {
            !p.is_empty()
                && p.split('.').all(|id| {
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
        })
}

/// The version rule: `tag` is `v` followed by `version`, a SemVer version.
pub fn version_matches(tag: &str, version: &str) -> Result<(), String> {
    if !semver(version) {
        return Err(format!("the workspace version {version} is not SemVer"));
    }
    match tag.strip_prefix('v') {
        Some(v) if v == version => Ok(()),
        Some(v) => Err(format!(
            "the tag {tag} names {v}, but the workspace version is {version}"
        )),
        None => Err(format!("the tag {tag} does not start with v")),
    }
}

/// The Apache-2.0 date of a release: its second anniversary (29 February gives 28 February).
pub fn apache_date(release: &str) -> Option<String> {
    let (y, rest) = release.split_once('-')?;
    let (m, d) = rest.split_once('-')?;
    let (y, m, d): (u32, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || release.len() != 10 {
        return None;
    }
    let d = if m == 2 && d == 29 { 28 } else { d };
    Some(format!("{:04}-{m:02}-{d:02}", y + 2))
}

/// The `CHANGELOG.md` entry of `version`: its heading names the release date and the Apache-2.0 date;
/// answers the entry's notes. A pre-release takes the notes of `## Unreleased`.
pub fn changelog_entry(text: &str, version: &str) -> Result<String, String> {
    let pre = version.contains('-');
    let want = if pre {
        "## Unreleased".to_owned()
    } else {
        format!("## v{version} — ")
    };
    let mut lines = text.lines();
    let heading = lines
        .by_ref()
        .find(|l| l.starts_with(&want))
        .ok_or_else(|| format!("CHANGELOG.md has no entry `{want}…`"))?;
    if !pre {
        let rest = &heading[want.len()..];
        let (date, apache) = rest.split_once(" (Apache-2.0 on ").ok_or_else(|| {
            format!("the CHANGELOG heading {heading:?} lacks \" (Apache-2.0 on <date>)\"")
        })?;
        let apache = apache.trim_end_matches(')');
        if apache_date(date.trim()).as_deref() != Some(apache) {
            return Err(format!(
                "CHANGELOG: v{version} released {date} becomes Apache-2.0 on {}, not {apache}",
                apache_date(date.trim()).unwrap_or_else(|| "?".into())
            ));
        }
    }
    let notes: Vec<&str> = lines.take_while(|l| !l.starts_with("## ")).collect();
    Ok(notes.join("\n").trim().to_owned())
}

/// The release notes of `version`: its CHANGELOG entry, with the release and Apache-2.0 dates.
pub fn notes(root: &Path, version: &str) -> Result<String, String> {
    let log = std::fs::read_to_string(root.join("CHANGELOG.md"))
        .map_err(|e| format!("CHANGELOG.md: {e}"))?;
    let body = changelog_entry(&log, version)?;
    let dates = log
        .lines()
        .find(|l| l.starts_with(&format!("## v{version} — ")))
        .map(|l| {
            l.trim_start_matches(&format!("## v{version} — "))
                .to_owned()
        });
    Ok(match dates {
        Some(d) => format!("Released {d}.\n\n{body}"),
        None => format!("A release candidate: not for production.\n\n{body}"),
    })
}

pub fn check_version(root: &Path, tag: &str) -> Result<(), String> {
    let version = crate::build_worker::workspace_version(root)?;
    version_matches(tag, &version)?;
    let log = std::fs::read_to_string(root.join("CHANGELOG.md"))
        .map_err(|e| format!("CHANGELOG.md: {e}"))?;
    changelog_entry(&log, &version)?;
    println!("{tag} matches the workspace version");
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// A reproducible `.tar.gz` of `stage` (sorted names, no owners, the mtime of `SOURCE_DATE_EPOCH`).
pub fn tar_gz(stage: &Path, out: &Path) -> Result<(), String> {
    let mtime = std::env::var("SOURCE_DATE_EPOCH").unwrap_or_else(|_| "0".into());
    let tar = out.with_extension("");
    run(
        Command::new("tar")
            .args([
                "--sort=name",
                "--owner=0",
                "--group=0",
                "--numeric-owner",
                &format!("--mtime=@{mtime}"),
                "-cf",
            ])
            .arg(&tar)
            .arg("-C")
            .arg(stage)
            .arg("."),
        "tar",
    )?;
    run(
        Command::new("gzip").args(["-n", "-9", "-f"]).arg(&tar),
        "gzip",
    )?;
    let gz = PathBuf::from(format!("{}.gz", tar.display()));
    if gz != out {
        std::fs::rename(&gz, out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The files of a `.tar.gz`, without a leading `./`.
#[cfg(test)]
pub fn tar_list(archive: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("tar")
        .arg("-tzf")
        .arg(archive)
        .output()
        .map_err(|e| format!("tar: {e}"))?;
    if !out.status.success() {
        return Err(format!("tar -tzf {}", archive.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim_start_matches("./").to_owned())
        .filter(|l| !l.is_empty() && !l.ends_with('/'))
        .collect())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", from.display()))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let target = to.join(e.file_name());
        if e.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            copy(&e.path(), &target)?;
        }
    }
    Ok(())
}

/// Stages the Worker bundle's files from the tree into `stage` (the built Worker must be in
/// `crates/worker/build/`).
pub fn stage_bundle(root: &Path, stage: &Path, version: &str) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(stage);
    for f in ["index.js", "index_bg.wasm", "worker/shim.mjs"] {
        copy(
            &root.join("crates/worker/build").join(f),
            &stage.join("build").join(f),
        )?;
    }
    copy_dir(&root.join("migrations/d1"), &stage.join("migrations/d1"))?;
    copy(
        &root.join("deploy/wrangler.toml.tmpl"),
        &stage.join("deploy/wrangler.toml.tmpl"),
    )?;
    copy_dir(&root.join("deploy/wrangler"), &stage.join("wrangler"))?;
    copy(&root.join("LICENSE.md"), &stage.join("LICENSE.md"))?;
    copy(
        &root.join(crate::licenses::WORKER_FILE),
        &stage.join("THIRD_PARTY_LICENSES"),
    )?;
    std::fs::write(stage.join("VERSION"), version).map_err(|e| e.to_string())?;
    Ok(())
}

/// One CLI archive: the binary, `LICENSE.md` and the CLI's `THIRD_PARTY_LICENSES`.
pub fn cli_archive(root: &Path, bin: &Path, name: &str, out: &Path) -> Result<(), String> {
    let stage = out.with_extension("stage");
    let _ = std::fs::remove_dir_all(&stage);
    copy(bin, &stage.join(name))?;
    copy(&root.join("LICENSE.md"), &stage.join("LICENSE.md"))?;
    copy(
        &root.join("crates/cli/THIRD_PARTY_LICENSES"),
        &stage.join("THIRD_PARTY_LICENSES"),
    )?;
    let r = tar_gz(&stage, out);
    let _ = std::fs::remove_dir_all(&stage);
    r
}

/// `SHA256SUMS`: `<sha256 hex>␠␠<filename>` per file of `dist`, in name order.
pub fn write_sums(dist: &Path) -> Result<String, String> {
    let mut names: Vec<String> = std::fs::read_dir(dist)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "SHA256SUMS" && n != "SHA256SUMS.sig")
        .collect();
    names.sort();
    let mut text = String::new();
    for n in names {
        let bytes = std::fs::read(dist.join(&n)).map_err(|e| e.to_string())?;
        text.push_str(&format!("{}  {n}\n", sha256_hex(&bytes)));
    }
    std::fs::write(dist.join("SHA256SUMS"), &text).map_err(|e| e.to_string())?;
    Ok(text)
}

/// Every `prefix` test name the register and the design pages use, with the register rows that name
/// it: the contract an SDK repository builds its inventory from.
pub fn required_tests(root: &Path, prefix: &str) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut scan = |text: &str, rows: bool| {
        for line in text.lines() {
            let row = rows
                .then(|| {
                    line.strip_prefix("| ")
                        .and_then(|l| l.split(" |").next())
                        .filter(|id| {
                            id.len() <= 5
                                && id.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                                && id[1..].bytes().all(|b| b.is_ascii_digit())
                        })
                })
                .flatten();
            for part in line.split('`').skip(1).step_by(2) {
                if part.starts_with(prefix)
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_:*".contains(&b))
                {
                    let e = out.entry(part.to_owned()).or_default();
                    if let Some(r) = row
                        && !e.iter().any(|x| x == r)
                    {
                        e.push(r.to_owned());
                    }
                }
            }
        }
    };
    let register = std::fs::read_to_string(root.join("docs/src/project/edge-cases.md"))
        .map_err(|e| format!("edge-cases.md: {e}"))?;
    scan(&register, true);
    let design = root.join("docs/src/project/design");
    let mut pages: Vec<PathBuf> = std::fs::read_dir(&design)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    pages.sort();
    for p in pages {
        scan(
            &std::fs::read_to_string(&p).map_err(|e| e.to_string())?,
            false,
        );
    }
    Ok(out)
}

/// `release --version <v>`: the version rule, the Worker build, then every file of `dist/`.
pub fn package(root: &Path, version: &str) -> Result<(), String> {
    version_matches(
        &format!("v{version}"),
        &crate::build_worker::workspace_version(root)?,
    )?;
    crate::licenses::check(root)?;
    crate::build_worker::run(root)?;
    let dist = root.join("dist");
    let bins = dist.join("bin");
    std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;

    let stage = root.join("target/release-stage/bundle");
    stage_bundle(root, &stage, version)?;
    tar_gz(
        &stage,
        &dist.join(format!("pylota-identity-worker-{version}.tar.gz")),
    )?;

    let yaml = std::fs::read_to_string(root.join("docs/src/reference/openapi.yaml"))
        .map_err(|e| format!("openapi.yaml: {e}"))?;
    let doc: serde_json::Value =
        yaml_serde::from_str(&yaml).map_err(|e| format!("openapi.yaml: {e}"))?;
    std::fs::write(
        dist.join("openapi.json"),
        serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let fixtures = root.join("sdks/fixtures");
    if fixtures.is_dir() {
        tar_gz(
            &fixtures,
            &dist.join(format!("sdk-fixtures-{version}.tar.gz")),
        )?;
    }
    for (prefix, file) in [
        ("ts::", "ts-tests-required.json"),
        ("py::", "py-tests-required.json"),
        ("go::", "go-tests-required.json"),
    ] {
        let tests: Vec<serde_json::Value> = required_tests(root, prefix)?
            .into_iter()
            .map(|(name, rows)| serde_json::json!({ "name": name, "rows": rows }))
            .collect();
        std::fs::write(
            dist.join(file),
            serde_json::to_vec_pretty(&serde_json::json!({ "version": version, "tests": tests }))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }

    // The CLI binaries the matrix built: `dist/bin/<target>/idvy[.exe]`.
    let mut missing = Vec::new();
    for (target, name) in CLI_TARGETS {
        let bin = bins.join(target).join(name);
        if bin.is_file() {
            cli_archive(
                root,
                &bin,
                name,
                &dist.join(format!("idvy-{version}-{target}.tar.gz")),
            )?;
        } else {
            missing.push(*target);
        }
    }
    let _ = std::fs::remove_dir_all(&bins);
    if !missing.is_empty() {
        println!("warning: no CLI binary for {}", missing.join(", "));
    }
    print!("{}", write_sums(&dist)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    /// `xtask::release_version_matches_tag` (Rust workspace § 9): the tag must be `v` plus the
    /// workspace version, a SemVer version with an optional pre-release part.
    #[test]
    fn release_version_matches_tag() {
        assert!(version_matches("v1.0.0", "1.0.0").is_ok());
        assert!(version_matches("v1.0.0-rc.1", "1.0.0-rc.1").is_ok());
        assert!(version_matches("1.0.0", "1.0.0").is_err(), "no v");
        assert!(
            version_matches("v1.0.1", "1.0.0").is_err(),
            "another version"
        );
        assert!(version_matches("v1.0.0-rc.2", "1.0.0-rc.1").is_err());
        assert!(version_matches("v1.0", "1.0").is_err(), "not SemVer");
        assert!(
            version_matches("v01.0.0", "01.0.0").is_err(),
            "a leading zero"
        );
        assert!(version_matches("v1.0.0-", "1.0.0-").is_err());
        let tree = root().join("target/release-version-test");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(
            tree.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.0.0-rc.1\"\n",
        )
        .unwrap();
        std::fs::write(
            tree.join("CHANGELOG.md"),
            "# Changelog\n\n## Unreleased\n\n- x\n",
        )
        .unwrap();
        assert!(check_version(&tree, "v1.0.0-rc.1").is_ok());
        assert!(check_version(&tree, "v99.0.0").is_err());
        std::fs::write(
            tree.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        assert!(
            check_version(&tree, "v1.0.0").is_err(),
            "a full release needs its CHANGELOG entry"
        );
    }

    /// `xtask::release_artefacts_carry_licence` (build plan M19): the Worker bundle and each CLI
    /// archive hold `LICENSE.md` and a `THIRD_PARTY_LICENSES`; each published crate's package lists
    /// both; the generated files are current.
    #[test]
    fn release_artefacts_carry_licence() {
        let root = root();
        crate::licenses::check(&root).unwrap();
        let tmp = root.join("target/release-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // A bundle staged from fake build outputs (the real Worker build is the release job's).
        let build = tmp.join("tree");
        for f in ["index.js", "index_bg.wasm", "worker/shim.mjs"] {
            let p = build.join("crates/worker/build").join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        for (from, to) in [
            ("migrations/d1", "migrations/d1"),
            ("deploy/wrangler", "deploy/wrangler"),
        ] {
            copy_dir(&root.join(from), &build.join(to)).unwrap();
        }
        for f in [
            "deploy/wrangler.toml.tmpl",
            "LICENSE.md",
            crate::licenses::WORKER_FILE,
            "crates/cli/THIRD_PARTY_LICENSES",
        ] {
            copy(&root.join(f), &build.join(f)).unwrap();
        }
        let stage = tmp.join("stage");
        stage_bundle(&build, &stage, "1.0.0-rc.1").unwrap();
        let bundle = tmp.join("pylota-identity-worker-1.0.0-rc.1.tar.gz");
        tar_gz(&stage, &bundle).unwrap();
        let files = tar_list(&bundle).unwrap();
        for f in [
            "LICENSE.md",
            "THIRD_PARTY_LICENSES",
            "VERSION",
            "wrangler/package-lock.json",
            "build/index_bg.wasm",
        ] {
            assert!(files.iter().any(|x| x == f), "bundle lacks {f}: {files:?}");
        }

        let bin = tmp.join("idvy");
        std::fs::write(&bin, "binary").unwrap();
        let archive = tmp.join("idvy-1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz");
        cli_archive(&build, &bin, "idvy", &archive).unwrap();
        let files = tar_list(&archive).unwrap();
        for f in ["idvy", "LICENSE.md", "THIRD_PARTY_LICENSES"] {
            assert!(
                files.iter().any(|x| x == f),
                "CLI archive lacks {f}: {files:?}"
            );
        }

        for c in crate::licenses::PUBLISHED {
            let out = Command::new(env!("CARGO"))
                .args(["package", "--list", "--allow-dirty", "--offline", "-p", c])
                .current_dir(&root)
                .output()
                .unwrap();
            let list = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success(),
                "{c}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            for f in ["LICENSE.md", "THIRD_PARTY_LICENSES"] {
                assert!(
                    list.lines().any(|l| l == f),
                    "{c}'s package lacks {f}:\n{list}"
                );
            }
        }

        // SHA256SUMS lists every file but itself.
        let sums = write_sums(&tmp).unwrap();
        assert!(sums.contains(&format!(
            "{}  idvy-1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz",
            sha256_hex(&std::fs::read(&archive).unwrap())
        )));
        assert!(!sums.contains("SHA256SUMS"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The changelog names each release's date and its Apache-2.0 date, two years later.
    #[test]
    fn release_changelog_dates() {
        let log = "# Changelog\n\n## Unreleased\n\n- next\n\n## v1.0.0 — 2027-01-15 (Apache-2.0 on 2029-01-15)\n\n- first\n";
        assert_eq!(changelog_entry(log, "1.0.0").unwrap(), "- first");
        assert_eq!(changelog_entry(log, "1.1.0-rc.1").unwrap(), "- next");
        assert!(changelog_entry(log, "1.1.0").is_err(), "no entry");
        let wrong = log.replace("2029-01-15", "2028-01-15");
        assert!(
            changelog_entry(&wrong, "1.0.0").is_err(),
            "a wrong Apache date"
        );
        let bare = log.replace(" (Apache-2.0 on 2029-01-15)", "");
        assert!(changelog_entry(&bare, "1.0.0").is_err(), "no Apache date");
        assert_eq!(apache_date("2028-02-29").as_deref(), Some("2030-02-28"));
        assert_eq!(apache_date("2027-13-01"), None);
    }

    #[test]
    fn required_tests_carry_rows() {
        let t = required_tests(&root(), "ts::").unwrap();
        assert!(t.keys().all(|k| k.starts_with("ts::")));
    }
}
