//! The config file, precedence and key sources (CLI and setup § 2, § 14).

mod support;

use pylota_identity_core::keys::{Mode, generate};
use serde_json::Value;
use support::{Cli, setup_args};

fn key(n: u8) -> String {
    generate(Mode::Live, [n; 8], [n; 32])
}

fn show(cli: &Cli, args: &[&str]) -> (i32, Value) {
    let mut a = vec!["config", "show", "--json"];
    a.extend_from_slice(args);
    let r = cli.run(&a);
    (r.code, r.json())
}

/// `cli::config::precedence` (FR-CLI-1): flag over environment over profile for the URL, the key, the
/// profile and the defaults.
#[test]
fn precedence() {
    let mut cli = Cli::new(None);
    let (a, b) = (key(1), key(2));
    cli.write_config(
        &format!(
            "default_profile = \"prod\"\n[profiles.prod]\nurl = \"https://prod.example.com\"\nkey = \"{a}\"\nidentity = \"idn_PROFILE\"\n\
             [profiles.staging]\nurl = \"https://staging.example.com/v1/\"\nkey_env = \"STAGING_KEY\"\n"
        ),
        0o600,
    );
    let (code, d) = show(&cli, &[]);
    assert_eq!(code, 0, "{d}");
    assert_eq!(d["profile"], "prod");
    assert_eq!(
        (d["url"]["value"].as_str(), d["url"]["source"].as_str()),
        (Some("https://prod.example.com"), Some("profile"))
    );
    assert_eq!(d["key"]["source"], "profile");
    assert!(d["key"]["value"].as_str().unwrap().ends_with('…') && !d.to_string().contains(&a));
    assert_eq!(d["identity"]["value"], "idn_PROFILE");

    cli.vars.insert(
        "PYLOTA_IDENTITY_URL".into(),
        "https://env.example.com".into(),
    );
    cli.vars.insert("PYLOTA_IDENTITY_KEY".into(), b.clone());
    let (_, d) = show(&cli, &[]);
    assert_eq!(
        (d["url"]["value"].as_str(), d["url"]["source"].as_str()),
        (Some("https://env.example.com"), Some("environment"))
    );
    assert_eq!(d["key"]["source"], "environment");
    let (_, d) = show(
        &cli,
        &[
            "--url",
            "https://flag.example.com",
            "--identity",
            "idn_FLAG",
        ],
    );
    assert_eq!(
        (d["url"]["value"].as_str(), d["url"]["source"].as_str()),
        (Some("https://flag.example.com"), Some("flag"))
    );
    assert_eq!(
        (
            d["identity"]["value"].as_str(),
            d["identity"]["source"].as_str()
        ),
        (Some("idn_FLAG"), Some("flag"))
    );

    cli.vars.remove("PYLOTA_IDENTITY_URL");
    cli.vars.remove("PYLOTA_IDENTITY_KEY");
    cli.vars
        .insert("PYLOTA_IDENTITY_PROFILE".into(), "staging".into());
    cli.vars.insert("STAGING_KEY".into(), b.clone());
    let (_, d) = show(&cli, &[]);
    assert_eq!(d["profile"], "staging");
    assert_eq!(
        d["url"]["value"], "https://staging.example.com",
        "a trailing /v1/ is removed"
    );
    let (_, d) = show(&cli, &["--profile", "prod"]);
    assert_eq!(d["profile"], "prod");
    assert_eq!(show(&cli, &["--profile", "missing"]).0, 3);
    assert_eq!(
        show(&cli, &["--url", "http://plain.example.com"]).0,
        3,
        "https is required off loopback"
    );
}

