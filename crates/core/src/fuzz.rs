//! The fuzz harnesses ([Testing § 8]): each takes arbitrary bytes, calls one entry point and asserts the
//! target's invariants beyond "no panic". `fuzz/` calls them from `fuzz_target!`, and
//! `core::fuzz_regressions::replay_all` replays committed crash inputs through the same functions.
//!
//! [Testing § 8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/testing.md
#![doc(hidden)]

use crate::{address, classify, mime, sanitize, text};

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
    let _ = address::fold(&s);
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
