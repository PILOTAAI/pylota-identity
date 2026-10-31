//! The owner's release steps and the gate (Rust workspace § 9):
//!
//! - `cargo xtask release sign <tag> [--repo <r>] --key <path>`, on the owner's machine with the
//!   offline key: checks every file of the draft against `SHA256SUMS` and its provenance (`release.yml`
//!   on that tag), signs `SHA256SUMS` with the trusted comment `{product} v{version}`, verifies its own
//!   output against the compiled-in keys, uploads `SHA256SUMS.sig` and starts `release-publish.yml`.
//! - `cargo xtask release crates <tag>`: publishes the three crates from a clean clone of a signed,
//!   full release.
//! - `cargo xtask release-gate --version <v> [--pre-release]`: the security pre-release checklist of
//!   `release-gates/v{version}.md` and the facts it can check.
//!
//! Every outside call goes through [`Env`], so the refusals are tested without GitHub.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::release::{sha256_hex, version_matches};

pub const REPO: &str = "PILOTAAI/pylota-identity";

/// The repositories `release sign --repo` accepts, with their trusted-comment product name.
pub const REPOS: &[(&str, &str)] = &[
    ("PILOTAAI/pylota-identity", "pylota-identity"),
    (
        "PILOTAAI/pylota-identity-typescript",
        "pylota-identity-typescript",
    ),
    ("PILOTAAI/pylota-identity-python", "pylota-identity-python"),
    ("PILOTAAI/pylota-identity-go", "pylota-identity-go"),
];

/// The outside world: `gh`, `git`, `cargo` and the password prompt.
pub trait Env {
    /// `gh <args>`: its stdout, or its stderr as the error.
    fn gh(&self, args: &[&str]) -> Result<String, String>;
    fn git(&self, dir: &Path, args: &[&str]) -> Result<String, String>;
    /// Whether `name` is on crates.io at `version`.
    fn published(&self, name: &str, version: &str) -> Result<bool, String>;
    fn cargo_publish(&self, dir: &Path, name: &str) -> Result<(), String>;
    /// `None`: minisign prompts for it.
    fn password(&self) -> Option<String>;
}

pub struct RealEnv;

