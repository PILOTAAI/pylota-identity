//! `cargo xtask gen-unicode [--check]`: regenerates `crates/core/src/address/confusables_table.rs` from
//! the Unicode 18.0.0 files pinned in `crates/core/data/` ([Identities › Confusable detection]). With
//! `--check` it fails when the checked-in table differs from what the files produce.
//!
//! [Identities › Confusable detection]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use sha2::{Digest as _, Sha256};

/// The files and their SHA-256, as downloaded from `https://www.unicode.org/Public/18.0.0/` on 2026-10-27.
const FILES: [(&str, &str); 8] = [
    (
        "confusables.txt",
        "6ed3ee967c9dfdf6677d563c9985182fbc50a2efb7d6059cd57b2e2ce18f5b92",
    ),
    (
        "DerivedCoreProperties.txt",
        "09c928886a178fcafd93c29e4bd59073a058e5a100b716d425cb563ab50f68c9",
    ),
    (
        "ScriptExtensions.txt",
        "5c9d34a922f687726f2a8bcf57d49f905987e51f1b21b58c95a00fbe255cec23",
    ),
    (
        "Scripts.txt",
        "0071fd81b6aeae25f6e8bce8efec3066a6476a91b49bdb2f52dc76e817862a6a",
    ),
    (
        "DerivedBidiClass.txt",
        "d9e23222522551348ea1ccfbb4f62efbf98982afb95840f8959c08ed992c5607",
    ),
    (
        "CaseFolding.txt",
        "a004797658a457bec4dc11683e39f69249ea3b595b752dbea6721c4c9f587b0d",
    ),
    (
        "PropertyValueAliases.txt",
        "06c4c8eaf7b0bf34abe73b113da1215bd784ac254d4c223600b90267caa4bbbd",
    ),
    (
        "emoji-data.txt",
        "80d00f8e616a0ef27fd6b8de3b758c06383b5d917e2977709578e68baf733bf1",
    ),
];

const OUT: &str = "crates/core/src/address/confusables_table.rs";

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let data = root.join("crates/core/data");
    let mut text = BTreeMap::new();
    for (name, sha) in FILES {
        let bytes = std::fs::read(data.join(name)).map_err(|e| format!("{name}: {e}"))?;
        let got: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != sha {
            return Err(format!("{name}: SHA-256 {got}, pinned {sha}"));
        }
        text.insert(
            name,
            String::from_utf8(bytes).map_err(|e| format!("{name}: {e}"))?,
        );
    }
    let table = generate(&text)?;
    let path = root.join(OUT);
    if check {
        let current = std::fs::read_to_string(&path).map_err(|e| format!("{OUT}: {e}"))?;
        if current != table {
            return Err(format!("{OUT} is stale: run `cargo xtask gen-unicode`"));
        }
        println!("gen-unicode: {OUT} is current");
    } else {
        std::fs::write(&path, table).map_err(|e| format!("{OUT}: {e}"))?;
        println!("gen-unicode: wrote {OUT}");
    }
    Ok(())
}

/// Data lines: the part before `#`, split on `;`, trimmed; blank lines dropped.
fn rows(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.lines().filter_map(|l| {
        let l = l.split('#').next().unwrap_or("").trim();
        (!l.is_empty()).then(|| l.split(';').map(str::trim).collect())
    })
}

fn range(field: &str) -> Result<(u32, u32), String> {
    let hex = |s: &str| u32::from_str_radix(s, 16).map_err(|e| format!("{s}: {e}"));
    match field.split_once("..") {
        Some((a, b)) => Ok((hex(a)?, hex(b)?)),
        None => Ok((hex(field)?, hex(field)?)),
    }
}

fn chars(field: &str) -> Result<String, String> {
    field
        .split_whitespace()
        .map(|h| {
            u32::from_str_radix(h, 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("bad code point {h}"))
        })
        .collect()
}

