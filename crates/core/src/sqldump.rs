//! D1 exports and the restore replay (Observability § 9, Restore from PITR step 3): [`parse`] reads
//! the `INSERT INTO "<table>" ("<col>",…) VALUES(…);` statements of `wrangler d1 export` (SQL
//! literals: integers, reals, `'…'` strings with doubled quotes, `X'…'` blobs, `NULL`) into rows;
//! [`replay`] compares the export taken before a restore with the one taken after it, row by row on
//! the primary keys of [`crate::schema`], and writes the statements that re-apply every change made
//! after the restore point except the excluded ones, in foreign-key order.

use std::collections::{BTreeMap, BTreeSet};

use crate::schema::Schema;

/// One SQL literal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lit {
    Null,
    Int(i64),
    /// Kept as written, so a value round-trips exactly.
    Real(String),
    Text(String),
    Blob(Vec<u8>),
}

impl Lit {
    /// The literal as SQL (blobs in `X'…'` form).
    pub fn sql(&self) -> String {
        match self {
            Lit::Null => "NULL".into(),
            Lit::Int(i) => i.to_string(),
            Lit::Real(r) => r.clone(),
            Lit::Text(t) => format!("'{}'", t.replace('\'', "''")),
            Lit::Blob(b) => format!(
                "X'{}'",
                b.iter().map(|x| format!("{x:02x}")).collect::<String>()
            ),
        }
    }
}

/// One table of an export.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Lit>>,
}

/// Every table of an export with its rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dump {
    pub tables: BTreeMap<String, Rows>,
}

/// Tables the replay never compares: D1's and SQLite's own, and the migration ledger.
pub const SKIPPED: &[&str] = &["schema_migrations", "d1_migrations", "sqlite_sequence"];

struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, b: u8) -> Result<(), String> {
        self.ws();
        if self.s.get(self.i) == Some(&b) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", b as char, self.i))
        }
    }
    fn ident(&mut self) -> Result<String, String> {
        self.ws();
        if self.s.get(self.i) != Some(&b'"') {
            return Err(format!("expected a quoted name at byte {}", self.i));
        }
        self.i += 1;
        let mut out = Vec::new();
        loop {
            match self.s.get(self.i) {
                Some(b'"') if self.s.get(self.i + 1) == Some(&b'"') => {
                    out.push(b'"');
                    self.i += 2;
                }
                Some(b'"') => {
                    self.i += 1;
                    break;
                }
                Some(c) => {
                    out.push(*c);
                    self.i += 1;
                }
                None => return Err("an unterminated name".into()),
            }
        }
        String::from_utf8(out).map_err(|_| "a name that is not UTF-8".into())
    }
    fn lit(&mut self) -> Result<Lit, String> {
        self.ws();
        let rest = &self.s[self.i..];
        if rest.starts_with(b"NULL") {
            self.i += 4;
            return Ok(Lit::Null);
        }
        match rest.first() {
            Some(b'\'') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    match self.s.get(self.i) {
                        Some(b'\'') if self.s.get(self.i + 1) == Some(&b'\'') => {
                            out.push(b'\'');
                            self.i += 2;
                        }
                        Some(b'\'') => {
                            self.i += 1;
                            break;
                        }
                        Some(c) => {
                            out.push(*c);
                            self.i += 1;
                        }
                        None => return Err("an unterminated string".into()),
                    }
                }
                String::from_utf8(out)
                    .map(Lit::Text)
                    .map_err(|_| "a string that is not UTF-8".into())
            }
            Some(b'X' | b'x') if rest.get(1) == Some(&b'\'') => {
                self.i += 2;
                let start = self.i;
                while self.s.get(self.i).is_some_and(|c| *c != b'\'') {
                    self.i += 1;
                }
                let hex = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| "bad blob")?;
                self.i += 1;
                if hex.len() % 2 != 0 {
                    return Err("a blob of odd length".into());
                }
                (0..hex.len())
                    .step_by(2)
                    .map(|k| u8::from_str_radix(&hex[k..k + 2], 16).map_err(|_| "bad blob".into()))
                    .collect::<Result<Vec<u8>, String>>()
                    .map(Lit::Blob)
            }
            Some(c) if c.is_ascii_digit() || *c == b'-' || *c == b'+' || *c == b'.' => {
                let start = self.i;
                while self.s.get(self.i).is_some_and(|c| {
                    c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E')
                }) {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| "bad number")?;
                Ok(match t.parse::<i64>() {
                    Ok(n) => Lit::Int(n),
                    Err(_) => Lit::Real(t.to_owned()),
                })
            }
            _ => Err(format!("an unknown literal at byte {}", self.i)),
        }
    }
}