fn output(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{what}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

impl Env for RealEnv {
    fn gh(&self, args: &[&str]) -> Result<String, String> {
        output(
            Command::new("gh").args(args),
            &format!("gh {}", args.first().unwrap_or(&"")),
        )
    }
    fn git(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        output(Command::new("git").args(args).current_dir(dir), "git")
    }
    fn published(&self, name: &str, version: &str) -> Result<bool, String> {
        let out = Command::new(crate::cargo().get_program())
            .args([
                "info",
                &format!("{name}@{version}"),
                "--registry",
                "crates-io",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        Ok(out.status.success())
    }
    fn cargo_publish(&self, dir: &Path, name: &str) -> Result<(), String> {
        output(
            crate::cargo()
                .args(["publish", "--locked", "-p", name])
                .current_dir(dir),
            &format!("cargo publish {name}"),
        )
        .map(|_| ())
    }
    fn password(&self) -> Option<String> {
        None
    }
}

/// `SHA256SUMS` as `(file, hex)` pairs.
pub fn parse_sums(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (hex, name) = line
            .split_once("  ")
            .ok_or_else(|| format!("SHA256SUMS: a malformed line {line:?}"))?;
        if hex.len() != 64 || name.contains('/') || name.is_empty() {
            return Err(format!("SHA256SUMS: a malformed line {line:?}"));
        }
        out.push((name.to_owned(), hex.to_owned()));
    }
    Ok(out)
}

fn product(repo: &str) -> Result<&'static str, String> {
    REPOS
        .iter()
        .find(|(r, _)| *r == repo)
        .map(|(_, p)| *p)
        .ok_or_else(|| format!("--repo {repo} is not one of the release repositories"))
}

fn tag_version(tag: &str) -> Result<&str, String> {
    let v = tag
        .strip_prefix('v')
        .ok_or_else(|| format!("{tag} is not a v<x.y.z> tag"))?;
    if !crate::release::semver(v) {
        return Err(format!("{tag} is not a v<x.y.z> tag"));
    }
    Ok(v)
}

/// The compiled-in release keys (`crates/cli/src/bundle/verify.rs`).
pub fn compiled_keys() -> Vec<String> {
    pylota_identity_cli::bundle::verify::RELEASE_KEYS
        .iter()
        .map(|k| (*k).to_owned())
        .collect()
}

/// `release sign`: every listed file present and matching, nothing unlisted, every file's provenance
/// naming `release.yml` on the tag; then the signature, its own verification, the upload and the
/// second workflow. Nothing is uploaded unless every check passed.
pub fn sign(
    env: &dyn Env,
    work: &Path,
    tag: &str,
    repo: &str,
    secret_key: &str,
    keys: &[String],
) -> Result<(), String> {
    let product = product(repo)?;
    let version = tag_version(tag)?;
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let dir = work.to_string_lossy().into_owned();
    env.gh(&["release", "download", tag, "--repo", repo, "--dir", &dir])?;
    let sums_bytes = std::fs::read(work.join("SHA256SUMS"))
        .map_err(|_| "the draft has no SHA256SUMS".to_owned())?;
    if work.join("SHA256SUMS.sig").exists() {
        return Err("the draft already has SHA256SUMS.sig".into());
    }
    let sums = parse_sums(&String::from_utf8_lossy(&sums_bytes))?;
    let listed: BTreeMap<&str, &str> = sums.iter().map(|(n, h)| (n.as_str(), h.as_str())).collect();
    for e in std::fs::read_dir(work)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        if name != "SHA256SUMS" && !listed.contains_key(name.as_str()) {
            return Err(format!("{name} is on the draft but not in SHA256SUMS"));
        }
    }
    let workflow = format!("{repo}/.github/workflows/release.yml");
    let source_ref = format!("refs/tags/{tag}");
    for (name, hex) in &sums {
        let path = work.join(name);
        let bytes = std::fs::read(&path).map_err(|_| format!("{name} is listed but missing"))?;
        if sha256_hex(&bytes) != *hex {
            return Err(format!("{name} does not match its SHA256SUMS line"));
        }
        env.gh(&[
            "attestation",
            "verify",
            &path.to_string_lossy(),
            "--repo",
            repo,
            "--signer-workflow",
            &workflow,
            "--source-ref",
            &source_ref,
            "--deny-self-hosted-runners",
        ])
        .map_err(|e| format!("{name}: no provenance from release.yml on {tag}: {e}"))?;
    }
    let sk_box = minisign::SecretKeyBox::from_string(secret_key)
        .map_err(|e| format!("the secret key: {e}"))?;
    let sk = sk_box
        .into_secret_key(env.password())
        .map_err(|e| format!("the secret key: {e}"))?;
    let trusted = format!("{product} v{version}");
    let sig = minisign::sign(
        None,
        &sk,
        std::io::Cursor::new(&sums_bytes),
        Some(&trusted),
        Some(&format!("signature of SHA256SUMS of {tag}")),
    )
    .map_err(|e| format!("signing: {e}"))?
    .into_string();
    // The key must be one idvy trusts: a wrong key fails here, not at the first deploy.
    if product == "pylota-identity" {
        pylota_identity_cli::bundle::verify::signature(&sums_bytes, &sig, version, keys).map_err(
            |e| format!("the new signature does not verify with the compiled-in keys: {e:?}"),
        )?;
    } else {
        verify_any(&sums_bytes, &sig, &trusted, keys)?;
    }
    let sig_path = work.join("SHA256SUMS.sig");
    std::fs::write(&sig_path, &sig).map_err(|e| e.to_string())?;
    env.gh(&[
        "release",
        "upload",
        tag,
        &sig_path.to_string_lossy(),
        "--repo",
        repo,
    ])?;
    env.gh(&[
        "workflow",
        "run",
        "release-publish.yml",
        "--ref",
        tag,
        "--repo",
        repo,
    ])?;
    println!("signed {tag}; release-publish.yml started");
    Ok(())
}

fn verify_any(data: &[u8], sig: &str, trusted: &str, keys: &[String]) -> Result<(), String> {
    let s = minisign_verify::Signature::decode(sig).map_err(|e| e.to_string())?;
    for k in keys {
        if let Ok(pk) = minisign_verify::PublicKey::from_base64(k)
            && pk.verify(data, &s, false).is_ok()
            && s.trusted_comment() == trusted
        {
            return Ok(());
        }
    }
    Err("the new signature does not verify with the release keys".into())
}

