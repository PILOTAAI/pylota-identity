//! Replays the minimised crash inputs committed under `tests/fuzz_regressions/<target>/` through the
//! fuzz harnesses (Testing § 8), so a fixed crash stays fixed in the normal test run.

use std::path::Path;

use pylota_identity_core::fuzz;

/// Covers: B2
#[test]
fn replay_all() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz_regressions");
    let targets: [(&str, fn(&[u8])); 4] = [
        ("mime_parse", fuzz::mime_parse),
        ("address_parse", fuzz::address_parse),
        ("sanitize", fuzz::sanitize),
        ("dsn_parse", fuzz::dsn_parse),
    ];
    for (name, harness) in targets {
        let Ok(entries) = std::fs::read_dir(root.join(name)) else {
            continue;
        };
        for e in entries.flatten() {
            harness(&std::fs::read(e.path()).unwrap());
        }
    }
    // Every committed corpus message passes every harness.
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/corpus");
    for dir in std::fs::read_dir(corpus).unwrap().flatten() {
        for e in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if e.path().extension().is_some_and(|x| x == "eml") {
                let raw = std::fs::read(e.path()).unwrap();
                for (_, harness) in targets {
                    harness(&raw);
                }
            }
        }
    }
}
