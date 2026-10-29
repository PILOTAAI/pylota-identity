//! D1 migrations through the D1 query API ([CLI and setup §8.5]): each pending `NNNN_name.sql` in one
//! request, ending with its `schema_migrations` row. A database one release newer than the bundle is a
//! warning; more than one is refused.
//!
//! [CLI and setup §8.5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#85-d1-migrations

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};

use crate::cloudflare::{Client, unexpected};
use crate::{CliError, Ctx};

/// `(version, file text)` of every migration in `dir`, in order.
pub fn files(dir: &Path) -> Result<Vec<(u32, String)>, CliError> {
    let io = |e: std::io::Error| CliError::internal(format!("{}: {e}", dir.display()));
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).map_err(io)? {
        let p = e.map_err(io)?.path();
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        let Some(stem) = name.strip_suffix(".sql") else {
            continue;
        };
        let Some(v) = stem.split('_').next().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        out.push((v, std::fs::read_to_string(&p).map_err(io)?));
    }
    out.sort_by_key(|(v, _)| *v);
    Ok(out)
}

/// Applies the pending migrations; returns their versions.
pub async fn apply(ctx: &Ctx, cf: &Client<'_>, db: &str, dir: &Path) -> Result<Vec<u32>, CliError> {
    let files = files(dir)?;
    let exists = !cf
        .d1_query(
            db,
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
            &[],
        )
        .await?
        .is_empty();
    let applied: BTreeSet<u32> = if exists {
        cf.d1_query(db, "SELECT version FROM schema_migrations", &[])
            .await?
            .iter()
            .filter_map(|r| r["version"].as_u64().map(|v| v as u32))
            .collect()
    } else {
        BTreeSet::new()
    };
    let known: BTreeSet<u32> = files.iter().map(|(v, _)| *v).collect();
    let newer: Vec<u32> = applied.difference(&known).copied().collect();
    match newer.len() {
        0 => {}
        1 => ctx.note(&format!(
            "warning: the database has migration {} that this release does not know (a one-release rollback)",
            newer[0]
        )),
        _ => {
            return Err(unexpected(
                "D1 migrations",
                &format!("the database is more than one release newer than this bundle (migrations {newer:?})"),
            ));
        }
    }
    let mut done = Vec::new();
    for (v, sql) in files.iter().filter(|(v, _)| !applied.contains(v)) {
        let full = format!(
            "{}\nINSERT INTO schema_migrations (version, applied_at) VALUES ({v}, {});",
            sql.trim_end(),
            ctx.now_ms()
        );
        let r = cf
            .post(
                "D1 migrations",
                &cf.acct(&format!("/d1/database/{db}/query")),
                &json!({ "sql": full }),
            )
            .await?;
        if r.as_array()
            .into_iter()
            .flatten()
            .any(|s: &Value| s["success"] == false)
        {
            return Err(unexpected(
                "D1 migrations",
                &format!("migration {v} failed"),
            ));
        }
        done.push(*v);
    }
    Ok(done)
}