/// Parses the `INSERT INTO` statements of an export; every other statement (`PRAGMA`, `CREATE …`) is
/// skipped.
pub fn parse(sql: &str) -> Result<Dump, String> {
    let mut dump = Dump::default();
    let marker = "INSERT INTO ";
    let bytes = sql.as_bytes();
    let mut from = 0;
    while let Some(off) = sql[from..].find(marker) {
        let start = from + off;
        // Only at the start of a line: a string literal can contain the marker.
        if start > 0 && bytes[start - 1] != b'\n' {
            from = start + marker.len();
            continue;
        }
        let mut c = Cursor {
            s: bytes,
            i: start + marker.len(),
        };
        let table = c.ident()?;
        c.eat(b'(')?;
        let mut columns = vec![c.ident()?];
        loop {
            c.ws();
            if c.s.get(c.i) == Some(&b',') {
                c.i += 1;
                columns.push(c.ident()?);
            } else {
                break;
            }
        }
        c.eat(b')')?;
        c.ws();
        if !c.s[c.i..].starts_with(b"VALUES") {
            return Err(format!("expected VALUES for {table}"));
        }
        c.i += "VALUES".len();
        c.eat(b'(')?;
        let mut row = vec![c.lit()?];
        loop {
            c.ws();
            if c.s.get(c.i) == Some(&b',') {
                c.i += 1;
                row.push(c.lit()?);
            } else {
                break;
            }
        }
        c.eat(b')')?;
        c.eat(b';')?;
        if row.len() != columns.len() {
            return Err(format!(
                "{table}: {} values for {} columns",
                row.len(),
                columns.len()
            ));
        }
        let t = dump.tables.entry(table.clone()).or_default();
        if t.columns.is_empty() {
            t.columns = columns;
        } else if t.columns != columns {
            return Err(format!("{table}: the columns change between rows"));
        }
        t.rows.push(row);
        from = c.i;
    }
    Ok(dump)
}

/// What the replay leaves at its restored value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Excludes {
    /// Whole tables.
    pub tables: BTreeSet<String>,
    /// `table.column`.
    pub columns: BTreeSet<String>,
    /// `table:key`, the key's values joined by `,` as SQL literals would print them unquoted.
    pub rows: BTreeSet<String>,
}

impl Excludes {
    /// Parses `--exclude <table>[.<column>]` and `--exclude-row <table>:<key>` against the schema; an
    /// unknown name is an error before anything is restored.
    pub fn parse(
        schema: &Schema,
        exclude: &[String],
        exclude_row: &[String],
    ) -> Result<Self, String> {
        let mut e = Excludes::default();
        for x in exclude {
            if !schema.knows(x) {
                return Err(format!("--exclude {x}: no such table or column"));
            }
            if x.contains('.') {
                e.columns.insert(x.clone());
            } else {
                e.tables.insert(x.clone());
            }
        }
        for r in exclude_row {
            let (t, _) = r
                .split_once(':')
                .ok_or_else(|| format!("--exclude-row {r}: expected <table>:<key>"))?;
            if !schema.knows(t) {
                return Err(format!("--exclude-row {r}: no such table"));
            }
            e.rows.insert(r.clone());
        }
        Ok(e)
    }
}

/// Counts per table, and the replay itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replay {
    /// `table → (inserted, updated, deleted)`.
    pub summary: BTreeMap<String, (u64, u64, u64)>,
    /// Columns the `before` export has and the restored schema lacks (a migration ran after `T`).
    pub missing_columns: Vec<String>,
    pub sql: String,
}

