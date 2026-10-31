//! `cargo xtask <command>` (Rust workspace § 9). M0 has `build-worker`, `check-layering`
//! and `trace`; M2 adds `gen-unicode` and `fuzz`,
//! M3 `openapi`, M18 the evaluations (`eval-setup`, `eval-search`, `eval-agentic`, `eval-triage`).
#![forbid(unsafe_code)]

mod build_worker;
mod dco;
mod docs_check;
mod eval;
mod fuzz;
mod gen_sdk;
mod itest;
mod layering;
mod licenses;
mod openapi;
mod release;
mod release_ops;
mod size;
mod trace;
mod unicode;
mod workflows;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <build-worker | check-layering | trace | gen-unicode [--check] | fuzz --target <t> --time <s> | openapi | itest | docs-check | dco <base> <head> | gen-licenses [--check] | release --check-version <tag> | release --version <v> | release --sums | release --notes <v> | release sign <tag> --key <path> | release crates <tag> | release-gate --version <v> | eval-setup | eval-search | eval-agentic | eval-triage>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let result = match args.first().map(String::as_str) {
        Some("build-worker") => build_worker::run(&root),
        Some("check-layering") => layering::run(&root),
        Some("trace") => trace::run(&root),
        Some("openapi") => openapi::run(&root),
        Some("fuzz") => fuzz::run(&root, &args[1..]),
        Some("itest") => itest::run(&root, &args[1..]),
        Some("eval-setup") => eval::setup(&root),
        Some("docs-check") => docs_check::run(&root),
        Some("dco") => dco::run(&root, &args[1..]),
        Some("release-gate") => release_ops::gate_cli(&root, &args[1..]),
        Some("gen-licenses") => licenses::run(&root, args.get(1).is_some_and(|a| a == "--check")),
        Some("release") => match (args.get(1).map(String::as_str), args.get(2)) {
            (Some("--check-version"), Some(tag)) => release::check_version(&root, tag),
            (Some("--version"), Some(v)) => release::package(&root, v),
            (Some("--sums"), _) => release::write_sums(&root.join("dist")).map(|t| print!("{t}")),
            (Some("--notes"), Some(v)) => release::notes(&root, v).map(|n| println!("{n}")),
            (Some("sign"), _) => release_ops::sign_cli(&root, &args[2..]),
            (Some("verify"), _) => release_ops::verify_cli(&args[2..]),
            (Some("crates"), _) => release_ops::crates_cli(&root, &args[2..]),
            _ => Err("usage: cargo xtask release --check-version <tag> | --version <v>".into()),
        },
        Some(s @ ("eval-search" | "eval-agentic" | "eval-triage")) => eval::run(&root, &s[5..]),
        Some("gen-sdk") => gen_sdk::run(&root, args.get(1).is_some_and(|a| a == "--check")),
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
