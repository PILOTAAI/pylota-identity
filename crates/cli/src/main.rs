//! `idvy`, the Pylota Identity command-line tool ([CLI and setup]). The library does the work; this
//! binary gives it the process's environment and a runner for child processes.
//!
//! [CLI and setup]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md
#![forbid(unsafe_code)]

use std::io::{Read as _, Write as _};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pylota_identity_cli::{Cmd, Env, Proc, Runner};

/// Runs child processes for real: stdin written then closed, stdout and stderr captured, killed at the
/// timeout.
struct System;

impl Runner for System {
    fn run(&self, cmd: &Cmd) -> Result<Proc, String> {
        let mut c = Command::new(&cmd.program);
        c.args(&cmd.args)
            .stdin(if cmd.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &cmd.env {
            c.env(k, v);
        }
        if let Some(d) = &cmd.cwd {
            c.current_dir(d);
        }
        let mut child = c.spawn().map_err(|e| format!("{}: {e}", cmd.program))?;
        if let (Some(bytes), Some(mut stdin)) = (cmd.stdin.clone(), child.stdin.take()) {
            std::thread::spawn(move || {
                let _ = stdin.write_all(&bytes);
            });
        }
        let mut out = child.stdout.take().ok_or("no stdout")?;
        let mut err = child.stderr.take().ok_or("no stderr")?;
        let o = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = out.read_to_end(&mut b);
            b
        });
        let e = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = err.read_to_end(&mut b);
            b
        });
        let deadline = cmd
            .timeout_s
            .map(|s| Instant::now() + Duration::from_secs(s));
        let status = loop {
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break s;
            }
            if deadline.is_some_and(|d| Instant::now() > d) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} timed out", cmd.program));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        Ok(Proc {
            code: status.code().unwrap_or(1),
            stdout: o.join().unwrap_or_default(),
            stderr: e.join().unwrap_or_default(),
        })
    }
}

fn main() {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: cli_internal: {e}");
            std::process::exit(1);
        }
    };
    let code = rt.block_on(async {
        let args: Vec<String> = std::env::args().collect();
        tokio::select! {
            code = pylota_identity_cli::run(args, Env::system(Arc::new(System))) => code,
            _ = tokio::signal::ctrl_c() => {
                eprintln!("error: cli_interrupted: Interrupted.");
                130
            }
        }
    });
    std::process::exit(code);
}
