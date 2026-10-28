//! Every case of the MIME conformance corpus (`crates/conformance/corpus/`) ingested end to end through
//! workerd: `email()`, `pm-inbound` and the mailbox (build plan M7). Each case is accepted and stored,
//! except a DSN naming no message of ours, which is dropped as backscatter (D4); the stored `kind`, and
//! the verdict of the authentication cases, match the case's expectations.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::{eventually, inbound, mailbox_fixture, mailbox_query};

struct Case {
    name: String,
    raw: String,
    from: String,
    message_id: Option<String>,
    expect: toml::Table,
}

fn cases() -> Vec<Case> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/corpus");
    let mut out = Vec::new();
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs.into_iter().filter(|d| d.is_dir()) {
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        files.sort();
        for eml in files
            .into_iter()
            .filter(|f| f.extension().is_some_and(|e| e == "eml"))
        {
            let raw = String::from_utf8_lossy(&std::fs::read(&eml).unwrap()).into_owned();
            let meta: toml::Table = std::fs::read_to_string(eml.with_extension("toml"))
                .unwrap()
                .parse()
                .unwrap();
            let from = meta
                .get("envelope")
                .and_then(|e| e.get("from"))
                .and_then(|f| f.as_str())
                .unwrap_or("sender@example.net")
                .to_owned();
            // The ID as the parser reads it (comments and folding removed, or the synthetic one).
            let message_id = Some(
                pylota_identity_core::mime::parse_headers_only(raw.as_bytes())
                    .message_id
                    .trim_matches(['<', '>'])
                    .to_ascii_lowercase(),
            )
            .filter(|m| !m.is_empty());
            let expect = meta
                .get("expect")
                .and_then(|e| e.as_table())
                .cloned()
                .unwrap_or_default();
            out.push(Case {
                name: eml.file_stem().unwrap().to_string_lossy().into_owned(),
                raw,
                from,
                message_id,
                expect,
            });
        }
    }
    out
}

#[test]
fn corpus_ingested_end_to_end() {
    let cases = cases();
    assert!(cases.len() > 300, "{} cases", cases.len());
    let mut failures = Vec::new();
    // Twenty cases per identity, so no per-identity cap throttles the corpus.
    for (n, chunk) in cases.chunks(20).enumerate() {
        let (_, idn, addr, _) = mailbox_fixture(&format!("corpus{n}"));
        let mut expected: BTreeMap<String, &Case> = BTreeMap::new();
        let mut stored_at_least = 0;
        for c in chunk {
            let (outcome, smtp) = inbound(&c.from, &addr, &c.raw);
            if outcome != "accepted" {
                failures.push(format!("{}: {outcome} {smtp}", c.name));
                continue;
            }
            let dsn = c.expect.get("kind").and_then(|k| k.as_str()) == Some("dsn");
            if !dsn {
                stored_at_least += 1;
                if let Some(id) = &c.message_id {
                    expected.insert(id.clone(), c);
                }
            }
        }
        let rows: Vec<Value> = eventually(90, &format!("corpus chunk {n}"), || {
            let r = mailbox_query(
                &idn,
                "SELECT rfc_message_id, kind, verdict, status FROM messages",
            );
            // Same-ID cases of the corpus are deduplicated, so count distinct IDs.
            (r.len() >= expected.len().min(stored_at_least)).then_some(r)
        });
        std::thread::sleep(std::time::Duration::from_secs(3));
        let rows = if rows.len() < stored_at_least {
            mailbox_query(
                &idn,
                "SELECT rfc_message_id, kind, verdict, status FROM messages",
            )
        } else {
            rows
        };
        for (id, c) in &expected {
            let Some(row) = rows.iter().find(|r| {
                r["rfc_message_id"]
                    .as_str()
                    .is_some_and(|r| r.trim_matches(['<', '>']).eq_ignore_ascii_case(id))
            }) else {
                failures.push(format!("{}: not stored", c.name));
                continue;
            };
            if let Some(k) = c.expect.get("kind").and_then(|k| k.as_str())
                && row["kind"].as_str() != Some(k)
            {
                failures.push(format!("{}: kind {} != {k}", c.name, row["kind"]));
            }
            let trusted_ar = c.name.contains("trusted") || c.name.contains("forged_ar");
            if let Some(v) = c.expect.get("verdict").and_then(|v| v.as_str())
                && !trusted_ar
                && row["verdict"].as_str() != Some(v)
            {
                failures.push(format!("{}: verdict {} != {v}", c.name, row["verdict"]));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