/// `release crates`: a published, full, signed release; a clean clone of its tag at its commit and
/// version; then `cargo publish --locked` for each crate not yet on crates.io, in order.
pub fn crates(env: &dyn Env, work: &Path, tag: &str, keys: &[String]) -> Result<(), String> {
    let version = tag_version(tag)?;
    if version.contains('-') {
        return Err(format!(
            "{tag} is a pre-release: it never reaches crates.io"
        ));
    }
    let view: serde_json::Value = serde_json::from_str(&env.gh(&[
        "release",
        "view",
        tag,
        "--repo",
        REPO,
        "--json",
        "isDraft,isPrerelease,tagName",
    ])?)
    .map_err(|e| format!("gh release view: {e}"))?;
    if view["isDraft"] != false || view["isPrerelease"] != false || view["tagName"] != tag {
        return Err(format!("{tag} is not a published full release"));
    }
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let dir = work.to_string_lossy().into_owned();
    env.gh(&[
        "release",
        "download",
        tag,
        "--repo",
        REPO,
        "--pattern",
        "SHA256SUMS",
        "--pattern",
        "SHA256SUMS.sig",
        "--dir",
        &dir,
    ])?;
    let sums = std::fs::read(work.join("SHA256SUMS")).map_err(|_| "no SHA256SUMS".to_owned())?;
    let sig = std::fs::read_to_string(work.join("SHA256SUMS.sig"))
        .map_err(|_| "no SHA256SUMS.sig".to_owned())?;
    pylota_identity_cli::bundle::verify::signature(&sums, &sig, version, keys)
        .map_err(|e| format!("{tag}: {e:?}"))?;
    let commit = env
        .gh(&[
            "api",
            &format!("repos/{REPO}/commits/{tag}"),
            "--jq",
            ".sha",
        ])?
        .trim()
        .to_owned();
    let clone = work.join("src");
    env.git(
        work,
        &[
            "clone",
            "--branch",
            tag,
            "--depth",
            "1",
            &format!("https://github.com/{REPO}.git"),
            &clone.to_string_lossy(),
        ],
    )?;
    let head = env.git(&clone, &["rev-parse", "HEAD"])?.trim().to_owned();
    if head.is_empty() || head != commit {
        return Err(format!("the clone is at {head}, but {tag} names {commit}"));
    }
    let have = crate::build_worker::workspace_version(&clone)?;
    version_matches(tag, &have)?;
    for name in crate::licenses::PUBLISHED {
        if env.published(name, version)? {
            println!("{name} {version} is already on crates.io");
            continue;
        }
        env.cargo_publish(&clone, name)?;
        println!("published {name} {version}");
    }
    Ok(())
}

/// What `release-gates/v{version}.md` states.
#[derive(Debug, Default, PartialEq)]
pub struct GateFile {
    pub candidate: Option<String>,
    pub fuzz_commit: Option<String>,
    pub live_manual: Option<String>,
    pub items: usize,
}

/// Every checklist item ticked and linked to its evidence, and the named facts.
pub fn parse_gate(text: &str) -> Result<GateFile, String> {
    let mut g = GateFile::default();
    for (n, line) in text.lines().enumerate() {
        let l = line.trim_start();
        if l.starts_with("- [ ]") {
            return Err(format!("line {}: an unticked item", n + 1));
        }
        if let Some(item) = l.strip_prefix("- [x]").or_else(|| l.strip_prefix("- [X]")) {
            if !item.contains("](") {
                return Err(format!(
                    "line {}: an item without a link to its evidence",
                    n + 1
                ));
            }
            g.items += 1;
        }
        let field = |k: &str| {
            l.strip_prefix(k)
                .map(|v| v.trim().trim_matches('`').to_owned())
        };
        if let Some(v) = field("candidate:") {
            g.candidate = Some(v);
        }
        if let Some(v) = field("fuzz-24h-commit:") {
            g.fuzz_commit = Some(v);
        }
        if let Some(v) = field("live-manual:") {
            g.live_manual = Some(v);
        }
    }
    if g.items == 0 {
        return Err("no checklist item".into());
    }
    Ok(g)
}

