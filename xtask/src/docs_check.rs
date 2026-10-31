//! `cargo xtask docs-check` (Rust workspace § 10, the `docs` job):
//!
//! - the forbidden claims of Vault § 1.2 and Launch readiness SITE-2, sentence by sentence over
//!   `docs/src` and the site's copy: "zero-knowledge", "zero knowledge" or "end-to-end" without an
//!   earlier negation; "we cannot see your passwords" outside double quotes; "open source" (or
//!   "open-source") without an earlier negation, unless it names other software or a licence
//!   ("open-source projects", "an open-source licence"). Code spans, HTML attributes and comments are
//!   not copy;
//! - the links of the site's own pages (`site/public/*.html`): every local `href` and `src` names a
//!   file of `site/public` (after `mdbook build docs`), and every `#fragment` an `id` in it.

use std::path::{Path, PathBuf};

const NEGATIONS: &[&str] = &["not", "no", "never", "must not", "nor", "isn't", "aren't"];
/// What may follow "open source" when it describes something else.
const OTHERS: &[&str] = &[
    "project",
    "projects",
    "licence",
    "licences",
    "license",
    "licenses",
    "package",
    "packages",
    "agent vault",
    "library",
    "libraries",
    "tool",
    "tools",
    "software",
];

/// The text of an HTML page without tags, comments, scripts and styles.
fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let skip_to = |end: &str| tail.find(end).map(|j| j + end.len()).unwrap_or(tail.len());
        let n = if tail.starts_with("<!--") {
            skip_to("-->")
        } else if tail.starts_with("<script") {
            skip_to("</script>")
        } else if tail.starts_with("<style") {
            skip_to("</style>")
        } else if tail.starts_with("<code") {
            skip_to("</code>")
        } else {
            out.push(' ');
            skip_to(">")
        };
        rest = &tail[n..];
    }
    out.push_str(rest);
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
}

/// Markdown without fenced blocks and code spans.
fn md_text(md: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut code = false;
        let mut text = String::new();
        for (i, part) in line.split('`').enumerate() {
            if i > 0 {
                code = !code;
            }
            if !code {
                text.push_str(part);
            }
        }
        // A link's target is not copy: `[text](target)` keeps its text.
        let mut rest = text.as_str();
        while let Some(i) = rest.find("](") {
            out.push_str(&rest[..=i]);
            let after = &rest[i + 2..];
            rest = after.find(')').map_or("", |j| &after[j + 1..]);
        }
        out.push_str(rest);
        out.push('\n');
    }
    out
}

/// Sentences: paragraphs (and table cells) split at `.`, `?` or `!` followed by a space.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for block in text.split("\n\n").flat_map(|b| b.split('|')) {
        let flat = block.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut cur = String::new();
        let chars: Vec<char> = flat.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            cur.push(*c);
            if matches!(c, '.' | '?' | '!') && chars.get(i + 1).is_none_or(|n| *n == ' ') {
                out.push(std::mem::take(&mut cur));
            }
        }
        if !cur.trim().is_empty() {
            out.push(cur);
        }
    }
    out
}

/// The sentence without its double-quoted parts.
fn unquoted(s: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for c in s.chars() {
        if matches!(c, '"' | '“' | '”') {
            quoted = !quoted;
            continue;
        }
        if !quoted {
            out.push(c);
        }
    }
    out
}

fn negated_before(lower: &str, at: usize) -> bool {
    let before = &lower[..at];
    NEGATIONS.iter().any(|n| {
        before.match_indices(n).any(|(i, _)| {
            let b = before.as_bytes();
            (i == 0 || !b[i - 1].is_ascii_alphanumeric())
                && b.get(i + n.len())
                    .is_none_or(|c| !c.is_ascii_alphanumeric())
        })
    })
}

/// Why `sentence` is a forbidden claim, if it is one.
pub fn claim(sentence: &str) -> Option<&'static str> {
    if sentence
        .to_lowercase()
        .contains("we cannot see your passwords")
        && unquoted(sentence)
            .to_lowercase()
            .contains("we cannot see your passwords")
    {
        return Some("\"we cannot see your passwords\" outside double quotes");
    }
    let lower = unquoted(sentence).to_lowercase();
    for term in [
        "zero-knowledge",
        "zero knowledge",
        "end-to-end encrypt",
        "end to end encrypt",
    ] {
        if let Some(at) = lower.find(term)
            && !negated_before(&lower, at)
        {
            return Some("a zero-knowledge or end-to-end claim without a negation");
        }
    }
    for term in ["open source", "open-source"] {
        for (at, _) in lower.match_indices(term) {
            let after = lower[at + term.len()..].trim_start();
            let names_other = OTHERS.iter().any(|o| {
                after.starts_with(o)
                    && after[o.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric())
            });
            if !names_other && !negated_before(&lower, at) {
                return Some(
                    "\"open source\" as a description (it is source available, Fair Source)",
                );
            }
        }
    }
    None
}

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

