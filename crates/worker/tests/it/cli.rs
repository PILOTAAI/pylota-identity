//! The landing page's CLI examples against the workerd harness (build plan M16): the commands are read
//! from `site/public/index.html` and run through `idvy`'s library with a profile for the harness. The
//! example addresses (`bookings@acme.example.com`) name identities this test creates; the setup and
//! deploy example runs against the recorded Cloudflare fake (`cli::landing_examples`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::{fakes, platform};

/// The text of the landing page's `<code>` block whose opening tag starts with `open`, tags removed.
fn block(open: &str, containing: &str) -> String {
    let page = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../site/public/index.html"
    ))
    .unwrap();
    let start = page
        .match_indices(open)
        .map(|(i, _)| i + open.len())
        .find(|i| {
            page[*i..]
                .split("</code>")
                .next()
                .is_some_and(|b| b.contains(containing))
        })
        .unwrap_or_else(|| panic!("no block {open} with {containing}"));
    let raw = &page[start..start + page[start..].find("</code>").unwrap()];
    let mut text = String::new();
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// The `idvy` command lines of a block: prompts and continuations joined, output and comments left out.
fn commands(text: &str) -> Vec<Vec<String>> {
    let joined = text.replace("\\\n", " ");
    joined
        .lines()
        .map(|l| l.trim().trim_start_matches("$ ").to_owned())
        .filter(|l| l.starts_with("idvy "))
        .map(|l| words(&l))
        .collect()
}

/// Shell words: spaces split, double quotes group.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            ' ' if !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct NoRunner;

impl pylota_identity_cli::Runner for NoRunner {
    fn run(&self, cmd: &pylota_identity_cli::Cmd) -> Result<pylota_identity_cli::Proc, String> {
        Err(format!("{} is not run here", cmd.program))
    }
}

/// Runs `args` (starting with `idvy`) with the harness profile in `home`.
fn idvy(home: &std::path::Path, args: &[String]) -> (i32, String, String) {
    let pem = std::fs::read(std::env::var("PM_ITEST_CA").unwrap()).unwrap();
    let url = std::env::var("PM_ITEST_URL").unwrap();
    let port: u16 = url.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (out, err) = (Buf::default(), Buf::default());
    let http = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(&pem).unwrap())
        .resolve(
            "localhost",
            std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        )
        .no_proxy()
        .build()
        .unwrap();
    let env = pylota_identity_cli::Env {
        vars: BTreeMap::from([("HOME".to_owned(), home.display().to_string())]),
        cwd: home.to_owned(),
        stdin: Box::new(std::io::empty()),
        stdin_tty: false,
        stdout_tty: false,
        stderr_tty: false,
        out: Box::new(out.clone()),
        err: Box::new(err.clone()),
        http: Some(http),
        cloudflare_api: String::new(),
        github_api: String::new(),
        runner: Arc::new(NoRunner),
        now_ms: None,
        release_keys: None,
        hosts: BTreeMap::new(),
    };
    let code = rt.block_on(pylota_identity_cli::run(args.to_vec(), env));
    let s = |b: &Buf| String::from_utf8_lossy(&b.0.lock().unwrap()).into_owned();
    (code, s(&out), s(&err))
}

fn substitute(cmd: &[String], map: &[(&str, &str)]) -> Vec<String> {
    cmd.iter()
        .map(|w| {
            map.iter()
                .fold(w.clone(), |w, (from, to)| w.replace(from, to))
        })
        .collect()
}

/// `it::cli::landing_examples` (build plan M16): the landing page's CLI block (create an identity,
/// search, ask) and its idempotent-send example run against the harness; the second send with the same
/// key returns the same message with `"deduplicated": true`.
#[test]
fn landing_examples() {
    let _r = fakes::Reset;
    let p = platform();
    let t = crate::tenant(&p, "landing examples");
    let home = std::env::temp_dir().join(format!("idvy-landing-{}", crate::slug("landing")));
    std::fs::create_dir_all(home.join(".config/pylota-identity")).unwrap();
    let config = home.join(".config/pylota-identity/config.toml");
    std::fs::write(
        &config,
        format!(
            "[profiles.default]\nurl = \"{}\"\nkey = \"{p}\"\ntenant = \"{}\"\n",
            std::env::var("PM_ITEST_URL").unwrap(),
            t["slug"].as_str().unwrap()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let cli_block = commands(&block("<code id=\"code-cli\">", "idvy identities create"));
    assert_eq!(cli_block.len(), 3, "{cli_block:?}");
    assert_eq!(&cli_block[0][..3], ["idvy", "identities", "create"]);

    // `idvy identities create --username bookings --display-name "Acme Car Hire"`, in the profile's tenant.
    let (code, out, err) = idvy(&home, &cli_block[0]);
    assert_eq!(code, 0, "{err}");
    let bookings: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(bookings["display_name"], "Acme Car Hire");
    let bookings_addr = bookings["primary_address"].as_str().unwrap().to_owned();
    let (code, out, err) = idvy(
        &home,
        &words("idvy identities create --username compliance --display-name Compliance"),
    );
    assert_eq!(code, 0, "{err}");
    let compliance: Value = serde_json::from_str(&out).unwrap();
    let compliance_addr = compliance["primary_address"].as_str().unwrap().to_owned();
    let map = [
        ("bookings@acme.example.com", bookings_addr.as_str()),
        ("compliance@acme.example.com", compliance_addr.as_str()),
    ];

    // `idvy search "from:@brightwell.example ref:AB12CDE has:attachment" --identity …`
    let (code, out, err) = idvy(&home, &substitute(&cli_block[1], &map));
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.starts_with("DATE"), "a table of hits: {out}");

    // `idvy ask "Did the insurer accept the Golf claim?" --identity …`, streamed.
    fakes::ai_script(
        "qwen",
        vec![crate::agentic::completion(
            &[(
                "a",
                "search",
                json!({ "q": "golf claim insurer", "mode": "keyword" }),
            )],
            None,
        )],
    );
    let (code, out, err) = idvy(&home, &substitute(&cli_block[2], &map));
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("insufficient_evidence") || out.contains("answered"),
        "{out}"
    );

    // The idempotent send: an owner and an open recipient policy first (a send needs both).
    let (code, _, err) = idvy(
        &home,
        &substitute(
            &words(
                "idvy identities update bookings@acme.example.com --owner-name \"Sam Patel\" --owner-email sam@acmecarhire.example --require-known-recipient false",
            ),
            &map,
        ),
    );
    assert_eq!(code, 0, "{err}");
    let sends = commands(&block(
        "<pre class=\"term-body\" tabindex=\"0\"><code>",
        "--idempotency-key",
    ));
    assert_eq!(sends.len(), 2, "{sends:?}");
    assert_eq!(sends[0], sends[1], "the retry is the same command");
    let (code, first, err) = idvy(&home, &substitute(&sends[0], &map));
    assert_eq!(code, 0, "{err}");
    let first: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(
        (first["status"].as_str(), first["deduplicated"].as_bool()),
        (Some("queued"), Some(false)),
        "{first}"
    );
    let (code, second, err) = idvy(&home, &substitute(&sends[1], &map));
    assert_eq!(code, 0, "{err}");
    let second: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(second["id"], first["id"], "same key, same message");
    assert_eq!(second["deduplicated"], true);
}