fn jobs_passed(env: &dyn Env, workflow: &str, tag: &str, jobs: &[&str]) -> Result<(), String> {
    let runs: serde_json::Value = serde_json::from_str(&env.gh(&[
        "run",
        "list",
        "--repo",
        REPO,
        "--workflow",
        workflow,
        "--branch",
        tag,
        "--json",
        "databaseId,conclusion",
        "--limit",
        "20",
    ])?)
    .map_err(|e| e.to_string())?;
    for run in runs.as_array().into_iter().flatten() {
        let id = run["databaseId"].to_string();
        let view: serde_json::Value =
            serde_json::from_str(&env.gh(&["run", "view", &id, "--repo", REPO, "--json", "jobs"])?)
                .map_err(|e| e.to_string())?;
        let ok = |job: &str| {
            view["jobs"].as_array().into_iter().flatten().any(|j| {
                j["name"]
                    .as_str()
                    .is_some_and(|n| n == job || n.starts_with(&format!("{job} ")))
                    && j["conclusion"] == "success"
            })
        };
        if jobs.iter().all(|j| ok(j)) {
            return Ok(());
        }
    }
    Err(format!(
        "no {workflow} run on {tag} passed {}",
        jobs.join(", ")
    ))
}

fn assets_present(env: &dyn Env, tag: &str) -> Result<(), String> {
    let v: serde_json::Value = serde_json::from_str(
        &env.gh(&["release", "view", tag, "--repo", REPO, "--json", "assets"])?,
    )
    .map_err(|e| e.to_string())?;
    let names: Vec<&str> = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .collect();
    for want in ["SHA256SUMS", "SHA256SUMS.sig"] {
        if !names.contains(&want) {
            return Err(format!("{tag} has no {want}"));
        }
    }
    if !names.iter().any(|n| n.ends_with(".cdx.json")) {
        return Err(format!("{tag} has no SBOM"));
    }
    Ok(())
}

/// `release-gate`.
pub fn gate(
    env: &dyn Env,
    root: &Path,
    version: &str,
    pre_release: bool,
    run_id: Option<&str>,
) -> Result<(), String> {
    let tag = format!("v{version}");
    version_matches(&tag, &crate::build_worker::workspace_version(root)?)?;
    // This run's earlier jobs.
    if let Some(run_id) = run_id {
        let view: serde_json::Value = serde_json::from_str(
            &env.gh(&["run", "view", run_id, "--repo", REPO, "--json", "jobs"])?,
        )
        .map_err(|e| e.to_string())?;
        for job in ["verify", "staging", "live"] {
            let ok = view["jobs"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|j| j["name"] == job && j["conclusion"] == "success");
            if !ok {
                return Err(format!("this run's {job} job did not pass"));
            }
        }
    }
    assets_present(env, &tag)?;
    let crashes = env.gh(&[
        "issue",
        "list",
        "--repo",
        REPO,
        "--label",
        "fuzz-crash",
        "--state",
        "open",
        "--json",
        "number",
    ])?;
    if serde_json::from_str::<serde_json::Value>(&crashes)
        .ok()
        .and_then(|v| v.as_array().map(Vec::len))
        .unwrap_or(1)
        > 0
    {
        return Err("an open issue carries the label fuzz-crash".into());
    }
    let alerts = env.gh(&[
        "api",
        &format!("repos/{REPO}/code-scanning/alerts?state=open&per_page=100"),
        "--jq",
        "[.[] | select(.rule.security_severity_level == \"high\" or .rule.security_severity_level == \"critical\")] | length",
    ])?;
    if alerts.trim() != "0" {
        return Err(format!(
            "CodeQL has {} open alert(s) of high severity or above",
            alerts.trim()
        ));
    }
    if pre_release {
        return Ok(());
    }

    let file = root.join(format!("release-gates/v{version}.md"));
    let text =
        std::fs::read_to_string(&file).map_err(|_| format!("{} is missing", file.display()))?;
    let g = parse_gate(&text)?;
    let candidate = g
        .candidate
        .ok_or("the gate file names no candidate (candidate: v<version>-rc.N)")?;
    if !candidate.starts_with(&format!("{tag}-rc.")) {
        return Err(format!(
            "the candidate {candidate} is not a release candidate of {tag}"
        ));
    }
    if g.live_manual.is_none() {
        return Err("the gate file records no live --manual run (live-manual: <date>)".into());
    }
    let cand_commit = env.git(root, &["rev-parse", &format!("{candidate}^{{commit}}")])?;
    if g.fuzz_commit.as_deref().map(str::trim) != Some(cand_commit.trim()) {
        return Err("the 24-hour fuzz record does not cover the candidate's commit".into());
    }
    let gate_file = format!("release-gates/v{version}.md");
    for changed in env
        .git(root, &["diff", "--name-only", &candidate, &tag])?
        .lines()
        .filter(|l| !l.is_empty())
    {
        if !["Cargo.toml", "Cargo.lock", gate_file.as_str()].contains(&changed) {
            return Err(format!("{changed} differs between {candidate} and {tag}"));
        }
    }
    jobs_passed(env, "release.yml", &candidate, &["gate"])?;
    jobs_passed(env, "release-publish.yml", &candidate, &["staging", "live"])?;
    assets_present(env, &candidate)?;
    println!("release-gate: {tag} passes");
    Ok(())
}