pub fn forbidden_claims(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    // The user-facing docs; the project's design pages discuss these claims (non-goals, rejected
    // alternatives, this rule's own wording) and are not copy.
    let mut md = Vec::new();
    files(&root.join("docs/src"), "md", &mut md);
    md.retain(|p| !p.starts_with(root.join("docs/src/project")));
    let mut html: Vec<PathBuf> = std::fs::read_dir(root.join("site/public"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "html"))
        .collect();
    md.sort();
    html.sort();
    let pages = md
        .iter()
        .map(|p| (p, md_text(&std::fs::read_to_string(p).unwrap_or_default())))
        .chain(html.iter().map(|p| {
            (
                p,
                html_text(&std::fs::read_to_string(p).unwrap_or_default()),
            )
        }));
    for (path, text) in pages {
        for s in sentences(&text) {
            if let Some(why) = claim(&s) {
                let rel = path.strip_prefix(root).unwrap_or(path).display();
                found.push(format!("{rel}: {why}: {}", s.trim()));
            }
        }
    }
    found
}

/// The `id`s of a page.
fn ids(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in [" id=\"", " name=\""] {
        for (i, _) in html.match_indices(attr) {
            let v = &html[i + attr.len()..];
            if let Some(end) = v.find('"') {
                out.push(v[..end].to_owned());
            }
        }
    }
    out
}

/// The broken local links of the site's own pages.
pub fn site_links(root: &Path) -> Vec<String> {
    let public = root.join("site/public");
    let mut broken = Vec::new();
    let mut pages: Vec<PathBuf> = std::fs::read_dir(&public)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "html"))
        .collect();
    pages.sort();
    for page in pages {
        let html = std::fs::read_to_string(&page).unwrap_or_default();
        let own = ids(&html);
        for attr in ["href=\"", "src=\""] {
            for (i, _) in html.match_indices(attr) {
                let v = &html[i + attr.len()..];
                let Some(end) = v.find('"') else { continue };
                let link = &v[..end];
                if link.is_empty()
                    || link.contains("://")
                    || link.starts_with("mailto:")
                    || link.starts_with("data:")
                    || link.starts_with("//")
                {
                    continue;
                }
                let link = link.split('?').next().unwrap_or(link);
                let (path, frag) = match link.split_once('#') {
                    Some((p, f)) => (p, Some(f)),
                    None => (link, None),
                };
                let page_name = page.file_name().unwrap().to_string_lossy();
                let target = if path.is_empty() {
                    page.clone()
                } else if let Some(abs) = path.strip_prefix('/') {
                    let t = public.join(abs);
                    if abs.is_empty() || abs.ends_with('/') {
                        t.join("index.html")
                    } else {
                        t
                    }
                } else {
                    page.parent().unwrap().join(path)
                };
                let target = if target.is_dir() {
                    target.join("index.html")
                } else {
                    target
                };
                if !target.is_file() {
                    broken.push(format!("{page_name}: {link}: no such file"));
                    continue;
                }
                if let Some(f) = frag.filter(|f| !f.is_empty()) {
                    let has = if target == page {
                        own.iter().any(|x| x == f)
                    } else {
                        ids(&std::fs::read_to_string(&target).unwrap_or_default())
                            .iter()
                            .any(|x| x == f)
                    };
                    if !has {
                        broken.push(format!("{page_name}: {link}: no element with that id"));
                    }
                }
            }
        }
    }
    broken.sort();
    broken.dedup();
    broken
}

pub fn run(root: &Path) -> Result<(), String> {
    let mut failures = forbidden_claims(root);
    if !root.join("site/public/docs/index.html").is_file() {
        failures.push("site/public/docs is not built: run mdbook build docs first".into());
    } else {
        failures.extend(site_links(root));
    }
    if failures.is_empty() {
        println!("docs-check: no forbidden claim, every site link resolves");
        Ok(())
    } else {
        Err(format!("docs-check failed:\n  {}", failures.join("\n  ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `xtask::docs_forbidden_claims` (Vault § 1.2, U69, SITE-2): claims of zero knowledge or end-to-end
    /// encryption without a negation, "we cannot see your passwords" outside quotes, and "open source"
    /// describing the product are refused; denials, quotations and other software pass; the docs and
    /// the site's copy hold none.
    #[test]
    fn docs_forbidden_claims() {
        for bad in [
            "The vault is zero-knowledge.",
            "Values are end-to-end encrypted between you and the site.",
            "Agents get end-to-end encryption.",
            "We use zero knowledge proofs for everything.",
            "Relax, we cannot see your passwords.",
            "Pylota Identity is open source.",
            "Is it open source?",
            "An open-source email service for agents.",
        ] {
            assert!(claim(bad).is_some(), "passed: {bad}");
        }
        for good in [
            "The vault is server-custody: it is not zero-knowledge and not end-to-end encrypted.",
            "No claim of zero knowledge or end-to-end encryption is made.",
            "Never say \"we cannot see your passwords\" in copy.",
            "FSL is not an OSI-approved open-source licence.",
            "Each release becomes Apache-2.0, a standard open-source licence, two years after it ships.",
            "Test fixtures from open-source projects under Apache-2.0 or MIT.",
            "Public copy never says \"open source\".",
            "It is Fair Source: the code is public.",
            "The live end-to-end suite passes on staging.",
        ] {
            assert!(claim(good).is_none(), "refused: {good}");
        }
        assert_eq!(
            sentences("A one. B two? C three"),
            ["A one.", " B two?", " C three"]
        );
        assert!(
            !html_text("<a id=\"open-source\" href=\"#x\">Self-host</a><!-- Open source -->")
                .contains("pen")
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let found = forbidden_claims(root);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn site_links_resolve() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        if root.join("site/public/docs/index.html").is_file() {
            let broken = site_links(root);
            assert!(broken.is_empty(), "{broken:#?}");
        }
    }
}
