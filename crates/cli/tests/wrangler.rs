//! Wrangler from the signed bundle (CLI and setup § 5.1, J34).

mod support;

use support::{Cli, Key, bundle_files, release_assets, setup_args, sign_assets, tarball};

const OLD: &str = "0.0.9";

fn calls(fake: &support::Fake) -> Vec<pylota_identity_cli::Cmd> {
    fake.with(|s| s.runner_calls.clone())
}

fn npm_ci(c: &pylota_identity_cli::Cmd) -> bool {
    c.program == "npm" && c.args.first().map(String::as_str) == Some("ci")
}

fn is_wrangler(c: &pylota_identity_cli::Cmd) -> bool {
    c.program.ends_with("/node_modules/.bin/wrangler")
}

/// `cli::wrangler::installs_from_signed_lockfile` (J34): `idvy deploy` installs Wrangler only with
/// `npm ci --ignore-scripts` from the verified bundle's `wrangler/package-lock.json` into
/// `<dir>/.wrangler/{version}/`, reuses it only while `.lock-sha256` matches, and runs it by absolute
/// path after a `--version` check; npm is never asked to install or execute a package by name. A
/// tampered lockfile is exit 11 before any install, and a package failing its `integrity` hash stops
/// the install (exit 10) before any Wrangler call.
#[test]
fn installs_from_signed_lockfile() {
    let (cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);
    let key = Key::new(7);
    fake.with(|s| s.releases.insert(OLD.to_owned(), release_assets(OLD, &key)));
    // A fresh install for the deploy.
    std::fs::remove_dir_all(cli.deploy_dir().join(".wrangler")).ok();
    let start = calls(&fake).len();
    let r = cli.run(&["deploy", "--version", OLD, "--force", "--json"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let all = calls(&fake)[start..].to_vec();
    let installs: Vec<_> = all.iter().filter(|c| npm_ci(c)).collect();
    for c in all.iter().filter(|c| c.program == "npm") {
        assert!(
            c.args == ["--version"]
                || c.args == ["ci", "--ignore-scripts", "--no-audit", "--no-fund"],
            "npm {:?}",
            c.args
        );
    }
    assert!(
        !all.iter().any(|c| c.program == "npx"),
        "the package runner"
    );
    let ci = installs.last().expect("an npm ci");
    let home = ci.cwd.clone().unwrap();
    assert!(
        home.ends_with(format!(".wrangler/{}", pylota_identity_cli::VERSION)),
        "{}",
        home.display()
    );
    let lock = bundle_files(OLD)
        .into_iter()
        .find(|(n, _)| n == "wrangler/package-lock.json")
        .unwrap()
        .1;
    assert_eq!(
        std::fs::read(home.join("package-lock.json")).unwrap(),
        lock,
        "the bundle's lockfile"
    );
    // After the install, the first Wrangler call is the version check, by absolute path.
    let at = all.iter().rposition(npm_ci).unwrap();
    let first = all[at + 1..]
        .iter()
        .find(|c| is_wrangler(c))
        .expect("a Wrangler call");
    assert_eq!(first.args, ["--version"]);
    assert!(std::path::Path::new(&first.program).is_absolute());

    // Reused while the lockfile is the same.
    let n = calls(&fake).iter().filter(|c| npm_ci(c)).count();
    let r = cli.run(&["deploy", "--version", OLD, "--force", "--json"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert_eq!(
        calls(&fake).iter().filter(|c| npm_ci(c)).count(),
        n,
        "reused"
    );

    // A lockfile changed after signing: refused before any install.
    let mut files = bundle_files(OLD);
    for (name, bytes) in &mut files {
        if name == "wrangler/package-lock.json" {
            bytes.extend_from_slice(b" ");
        }
    }
    let mut assets = release_assets(OLD, &key);
    assets.insert(
        format!("pylota-identity-worker-{OLD}.tar.gz"),
        tarball(&files, None),
    );
    fake.with(|s| s.releases.insert(OLD.to_owned(), assets));
    // Not the extraction cached from the deploys above.
    std::fs::remove_dir_all(cli.deploy_dir().join(".bundle")).ok();
    std::fs::remove_dir_all(cli.deploy_dir().join(".wrangler")).ok();
    let before = calls(&fake).len();
    let r = cli.run(&["deploy", "--version", OLD, "--force", "--json"]);
    assert_eq!(r.code, 11, "{}", r.out);
    assert!(
        !calls(&fake)[before..].iter().any(npm_ci),
        "installed from a tampered lockfile"
    );

    // A package whose bytes fail its integrity hash: the install stops, Wrangler never runs.
    fake.with(|s| {
        s.releases.insert(
            OLD.to_owned(),
            sign_assets(
                OLD,
                &key,
                tarball(&bundle_files(OLD), None),
                &format!("pylota-identity v{OLD}"),
            ),
        );
        s.npm_integrity_fails = true;
    });
    std::fs::remove_dir_all(cli.deploy_dir().join(".bundle")).ok();
    std::fs::remove_dir_all(cli.deploy_dir().join(".wrangler")).ok();
    let before = calls(&fake).len();
    let r = cli.run(&["deploy", "--version", OLD, "--force", "--json"]);
    assert_eq!(r.code, 10, "{}", r.out);
    let after = &calls(&fake)[before..];
    assert!(after.iter().any(npm_ci));
    assert!(
        !after.iter().any(is_wrangler),
        "Wrangler ran after a failed install"
    );
}
