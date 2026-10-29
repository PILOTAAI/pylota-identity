//! Human, JSON and quiet output ([CLI and setup §3]): one JSON document per invocation in JSON mode,
//! tables and indented objects for people, and untrusted text neutralised before it reaches a terminal.
//!
//! [CLI and setup §3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#3-output

use serde_json::Value;

use crate::{CliError, Ctx};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Human,
    Json,
    Quiet,
}

/// What a command produced.
#[derive(Clone, Debug, Default)]
pub struct Output {
    /// The JSON-mode document: the API's response body unchanged, or the command's own result.
    pub doc: Value,
    /// The human rendering; `None` prints `doc` as indented JSON.
    pub human: Option<String>,
    /// The quiet rendering; `None` prints `doc["id"]` when it is a string, else nothing.
    pub quiet: Option<String>,
    /// Lines for stderr in human mode (the secret notice, warnings).
    pub notes: Vec<String>,
    /// The exit code of a successful run that still reports something (`wait` timing out is 13).
    pub exit: i32,
    /// The command printed its output as it went (NDJSON); nothing more is printed.
    pub streamed: bool,
}

impl Output {
    /// A single object, printed as indented JSON for people.
    pub fn object(doc: Value) -> Self {
        Self {
            doc,
            ..Self::default()
        }
    }

    /// A list page (`{ "data": [...] }`), printed as a table of `cols`; quiet prints the IDs.
    pub fn list(doc: Value, cols: &[Col]) -> Self {
        let rows = doc["data"].as_array().cloned().unwrap_or_default();
        let quiet = rows
            .iter()
            .filter_map(|r| r["id"].as_str().map(|s| format!("{}\n", terminal_safe(s))))
            .collect::<String>();
        Self {
            human: Some(table(&rows, cols)),
            quiet: Some(quiet),
            doc,
            ..Self::default()
        }
    }

    pub fn human(mut self, s: impl Into<String>) -> Self {
        self.human = Some(s.into());
        self
    }

    pub fn quiet(mut self, s: impl Into<String>) -> Self {
        self.quiet = Some(s.into());
        self
    }

    pub fn streamed(mut self) -> Self {
        self.streamed = true;
        self
    }

    pub fn note(mut self, s: impl Into<String>) -> Self {
        self.notes.push(s.into());
        self
    }
}

/// One table column: a title, a path into the row (`a.b`, or `a.0.b` for an array item) and a width.
#[derive(Clone, Copy, Debug)]
pub struct Col {
    pub title: &'static str,
    pub path: &'static str,
    pub width: usize,
}

pub const fn col(title: &'static str, path: &'static str, width: usize) -> Col {
    Col { title, path, width }
}

/// The value at a dotted path.
pub fn at<'a>(v: &'a Value, path: &str) -> &'a Value {
    let mut cur = v;
    for part in path.split('.') {
        cur = match part.parse::<usize>() {
            Ok(i) if cur.is_array() => &cur[i],
            _ => &cur[part],
        };
    }
    cur
}

