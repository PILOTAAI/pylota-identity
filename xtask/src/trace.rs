//! `cargo xtask trace`: the edge-case register and the PRD against the test names and `Covers:`
//! lines of the source tree (Testing § 11.2).
//!
//! Until `MILESTONES` lists `M20`, only landed milestones are checked: every test their acceptance
//! names (directly, through a `*` wildcard, or through an edge row it lists) must exist. Rows and
//! names of later milestones are reported as pending. With `M20` listed the full check runs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const PLAN: &str = "docs/src/project/build-plan.md";
const REGISTER: &str = "docs/src/project/edge-cases.md";
const PRD: &str = "docs/src/project/prd.md";
/// Its § 2 table lists the security invariants (`SEC-1` …), which tests may cover like requirements.
const SECURITY: &str = "docs/src/project/design/security.md";
const DESIGN: &str = "docs/src/project/design";
const TS_INVENTORY: &str = "sdks/typescript-tests.json";

/// Test-name prefixes (Testing § 11.1).
const PREFIXES: &[&str] = &[
    "core",
    "conf",
    "it",
    "live",
    "cli",
    "platform",
    "sdk",
    "xtask",
    "browser",
    "ts",
    "eval",
    "worker",
    "api_types",
];

pub fn run(root: &Path) -> Result<(), String> {
    let report = check(root)?;
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .map_err(|e| format!("GITHUB_STEP_SUMMARY: {e}"))?;
        writeln!(f, "{}\n\n{}", report.summary, report.matrix)
            .map_err(|e| format!("GITHUB_STEP_SUMMARY: {e}"))?;
    }
    println!("{}", report.summary);
    if report.failures.is_empty() {
        Ok(())
    } else {
        Err(format!("trace failed:\n  {}", report.failures.join("\n  ")))
    }
}

pub struct Report {
    pub failures: Vec<String>,
    /// One line per required test, then the names not checkable here (`ts::` without an inventory
    /// entry, `eval::`), which are pending and never fail.
    pub summary: String,
    /// The traceability matrix: every register row with its milestone and status.
    pub matrix: String,
}

struct Milestone {
    id: String,
    /// Its `**Implements:**` paragraph and acceptance; `None` when the section has neither (M1).
    acceptance: Option<String>,
}

struct Row {
    id: String,
    owner: String,
    tests: Vec<String>,
}

/// What a milestone's acceptance names.
#[derive(Default)]
struct Named {
    tests: Vec<String>,
    rows: Vec<String>,
    /// Prefixes from "every `core::` row": the rows' tests required here are only those under one
    /// of them, plus the rows' `conf::` corpus cases; the rest belong to the milestones that build
    /// what they exercise.
    row_prefixes: Vec<String>,
}

/// Whether a row's test is required by an acceptance whose row scope is `prefixes`.
fn row_test_in_scope(test: &str, prefixes: &[String]) -> bool {
    prefixes.is_empty()
        || test.starts_with("conf::")
        || prefixes.iter().any(|p| test.starts_with(p.as_str()))
}

#[derive(Default)]
struct Tests {
    /// Test name → its `Covers:` IDs.
    names: BTreeMap<String, Vec<String>>,
}