fn key_text(k: &[Lit]) -> String {
    k.iter()
        .map(|l| match l {
            Lit::Text(t) => t.clone(),
            other => other.sql(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn q(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The statements that bring the restored database (`after`) back to `before`, except the excludes:
/// a row only in `before` is inserted, a row only in `after` deleted, a changed row updated to its
/// `before` values (excluded columns keep their restored values). Inserts and updates run parents
/// first, deletes children first.
pub fn replay(before: &Dump, after: &Dump, schema: &Schema, ex: &Excludes) -> Replay {
    let mut out = Replay::default();
    let empty = Rows::default();
    let mut writes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut deletes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut names: BTreeSet<&String> = before.tables.keys().collect();
    names.extend(after.tables.keys());
    for name in names {
        if SKIPPED.contains(&name.as_str()) || ex.tables.contains(name) || name.starts_with("_cf_")
        {
            continue;
        }
        let b = before.tables.get(name).unwrap_or(&empty);
        let a = after.tables.get(name).unwrap_or(&empty);
        let cols: Vec<String> = if b.columns.is_empty() {
            a.columns.clone()
        } else {
            b.columns.clone()
        };
        for c in &b.columns {
            if !a.columns.is_empty() && !a.columns.contains(c) {
                out.missing_columns.push(format!("{name}.{c}"));
            }
        }
        let key_cols: Vec<String> = schema
            .table(name)
            .map(|t| t.key.clone())
            .unwrap_or_else(|| cols.clone());
        let index = |r: &Rows| -> BTreeMap<Vec<Lit>, BTreeMap<String, Lit>> {
            r.rows
                .iter()
                .map(|row| {
                    let m: BTreeMap<String, Lit> =
                        r.columns.iter().cloned().zip(row.iter().cloned()).collect();
                    let k = key_cols
                        .iter()
                        .map(|c| m.get(c).cloned().unwrap_or(Lit::Null))
                        .collect();
                    (k, m)
                })
                .collect()
        };
        let (bi, ai) = (index(b), index(a));
        let skip_row = |k: &[Lit]| ex.rows.contains(&format!("{name}:{}", key_text(k)));
        let excluded_col = |c: &str| ex.columns.contains(&format!("{name}.{c}"));
        let where_of = |k: &[Lit]| {
            key_cols
                .iter()
                .zip(k)
                .map(|(c, v)| match v {
                    Lit::Null => format!("{} IS NULL", q(c)),
                    v => format!("{} = {}", q(c), v.sql()),
                })
                .collect::<Vec<_>>()
                .join(" AND ")
        };
        let mut counts = (0, 0, 0);
        for (k, row) in &bi {
            if skip_row(k) {
                continue;
            }
            match ai.get(k) {
                None => {
                    let present: Vec<&String> = cols
                        .iter()
                        .filter(|c| a.columns.is_empty() || a.columns.contains(c))
                        .collect();
                    writes.entry(name.clone()).or_default().push(format!(
                        "INSERT INTO {} ({}) VALUES({});",
                        q(name),
                        present.iter().map(|c| q(c)).collect::<Vec<_>>().join(","),
                        present
                            .iter()
                            .map(|c| row.get(*c).unwrap_or(&Lit::Null).sql())
                            .collect::<Vec<_>>()
                            .join(",")
                    ));
                    counts.0 += 1;
                }
                Some(old) => {
                    let set: Vec<String> = cols
                        .iter()
                        .filter(|c| !key_cols.contains(c) && !excluded_col(c))
                        .filter(|c| old.contains_key(*c) && row.get(*c) != old.get(*c))
                        .map(|c| format!("{} = {}", q(c), row.get(c).unwrap_or(&Lit::Null).sql()))
                        .collect();
                    if !set.is_empty() {
                        writes.entry(name.clone()).or_default().push(format!(
                            "UPDATE {} SET {} WHERE {};",
                            q(name),
                            set.join(", "),
                            where_of(k)
                        ));
                        counts.1 += 1;
                    }
                }
            }
        }
        for k in ai.keys() {
            if !bi.contains_key(k) && !skip_row(k) {
                deletes.entry(name.clone()).or_default().push(format!(
                    "DELETE FROM {} WHERE {};",
                    q(name),
                    where_of(k)
                ));
                counts.2 += 1;
            }
        }
        if counts != (0, 0, 0) {
            out.summary.insert(name.clone(), counts);
        }
    }
    let mut order = schema.fk_order();
    for t in writes.keys().chain(deletes.keys()) {
        if !order.contains(t) {
            order.push(t.clone());
        }
    }
    let mut sql = String::from("PRAGMA defer_foreign_keys = true;\n");
    for t in order.iter().rev() {
        for s in deletes.get(t).into_iter().flatten() {
            sql.push_str(s);
            sql.push('\n');
        }
    }
    for t in &order {
        for s in writes.get(t).into_iter().flatten() {
            sql.push_str(s);
            sql.push('\n');
        }
    }
    out.sql = sql;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_round_trip() {
        let d = parse(
            "PRAGMA x;\nINSERT INTO \"t\" (\"a\",\"b\",\"c\",\"d\",\"e\") VALUES(1,'it''s',X'00ff',NULL,-2.5e3);\n",
        )
        .unwrap();
        let row = &d.tables["t"].rows[0];
        assert_eq!(
            row,
            &[
                Lit::Int(1),
                Lit::Text("it's".into()),
                Lit::Blob(vec![0, 255]),
                Lit::Null,
                Lit::Real("-2.5e3".into())
            ]
        );
        assert_eq!(row[1].sql(), "'it''s'");
        assert_eq!(row[2].sql(), "X'00ff'");
        assert!(parse("INSERT INTO \"t\" (\"a\") VALUES(1,2);\n").is_err());
    }
}