/// `release sign <tag> [--repo <r>] --key <path>` from the command line.
pub fn sign_cli(root: &Path, args: &[String]) -> Result<(), String> {
    let tag = args
        .first()
        .ok_or("usage: cargo xtask release sign <tag> --key <path> [--repo <r>]")?;
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let repo = opt("--repo").unwrap_or_else(|| REPO.to_owned());
    let key_path: PathBuf = opt("--key")
        .ok_or("--key <path>: the secret key on the removable drive")?
        .into();
    let secret =
        std::fs::read_to_string(&key_path).map_err(|e| format!("{}: {e}", key_path.display()))?;
    // The SDK repositories' releases are signed with the same keys.
    let keys = compiled_keys();
    if keys.is_empty() {
        return Err("crates/cli/src/bundle/verify.rs has no release keys yet".into());
    }
    sign(
        &RealEnv,
        &root.join("target/release-sign"),
        tag,
        &repo,
        &secret,
        &keys,
    )
}

pub fn crates_cli(root: &Path, args: &[String]) -> Result<(), String> {
    let tag = args
        .first()
        .ok_or("usage: cargo xtask release crates <tag>")?;
    crates(
        &RealEnv,
        &root.join("target/release-crates"),
        tag,
        &compiled_keys(),
    )
}

