//! `idvy deploy` against the recorded fakes (CLI and setup § 8).

mod support;

use support::{Cli, Key, bundle_files, release_assets, setup_args, sign_assets, tarball};

/// The number of Wrangler deploys so far.
fn deploys(fake: &support::Fake) -> u32 {
    fake.with(|s| s.scripts.values().map(|sc| sc.deploys).sum())
}

const OLD: &str = "0.0.9";

/// `cli::deploy::tampered_bundle_refused` (FR-OPS-2, M16): a modified tarball, `SHA256SUMS` or
/// signature, a key this idvy does not trust and a replayed trusted comment are each exit 11; nothing is
/// deployed and no extracted bundle is left. The untampered release then deploys.
#[test]
fn tampered_bundle_refused() {
    let (cli, fake) = Cli::with_fake();
    let r = cli.run(&setup_args());
    assert_eq!(r.code, 0, "{}", r.err);
    let before = deploys(&fake);
    let key = Key::new(7);
    let good = release_assets(OLD, &key);
    let tar_name = format!("pylota-identity-worker-{OLD}.tar.gz");

    let mut tampered_tar = good.clone();
    tampered_tar.get_mut(&tar_name).unwrap().push(0);
    let mut tampered_sums = good.clone();
    tampered_sums
        .get_mut("SHA256SUMS")
        .unwrap()
        .extend_from_slice(b"\n");
    let mut bad_sig = good.clone();
    bad_sig.insert(
        "SHA256SUMS.sig".into(),
        b"untrusted comment: x\nnot base64\n".to_vec(),
    );
    let other_key = sign_assets(
        OLD,
        &Key::new(9),
        tarball(&bundle_files(OLD), None),
        &format!("pylota-identity v{OLD}"),
    );
    let replayed = sign_assets(
        OLD,
        &key,
        tarball(&bundle_files(OLD), None),
        "pylota-identity v0.0.8",
    );

    for (case, assets) in [
        ("modified tarball", tampered_tar),
        ("modified SHA256SUMS", tampered_sums),
        ("modified signature", bad_sig),
        ("another key", other_key),
        ("replayed trusted comment", replayed),
    ] {
        fake.with(|s| s.releases.insert(OLD.to_owned(), assets));
        let r = cli.run(&["deploy", "--version", OLD, "--json"]);
        assert_eq!(r.code, 11, "{case}: {}", r.out);
        assert_eq!(r.json()["error"]["code"], "cli_verification", "{case}");
        assert_eq!(deploys(&fake), before, "{case}: something was deployed");
        assert!(
            !cli.deploy_dir().join(".bundle").join(OLD).exists(),
            "{case}"
        );
        assert!(
            !cli.deploy_dir()
                .join(".bundle")
                .join(format!("{OLD}.partial"))
                .exists(),
            "{case}"
        );
    }

    fake.with(|s| s.releases.insert(OLD.to_owned(), good));
    let r = cli.run(&["deploy", "--version", OLD, "--json"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert_eq!(r.json()["version"], OLD);
    assert_eq!(deploys(&fake), before + 1);
}

/// `cli::deploy::safe_extraction`: symbolic links and `..` paths are refused, and a bundle without an
/// expected file too.
#[test]
fn safe_extraction() {
    let (cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);
    let key = Key::new(7);
    let trusted = format!("pylota-identity v{OLD}");
    let mut files = bundle_files(OLD);
    let link = tarball(&files, Some(("build/evil", "/etc/passwd")));
    files.push(("../escape".into(), b"x".to_vec()));
    let mut no_version = bundle_files(OLD);
    no_version.retain(|(n, _)| n != "VERSION");
    for (case, tar) in [
        ("symlink", link),
        ("missing VERSION", tarball(&no_version, None)),
    ] {
        fake.with(|s| {
            s.releases
                .insert(OLD.to_owned(), sign_assets(OLD, &key, tar, &trusted))
        });
        let r = cli.run(&["deploy", "--version", OLD, "--json"]);
        assert_eq!(r.code, 11, "{case}: {}", r.out);
    }
    assert!(
        pylota_identity_cli::bundle::extract::extract(
            &tarball(&files, None),
            &cli.home.join("x"),
            OLD
        )
        .is_err()
    );
}

/// `cli::deploy::noop_redeploy`: `deploy` straight after `setup` calls no Wrangler deploy and exits 0;
/// `--force` deploys anyway.
#[test]
fn noop_redeploy() {
    let (cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);
    let before = deploys(&fake);
    let r = cli.run(&["deploy"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("Nothing to deploy"), "{}", r.out);
    assert_eq!(deploys(&fake), before);
    let r = cli.run(&["deploy", "--force"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(deploys(&fake), before + 1);
}