pub fn check(root: &Path) -> Result<Report, String> {
    let read =
        |rel: &str| std::fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"));
    let plan = parse_plan(&read(PLAN)?);
    let register = parse_register(&read(REGISTER)?);
    let mut requirements = parse_requirements(&read(PRD)?);
    // Absent only in the unit tests' synthetic trees.
    requirements.extend(parse_invariants(&read(SECURITY).unwrap_or_default()));
    let row_ids: BTreeSet<&str> = register.iter().map(|r| r.id.as_str()).collect();
    let tests = collect_tests(root)?;
    let ts = read_ts_inventory(root)?;

    let mut failures = Vec::new();
    let landed = read_milestones(root, &plan, &mut failures)?;
    let full = landed.iter().any(|m| m == "M20");

    let named: BTreeMap<&str, Named> = plan
        .iter()
        .map(|m| {
            (
                m.id.as_str(),
                m.acceptance
                    .as_deref()
                    .map(|a| named_in(a, m.id.split(' ').next().unwrap_or_default(), &row_ids))
                    .unwrap_or_default(),
            )
        })
        .collect();

    // Required names, each with the reason it is required.
    let mut required: BTreeMap<String, String> = BTreeMap::new();
    for m in &plan {
        if !is_landed(&m.id, &landed, full) {
            continue;
        }
        let n = &named[m.id.as_str()];
        for t in &n.tests {
            required
                .entry(t.clone())
                .or_insert_with(|| format!("{} acceptance", m.id));
        }
        for row in register
            .iter()
            .filter(|r| n.rows.contains(&r.id) && r.owner != "I")
        {
            for t in row
                .tests
                .iter()
                .filter(|t| row_test_in_scope(t, &n.row_prefixes))
            {
                required
                    .entry(t.clone())
                    .or_insert_with(|| format!("{} acceptance, row {}", m.id, row.id));
            }
        }
    }
    if full {
        for row in register.iter().filter(|r| r.owner != "I") {
            for t in &row.tests {
                required
                    .entry(t.clone())
                    .or_insert_with(|| format!("row {}", row.id));
            }
        }
        for (name, page) in design_table_tests(root)? {
            required.entry(name).or_insert(page);
        }
    }

    let mut pending_names = Vec::new();
    let mut checked = Vec::new();
    for (name, why) in &required {
        match exists(name, &tests, ts.as_ref()) {
            Exists::Yes => checked.push(format!("- ok `{name}` ({why})")),
            Exists::Pending(kind) => {
                pending_names.push(format!("- pending ({kind}) `{name}` ({why})"))
            }
            Exists::No => {
                checked.push(format!("- missing `{name}` ({why})"));
                failures.push(format!("missing test `{name}` ({why})"));
            }
        }
    }

    // Every Covers: line names known rows or requirements, here and in the TypeScript inventory.
    let known = |id: &str| row_ids.contains(id) || requirements.contains(id);
    let ts_names = ts.as_ref().map(|t| &t.names);
    for (name, covers) in tests.names.iter().chain(ts_names.into_iter().flatten()) {
        for id in covers.iter().filter(|id| !known(id)) {
            failures.push(format!(
                "`{name}` covers `{id}`, which is neither an edge row nor a requirement"
            ));
        }
    }

    if full {
        let covered: BTreeSet<&str> = tests
            .names
            .values()
            .chain(ts_names.into_iter().flat_map(|t| t.values()))
            .flatten()
            .map(String::as_str)
            .collect();
        for r in requirements
            .iter()
            .filter(|r| !covered.contains(r.as_str()))
        {
            failures.push(format!("requirement {r} has no test with it in `Covers:`"));
        }
    }

    let matrix = matrix(&plan, &register, &named, &landed, full, &tests, ts.as_ref());
    let pending_rows = matrix
        .lines()
        .filter(|l| l.ends_with("| pending |"))
        .count();
    let mut summary = format!(
        "trace: {} mode, landed {}; {} required tests, {} pending names, {} register rows pending; {} failures",
        if full { "full" } else { "landed-milestone" },
        if landed.is_empty() {
            "nothing".to_owned()
        } else {
            landed.join(" ")
        },
        required.len(),
        pending_names.len(),
        pending_rows,
        failures.len()
    );
    for line in checked.iter().chain(&pending_names) {
        summary.push('\n');
        summary.push_str(line);
    }
    Ok(Report {
        failures,
        summary,
        matrix,
    })
}

fn read_milestones(
    root: &Path,
    plan: &[Milestone],
    failures: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    let text =
        std::fs::read_to_string(root.join("MILESTONES")).map_err(|e| format!("MILESTONES: {e}"))?;
    let mut landed = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !plan.iter().any(|m| m.id == line) {
            failures.push(format!(
                "MILESTONES lists `{line}`, which is not a build-plan milestone"
            ));
        } else if landed.iter().any(|l| l == line) {
            failures.push(format!("MILESTONES lists `{line}` twice"));
        } else {
            landed.push(line.to_owned());
        }
    }
    Ok(landed)
}

/// The build plan's milestones, in plan order. Each `## M<n> · …` section names tests and rows in
/// its `**Implements:**` paragraph and its acceptance. A section whose acceptance is split into named
/// blocks (`**Acceptance, M17 Foundation**`) gives one unit per block, listed in `MILESTONES` by the
/// block's name, then the milestone itself, which stands for all of its blocks.
fn parse_plan(text: &str) -> Vec<Milestone> {
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    let mut current: Option<usize> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            current = None;
            let id = rest.split_whitespace().next().unwrap_or_default();
            if id.len() > 1 && id.starts_with('M') && id[1..].bytes().all(|b| b.is_ascii_digit()) {
                sections.push((id.to_owned(), Vec::new()));
                current = Some(sections.len() - 1);
            }
        } else if let Some(i) = current {
            sections[i].1.push(line);
        }
    }
    let mut units = Vec::new();
    for (id, lines) in sections {
        let implements: Vec<&str> =
            match lines.iter().position(|l| l.starts_with("**Implements:**")) {
                Some(start) => lines[start..]
                    .iter()
                    .take_while(|l| !l.trim().is_empty())
                    .copied()
                    .collect(),
                None => Vec::new(),
            };
        let starts: Vec<usize> = (0..lines.len())
            .filter(|&i| lines[i].starts_with("**Acceptance"))
            .collect();
        let mut all = implements.join("\n");
        for (k, &start) in starts.iter().enumerate() {
            let end = starts.get(k + 1).copied().unwrap_or(lines.len());
            let block = lines[start..end].join("\n");
            let label = lines[start]
                .strip_prefix("**Acceptance, ")
                .and_then(|rest| rest.split("**").next())
                .map(str::trim)
                .filter(|l| !l.is_empty());
            if let Some(label) = label {
                units.push(Milestone {
                    id: label.to_owned(),
                    acceptance: Some(block.clone()),
                });
            }
            all.push('\n');
            all.push_str(&block);
        }
        let named_anything = !implements.is_empty() || !starts.is_empty();
        units.push(Milestone {
            id,
            acceptance: named_anything.then_some(all),
        });
    }
    units
}