/// A cell: strings as they are, other scalars as JSON writes them, `null` empty.
fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(cell).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// An aligned table; every cell neutralised, newlines shown as `⏎`, cut to its column's width.
pub fn table(rows: &[Value], cols: &[Col]) -> String {
    let fit = |s: &str, w: usize| -> String {
        let s = terminal_safe(s).replace('\n', "⏎").replace('\t', " ");
        let chars: Vec<char> = s.chars().collect();
        if chars.len() > w {
            let mut out: String = chars[..w.saturating_sub(1)].iter().collect();
            out.push('…');
            out
        } else {
            s
        }
    };
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            cols.iter()
                .map(|c| fit(&cell(at(r, c.path)), c.width))
                .collect()
        })
        .collect();
    let widths: Vec<usize> = cols
        .iter()
        .enumerate()
        .map(|(i, c)| {
            cells
                .iter()
                .map(|r| r[i].chars().count())
                .chain([c.title.len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |items: Vec<String>| -> String {
        let mut s = String::new();
        for (i, item) in items.iter().enumerate() {
            if i + 1 == items.len() {
                s.push_str(item);
            } else {
                s.push_str(item);
                s.push_str(&" ".repeat(widths[i] - item.chars().count() + 2));
            }
        }
        s.trim_end().to_owned() + "\n"
    };
    let mut out = line(cols.iter().map(|c| c.title.to_owned()).collect());
    for r in cells {
        out.push_str(&line(r));
    }
    out
}

/// Neutralises text from email for a terminal ([CLI and setup §3.2]): C0 controls other than newline
/// and tab, DEL and C1 controls become U+FFFD (which removes ANSI escapes), and bidirectional and
/// zero-width characters are shown as `<U+202E>`-style markers.
///
/// [CLI and setup §3.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#32-untrusted-text-in-a-terminal
pub fn terminal_safe(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        match u {
            0x0A | 0x09 => out.push(c),
            0x00..=0x1F | 0x7F | 0x80..=0x9F => out.push('\u{FFFD}'),
            0x202A..=0x202E | 0x2066..=0x2069 | 0x200B..=0x200D | 0x2060 | 0xFEFF => {
                out.push_str(&format!("<U+{u:04X}>"));
            }
            _ => out.push(c),
        }
    }
    out
}

/// A copy of `v` with every string neutralised, for printing as indented JSON to a person.
pub fn safe_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(terminal_safe(s)),
        Value::Array(a) => Value::Array(a.iter().map(safe_value).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (terminal_safe(k), safe_value(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Indented JSON for a person.
pub fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(&safe_value(v)).unwrap_or_default() + "\n"
}

/// Prints a command's result or error and returns the exit code.
pub fn finish(ctx: &Ctx, result: Result<Output, CliError>) -> i32 {
    match result {
        Ok(o) if o.streamed => o.exit,
        Ok(o) => {
            match ctx.mode {
                Mode::Json => ctx.print(&format!("{}\n", o.doc)),
                Mode::Quiet => {
                    let q = o.quiet.unwrap_or_else(|| {
                        o.doc["id"]
                            .as_str()
                            .map(|s| format!("{}\n", terminal_safe(s)))
                            .unwrap_or_default()
                    });
                    ctx.print(&q);
                }
                Mode::Human => {
                    ctx.print(&o.human.unwrap_or_else(|| pretty(&o.doc)));
                    for n in &o.notes {
                        ctx.note(n);
                    }
                }
            }
            o.exit
        }
        Err(e) => {
            if ctx.mode == Mode::Json {
                ctx.print(&format!("{}\n", e.envelope()));
            } else {
                print_error(ctx, &e);
            }
            e.exit_code()
        }
    }
}

/// The human form of an error, on stderr ([CLI and setup §3.3]).
///
/// [CLI and setup §3.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#33-errors
pub fn print_error(ctx: &Ctx, e: &CliError) {
    let env = e.envelope();
    let err = &env["error"];
    let s = |v: &Value| terminal_safe(v.as_str().unwrap_or(""));
    let head = match e {
        CliError::Api(a) => format!(
            "error: {} ({}): {}",
            terminal_safe(&a.code),
            a.status,
            s(&err["message"])
        ),
        _ => format!("error: {}: {}", s(&err["code"]), s(&err["message"])),
    };
    ctx.note(&head);
    if let CliError::Cloudflare { errors, .. } = e {
        for c in errors {
            ctx.note(&format!(
                "  cloudflare {}: {}",
                c.code,
                terminal_safe(&c.message)
            ));
        }
    }
    for (label, key) in [
        ("fix", "fix"),
        ("docs", "docs"),
        ("request_id", "request_id"),
    ] {
        if let Some(v) = err[key].as_str() {
            ctx.note(&format!("  {label}: {}", terminal_safe(v)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `cli::output::terminal_safe`: ANSI escapes, C1 controls, bidi and zero-width characters from
    /// mail are neutralised in human output, in objects and in tables.
    #[test]
    fn terminal_safe() {
        let hostile =
            "Invoice\u{1b}]0;pwned\u{7}\u{1b}[2J \u{9b}31m \u{202e}fdp.exe\u{200b}\nnext\tcol";
        let s = super::terminal_safe(hostile);
        assert!(
            !s.chars()
                .any(|c| (c as u32) < 0x20 && c != '\n' && c != '\t')
        );
        assert!(!s.chars().any(|c| (0x80..=0x9F).contains(&(c as u32))));
        assert!(s.contains("<U+202E>") && s.contains("<U+200B>"));
        assert!(s.contains("\nnext\tcol"));
        let rows = [json!({ "subject": hostile, "id": "msg_1" })];
        let t = table(&rows, &[col("SUBJECT", "subject", 200)]);
        assert!(!t.contains('\u{1b}') && t.contains('⏎'));
        assert!(!pretty(&rows[0]).contains('\u{202e}'));
        let cut = table(&[json!({ "s": "abcdefghij" })], &[col("S", "s", 5)]);
        assert!(cut.contains("abcd…"));
    }
}
