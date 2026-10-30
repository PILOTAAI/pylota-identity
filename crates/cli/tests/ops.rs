//! `idvy ops` against the recorded fakes (CLI and setup § 20, Observability § 9).

mod support;

use pylota_identity_core::schema::Schema;
use pylota_identity_core::sqldump::{self, Lit};
use support::{Cli, setup_args};

const EXPORT: &str = include_str!("fixtures/d1-export.sql");

/// `cli::ops::sqldump_parses_export` (Restore from PITR, step 3): the export of the pinned Wrangler
/// (4.139.0, recorded from a local D1 with the migrations applied) parses into rows with their
/// literals (a doubled quote, a blob in `X'…'`, `NULL`); every table it holds is in `core::schema`
/// with the same columns in the same order; a row written back as SQL parses to the same values.
#[test]
fn sqldump_parses_export() {
    let d = sqldump::parse(EXPORT).unwrap();
    let partners = &d.tables["partners"];
    assert_eq!(partners.rows.len(), 1);
    let col = |t: &sqldump::Rows, c: &str| t.columns.iter().position(|x| x == c).unwrap();
    assert_eq!(
        partners.rows[0][col(partners, "name")],
        Lit::Text("O'Brien & Co".into())
    );
    assert_eq!(partners.rows[0][col(partners, "deleted_at")], Lit::Null);
    let keys = &d.tables["signing_keys"];
    assert_eq!(
        keys.rows[0][col(keys, "ciphertext")],
        Lit::Blob(vec![0x00, 0x01, 0xff, 0x70])
    );
    let schema = Schema::d1();
    for (name, rows) in &d.tables {
        if sqldump::SKIPPED.contains(&name.as_str()) {
            continue;
        }
        assert_eq!(
            schema.table(name).map(|t| &t.columns),
            Some(&rows.columns),
            "{name}"
        );
    }
    let row = &partners.rows[0];
    let sql = format!(
        "INSERT INTO \"partners\" ({}) VALUES({});\n",
        partners
            .columns
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(","),
        row.iter().map(Lit::sql).collect::<Vec<_>>().join(",")
    );
    assert_eq!(
        &sqldump::parse(&sql).unwrap().tables["partners"].rows[0],
        row
    );
}