/// The register's table rows: `| ID | case | behaviour | owner | tests |`.
fn parse_register(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for line in text.lines().filter(|l| l.starts_with('|')) {
        let cells = split_cells(line);
        let Some(id) = cells.first().map(|c| c.trim()) else {
            continue;
        };
        if cells.len() < 3 || !is_row_shape(id) {
            continue;
        }
        let owner = cells[cells.len() - 2].trim().to_owned();
        let tests = code_spans(cells[cells.len() - 1])
            .into_iter()
            .filter(|s| is_test_name(s))
            .map(str::to_owned)
            .collect();
        rows.push(Row {
            id: id.to_owned(),
            owner,
            tests,
        });
    }
    rows
}

/// Requirement IDs the PRD defines: `**FR-…-n**` items and `| NFR-…-n |` table rows.
fn parse_requirements(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim_start();
        let candidate = if let Some(rest) = line.strip_prefix("- **") {
            rest.split("**").next()
        } else if let Some(rest) = line.strip_prefix("| ") {
            rest.split(" |").next()
        } else {
            None
        };
        if let Some(id) = candidate.map(str::trim).filter(|id| is_requirement(id)) {
            out.insert(id.to_owned());
        }
    }
    out
}

/// The `| SEC-n |` rows of the security invariants table.
fn parse_invariants(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("| SEC-"))
        .filter_map(|rest| rest.split(" |").next())
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .map(|n| format!("SEC-{n}"))
        .collect()
}

fn is_requirement(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.len() == 3
        && (parts[0] == "FR" || parts[0] == "NFR")
        && !parts[1].is_empty()
        && parts[1].bytes().all(|b| b.is_ascii_uppercase())
        && !parts[2].is_empty()
        && parts[2].bytes().all(|b| b.is_ascii_digit())
}

/// One upper-case letter followed by digits: the shape of a register row ID.
fn is_row_shape(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase()) && {
        let rest = chars.as_str();
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
    }
}

fn is_test_name(s: &str) -> bool {
    let Some((prefix, rest)) = s.split_once("::") else {
        return false;
    };
    PREFIXES.contains(&prefix)
        && !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '*')
        && !rest.ends_with(':')
}

/// Drops the sentence that hands rows to later milestones ("Rows that need a later milestone are
/// accepted there: A7 in M9 (…), A13 in M7 …"): its rows and tests are required where it sends them.
fn drop_accepted_there(acceptance: &str) -> String {
    let text = acceptance.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some(start) = text.find("are accepted there:") else {
        return acceptance.to_owned();
    };
    let start = text[..start].rfind(". ").map_or(0, |i| i + 2);
    let rest = &text[start..];
    let end = rest
        .match_indices(". ")
        .find(|(i, _)| rest[i + 2..].starts_with(|c: char| c.is_ascii_uppercase()))
        .map_or(rest.len(), |(i, _)| i + 2);
    format!("{}{}", &text[..start], &rest[end..])
}

/// Whether `text` assigns the acceptance of row `id` to a milestone other than `own`.
fn deferred(text: &str, id: &str, own: &str) -> bool {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.as_str();
    let later = |rest: &str| {
        let m: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        m.starts_with('M') && m != own
    };
    text.match_indices(&format!("{id} is accepted in "))
        .any(|(i, s)| later(&text[i + s.len()..]))
        || text
            .match_indices(&format!(", which accepts {id}"))
            .any(|(i, _)| {
                let before = &text[..i];
                let m = before.rsplit(' ').next().unwrap_or("");
                m.starts_with('M') && m != own
            })
}

