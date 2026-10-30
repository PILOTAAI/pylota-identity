//! `cargo xtask eval-setup`, `eval-search`, `eval-agentic` and `eval-triage` (build plan M18, Testing
//! § 9.2). They need the evaluation account: `PM_EVAL_CF_ACCOUNT_ID` and `PM_EVAL_CF_API_TOKEN` (a
//! Cloudflare account of its own, never the production account; the token holds Workers AI Read and
//! Edit and Vectorize Edit there and nothing else).
//!
//! `eval-setup` deletes `pm-mail-chunks-eval` if it exists, creates it again with the eight metadata
//! indexes of Data model § 5, and waits until its `info` answers. Each `eval-<suite>` then runs the
//! harness of `cargo xtask itest` with `PM_EVAL_REST = "on"` against that index and the `#[ignore]`d
//! `it::eval::<suite>` test, which scores the suite with the repeat policy and fails on its gate; the
//! report is `target/eval/<suite>.json`.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

pub const INDEX: &str = "pm-mail-chunks-eval";

/// Data model § 5.
const METADATA: &[(&str, &str)] = &[
    ("identity_id", "string"),
    ("thread_id", "string"),
    ("sent_at", "number"),
    ("sender_domain", "string"),
    ("direction", "string"),
    ("has_attachment", "boolean"),
    ("verdict", "string"),
    ("kind", "string"),
];

struct Account {
    id: String,
    token: String,
}

fn account() -> Result<Account, String> {
    let get = |k: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                format!("{k} is not set: the evaluations run in their own Cloudflare account")
            })
    };
    Ok(Account {
        id: get("PM_EVAL_CF_ACCOUNT_ID")?,
        token: get("PM_EVAL_CF_API_TOKEN")?,
    })
}

/// One Cloudflare API call through curl; the token goes in a header file, never on the command line.
fn api(
    dir: &Path,
    a: &Account,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<(u16, serde_json::Value), String> {
    let header = dir.join("eval-auth-header");
    std::fs::write(&header, format!("Authorization: Bearer {}\n", a.token))
        .map_err(|e| e.to_string())?;
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&header, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-X", method, "-w", "\n%{http_code}", "-H"])
        .arg(format!("@{}", header.display()))
        .arg(format!(
            "https://api.cloudflare.com/client/v4/accounts/{}{path}",
            a.id
        ));
    if let Some(b) = body {
        cmd.args([
            "-H",
            "content-type: application/json",
            "--data",
            &b.to_string(),
        ]);
    }
    let out = cmd.output().map_err(|e| format!("curl: {e}"));
    let _ = std::fs::remove_file(&header);
    let out = out?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = text.rsplit_once('\n').unwrap_or((&text, "0"));
    Ok((
        code.trim().parse().unwrap_or(0),
        serde_json::from_str(body).unwrap_or(serde_json::Value::Null),
    ))
}

pub fn setup(root: &Path) -> Result<(), String> {
    let a = account()?;
    let dir = root.join("target/eval");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = format!("/vectorize/v2/indexes/{INDEX}");
    let (code, body) = api(&dir, &a, "DELETE", &base, None)?;
    if code >= 300 && code != 404 && code != 410 {
        return Err(format!("deleting {INDEX}: {code} {body}"));
    }
    let create = serde_json::json!({
        "name": INDEX,
        "description": "Pylota Identity evaluations (cargo xtask eval-setup)",
        "config": { "dimensions": 1024, "metric": "cosine" },
    });
    // A deleted index's name can take a moment to free.
    let start = Instant::now();
    loop {
        let (code, body) = api(&dir, &a, "POST", "/vectorize/v2/indexes", Some(&create))?;
        if code < 300 {
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            return Err(format!("creating {INDEX}: {code} {body}"));
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    for (name, kind) in METADATA {
        let (code, body) = api(
            &dir,
            &a,
            "POST",
            &format!("{base}/metadata_index/create"),
            Some(&serde_json::json!({ "propertyName": name, "indexType": kind })),
        )?;
        if code >= 300 {
            return Err(format!("metadata index {name}: {code} {body}"));
        }
    }
    let start = Instant::now();
    loop {
        let (code, body) = api(&dir, &a, "GET", &format!("{base}/info"), None)?;
        if code == 200 && body["result"]["dimensions"].as_u64() == Some(1024) {
            println!("{INDEX} is ready");
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(300) {
            return Err(format!("{INDEX} did not answer info: {code} {body}"));
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// `eval-<suite>`: the harness with the evaluation account, running `it::eval::<suite>`.
pub fn run(root: &Path, suite: &str) -> Result<(), String> {
    let a = account()?;
    let extra = crate::itest::Extra {
        vars: vec![
            ("PM_EVAL_REST".into(), "on".into()),
            ("PM_VECTORS_INDEX".into(), INDEX.into()),
        ],
        account_id: Some(a.id),
        cf_api_token: Some(a.token),
        ignored: true,
    };
    let result = crate::itest::run_with(
        root,
        &[
            format!("eval::{suite}"),
            "--exact".into(),
            "--nocapture".into(),
        ],
        &extra,
    );
    if let Ok(text) = std::fs::read_to_string(root.join(format!("target/eval/{suite}.json")))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
    {
        println!(
            "{suite}: median {} over runs {} (spread {})",
            v["median"], v["scores"], v["spread"]
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use pylota_identity_conformance::golden::policy::{Next, gate, next};

    /// `xtask::eval_median_policy` (Testing § 9.3): from recorded per-run results, an evaluation scores
    /// the median of three runs, adds three runs when the spread exceeds 0.02, fails as `unstable`
    /// when the six still spread more than 0.02, and fails a gate only when the median drops more than
    /// 0.01 below the baseline.
    #[test]
    fn eval_median_policy() {
        assert_eq!(next(&[]), Next::More(3));
        assert_eq!(next(&[0.91]), Next::More(2));
        assert_eq!(next(&[0.91, 0.93, 0.92]), Next::Scored(0.92));
        assert_eq!(
            next(&[0.90, 0.92, 0.91]),
            Next::Scored(0.91),
            "a spread of exactly 0.02"
        );
        assert_eq!(next(&[0.90, 0.95, 0.92]), Next::More(3));
        assert!(
            matches!(next(&[0.90, 0.95, 0.92, 0.91, 0.92, 0.93]), Next::Unstable { spread } if (spread - 0.05).abs() < 1e-9),
            "six runs still spread 0.05"
        );
        assert_eq!(
            next(&[0.91, 0.92, 0.92, 0.92, 0.93, 0.92]),
            Next::Scored(0.92)
        );
        assert_eq!(
            next(&[0.91, 0.93, 0.92, 0.92, 0.92, 0.92]),
            Next::Scored(0.92)
        );
        // Gates: the floor, and a drop of more than 0.01.
        assert!(gate(0.92, Some(0.90), Some(0.925)).is_ok());
        assert!(
            gate(0.92, Some(0.90), Some(0.93)).is_ok(),
            "exactly 0.01 below"
        );
        assert!(gate(0.915, Some(0.90), Some(0.93)).is_err());
        assert!(gate(0.89, Some(0.90), None).is_err());
        assert!(gate(0.95, Some(0.90), None).is_ok(), "no baseline yet");
    }
}
