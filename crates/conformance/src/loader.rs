//! Loads the corpus (Testing § 5.1): `corpus/<dir>/<case>.eml` with its `<case>.toml`, and the DNS
//! fixtures of `corpus/dns/zones.toml`. Test-only: it reads files and uses the `toml` dev-dependency.

use std::path::{Path, PathBuf};

use pylota_identity_core::auth::{DnsAnswers, TxtAnswer};

/// Directories under `corpus/` that hold cases.
pub const CASE_DIRS: &[&str] = &[
    "attachments",
    "auth",
    "calendar",
    "dsn",
    "mime",
    "sanitize",
    "threading",
];

pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

pub struct Case {
    /// `<dir>/<id>`, the part after `conf::`.
    pub name: String,
    pub raw: Vec<u8>,
    pub meta: toml::Table,
}

impl Case {
    pub fn envelope_from(&self) -> &str {
        self.meta
            .get("envelope")
            .and_then(|e| e.get("from"))
            .and_then(toml::Value::as_str)
            .unwrap_or("")
    }

    pub fn expect(&self) -> toml::Table {
        self.meta
            .get("expect")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default()
    }
}

/// Every case, sorted by name. Panics on an unreadable file: the corpus is part of the source tree.
pub fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for dir in CASE_DIRS {
        let path = corpus_dir().join(dir);
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        for eml in files
            .iter()
            .filter(|p| p.extension().is_some_and(|x| x == "eml"))
        {
            let stem = eml
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let raw = std::fs::read(eml).unwrap_or_else(|e| panic!("{}: {e}", eml.display()));
            let toml_path = eml.with_extension("toml");
            let meta = std::fs::read_to_string(&toml_path)
                .ok()
                .and_then(|s| s.parse::<toml::Table>().ok())
                .unwrap_or_default();
            out.push(Case {
                name: format!("{dir}/{stem}"),
                raw,
                meta,
            });
        }
    }
    out
}

/// The DNS fixtures: listed TXT names have their records, `[temperror] names` fail, everything else is
/// NXDOMAIN (absent from the map is treated as not found by the runner).
pub fn zones() -> DnsAnswers {
    let text = std::fs::read_to_string(corpus_dir().join("dns/zones.toml")).unwrap_or_default();
    let table: toml::Table = text.parse().unwrap_or_default();
    let mut answers = DnsAnswers::new();
    if let Some(txt) = table.get("txt").and_then(toml::Value::as_table) {
        for (name, records) in txt {
            let records = records
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|r| r.as_str().map(|s| s.as_bytes().to_vec()))
                        .collect()
                })
                .unwrap_or_default();
            answers.insert(name.clone(), TxtAnswer::Records(records));
        }
    }
    if let Some(names) = table
        .get("temperror")
        .and_then(|t| t.get("names"))
        .and_then(toml::Value::as_array)
    {
        for n in names.iter().filter_map(toml::Value::as_str) {
            answers.insert(n.to_owned(), TxtAnswer::TempError);
        }
    }
    answers
}

/// The answers for one message: the fixtures, with every other name it needs answered NXDOMAIN.
pub fn answers_for(raw: &[u8], from_domain: Option<&str>, zones: &DnsAnswers) -> DnsAnswers {
    let mut a = zones.clone();
    for name in pylota_identity_core::auth::prefetch_names(raw, from_domain) {
        a.entry(name).or_insert(TxtAnswer::NotFound);
    }
    a
}

/// Domains allowed in corpus addresses (RFC 2606 and `.invalid`).
fn allowed_domain(d: &str) -> bool {
    let d = d.trim_end_matches('.').to_ascii_lowercase();
    ["example", "invalid", "test", "localhost"]
        .iter()
        .any(|tld| d == *tld || d.ends_with(&format!(".{tld}")))
        || ["example.com", "example.net", "example.org"]
            .iter()
            .any(|e| d == *e || d.ends_with(&format!(".{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Covers: B2, B4, B5, B6, B7, B9, B13
    #[test]
    fn every_case_has_expectations() {
        let cases = cases();
        assert!(
            cases.len() >= 300,
            "{} cases; the M2 corpus has at least 300",
            cases.len()
        );
        for c in &cases {
            let id = c.name.rsplit('/').next().unwrap_or("");
            assert_eq!(
                c.meta.get("id").and_then(toml::Value::as_str),
                Some(id),
                "{}: id",
                c.name
            );
            for key in ["edge", "source", "licence"] {
                assert!(c.meta.contains_key(key), "{}: {key}", c.name);
            }
            assert!(
                c.meta.get("expect").is_some_and(toml::Value::is_table),
                "{}: [expect]",
                c.name
            );
            let source = c.meta["source"].as_str().unwrap_or("");
            assert!(
                !source.starts_with("imported"),
                "{}: imported cases need THIRD_PARTY.md",
                c.name
            );
        }
    }

    /// Covers: B2
    #[test]
    fn no_real_domains() {
        for c in cases() {
            // Quoted-printable soft line breaks would cut a domain in two.
            let text = String::from_utf8_lossy(&c.raw).replace("=\r\n", "");
            let bytes = text.as_bytes();
            for (i, _) in text.match_indices('@') {
                let end = bytes[i + 1..]
                    .iter()
                    .position(|b| !(b.is_ascii_alphanumeric() || *b == b'.' || *b == b'-'))
                    .map_or(bytes.len(), |p| i + 1 + p);
                let domain = &text[i + 1..end];
                if domain.contains('.') && !domain.starts_with("xn--") {
                    assert!(
                        allowed_domain(domain),
                        "{}: address domain {domain}",
                        c.name
                    );
                }
            }
        }
    }
}