/// Ranges of code points with a property, merged.
fn merge(mut v: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    v.sort();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn generate(t: &BTreeMap<&str, String>) -> Result<String, String> {
    // Confusables whose prototype is entirely ASCII.
    let mut confusables = Vec::new();
    for r in rows(&t["confusables.txt"]) {
        let source = chars(r[0])?;
        let target = chars(r[1])?;
        if target.is_ascii() {
            let c = source.chars().next().ok_or("empty source")? as u32;
            confusables.push((c, target));
        }
    }
    confusables.sort();

    let mut ignorable = Vec::new();
    for r in rows(&t["DerivedCoreProperties.txt"]) {
        if r[1] == "Default_Ignorable_Code_Point" {
            ignorable.push(range(r[0])?);
        }
    }

    let mut pictographic = Vec::new();
    for r in rows(&t["emoji-data.txt"]) {
        if r[1] == "Extended_Pictographic" {
            pictographic.push(range(r[0])?);
        }
    }

    let mut rtl = Vec::new();
    for r in rows(&t["DerivedBidiClass.txt"]) {
        if r[1] == "R" || r[1] == "AL" {
            rtl.push(range(r[0])?);
        }
    }

    // Full case folding: statuses C and F.
    let mut folding = Vec::new();
    for r in rows(&t["CaseFolding.txt"]) {
        if r[1] == "C" || r[1] == "F" {
            let (c, _) = range(r[0])?;
            folding.push((c, chars(r[2])?));
        }
    }
    folding.sort();

    // Script names to short codes; then Script_Extensions over Script, augmented (UTS #39 § 5.1).
    let mut short = BTreeMap::new();
    for r in rows(&t["PropertyValueAliases.txt"]) {
        if r[0] == "sc" {
            short.insert(r[2].to_owned(), r[1].to_owned());
        }
    }
    let mut script: Vec<Option<BTreeSet<String>>> = vec![None; 0x11_0000];
    let unknown: BTreeSet<String> = ["Zzzz".to_owned()].into();
    for r in rows(&t["Scripts.txt"]) {
        let (a, b) = range(r[0])?;
        let code = short
            .get(r[1])
            .ok_or_else(|| format!("unknown script {}", r[1]))?;
        for c in a..=b {
            script[c as usize] = Some([code.clone()].into());
        }
    }
    for r in rows(&t["ScriptExtensions.txt"]) {
        let (a, b) = range(r[0])?;
        let set: BTreeSet<String> = r[1].split_whitespace().map(str::to_owned).collect();
        for c in a..=b {
            script[c as usize] = Some(set.clone());
        }
    }
    let augment = |set: &BTreeSet<String>| -> BTreeSet<String> {
        let mut out = set.clone();
        for s in set {
            let extra: &[&str] = match s.as_str() {
                "Hani" => &["Hanb", "Jpan", "Kore"],
                "Hira" | "Kana" => &["Jpan"],
                "Hang" => &["Kore"],
                "Bopo" => &["Hanb"],
                _ => &[],
            };
            out.extend(extra.iter().map(|e| (*e).to_owned()));
        }
        out
    };
    // Set 0 means every script (Common and Inherited).
    let mut names: Vec<String> = Vec::new();
    let mut sets: Vec<Vec<u8>> = vec![Vec::new()];
    let mut set_index: BTreeMap<Vec<u8>, u16> = BTreeMap::new();
    let mut script_ranges: Vec<(u32, u32, u16)> = Vec::new();
    for (c, s) in script.iter().enumerate() {
        let s = s.as_ref().unwrap_or(&unknown);
        let idx = if s.contains("Zyyy") || s.contains("Zinh") {
            0
        } else {
            let mut ids: Vec<u8> = augment(s)
                .iter()
                .map(|n| {
                    let i = names.iter().position(|x| x == n).unwrap_or_else(|| {
                        names.push(n.clone());
                        names.len() - 1
                    });
                    u8::try_from(i).expect("fewer than 256 scripts")
                })
                .collect();
            ids.sort_unstable();
            *set_index.entry(ids.clone()).or_insert_with(|| {
                sets.push(ids);
                u16::try_from(sets.len() - 1).expect("fewer than 65536 sets")
            })
        };
        let c = c as u32;
        match script_ranges.last_mut() {
            Some(last) if last.2 == idx && last.1 + 1 == c => last.1 = c,
            _ => script_ranges.push((c, c, idx)),
        }
    }

    let mut o = String::new();
    o.push_str(
        "// @generated by `cargo xtask gen-unicode` from crates/core/data (Unicode 18.0.0). Do not edit.\n\n",
    );
    let esc = |s: &str| s.escape_default().to_string();
    writeln!(
        o,
        "/// UTS #39 confusables whose prototype is ASCII, sorted by source."
    )
    .ok();
    writeln!(o, "pub static CONFUSABLES: &[(char, &str)] = &[").ok();
    for (c, target) in &confusables {
        writeln!(o, "    ('\\u{{{c:x}}}', \"{}\"),", esc(target)).ok();
    }
    o.push_str("];\n\n/// Full case folding (statuses C and F), sorted.\n");
    writeln!(o, "pub static CASE_FOLDING: &[(char, &str)] = &[").ok();
    for (c, target) in &folding {
        writeln!(o, "    ('\\u{{{c:x}}}', \"{}\"),", esc(target)).ok();
    }
    o.push_str("];\n\n");
    let ranges = |o: &mut String, name: &str, doc: &str, v: Vec<(u32, u32)>| {
        writeln!(o, "/// {doc}\npub static {name}: &[(u32, u32)] = &[").ok();
        for (a, b) in merge(v) {
            writeln!(o, "    (0x{a:x}, 0x{b:x}),").ok();
        }
        o.push_str("];\n\n");
    };
    ranges(
        &mut o,
        "DEFAULT_IGNORABLE",
        "`Default_Ignorable_Code_Point`.",
        ignorable,
    );
    ranges(&mut o, "STRONG_RTL", "Bidi classes R and AL.", rtl);
    ranges(
        &mut o,
        "EXTENDED_PICTOGRAPHIC",
        "`Extended_Pictographic` (emoji sequences keep their U+200D).",
        pictographic,
    );
    writeln!(
        o,
        "/// Script codes referenced by `SCRIPT_SETS` (tests and diagnostics)."
    )
    .ok();
    writeln!(o, "#[cfg_attr(not(test), allow(dead_code))]").ok();
    writeln!(o, "pub static SCRIPT_NAMES: &[&str] = &{names:?};\n").ok();
    writeln!(
        o,
        "/// Augmented script sets; set 0 is every script (Common, Inherited)."
    )
    .ok();
    writeln!(o, "pub static SCRIPT_SETS: &[&[u8]] = &[").ok();
    for s in &sets {
        writeln!(o, "    &{s:?},").ok();
    }
    o.push_str("];\n\n/// Code points to their script set, as sorted inclusive ranges.\n");
    writeln!(o, "pub static SCRIPTS: &[(u32, u32, u16)] = &[").ok();
    for (a, b, i) in script_ranges {
        writeln!(o, "    (0x{a:x}, 0x{b:x}, {i}),").ok();
    }
    o.push_str("];\n");
    Ok(o)
}
