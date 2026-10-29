//! `idvy secrets rotate-master` against the recorded fakes (CLI and setup § 12.1, Security § 6.2).

mod support;

use support::{Cli, setup_args};

/// The secret puts Wrangler got, in order: `(name, value)`.
fn puts(fake: &support::Fake) -> Vec<(String, String)> {
    fake.with(|s| {
        s.runner_calls
            .iter()
            .filter(|c| c.args.iter().any(|a| a == "secret"))
            .filter_map(|c| {
                let i = c.args.iter().position(|a| a == "put")?;
                Some((
                    c.args[i + 1].clone(),
                    String::from_utf8_lossy(c.stdin.as_deref().unwrap_or_default()).into_owned(),
                ))
            })
            .collect()
    })
}

/// `cli::secrets::rotate_master` (Security § 6.2): the first rotation writes a new 32-byte key into
/// the empty slot `b` through Wrangler's stdin, then `PM_MASTER_KEY_ACTIVE = b`, and polls until the
/// re-seal reports 0, printing the new key ID (never the key); a second rotation within 30 days of the
/// re-seal is refused with exit 15 and the allowed date, and goes ahead with `--discard-previous`
/// into slot `a`; an unfinished re-seal refuses a new rotation (exit 15, fix `--resume`), and
/// `--resume` only polls.
#[test]
fn rotate_master() {
    let (cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);
    let before = puts(&fake).len();
    let original = fake.with(|s| {
        s.scripts
            .values()
            .find_map(|sc| sc.secrets.get("PM_MASTER_KEY").cloned())
            .unwrap()
    });

    // The setup key's re-seal finished long ago.
    let original_kid = pylota_identity_core::crypto::kid(
        &base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &original)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    fake.with(|s| {
        s.resealed = Some((original_kid, 0));
        s.reseal_script = vec![1_200, 400, 0];
    });
    let r = cli.run(&["secrets", "rotate-master", "--json"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let new = puts(&fake)[before..].to_vec();
    assert_eq!(
        new.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["PM_MASTER_KEY_B", "PM_MASTER_KEY_ACTIVE"],
        "the key first, then the selector"
    );
    let key = &new[0].1;
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, key).unwrap();
    assert_eq!(bytes.len(), 32);
    assert_ne!(*key, original);
    assert_eq!(new[1].1, "b");
    let doc = r.json();
    let kid = pylota_identity_core::crypto::kid(&bytes.try_into().unwrap());
    assert_eq!(doc["active_slot"], "b");
    assert_eq!(doc["kid"], kid.as_str());
    assert!(doc["next_rotation_from"].is_string());
    assert!(r.err.contains("remaining 400 (about 1 min)"), "{}", r.err);
    assert!(!r.out.contains(key.as_str()) && !r.err.contains(key.as_str()));
    // The key went to Wrangler on stdin only, never as an argument.
    assert!(fake.with(|s| {
        s.runner_calls
            .iter()
            .all(|c| !c.args.iter().any(|a| a.contains(key.as_str())))
    }));

    // Within 30 days of the re-seal, slot `a` still holds the previous key: refused.
    let r = cli.run(&["secrets", "rotate-master", "--json"]);
    assert_eq!(r.code, 15, "{}", r.out);
    assert!(
        r.json()["error"]["details"]["allowed_from"].is_string(),
        "{}",
        r.out
    );
    assert_eq!(puts(&fake).len(), before + 2);

    // After a suspected leak: overwrite slot `a` and switch back to it.
    let r = cli.run(&[
        "secrets",
        "rotate-master",
        "--discard-previous",
        "--yes",
        "--json",
    ]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let names: Vec<String> = puts(&fake)[before + 2..]
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    assert_eq!(names, ["PM_MASTER_KEY", "PM_MASTER_KEY_ACTIVE"]);
    assert_eq!(r.json()["active_slot"], "a");

    // An unfinished re-seal: refused without --resume; --resume polls and writes nothing.
    fake.with(|s| {
        s.reseal_script = vec![300, 300, 0];
        s.resealed = None;
    });
    let r = cli.run(&["secrets", "rotate-master", "--json"]);
    assert_eq!(r.code, 15, "{}", r.out);
    assert_eq!(
        r.json()["error"]["fix"],
        "idvy secrets rotate-master --resume"
    );
    let n = puts(&fake).len();
    let r = cli.run(&["secrets", "rotate-master", "--resume", "--json"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert_eq!(r.json()["active_slot"], "a");
    assert_eq!(puts(&fake).len(), n, "--resume writes no secret");
}
