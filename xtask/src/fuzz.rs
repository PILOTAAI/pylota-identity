//! `cargo xtask fuzz --target <t> --time <s>`: runs one cargo-fuzz target in `fuzz/` (Rust workspace § 9),
//! seeded from the conformance corpus, on the nightly pinned in `fuzz/rust-toolchain.toml`.

use std::path::Path;

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut target = None;
    let mut time = 60u32;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--target" => target = it.next().cloned(),
            "--time" => {
                time = it
                    .next()
                    .and_then(|t| t.parse().ok())
                    .ok_or("--time takes seconds")?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let target = target.ok_or("usage: cargo xtask fuzz --target <t> --time <s>")?;
    let fuzz = root.join("fuzz");
    let seeds = fuzz.join("corpus").join(&target);
    std::fs::create_dir_all(&seeds).map_err(|e| format!("{}: {e}", seeds.display()))?;
    let cases = root.join("crates/conformance/corpus");
    for dir in std::fs::read_dir(&cases)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        for f in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if f.path().extension().is_some_and(|x| x == "eml") {
                let _ = std::fs::copy(f.path(), seeds.join(f.file_name()));
            }
        }
    }
    // rustup picks fuzz/rust-toolchain.toml from the working directory.
    let status = std::process::Command::new("cargo")
        .current_dir(&fuzz)
        .env_remove("RUSTUP_TOOLCHAIN")
        .args(["fuzz", "run", "-O", &target])
        .arg(&seeds)
        .args([
            "--",
            &format!("-max_total_time={time}"),
            "-timeout=1",
            "-rss_limit_mb=2048",
        ])
        .status()
        .map_err(|e| format!("cargo fuzz: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "fuzz target {target} failed (artifacts in fuzz/artifacts/{target})"
        ))
    }
}