pub fn gate_cli(root: &Path, args: &[String]) -> Result<(), String> {
    let v = args
        .iter()
        .position(|a| a == "--version")
        .and_then(|i| args.get(i + 1))
        .ok_or("usage: cargo xtask release-gate --version <v> [--pre-release]")?;
    let run_id = std::env::var("GITHUB_RUN_ID").ok();
    gate(
        &RealEnv,
        root,
        v,
        args.iter().any(|a| a == "--pre-release"),
        run_id.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// GitHub, git and crates.io as data: answers by argument prefix, the draft's assets, and a record
    /// of every write.
    #[derive(Default)]
    struct Fake {
        assets: BTreeMap<String, Vec<u8>>,
        answers: Vec<(String, Result<String, String>)>,
        git: Vec<(String, Result<String, String>)>,
        published: Vec<String>,
        writes: RefCell<Vec<String>>,
    }

    impl Fake {
        fn answer(mut self, prefix: &str, out: Result<&str, &str>) -> Self {
            self.answers.push((
                prefix.to_owned(),
                out.map(str::to_owned).map_err(str::to_owned),
            ));
            self
        }
        fn git(mut self, prefix: &str, out: Result<&str, &str>) -> Self {
            self.git.push((
                prefix.to_owned(),
                out.map(str::to_owned).map_err(str::to_owned),
            ));
            self
        }
    }

    impl Env for Fake {
        fn gh(&self, args: &[&str]) -> Result<String, String> {
            let line = args.join(" ");
            if args[..2] == ["release", "download"] {
                let dir = args[args.iter().position(|a| *a == "--dir").unwrap() + 1];
                for (n, b) in &self.assets {
                    std::fs::write(Path::new(dir).join(n), b).unwrap();
                }
                return Ok(String::new());
            }
            if args[..2] == ["release", "upload"] || args[..2] == ["workflow", "run"] {
                self.writes.borrow_mut().push(line);
                return Ok(String::new());
            }
            self.answers
                .iter()
                .find(|(p, _)| line.starts_with(p.as_str()) || line.contains(p.as_str()))
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| Err(format!("unexpected gh {line}")))
        }
        fn git(&self, _dir: &Path, args: &[&str]) -> Result<String, String> {
            let line = args.join(" ");
            if args[0] == "clone" {
                let to = Path::new(args.last().unwrap());
                std::fs::create_dir_all(to).unwrap();
                std::fs::write(
                    to.join("Cargo.toml"),
                    "[workspace.package]\nversion = \"1.0.0\"\n",
                )
                .unwrap();
            }
            self.git
                .iter()
                .find(|(p, _)| line.starts_with(p.as_str()))
                .map(|(_, r)| r.clone())
                .unwrap_or(Ok(String::new()))
        }
        fn published(&self, name: &str, _v: &str) -> Result<bool, String> {
            Ok(self.published.iter().any(|p| p == name))
        }
        fn cargo_publish(&self, _dir: &Path, name: &str) -> Result<(), String> {
            self.writes.borrow_mut().push(format!("publish {name}"));
            Ok(())
        }
        fn password(&self) -> Option<String> {
            Some("pw".into())
        }
    }

    fn keypair() -> (String, String) {
        let kp = minisign::KeyPair::generate_encrypted_keypair(Some("pw".into())).unwrap();
        (kp.sk.to_box(None).unwrap().into_string(), kp.pk.to_base64())
    }

    fn draft() -> BTreeMap<String, Vec<u8>> {
        let mut a = BTreeMap::new();
        a.insert(
            "pylota-identity-worker-1.0.0-rc.1.tar.gz".to_owned(),
            b"bundle".to_vec(),
        );
        a.insert(
            "idvy-1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz".to_owned(),
            b"cli".to_vec(),
        );
        let sums: String = a
            .iter()
            .map(|(n, b)| format!("{}  {n}\n", sha256_hex(b)))
            .collect();
        a.insert("SHA256SUMS".into(), sums.into_bytes());
        a
    }

    fn work(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../target/release-ops-test/{name}"))
    }

    const TAG: &str = "v1.0.0-rc.1";

    /// `xtask::release_sign_refuses` (Rust workspace § 9): a file whose checksum does not match, a file
    /// not listed, a missing or foreign attestation, and a key idvy does not trust are each refused with
    /// nothing uploaded; a good draft is signed with the trusted comment, uploaded and handed to
    /// `release-publish.yml`.
    #[test]
    fn release_sign_refuses() {
        let (sk, pk) = keypair();
        let keys = vec![pk.clone()];
        let attested = Ok("verified");

        let mut bad = draft();
        bad.insert(
            "idvy-1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz".into(),
            b"rebuilt".to_vec(),
        );
        let mut extra = draft();
        extra.insert("unlisted.txt".into(), b"x".to_vec());
        let cases: Vec<(&str, Fake, Vec<String>)> = vec![
            (
                "checksum",
                Fake {
                    assets: bad,
                    ..Fake::default()
                }
                .answer("attestation", attested),
                keys.clone(),
            ),
            (
                "unlisted file",
                Fake {
                    assets: extra,
                    ..Fake::default()
                }
                .answer("attestation", attested),
                keys.clone(),
            ),
            (
                "no attestation",
                Fake {
                    assets: draft(),
                    ..Fake::default()
                }
                .answer("attestation", Err("no matching attestations")),
                keys.clone(),
            ),
            (
                "another workflow or tag",
                Fake {
                    assets: draft(),
                    ..Fake::default()
                }
                .answer("attestation", Err("signer workflow does not match")),
                keys.clone(),
            ),
            (
                "an untrusted key",
                Fake {
                    assets: draft(),
                    ..Fake::default()
                }
                .answer("attestation", attested),
                vec![keypair().1],
            ),
        ];
        for (case, fake, keys) in cases {
            let r = sign(&fake, &work(case), TAG, REPO, &sk, &keys);
            assert!(r.is_err(), "{case}: signed");
            assert!(
                fake.writes.borrow().is_empty(),
                "{case}: uploaded {:?}",
                fake.writes.borrow()
            );
        }

        let fake = Fake {
            assets: draft(),
            ..Fake::default()
        }
        .answer("attestation", attested);
        sign(&fake, &work("good"), TAG, REPO, &sk, &keys).unwrap();
        let writes = fake.writes.borrow();
        assert!(
            writes[0].starts_with(&format!("release upload {TAG} ")),
            "{writes:?}"
        );
        assert!(writes[1].starts_with(&format!("workflow run release-publish.yml --ref {TAG}")));
        let sig = std::fs::read_to_string(work("good").join("SHA256SUMS.sig")).unwrap();
        let sums = &draft()["SHA256SUMS"];
        pylota_identity_cli::bundle::verify::signature(sums, &sig, "1.0.0-rc.1", &keys).unwrap();
        assert!(
            sign(&fake, &work("repo"), TAG, "someone/else", &sk, &keys).is_err(),
            "another repository"
        );
    }

    fn signed_release(pk_sk: &(String, String)) -> BTreeMap<String, Vec<u8>> {
        let sums =
            b"0000000000000000000000000000000000000000000000000000000000000000  x\n".to_vec();
        let sk = minisign::SecretKeyBox::from_string(&pk_sk.0)
            .unwrap()
            .into_secret_key(Some("pw".into()))
            .unwrap();
        let sig = minisign::sign(
            None,
            &sk,
            std::io::Cursor::new(&sums),
            Some("pylota-identity v1.0.0"),
            None,
        )
        .unwrap()
        .into_string();
        let mut a = BTreeMap::new();
        a.insert("SHA256SUMS".into(), sums);
        a.insert("SHA256SUMS.sig".into(), sig.into_bytes());
        a
    }

    /// `xtask::release_crates_refuses` (J35): a pre-release, a draft, a release whose signature does not
    /// verify, a clone at another commit or version each publish nothing; a full signed release
    /// publishes the three crates in order, skipping one already on crates.io.
    #[test]
    fn release_crates_refuses() {
        let kp = keypair();
        let keys = vec![kp.1.clone()];
        let full = r#"{"isDraft":false,"isPrerelease":false,"tagName":"v1.0.0"}"#;
        let base = |view: &str| {
            Fake {
                assets: signed_release(&kp),
                ..Fake::default()
            }
            .answer("release view", Ok(view))
            .answer("api repos/", Ok("abc123\n"))
            .git("rev-parse HEAD", Ok("abc123\n"))
        };
        assert!(
            crates(&base(full), &work("c-rc"), "v1.0.0-rc.1", &keys).is_err(),
            "a pre-release tag"
        );
        for (case, fake, keys) in [
            (
                "draft",
                base(r#"{"isDraft":true,"isPrerelease":false,"tagName":"v1.0.0"}"#),
                keys.clone(),
            ),
            (
                "pre-release",
                base(r#"{"isDraft":false,"isPrerelease":true,"tagName":"v1.0.0"}"#),
                keys.clone(),
            ),
            ("signature", base(full), vec![keypair().1]),
            (
                "commit",
                base(full).git("rev-parse HEAD", Ok("def456\n")),
                keys.clone(),
            ),
        ] {
            // A later git answer for the same prefix comes after the first, so rebuild the commit case.
            let fake = if case == "commit" {
                Fake {
                    assets: signed_release(&kp),
                    ..Fake::default()
                }
                .answer("release view", Ok(full))
                .answer("api repos/", Ok("abc123\n"))
                .git("rev-parse HEAD", Ok("def456\n"))
            } else {
                fake
            };
            assert!(
                crates(&fake, &work(&format!("c-{case}")), "v1.0.0", &keys).is_err(),
                "{case}"
            );
            assert!(fake.writes.borrow().is_empty(), "{case}: published");
        }
        let mut fake = base(full);
        fake.published = vec!["pylota-identity-api-types".into()];
        crates(&fake, &work("c-good"), "v1.0.0", &keys).unwrap();
        assert_eq!(
            *fake.writes.borrow(),
            ["publish pylota-identity", "publish pylota-identity-cli"]
        );
    }

    fn gate_root(text: Option<&str>) -> PathBuf {
        let root = work("gate-root");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("release-gates")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        if let Some(t) = text {
            std::fs::write(root.join("release-gates/v1.0.0.md"), t).unwrap();
        }
        root
    }

    const GOOD_GATE: &str = "# v1.0.0\n\ncandidate: v1.0.0-rc.2\nfuzz-24h-commit: c0ffee\nlive-manual: 2027-01-10\n\n- [x] Pen test and retest ([report](https://example.invalid/pentest))\n- [x] Dependency review ([log](https://example.invalid/deps))\n";

    fn gate_env() -> Fake {
        let jobs = r#"{"jobs":[{"name":"gate","conclusion":"success"},{"name":"staging","conclusion":"success"},{"name":"live","conclusion":"success"}]}"#;
        Fake::default()
            .answer("release view", Ok(r#"{"assets":[{"name":"SHA256SUMS"},{"name":"SHA256SUMS.sig"},{"name":"pylota-identity-worker.cdx.json"}]}"#))
            .answer("issue list", Ok("[]"))
            .answer("code-scanning", Ok("0\n"))
            .answer("run list", Ok(r#"[{"databaseId":7,"conclusion":"success"}]"#))
            .answer("run view", Ok(jobs))
            .git("rev-parse", Ok("c0ffee\n"))
            .git("diff", Ok("Cargo.toml\nCargo.lock\nrelease-gates/v1.0.0.md\n"))
    }

    /// `xtask::release_gate_refuses` (Security § 16): an unticked item, an item without evidence, no
    /// candidate, a fuzz record of another commit, no manual live run, a tree that differs from the
    /// candidate's beyond the version and the gate file, an open `fuzz-crash` issue, a high CodeQL
    /// alert and a missing signature are each refused; the complete gate passes, and `--pre-release`
    /// checks only the artefacts, the crash label and CodeQL.
    #[test]
    fn release_gate_refuses() {
        assert!(
            gate(
                &gate_env(),
                &gate_root(Some(GOOD_GATE)),
                "1.0.0",
                false,
                None
            )
            .is_ok()
        );
        let edits: &[(&str, &str, &str)] = &[
            (
                "unticked",
                "- [x] Dependency review",
                "- [ ] Dependency review",
            ),
            (
                "no evidence",
                "([log](https://example.invalid/deps))",
                "(see the log)",
            ),
            ("no candidate", "candidate: v1.0.0-rc.2\n", ""),
            (
                "not a candidate",
                "candidate: v1.0.0-rc.2",
                "candidate: v0.9.0",
            ),
            (
                "fuzz commit",
                "fuzz-24h-commit: c0ffee",
                "fuzz-24h-commit: 123abc",
            ),
            ("no manual run", "live-manual: 2027-01-10\n", ""),
        ];
        for (case, from, to) in edits {
            let text = GOOD_GATE.replace(from, to);
            assert!(
                gate(&gate_env(), &gate_root(Some(&text)), "1.0.0", false, None).is_err(),
                "{case}"
            );
        }
        assert!(
            gate(&gate_env(), &gate_root(None), "1.0.0", false, None).is_err(),
            "no gate file"
        );
        let swaps: Vec<(&str, Fake)> = vec![
            ("tree", {
                let mut e = gate_env();
                e.git
                    .insert(0, ("diff".into(), Ok("crates/worker/src/lib.rs\n".into())));
                e
            }),
            ("fuzz-crash", {
                let mut e = gate_env();
                e.answers
                    .insert(0, ("issue list".into(), Ok("[{\"number\":4}]".into())));
                e
            }),
            ("codeql", {
                let mut e = gate_env();
                e.answers
                    .insert(0, ("code-scanning".into(), Ok("2\n".into())));
                e
            }),
            ("signature", {
                let mut e = gate_env();
                e.answers.insert(
                    0,
                    (
                        "release view".into(),
                        Ok(r#"{"assets":[{"name":"SHA256SUMS"},{"name":"x.cdx.json"}]}"#.into()),
                    ),
                );
                e
            }),
            ("candidate jobs", {
                let mut e = gate_env();
                e.answers.insert(
                    0,
                    (
                        "run view".into(),
                        Ok(r#"{"jobs":[{"name":"gate","conclusion":"failure"}]}"#.into()),
                    ),
                );
                e
            }),
        ];
        for (case, e) in swaps {
            assert!(
                gate(&e, &gate_root(Some(GOOD_GATE)), "1.0.0", false, None).is_err(),
                "{case}"
            );
        }
        // A pre-release gate needs no file.
        let root = gate_root(None);
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.0.0-rc.1\"\n",
        )
        .unwrap();
        assert!(gate(&gate_env(), &root, "1.0.0-rc.1", true, None).is_ok());
        let mut crash = gate_env();
        crash
            .answers
            .insert(0, ("issue list".into(), Ok("[{\"number\":4}]".into())));
        assert!(gate(&crash, &root, "1.0.0-rc.1", true, None).is_err());
    }
}