/// `cli::ops::freeze_unfreeze` (CLI and setup § 20): `ops freeze` sets `PM_FREEZE = "on"` and
/// `PM_FREEZE_SINCE` under `[vars]`, deploys the deployed bundle's code and waits for `/health` to
/// report `frozen: true`; `ops unfreeze` sets it `off`, removes `PM_FREEZE_SINCE`, deploys again and
/// waits for `frozen` to go.
#[test]
fn freeze_unfreeze() {
    let (cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);
    let deploys = || fake.with(|s| s.scripts.values().map(|sc| sc.deploys).sum::<u32>());
    let before = deploys();
    let file = cli.home.join("deploy/wrangler.toml");
    let r = cli.run(&["ops", "freeze", "--json"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert_eq!(r.json()["frozen"], true);
    let vars = |text: &str| -> toml::Table {
        let doc: toml::Table = toml::from_str(text).unwrap();
        doc["vars"].as_table().unwrap().clone()
    };
    let v = vars(&std::fs::read_to_string(&file).unwrap());
    assert_eq!(v["PM_FREEZE"].as_str(), Some("on"));
    assert!(
        v["PM_FREEZE_SINCE"]
            .as_str()
            .is_some_and(|s| s.ends_with('Z'))
    );
    assert_eq!(deploys(), before + 1);
    assert!(fake.with(|s| s.scripts.values().any(|sc| sc.frozen)));

    let r = cli.run(&["ops", "unfreeze", "--json"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let v = vars(&std::fs::read_to_string(&file).unwrap());
    assert_eq!(v["PM_FREEZE"].as_str(), Some("off"));
    assert!(v.get("PM_FREEZE_SINCE").is_none());
    assert_eq!(deploys(), before + 2);
    assert!(fake.with(|s| s.scripts.values().all(|sc| !sc.frozen)));
}

/// Statements of one table in an export's format.
fn table(name: &str, cols: &[&str], rows: &[&str]) -> String {
    let c = cols
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    rows.iter()
        .map(|r| format!("INSERT INTO \"{name}\" ({c}) VALUES({r});\n"))
        .collect()
}

/// `cli::ops::restore_replay_diff` (Restore from PITR, step 3; I6): `ops restore d1` refuses unless
/// the deployment is frozen (exit 15) and refuses an unknown `--exclude` before anything is restored
/// (exit 2); then it exports, restores to the bookmark of `--at` (printing the undo bookmark), exports
/// again, and applies the replay: a row created after `T` is inserted, a row changed after `T` is
/// updated except the excluded column, a row deleted after `T` is deleted, an `--exclude-row` is left
/// alone; parents are inserted before children and children deleted before parents.
#[test]
fn restore_replay_diff() {
    let (cli, fake) = Cli::with_fake();
    assert_eq!(cli.run(&setup_args()).code, 0);
    let tcols = [
        "id",
        "slug",
        "name",
        "mode",
        "address_suffix",
        "suffix_fold",
        "policy_json",
        "quota_do_id",
        "notify_do_id",
        "created_at",
        "updated_at",
    ];
    let icols = [
        "id",
        "tenant_id",
        "username",
        "display_name",
        "created_at",
        "updated_at",
    ];
    let scols = ["tenant_id", "address_hash", "reason", "created_at"];
    // At T: tenant A (named "A", policy {}), suppressions s1 and s2.
    let after = format!(
        "{}{}",
        table(
            "tenants",
            &tcols,
            &["'ten_a','a','A','live','','','{}','q','',1,1"]
        ),
        table(
            "suppressions",
            &scols,
            &["'ten_a','h1','bounce',1", "'ten_a','h2','bounce',1"]
        )
    );
    // Now: A renamed and its policy changed (excluded), tenant B and its identity created, s1
    // deleted, s2 deleted too (but excluded by row).
    let before = format!(
        "{}{}",
        table(
            "tenants",
            &tcols,
            &[
                "'ten_a','a','A renamed','live','','','{\"x\":1}','q','',1,2",
                "'ten_b','b','B','live','b','b','{}','q','',3,3"
            ]
        ),
        table(
            "identities",
            &icols,
            &["'idn_b','ten_b','bookings','Bookings',3,3"]
        )
    );
    fake.with(|s| s.d1_exports = vec![before.clone(), after.clone()]);

    // Not frozen: refused, nothing restored.
    let r = cli.run(&[
        "ops",
        "restore",
        "d1",
        "--at",
        "2026-10-29T10:00:00Z",
        "--yes",
    ]);
    assert_eq!(r.code, 15, "{}", r.err);
    assert!(fake.with(|s| s.time_travel.is_empty()));
    assert_eq!(cli.run(&["ops", "freeze"]).code, 0);
    // An unknown exclude: exit 2 before anything is restored.
    let r = cli.run(&[
        "ops",
        "restore",
        "d1",
        "--at",
        "2026-10-29T10:00:00Z",
        "--exclude",
        "tenants.nope",
        "--yes",
    ]);
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(fake.with(|s| s.time_travel.is_empty()));

    let r = cli.run(&[
        "ops",
        "restore",
        "d1",
        "--at",
        "2026-10-29T10:00:00Z",
        "--exclude",
        "tenants.policy_json",
        "--exclude-row",
        "suppressions:ten_a,h2",
        "--yes",
        "--json",
    ]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(
        r.err
            .contains("Undo bookmark: 00000099-00000aaa-0000bbbb-undo"),
        "{}",
        r.err
    );
    let calls = fake.with(|s| s.time_travel.clone());
    assert_eq!(calls[0], ["info", "--timestamp=2026-10-29T10:00:00Z"]);
    assert_eq!(calls[1][0], "restore");
    let sql = fake.with(|s| s.d1_executed.last().cloned().unwrap());
    let pos = |needle: &str| {
        sql.find(needle)
            .unwrap_or_else(|| panic!("{needle} in\n{sql}"))
    };
    let ins_tenant = pos("INSERT INTO \"tenants\"");
    let ins_identity = pos("INSERT INTO \"identities\"");
    assert!(ins_tenant < ins_identity, "parents first:\n{sql}");
    assert!(sql.contains("UPDATE \"tenants\" SET \"name\" = 'A renamed', \"updated_at\" = 2 WHERE \"id\" = 'ten_a';"), "{sql}");
    assert!(
        !sql.contains("policy_json\" = "),
        "the excluded column keeps its restored value"
    );
    assert!(sql.contains(
        "DELETE FROM \"suppressions\" WHERE \"tenant_id\" = 'ten_a' AND \"address_hash\" = 'h1';"
    ));
    assert!(!sql.contains("'h2'"), "the excluded row is left alone");
    assert!(pos("DELETE FROM") < ins_tenant, "deletes before inserts");
    let doc = r.json();
    assert_eq!(doc["tables"]["tenants"]["inserted"], 1);
    assert_eq!(doc["tables"]["tenants"]["updated"], 1);
    assert_eq!(doc["tables"]["suppressions"]["deleted"], 1);
    let work = std::path::PathBuf::from(doc["work_dir"].as_str().unwrap());
    for f in [
        "before.sql",
        "after.sql",
        "undo-bookmark",
        "replay.sql",
        "summary.json",
    ] {
        assert!(work.join(f).exists(), "{f}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&work).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    assert_eq!(cli.run(&["ops", "restore", "cleanup"]).code, 0);
    assert!(!work.exists());
}
