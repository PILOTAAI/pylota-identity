//! The landing page's self-hosting example (build plan M16): `idvy setup … && idvy deploy`, read from
//! `site/public/index.html` and run against the recorded Cloudflare fake. Setup asks for the console
//! owner's email, as it does on a terminal. The API examples run against the workerd harness
//! (`it::cli::landing_examples`).

mod support;

use support::Cli;

/// The `idvy` lines of the landing page's `code-os` block.
fn commands() -> Vec<Vec<String>> {
    let page = std::fs::read_to_string(support::root().join("site/public/index.html")).unwrap();
    let open = "<code id=\"code-os\">";
    let start = page.find(open).unwrap() + open.len();
    let block = &page[start..start + page[start..].find("</code>").unwrap()];
    block
        .lines()
        .filter(|l| l.starts_with("idvy "))
        .map(|l| l.split_whitespace().map(str::to_owned).collect())
        .collect()
}

/// `idvy setup --domain mail.example.com --mail-domain agents.example --jurisdiction eu`, then
/// `idvy deploy`, which finds nothing to change.
#[test]
fn setup_then_deploy() {
    let (cli, fake) = Cli::with_fake();
    let lines = commands();
    assert_eq!(lines.len(), 2, "{lines:?}");
    fn args(l: &[String]) -> Vec<&str> {
        l[1..].iter().map(String::as_str).collect()
    }
    assert_eq!(lines[0][1], "setup");
    let r = cli.run_tty(&args(&lines[0]), b"sam@acmecarhire.example\n");
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(r.err.contains("Console owner's email"), "{}", r.err);
    assert!(r.out.contains("is running at"), "{}", r.out);
    let deploys =
        |f: &support::Fake| f.with(|s| s.scripts.values().map(|sc| sc.deploys).sum::<u32>());
    let before = deploys(&fake);
    assert_eq!(lines[1], ["idvy", "deploy"]);
    let r = cli.run(&args(&lines[1]));
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        deploys(&fake),
        before,
        "deploy straight after setup changes nothing"
    );
}
