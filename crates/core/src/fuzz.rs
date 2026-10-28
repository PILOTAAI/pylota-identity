//! The fuzz harnesses ([Testing § 8]): each takes arbitrary bytes, calls one entry point and asserts the
//! target's invariants beyond "no panic". `fuzz/` calls them from `fuzz_target!`, and
//! `core::fuzz_regressions::replay_all` replays committed crash inputs through the same functions.
//!
//! [Testing § 8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/testing.md
#![doc(hidden)]

use crate::{address, classify, mime, query, refs, sanitize, text};

/// `mime_parse`: depth and part caps hold in the output.
pub fn mime_parse(data: &[u8]) {
    let caps = mime::Caps::default();
    let p = mime::parse(data, &caps);
    assert!(
        p.attachments.len() <= caps.max_parts,
        "{} attachments",
        p.attachments.len()
    );
    if p.has(mime::Flag::Encrypted) {
        assert!(p.text.is_none() && p.html.is_none());
    }
    let h = mime::parse_headers_only(data);
    assert!(h.text.is_none() && h.attachments.is_empty());
}

/// `address_parse`: normalisation is idempotent; a validated username matches the stored form.
pub fn address_parse(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    if let Ok(e) = address::parse_envelope(&s) {
        let again = match &e.detail {
            Some(d) => format!("{}+{d}@{}", e.base_local, e.domain),
            None => e.base(),
        };
        assert_eq!(address::parse_envelope(&again).as_ref(), Ok(&e), "{again}");
    }
    for class in [address::DomainClass::Platform, address::DomainClass::Tenant] {
        if let Ok(u) = address::validate_username(&s, class, address::USERNAME_MAX, "", &[]) {
            let b = u.as_bytes();
            assert!((1..=24).contains(&b.len()), "{u}");
            assert!(b[0].is_ascii_lowercase() || b[0].is_ascii_digit(), "{u}");
            assert!(
                b.iter()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(c)),
                "{u}"
            );
        }
    }
    // Display names and domains reach `fold` capped at 256 characters.
    let _ = address::fold(&s.chars().take(256).collect::<String>());
}

/// The hidden code points of B11 that derived text must never contain.
fn hidden_char(c: char) -> bool {
    matches!(c, '\u{200b}' | '\u{200c}' | '\u{2060}'..='\u{2064}' | '\u{feff}' | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}' | '\u{e0000}'..='\u{e007f}')
}

/// `sanitize`: no script, no event handler, no remote source; idempotent; derived text free of hidden
/// characters.
pub fn sanitize(data: &[u8]) {
    let html = String::from_utf8_lossy(data);
    let (out, _) = sanitize::sanitize(&html);
    let lower = out.to_ascii_lowercase();
    assert!(!lower.contains("<script"), "{out}");
    assert!(
        !lower.contains("src=\"http") && !lower.contains("src=\"//"),
        "{out}"
    );
    for (i, _) in lower.match_indices(" on") {
        let rest = &lower[i + 3..];
        let name_end = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        assert!(
            !(name_end > 0
                && rest[name_end..].starts_with('=')
                && lower[..i].rfind('<') > lower[..i].rfind('>')),
            "{out}"
        );
    }
    let (twice, _) = sanitize::sanitize(&out);
    assert_eq!(twice, out, "sanitize is not idempotent");
    let doc = sanitize::parse(&html);
    let (derived, _) = sanitize::strip_hidden_chars(&text::derive_text(&doc.body()));
    assert!(!derived.chars().any(hidden_char), "{derived:?}");
}

/// `dsn_parse`: the kind is one of the defined kinds, and DSN recipients are addresses or absent.
pub fn dsn_parse(data: &[u8]) {
    let d = mime::dsn::parse(&String::from_utf8_lossy(data));
    for r in &d.recipients {
        if let Some(a) = &r.final_recipient {
            assert_eq!(mime::normalise_address(a).as_ref(), Some(a), "{a}");
        }
    }
    assert!(d.recipients.len() <= mime::dsn::MAX_RECIPIENTS);
    let p = mime::parse(data, &mime::Caps::default());
    let c = classify::classify(&p, "");
    assert_eq!(c.automated, c.kind.automated());
    if let Some(d) = c.dsn {
        for r in &d.recipients {
            if let Some(a) = &r.final_recipient {
                assert!(a.contains('@'), "{a}");
            }
        }
    }
}

/// Whether an FTS5 expression holds only quoted strings (an optional ` *` after one), `AND`, `OR`,
/// `NOT`, balanced parentheses and `{column} :` prefixes.
fn fts_is_quoted(m: &str) -> bool {
    let cs: Vec<char> = m.chars().collect();
    let (mut i, mut depth) = (0usize, 0i32);
    while i < cs.len() {
        match cs[i] {
            ' ' => i += 1,
            '(' => {
                depth += 1;
                i += 1;
            }
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
                i += 1;
            }
            '"' => {
                i += 1;
                loop {
                    match cs.get(i) {
                        None => return false,
                        Some('"') if cs.get(i + 1) == Some(&'"') => i += 2,
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some(_) => i += 1,
                    }
                }
                if cs.get(i..i + 2) == Some(&[' ', '*']) {
                    i += 2;
                }
            }
            '{' => {
                let Some(end) = cs[i..].iter().position(|&c| c == '}') else {
                    return false;
                };
                if !cs[i + 1..i + end]
                    .iter()
                    .all(|c| c.is_ascii_lowercase() || *c == '_' || *c == ' ')
                {
                    return false;
                }
                i += end + 1;
                if cs.get(i..i + 2) == Some(&[' ', ':']) {
                    i += 2;
                } else if cs.get(i) == Some(&':') {
                    i += 1;
                } else {
                    return false;
                }
            }
            _ => {
                let word: String = cs[i..]
                    .iter()
                    .take_while(|c| c.is_ascii_uppercase())
                    .collect();
                if !matches!(word.as_str(), "AND" | "OR" | "NOT") {
                    return false;
                }
                i += word.len();
            }
        }
    }
    depth == 0
}

/// `query_parse`: every compiled FTS expression quotes every term; errors carry a position inside the
/// input.
pub fn query_parse(data: &[u8]) {
    let q = String::from_utf8_lossy(data);
    match query::parse(&q, &query::ParseCtx::default()) {
        Ok(tree) => {
            let ctx = query::CompileCtx {
                tz: query::Tz::Europe__London,
                now_ms: 1_793_138_400_000,
                ref_packs: &[refs::Pack::Core, refs::Pack::UkVehicle],
                custom_refs: &[],
                quarantine_review: true,
            };
            let c = query::compile(&tree, &ctx);
            let mut matches: Vec<&str> = [c.fts_match.as_deref(), c.tri_match.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            // And every `MATCH ?` parameter of the SQL filter.
            let sql = &c.filter.sql;
            for (idx, (at, _)) in sql.match_indices('?').enumerate() {
                if sql[..at].ends_with("MATCH ")
                    && let Some(query::SqlParam::Text(t)) = c.filter.params.get(idx)
                {
                    matches.push(t);
                }
            }
            for m in matches {
                assert!(fts_is_quoted(m), "{m:?} from {q:?}");
            }
        }
        Err(e) => assert!(e.position as usize <= q.chars().count(), "{e:?}"),
    }
}
