//! `cargo xtask openapi` (Rust workspace § 9): writes `target/openapi.json`, generated from
//! `crates/api-types`, and compares it semantically with `docs/src/reference/openapi.yaml`; any
//! difference fails (the CI `openapi` job).

use std::path::Path;

use pylota_identity_api_types::openapi::{contract, openapi_json};

pub const CONTRACT: &str = "docs/src/reference/openapi.yaml";

/// The differences between the contract at `root` and the generated document.
pub fn differences(root: &Path) -> Result<Vec<String>, String> {
    let text =
        std::fs::read_to_string(root.join(CONTRACT)).map_err(|e| format!("{CONTRACT}: {e}"))?;
    let doc: serde_json::Value =
        yaml_serde::from_str(&text).map_err(|e| format!("{CONTRACT}: {e}"))?;
    Ok(contract::compare(&doc, &openapi_json()))
}

pub fn run(root: &Path) -> Result<(), String> {
    let out = root.join("target/openapi.json");
    let json = serde_json::to_string_pretty(&openapi_json()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("target")).map_err(|e| e.to_string())?;
    std::fs::write(&out, json + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
    let diffs = differences(root)?;
    if diffs.is_empty() {
        println!("openapi: target/openapi.json matches {CONTRACT}");
        Ok(())
    } else {
        Err(format!(
            "{} differences from {CONTRACT}:\n  {}",
            diffs.len(),
            diffs.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo xtask openapi`: the generated `openapi.json` equals the contract semantically (FR-API-1).
    /// Covers: FR-API-1
    #[test]
    fn openapi_matches_contract() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let diffs = differences(root).expect("the contract reads");
        assert!(diffs.is_empty(), "{}", diffs.join("\n"));
    }
}
