//! Checks over `.github/workflows/` (Rust workspace § 10, J34, J35), run by `cargo test -p xtask`.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
    }

    fn workflow(name: &str) -> (String, serde_json::Value) {
        let text = std::fs::read_to_string(root().join(".github/workflows").join(name)).unwrap();
        let doc: serde_json::Value = yaml_serde::from_str(&text).unwrap();
        (text, doc)
    }

    fn steps(doc: &serde_json::Value, job: &str) -> Vec<String> {
        doc["jobs"][job]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| format!("{} {}", s["name"].as_str().unwrap_or(""), s["run"].as_str().unwrap_or("")))
            .collect()
    }

    /// `xtask::cloud_deploy_workflow_pins` (J34, J35): `cloud-deploy.yml` verifies the downloaded idvy
    /// with minisign at a pinned version and archive SHA-256, against keys written in the workflow that
    /// equal the keys idvy compiles in, with the version's trusted comment; checks its line of
    /// `SHA256SUMS` and its attestation from `release.yml` on the tag; refuses a pre-release; and runs
    /// idvy only after all of that. Every action of every workflow is pinned to a full commit SHA, no
    /// workflow holds a crates.io token or publishes a crate, and Wrangler is installed only with
    /// `npm ci --ignore-scripts`.
    #[test]
    fn cloud_deploy_workflow_pins() {
        let (text, doc) = workflow("cloud-deploy.yml");
        let env = &doc["env"];
        let keys: Vec<String> = env["RELEASE_KEYS"]
            .as_str()
            .unwrap_or("")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        let compiled: Vec<String> = pylota_identity_cli::bundle::verify::RELEASE_KEYS
            .iter()
            .map(|k| (*k).to_owned())
            .collect();
        assert_eq!(keys, compiled, "RELEASE_KEYS must equal crates/cli/src/bundle/verify.rs");
        assert!(env["MINISIGN_VERSION"].as_str().is_some_and(|v| !v.is_empty()));
        let sha = env["MINISIGN_SHA256"].as_str().unwrap_or("");
        assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()), "{sha}");
        assert!(doc["on"].get("workflow_dispatch").is_some() && doc["on"].as_object().unwrap().len() == 1);
        assert_eq!(doc["jobs"]["deploy"]["environment"], "production");

        let s = steps(&doc, "deploy");
        let at = |needle: &str| {
            s.iter()
                .position(|x| x.contains(needle))
                .unwrap_or_else(|| panic!("no step with {needle}"))
        };
        let refuse = at("Refuse a pre-release");
        let minisign = at("sha256sum --check --strict");
        let sig = at("minisign -V -m SHA256SUMS -x SHA256SUMS.sig -P \"$key\"");
        assert!(s[sig].contains("Trusted comment: pylota-identity v$VERSION"));
        let sums = at("2. The archive's checksum");
        let attest = at("gh attestation verify");
        for flag in [
            "--signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/release.yml\"",
            "--source-ref \"refs/tags/v$VERSION\"",
            "--deny-self-hosted-runners",
        ] {
            assert!(s[attest].contains(flag), "the attestation check lacks {flag}");
        }
        let run = at("./idvy deploy --version \"$VERSION\" --gradual");
        assert!(refuse < run && minisign < sig && sig < run && sums < run && attest < run);
        assert_eq!(s.iter().filter(|x| x.contains("idvy ")).count(), 1, "idvy runs once, last");
        assert!(!text.contains("--from-source"));

        // Every workflow.
        for e in std::fs::read_dir(root().join(".github/workflows")).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(e.path()).unwrap();
            for line in text.lines() {
                let l = line.trim_start().trim_start_matches("- ");
                if let Some(u) = l.strip_prefix("uses: ") {
                    let u = u.split_whitespace().next().unwrap_or("");
                    let pinned = u.starts_with("./")
                        || u.split_once('@').is_some_and(|(_, r)| r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()));
                    assert!(pinned, "{name}: {u} is not pinned to a commit SHA");
                }
            }
            for forbidden in ["CARGO_REGISTRY_TOKEN", "cargo publish", "NPM_TOKEN", "npm publish"] {
                assert!(!text.contains(forbidden), "{name} mentions {forbidden}");
            }
            for l in text.lines().filter(|l| l.contains("npm ") && !l.trim_start().starts_with('#')) {
                if l.contains("npm ci") {
                    assert!(l.contains("--ignore-scripts"), "{name}: {l}");
                } else {
                    assert!(!l.contains("npm install") && !l.contains("npx "), "{name}: {l}");
                }
            }
        }
    }
}