/// Splits a table row on `|` outside code spans; drops the empty edge cells.
fn split_cells(line: &str) -> Vec<&str> {
    let mut cells = Vec::new();
    let (mut start, mut in_code) = (0, false);
    for (i, c) in line.char_indices() {
        match c {
            '`' => in_code = !in_code,
            '|' if !in_code => {
                cells.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    cells.push(&line[start..]);
    if cells.first().is_some_and(|c| c.trim().is_empty()) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|c| c.trim().is_empty()) {
        cells.pop();
    }
    cells
}

fn code_spans(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

/// The tests and rows a milestone's acceptance names. Link targets are dropped, and so is any
/// parenthesis that names another milestone ("… so it belongs to M7."), because it assigns its
/// tests and rows elsewhere. Outside code spans, `D1` and `R2` alone are Cloudflare's database and
/// object store; they count as rows only inside a range such as `D1–D3`.
fn named_in(acceptance: &str, own: &str, row_ids: &BTreeSet<&str>) -> Named {
    let acceptance = &drop_accepted_there(acceptance);
    let text = drop_other_milestone_parens(&drop_link_targets(acceptance), own);
    let mut named = Named::default();
    for (i, _) in text.match_indices("every `") {
        let rest = &text[i + 7..];
        if let Some(end) = rest.find('`')
            && rest[..end].ends_with("::")
            && is_test_name(&format!("{}x", &rest[..end]))
            && rest[end + 1..].trim_start().starts_with("row")
        {
            named.row_prefixes.push(rest[..end].to_owned());
        }
    }
    for (i, chunk) in text.split('`').enumerate() {
        if i % 2 == 1 {
            if is_test_name(chunk) && !named.tests.iter().any(|t| t == chunk) {
                named.tests.push(chunk.to_owned());
            }
            continue;
        }
        let push = |id: String, named: &mut Named| {
            if row_ids.contains(id.as_str()) && !named.rows.contains(&id) {
                named.rows.push(id);
            }
        };
        let tokens = row_tokens(chunk);
        let mut k = 0;
        while k < tokens.len() {
            let (tok, joined_to_next) = &tokens[k];
            if *joined_to_next && k + 1 < tokens.len() {
                let end = &tokens[k + 1].0;
                if let Some(range) = expand_range(tok, end) {
                    for id in range {
                        push(id, &mut named);
                    }
                    k += 2;
                    continue;
                }
            }
            if *tok != "D1" && *tok != "R2" {
                push((*tok).to_owned(), &mut named);
            }
            k += 1;
        }
    }
    // A row the acceptance says another milestone accepts ("J13 is accepted in M9", "M6, which accepts
    // J21") is only partly run here: its tests are required by that milestone, and here only those
    // the acceptance names itself.
    let raw = drop_link_targets(acceptance);
    named.rows.retain(|id| !deferred(&raw, id, own));
    named
}

/// Row-shaped tokens in prose, each flagged when a dash (`–` or `-`) joins it to the next token.
fn row_tokens(text: &str) -> Vec<(&str, bool)> {
    let mut out: Vec<(&str, bool)> = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < text.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        if c.is_ascii_alphanumeric() {
            let start = i;
            while i < text.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let word = &text[start..i];
            if is_row_shape(word) {
                let rest = &text[i..];
                let joined = rest.starts_with('–') || (rest.starts_with('-') && !word.is_empty());
                out.push((word, joined));
            }
        } else {
            i += c.len_utf8();
        }
    }
    out
}

fn expand_range(start: &str, end: &str) -> Option<Vec<String>> {
    let (a, b) = (start.chars().next()?, end.chars().next()?);
    if a != b {
        return None;
    }
    let (from, to): (u32, u32) = (start[1..].parse().ok()?, end[1..].parse().ok()?);
    (from < to && to - from < 200).then(|| (from..=to).map(|n| format!("{a}{n}")).collect())
}

/// `[text](target)` → `[text]`.
fn drop_link_targets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        out.push_str(&rest[..=i]);
        let after = &rest[i + 2..];
        let mut depth = 1;
        let mut end = after.len();
        for (j, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = j + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &after[end.min(after.len())..];
    }
    out.push_str(rest);
    out
}

fn drop_other_milestone_parens(text: &str, own: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '(' {
            out.push(c);
            continue;
        }
        let mut depth = 1;
        let mut end = text.len();
        for (j, d) in chars.by_ref() {
            match d {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = j + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let group = &text[i..end];
        let names_other = group.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| {
            w.len() > 1
                && w.starts_with('M')
                && w[1..].bytes().all(|b| b.is_ascii_digit())
                && w != own
        });
        if !names_other {
            out.push_str(group);
        }
    }
    out
}

enum Exists {
    Yes,
    No,
    Pending(&'static str),
}

fn exists(name: &str, tests: &Tests, ts: Option<&Tests>) -> Exists {
    let matches = |set: &Tests| match name.strip_suffix('*') {
        Some(prefix) => set.names.keys().any(|n| n.starts_with(prefix)),
        None => set.names.contains_key(name),
    };
    if name.starts_with("ts::") {
        return if ts.is_some_and(matches) {
            Exists::Yes
        } else {
            Exists::Pending("SDK")
        };
    }
    if name.starts_with("eval::") {
        return Exists::Pending("evaluation");
    }
    if matches(tests) {
        Exists::Yes
    } else {
        Exists::No
    }
}

/// Every test in the tree: Rust test functions under `crates/*/{src,tests}` and `xtask/`, named by
/// prefix, module path and function (`xtask::` names are flat), and the corpus cases
/// `crates/conformance/corpus/<dir>/<case>.toml` as `conf::<dir>::<case>`.
fn collect_tests(root: &Path) -> Result<Tests, String> {
    let mut tests = Tests::default();
    let mut crates: Vec<(String, std::path::PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for e in entries.flatten() {
            crates.push((e.file_name().to_string_lossy().into_owned(), e.path()));
        }
    }
    crates.push(("xtask".to_owned(), root.join("xtask")));
    for (dir, path) in crates {
        let prefix = match dir.as_str() {
            "conformance" => "conf".to_owned(),
            other => other.replace('-', "_"),
        };
        for (sub, is_tests) in [("src", false), ("tests", true)] {
            let base = path.join(sub);
            for file in files_with_ext(&base, "rs") {
                let rel: Vec<String> = file
                    .strip_prefix(&base)
                    .unwrap_or(&file)
                    .iter()
                    .map(|c| c.to_string_lossy().trim_end_matches(".rs").to_owned())
                    .collect();
                let (prefix, mut modules) = if is_tests && dir == "worker" && rel.len() > 1 {
                    (rel[0].clone(), rel[1..].to_vec())
                } else {
                    (prefix.clone(), rel)
                };
                if modules
                    .last()
                    .is_some_and(|m| m == "mod" || m == "lib" || m == "main")
                {
                    modules.pop();
                }
                let source = std::fs::read_to_string(&file)
                    .map_err(|e| format!("{}: {e}", file.display()))?;
                for t in tests_in_source(&source) {
                    let mut path: Vec<&str> = vec![prefix.as_str()];
                    if prefix != "xtask" {
                        path.extend(modules.iter().map(String::as_str));
                        path.extend(t.modules.iter().map(String::as_str));
                    }
                    path.retain(|m| *m != "tests" && *m != "test");
                    path.push(&t.name);
                    tests.names.insert(path.join("::"), t.covers);
                }
            }
        }
    }
    let corpus = root.join("crates/conformance/corpus");
    for file in files_with_ext(&corpus, "toml") {
        let rel: Vec<String> = file
            .strip_prefix(&corpus)
            .unwrap_or(&file)
            .iter()
            .map(|c| c.to_string_lossy().trim_end_matches(".toml").to_owned())
            .collect();
        tests
            .names
            .insert(format!("conf::{}", rel.join("::")), Vec::new());
    }
    Ok(tests)
}

/// Files with extension `ext` under `dir`, skipping `fixtures` and `target` directories.
fn files_with_ext(dir: &Path, ext: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let name = e.file_name();
        if path.is_dir() {
            if name != "fixtures" && name != "target" {
                out.extend(files_with_ext(&path, ext));
            }
        } else if path.extension().is_some_and(|x| x == ext) {
            out.push(path);
        }
    }
    out
}

struct FoundTest {
    modules: Vec<String>,
    name: String,
    covers: Vec<String>,
}

#[derive(PartialEq)]
enum Tok {
    Ident(String),
    Doc(String),
    Punct(char),
}

/// Test functions in one Rust file: `fn`s after an attribute whose path ends in `test`
/// (`#[test]`, `#[tokio::test]`, `#[wasm_bindgen_test]`, …), with their inline module path and the
/// IDs of their `/// Covers:` line.
fn tests_in_source(source: &str) -> Vec<FoundTest> {
    let toks = lex(source);
    let mut found = Vec::new();
    let mut modules: Vec<(String, usize)> = Vec::new();
    let (mut depth, mut pending_mod, mut is_test) = (0usize, None::<String>, false);
    let mut docs: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Doc(d) => docs.push(d.clone()),
            Tok::Punct('#') if toks.get(i + 1) == Some(&Tok::Punct('[')) => {
                let mut path = Vec::new();
                let mut j = i + 2;
                while let Some(Tok::Ident(s)) = toks.get(j) {
                    path.push(s.clone());
                    j += 1;
                    while toks.get(j) == Some(&Tok::Punct(':')) {
                        j += 1;
                    }
                }
                if path
                    .last()
                    .is_some_and(|p| p == "test" || p.ends_with("_test") || p == "rstest")
                {
                    is_test = true;
                }
                let mut bracket = 0;
                while let Some(t) = toks.get(j) {
                    match t {
                        Tok::Punct('[') => bracket += 1,
                        Tok::Punct(']') if bracket == 0 => break,
                        Tok::Punct(']') => bracket -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                i = j;
            }
            Tok::Ident(kw) if kw == "mod" => {
                if let Some(Tok::Ident(name)) = toks.get(i + 1) {
                    pending_mod = Some(name.clone());
                    i += 1;
                }
            }
            Tok::Ident(kw) if kw == "fn" => {
                if let Some(Tok::Ident(name)) = toks.get(i + 1) {
                    if is_test {
                        let covers = docs
                            .iter()
                            .find_map(|d| d.trim().strip_prefix("Covers:"))
                            .map(|ids| {
                                ids.split(',')
                                    .map(|s| s.trim().trim_end_matches('.').to_owned())
                                    .filter(|s| !s.is_empty())
                                    .collect()
                            })
                            .unwrap_or_default();
                        found.push(FoundTest {
                            modules: modules.iter().map(|(m, _)| m.clone()).collect(),
                            name: name.clone(),
                            covers,
                        });
                    }
                    i += 1;
                }
                is_test = false;
                docs.clear();
            }
            Tok::Punct('{') => {
                depth += 1;
                docs.clear();
                if let Some(m) = pending_mod.take() {
                    modules.push((m, depth));
                }
            }
            Tok::Punct('}') => {
                docs.clear();
                if modules.last().is_some_and(|(_, d)| *d == depth) {
                    modules.pop();
                }
                depth = depth.saturating_sub(1);
            }
            Tok::Punct(';') => {
                pending_mod = None;
                docs.clear();
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// A small Rust lexer: identifiers, `///` doc lines and single punctuation characters. Strings,
/// characters, numbers and other comments are skipped, so braces inside them do not count.
fn lex(src: &str) -> Vec<Tok> {
    let c: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let n = c.len();
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    while i < n {
        let ch = c[i];
        if ch == '/' && at(i + 1) == '/' {
            let start = i;
            while i < n && c[i] != '\n' {
                i += 1;
            }
            let line: String = c[start..i].iter().collect();
            if let Some(doc) = line.strip_prefix("///").filter(|d| !d.starts_with('/')) {
                toks.push(Tok::Doc(doc.to_owned()));
            }
        } else if ch == '/' && at(i + 1) == '*' {
            let mut depth = 0;
            while i < n {
                if c[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if ch == '"' {
            i = skip_string(&c, i + 1);
        } else if (ch == 'r' || ch == 'b' || ch == 'c') && is_raw_or_prefixed_string(&c, i) {
            let mut j = i + 1;
            if c[i] == 'b' && at(j) == 'r' {
                j += 1;
            }
            let mut hashes = 0;
            while at(j) == '#' {
                hashes += 1;
                j += 1;
            }
            let raw = c[i] == 'r' || (c[i] == 'b' && at(i + 1) == 'r');
            i = if raw {
                skip_raw_string(&c, j + 1, hashes)
            } else {
                skip_string(&c, j + 1)
            };
        } else if ch == '\'' {
            if at(i + 1) == '\\' {
                i += 2;
                while i < n && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if at(i + 2) == '\'' {
                i += 3;
            } else {
                i += 1; // a lifetime: its name lexes as an identifier
            }
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < n && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            if word == "r" && at(i) == '#' {
                i += 1; // raw identifier
                continue;
            }
            toks.push(Tok::Ident(word));
        } else if ch.is_ascii_digit() {
            while i < n && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '.') {
                i += 1;
            }
        } else {
            if !ch.is_whitespace() {
                toks.push(Tok::Punct(ch));
            }
            i += 1;
        }
    }
    toks
}

fn is_raw_or_prefixed_string(c: &[char], i: usize) -> bool {
    let at = |k: usize| c.get(k).copied().unwrap_or('\0');
    let prev_ident = i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_');
    if prev_ident {
        return false;
    }
    let mut j = i + 1;
    if c[i] == 'b' && at(j) == 'r' {
        j += 1;
    }
    while at(j) == '#' && (c[i] == 'r' || at(i + 1) == 'r') {
        j += 1;
    }
    at(j) == '"'
}

fn skip_string(c: &[char], mut i: usize) -> usize {
    while i < c.len() {
        match c[i] {
            '\\' => i += 2,
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

fn skip_raw_string(c: &[char], mut i: usize, hashes: usize) -> usize {
    while i < c.len() {
        if c[i] == '"' && (0..hashes).all(|k| c.get(i + 1 + k) == Some(&'#')) {
            return i + 1 + hashes;
        }
        i += 1;
    }
    i
}

/// `sdks/typescript-tests.json`: `[{ "name": "ts::…", "covers": ["…"] }]` or `{ "tests": [ … ] }`.
fn read_ts_inventory(root: &Path) -> Result<Option<Tests>, String> {
    let path = root.join(TS_INVENTORY);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{TS_INVENTORY}: {e}"))?;
    let list = value.get("tests").unwrap_or(&value);
    let mut tests = Tests::default();
    for item in list.as_array().into_iter().flatten() {
        let Some(name) = item.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let covers = match item.get("covers") {
            Some(serde_json::Value::String(s)) => {
                s.split(',').map(|s| s.trim().to_owned()).collect()
            }
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter_map(|v| v.as_str())
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        tests.names.insert(name.to_owned(), covers);
    }
    Ok(Some(tests))
}

/// Test names in the first column of the design pages' tables, with their page.
fn design_table_tests(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for file in files_with_ext(&root.join(DESIGN), "md") {
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let page = file
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        for line in text.lines().filter(|l| l.starts_with('|')) {
            if let Some(first) = split_cells(line).first() {
                for span in code_spans(first).into_iter().filter(|s| is_test_name(s)) {
                    out.push((span.to_owned(), format!("Tests table of {page}")));
                }
            }
        }
    }
    Ok(out)
}

/// A unit is landed when `MILESTONES` lists it or the milestone it belongs to (`M17` lands
/// `M17 Foundation`), and always in full mode.
fn is_landed(id: &str, landed: &[String], full: bool) -> bool {
    full || landed
        .iter()
        .any(|l| l == id || id.split(' ').next() == Some(l.as_str()))
}

fn matrix(
    plan: &[Milestone],
    register: &[Row],
    named: &BTreeMap<&str, Named>,
    landed: &[String],
    full: bool,
    tests: &Tests,
    ts: Option<&Tests>,
) -> String {
    let mut out = String::from("| Row | Owner | Milestone | Status |\n|---|---|---|---|\n");
    for row in register {
        let milestone = plan.iter().map(|m| m.id.as_str()).find(|m| {
            let n = &named[m];
            n.rows.contains(&row.id)
                || row.tests.iter().any(|t| {
                    n.tests
                        .iter()
                        .any(|a| a == t || a.strip_suffix('*').is_some_and(|p| t.starts_with(p)))
                })
        });
        let status = if row.owner == "I" {
            "integrator".to_owned()
        } else {
            let states: Vec<Exists> = row.tests.iter().map(|t| exists(t, tests, ts)).collect();
            let required = full || milestone.is_some_and(|m| is_landed(m, landed, full));
            if states.iter().all(|s| matches!(s, Exists::Yes)) && !states.is_empty() {
                "ok".to_owned()
            } else if !required {
                "pending".to_owned()
            } else if states.iter().any(|s| matches!(s, Exists::No)) {
                "missing".to_owned()
            } else {
                "pending (SDK or evaluation)".to_owned()
            }
        };
        out += &format!(
            "| {} | {} | {} | {} |\n",
            row.id,
            row.owner,
            milestone.unwrap_or("–"),
            status
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            let dir = std::env::temp_dir()
                .join(format!("pm-trace-{name}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Fixture(dir)
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        /// A build plan with M5 (row A1 and a named test) and M9 (row B1), a register, a PRD and
        /// a core crate that has A1's test only.
        fn standard(name: &str) -> Self {
            let fx = Fixture::new(name);
            fx.write(
                PLAN,
                "# Plan\n\n## M5 · Five\n\n**Acceptance:**\n\n- Rows A1 and `core::a::named_in_plan`.\n\n\
                 ## M9 · Nine\n\n**Acceptance:**\n\n- Row B1 (and B2, which belongs to M10).\n\n\
                 ## M10 · Ten\n\n**Acceptance:**\n\n- Row B2.\n",
            );
            fx.write(
                REGISTER,
                "| # | Case | Behaviour | Owner | Test |\n|---|---|---|---|---|\n\
                 | A1 | a | b | S | `core::a::a1_present` |\n\
                 | B1 | a | b | S | `core::b::b1_missing` |\n\
                 | B2 | a | b | S | `core::b::b2_missing` |\n\
                 | K1 | a | b | I | – |\n",
            );
            fx.write(
                PRD,
                "- **FR-OUT-1** The service must send.\n\n| NFR-SEC-2 | Size | small |\n",
            );
            fx.write(
                "crates/core/src/a.rs",
                "#[cfg(test)]\nmod tests {\n    /// Covers: A1, FR-OUT-1\n    #[test]\n    fn a1_present() {}\n\n    #[test]\n    fn named_in_plan() {}\n}\n",
            );
            fx
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// With `MILESTONES` listing only M5, the missing test of M9's row is pending and the check
    /// passes; with M9 added it fails, naming the test (Testing § 13).
    #[test]
    fn trace_detects_missing_test() {
        let fx = Fixture::standard("missing");
        fx.write("MILESTONES", "M5\n");
        let report = check(&fx.0).unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert!(
            report.matrix.contains("| B1 | S | M9 | pending |"),
            "{}",
            report.matrix
        );
        assert!(
            report.matrix.contains("| A1 | S | M5 | ok |"),
            "{}",
            report.matrix
        );

        fx.write("MILESTONES", "M5\nM9\n");
        let report = check(&fx.0).unwrap();
        assert_eq!(report.failures.len(), 1, "{:?}", report.failures);
        assert!(
            report.failures[0].contains("core::b::b1_missing"),
            "{:?}",
            report.failures
        );
        assert!(
            report.matrix.contains("| B1 | S | M9 | missing |"),
            "{}",
            report.matrix
        );
        // B2 sits in a parenthesis that names M10, so M9 does not require it.
        assert!(
            report.matrix.contains("| B2 | S | M10 | pending |"),
            "{}",
            report.matrix
        );
    }

    /// A `Covers:` line naming neither a row nor a requirement fails; an unknown milestone in
    /// `MILESTONES` fails; with M20 listed every requirement needs a covering test.
    #[test]
    fn trace_rejects_unknown_covers_and_milestones() {
        let fx = Fixture::standard("covers");
        fx.write(
            "crates/core/src/c.rs",
            "/// Covers: Z99\n#[test]\nfn z() {}\n",
        );
        fx.write("MILESTONES", "M5\nM77\n");
        let report = check(&fx.0).unwrap();
        assert!(
            report.failures.iter().any(|f| f.contains("`Z99`")),
            "{:?}",
            report.failures
        );
        assert!(
            report.failures.iter().any(|f| f.contains("`M77`")),
            "{:?}",
            report.failures
        );
    }

    /// Test names come from the crate, the file's module path and inline modules, with `tests`
    /// modules left out; braces in strings, characters and comments do not move the module path.
    #[test]
    fn trace_names_tests_by_module_path() {
        let src = r##"
            const OPEN: char = '{';
            const S: &str = "}}}";
            const R: &str = r#"mod fake { "#;
            // mod commented {
            pub mod address {
                fn helper<'a>(x: &'a str) -> &'a str { x }
                #[cfg(test)]
                mod tests {
                    /// Covers: A1
                    #[test]
                    fn a1_case_and_dots() { let _ = '}'; }
                }
            }
            #[tokio::test]
            async fn top_level() {}
        "##;
        let found = tests_in_source(src);
        let names: Vec<String> = found
            .iter()
            .map(|t| format!("{}::{}", t.modules.join("::"), t.name))
            .collect();
        assert_eq!(names, ["address::tests::a1_case_and_dots", "::top_level"]);
        assert_eq!(found[0].covers, ["A1"]);

        let fx = Fixture::new("names");
        fx.write(
            "crates/core/src/thread_token.rs",
            "#[cfg(test)]\nmod tests {\n#[test]\nfn a2_round_trip() {}\n}\n",
        );
        fx.write(
            "crates/worker/tests/it/inbound.rs",
            "#[test]\nfn a6_reject_codes() {}\n",
        );
        fx.write(
            "xtask/src/size.rs",
            "mod tests {\n#[test]\nfn size_budget() {}\n}\n",
        );
        fx.write(
            "crates/conformance/corpus/mime/b5_shift_jis_subject.toml",
            "id = 1\n",
        );
        let tests = collect_tests(&fx.0).unwrap();
        let names: Vec<&str> = tests.names.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "conf::mime::b5_shift_jis_subject",
                "core::thread_token::a2_round_trip",
                "it::inbound::a6_reject_codes",
                "xtask::size_budget"
            ]
        );
    }

    /// Rows come from ranges and lists; `D1` and `R2` alone are Cloudflare products; link targets
    /// are ignored.
    #[test]
    fn trace_reads_rows_from_acceptance() {
        let ids: BTreeSet<&str> = ["A1", "A2", "A3", "A4", "B2", "D1", "D2", "R2", "J35"]
            .into_iter()
            .collect();
        let named = named_in(
            "**Acceptance:**\n- A1–A3, B2 and D1–D2; a D1 query, an R2 object ([J35](../edge-cases.md)); `xtask::x_*`.",
            "M2",
            &ids,
        );
        assert_eq!(named.rows, ["A1", "A2", "A3", "B2", "D1", "D2", "J35"]);
        assert_eq!(named.tests, ["xtask::x_*"]);
    }

    /// "every `core::` row" limits the rows' required tests to `core::` and the corpus's `conf::`.
    /// A row accepted by a later milestone contributes only the tests the acceptance names.
    #[test]
    fn trace_defers_rows_accepted_later() {
        let ids: BTreeSet<&str> = ["J13", "J21", "J20"].into_iter().collect();
        let n = named_in(
            "Keys (J20): `it::keys::a`. The authentication part of J13 runs here (`it::partners::j13_x`); J13 is accepted in M9. A lost Init (J21; its mailbox part lands in M6, which accepts J21).",
            "M5",
            &ids,
        );
        assert_eq!(n.rows, ["J20"]);
        assert!(n.tests.contains(&"it::partners::j13_x".to_owned()));
        assert!(deferred("J13 is accepted in M9", "J13", "M5"));
        assert_eq!(
            drop_accepted_there(
                "Tests `a::b`. Rows that need a later milestone are accepted there: A7 in M9\n  (`it::send::a7`, needs the send path), and A11 in M13. The pause ships here."
            ),
            "Tests `a::b`. The pause ships here."
        );
        assert!(!deferred("J13 is accepted in M5", "J13", "M5"));
        assert!(deferred("W35 is\n  accepted in M21", "W35", "M5"));
    }

    #[test]
    fn trace_scopes_rows_to_a_prefix() {
        let ids: BTreeSet<&str> = ["A2", "B4"].into_iter().collect();
        let named = named_in("- Unit tests for every `core::` row: A2, B4.", "M2", &ids);
        assert_eq!(named.rows, ["A2", "B4"]);
        assert_eq!(named.row_prefixes, ["core::"]);
        assert!(row_test_in_scope(
            "core::thread_token::a2_x",
            &named.row_prefixes
        ));
        assert!(row_test_in_scope(
            "conf::mime::b4_html_only",
            &named.row_prefixes
        ));
        assert!(!row_test_in_scope(
            "it::inbound::a2_forged_token_ignored",
            &named.row_prefixes
        ));
        assert!(row_test_in_scope("it::anything", &[]));
    }

    /// Rows on the `**Implements:**` line count; a named acceptance block is listed by its name and
    /// requires only its own tests, while the milestone's ID stands for all of its blocks.
    #[test]
    fn trace_reads_named_blocks_and_implements() {
        let fx = Fixture::standard("blocks");
        fx.write(
            PLAN,
            "## M17 · Ops\n\n**Implements:** FR-OUT-1 and edge rows A1.\n\n\
             **Acceptance, M17 Foundation** (it merges before M7):\n\n- `core::a::named_in_plan`.\n\n\
             **Acceptance, M17 Completion** (accepted at M20):\n\n- Row B1.\n",
        );
        fx.write("MILESTONES", "M17 Foundation\n");
        let report = check(&fx.0).unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert!(
            report
                .summary
                .contains("ok `core::a::named_in_plan` (M17 Foundation acceptance)"),
            "{}",
            report.summary
        );

        fx.write("MILESTONES", "M17\n");
        let report = check(&fx.0).unwrap();
        assert_eq!(report.failures.len(), 1, "{:?}", report.failures);
        assert!(
            report.failures[0].contains("core::b::b1_missing"),
            "{:?}",
            report.failures
        );
        assert!(
            report.matrix.contains("| A1 | S | M17 | ok |"),
            "{}",
            report.matrix
        );
        assert!(
            report
                .matrix
                .contains("| B1 | S | M17 Completion | missing |"),
            "{}",
            report.matrix
        );
    }

    #[test]
    fn security_invariants_are_coverable() {
        let ids = parse_invariants(
            "| ID | Invariant |\n|---|---|\n| SEC-1 | Scope |\n| SEC-12 | Mail |\n| SEC-x | no |\n",
        );
        assert_eq!(ids.into_iter().collect::<Vec<_>>(), ["SEC-1", "SEC-12"]);
    }
}
