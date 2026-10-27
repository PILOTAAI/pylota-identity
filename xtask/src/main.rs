//! `cargo xtask <command>` (Rust workspace § 9). M0 has `build-worker`, `check-layering`
//! and `trace`; M2 adds `gen-unicode` and `fuzz`.
#![forbid(unsafe_code)]

mod build_worker;
mod fuzz;
mod layering;
mod size;
mod trace;
mod unicode;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <build-worker | check-layering | trace | gen-unicode [--check] | fuzz --target <t> --time <s>>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let result = match args.first().map(String::as_str) {
        Some("build-worker") => build_worker::run(&root),
        Some("check-layering") => layering::run(&root),
        Some("trace") => trace::run(&root),
        Some("fuzz") => fuzz::run(&root, &args[1..]),
        Some("gen-unicode") => unicode::run(&root, args.get(1).is_some_and(|a| a == "--check")),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The repository root: the parent of this crate's directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .to_path_buf()
}

/// The `cargo` that runs us (the pinned toolchain's), or the one on `PATH`.
fn cargo() -> std::process::Command {
    std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}