/// `cli::config::insecure_file_refused`: a file group or others can read is refused with exit 3 and the
/// chmod fix; writes are atomic, with mode 0600 in a 0700 directory.
#[test]
#[cfg(unix)]
fn insecure_file_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let cli = Cli::new(None);
    let p = cli.write_config(
        &format!("[profiles.default]\nkey = \"{}\"\n", key(3)),
        0o644,
    );
    let r = cli.run(&["config", "show"]);
    assert_eq!(r.code, 3, "{}", r.err);
    assert!(
        r.err.contains(&format!("chmod 600 {}", p.display())),
        "{}",
        r.err
    );
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(cli.run(&["config", "show"]).code, 0);
    let r = cli.run(&["config", "set", "identity", "bookings@acme.example.com"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(p.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(
        std::fs::read_to_string(&p)
            .unwrap()
            .contains("bookings@acme.example.com")
    );
    let leftovers: Vec<_> = std::fs::read_dir(p.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
    assert_eq!(
        cli.run(&["config", "set", "key", "x"]).code,
        2,
        "a key set this way would land in shell history"
    );
}

/// `cli::config::key_sources`: `key`, `key_env`, `key_command` (non-zero exit, bad output, timeout) and
/// the one-source rule.
#[test]
fn key_sources() {
    let cli = Cli::new(None);
    let k = key(4);
    let with = |profile: &str| {
        cli.write_config(
            &format!("[profiles.default]\nurl = \"https://mail.example.com\"\n{profile}\n"),
            0o600,
        );
        cli.run(&["config", "show", "--json"])
    };
    let r = with(&format!("key_command = \"printf {k}\""));
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.json()["key"]["source"], "profile");
    assert!(!r.out.contains(&k), "the command's output is never shown");
    assert_eq!(with("key_command = \"exit 3\"").code, 3);
    assert_eq!(with("key_command = \"echo not-a-key\"").code, 3);
    assert_eq!(with("key_env = \"UNSET_VARIABLE\"").code, 3);
    assert_eq!(with(&format!("key = \"{k}\"\nkey_env = \"X\"")).code, 3);
    assert_eq!(with("key = \"pmk_live_short\"").code, 3);
    let start = std::time::Instant::now();
    assert_eq!(with("key_command = \"sleep 30\"").code, 3);
    assert!(
        start.elapsed().as_secs() < 20,
        "the 10-second timeout applies"
    );
}

/// `cli::config::profile_written` (the login half; setup's is in `cli::setup::idempotent_rerun`):
/// `login` writes the profile named by `--profile`, else `default`, whatever `PYLOTA_IDENTITY_PROFILE`
/// and `default_profile` say, and warns when `PYLOTA_IDENTITY_KEY` differs from the saved key.
#[test]
fn profile_written() {
    let (mut cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);
    let config =
        std::fs::read_to_string(cli.home.join(".config/pylota-identity/config.toml")).unwrap();
    let file: toml::Table = toml::from_str(&config).unwrap();
    let bootstrap = file["profiles"]["default"]["key"]
        .as_str()
        .unwrap()
        .to_owned();
    let url = fake.base.clone();

    cli.vars
        .insert("PYLOTA_IDENTITY_PROFILE".into(), "elsewhere".into());
    cli.vars
        .insert("PYLOTA_IDENTITY_KEY".into(), bootstrap.clone());
    let r = cli.run(&["login", "--profile", "staging", "--url", &url]);
    assert_eq!(r.code, 0, "{}", r.err);
    let file: toml::Table = toml::from_str(
        &std::fs::read_to_string(cli.home.join(".config/pylota-identity/config.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        file["profiles"]["staging"]["key"].as_str(),
        Some(bootstrap.as_str())
    );
    assert!(file["profiles"].get("elsewhere").is_none());
    assert!(!r.err.contains("differs"));

    cli.vars.insert("SAVED_KEY".into(), bootstrap.clone());
    cli.vars.insert("PYLOTA_IDENTITY_KEY".into(), key(9));
    let r = cli.run(&["login", "--url", &url, "--key-env", "SAVED_KEY"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(
        r.err.contains("PYLOTA_IDENTITY_KEY is set and differs"),
        "{}",
        r.err
    );
    let file: toml::Table = toml::from_str(
        &std::fs::read_to_string(cli.home.join(".config/pylota-identity/config.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        file["profiles"]["default"]["key_env"].as_str(),
        Some("SAVED_KEY")
    );
    assert!(file["profiles"]["default"].get("key").is_none());
}
