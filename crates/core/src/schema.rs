//! The control-plane schema as data (Observability § 9, Restore from PITR): every table of
//! `migrations/d1/` with its columns in order, its primary key and the tables its foreign keys name,
//! read from the migrations compiled into this crate. The restore replay keys rows by these primary
//! keys and orders statements by these foreign keys.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// The migrations, in order.
const MIGRATIONS: &[&str] = &[include_str!("../../../migrations/d1/0001_init.sql")];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub columns: Vec<String>,
    /// The primary key's columns; every column for a table without one.
    pub key: Vec<String>,
    /// The tables this one references.
    pub parents: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schema {
    pub tables: BTreeMap<String, Table>,
}

impl Schema {
    /// The schema of the compiled-in migrations.
    pub fn d1() -> &'static Schema {
        static S: OnceLock<Schema> = OnceLock::new();
        S.get_or_init(|| Schema::parse(&MIGRATIONS.join("\n")))
    }

    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    /// Whether `name` is a table, or `table.column` names a column of one.
    pub fn knows(&self, name: &str) -> bool {
        match name.split_once('.') {
            Some((t, c)) => self
                .table(t)
                .is_some_and(|t| t.columns.iter().any(|x| x == c)),
            None => self.tables.contains_key(name),
        }
    }

    /// Parents before children (inserts and updates in this order, deletes in reverse); ties by name.
    pub fn fk_order(&self) -> Vec<String> {
        let mut done: Vec<String> = Vec::new();
        let mut left: BTreeSet<&String> = self.tables.keys().collect();
        while !left.is_empty() {
            let ready: Vec<&String> = left
                .iter()
                .filter(|t| {
                    self.tables[**t]
                        .parents
                        .iter()
                        .all(|p| p == **t || done.contains(p) || !self.tables.contains_key(p))
                })
                .copied()
                .collect();
            // A cycle (none in the schema today): take the rest in name order.
            let take = if ready.is_empty() {
                left.iter().copied().collect()
            } else {
                ready
            };
            for t in take {
                left.remove(t);
                done.push(t.clone());
            }
        }
        done
    }

    /// Parses `CREATE TABLE` statements: one column per line at two spaces' indent, table-level
    /// `PRIMARY KEY (…)` lines, and `REFERENCES t(c)` anywhere on a column's line.
    pub fn parse(sql: &str) -> Schema {
        let mut tables = BTreeMap::new();
        let mut current: Option<Table> = None;
        for line in sql.lines() {
            let code = line.split("--").next().unwrap_or("");
            if let Some(rest) = code.strip_prefix("CREATE TABLE ") {
                let name = rest
                    .split(|c: char| c.is_whitespace() || c == '(')
                    .find(|w| !w.is_empty() && *w != "IF" && *w != "NOT" && *w != "EXISTS")
                    .unwrap_or("")
                    .trim_matches('"')
                    .to_owned();
                // A one-line table: `CREATE TABLE t (a INTEGER PRIMARY KEY, b TEXT);`.
                if let (Some(open), Some(close)) = (rest.find('('), rest.rfind(");")) {
                    let mut t = Table {
                        name,
                        columns: Vec::new(),
                        key: Vec::new(),
                        parents: BTreeSet::new(),
                    };
                    for def in rest[open + 1..close].split(',') {
                        let col = def.split_whitespace().next().unwrap_or("").to_owned();
                        if def.contains("PRIMARY KEY") {
                            t.key = vec![col.clone()];
                        }
                        t.columns.push(col);
                    }
                    if t.key.is_empty() {
                        t.key = t.columns.clone();
                    }
                    tables.insert(t.name.clone(), t);
                    continue;
                }
                current = Some(Table {
                    name,
                    columns: Vec::new(),
                    key: Vec::new(),
                    parents: BTreeSet::new(),
                });
                continue;
            }
            let Some(t) = current.as_mut() else { continue };
            if code.starts_with(");") || code.trim_start().starts_with(") WITHOUT") {
                let mut t = current.take().unwrap();
                if t.key.is_empty() {
                    t.key = t.columns.clone();
                }
                tables.insert(t.name.clone(), t);
                continue;
            }
            if let Some(cols) = code.trim_start().strip_prefix("PRIMARY KEY (") {
                t.key = cols
                    .split(')')
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .map(|c| c.trim().to_owned())
                    .collect();
                continue;
            }
            let is_column = code.len() > 2
                && code.starts_with("  ")
                && code[2..].starts_with(|c: char| c.is_ascii_lowercase());
            if is_column {
                let col = code[2..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(',')
                    .to_owned();
                if code.contains(" PRIMARY KEY") {
                    t.key = vec![col.clone()];
                }
                t.columns.push(col);
            }
            for (i, _) in code.match_indices("REFERENCES ") {
                let parent: String = code[i + "REFERENCES ".len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !parent.is_empty() && parent != t.name {
                    t.parents.insert(parent);
                }
            }
        }
        Schema { tables }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parsed schema has every table of the migration, the keys it declares and an order with
    /// parents first.
    #[test]
    fn d1_schema() {
        let s = Schema::d1();
        let sql = MIGRATIONS.join("\n");
        let n =
            sql.matches("\nCREATE TABLE ").count() + usize::from(sql.starts_with("CREATE TABLE "));
        assert_eq!(s.tables.len(), n);
        assert_eq!(s.table("tenants").unwrap().key, ["id"]);
        assert_eq!(
            s.table("signing_keys").unwrap().key,
            ["purpose", "kid"],
            "a table-level key"
        );
        assert!(s.table("identities").unwrap().parents.contains("tenants"));
        assert!(
            s.table("partners")
                .unwrap()
                .columns
                .contains(&"max_tenants".to_owned())
        );
        assert!(s.knows("tenants.policy_json") && !s.knows("tenants.nope") && !s.knows("nope"));
        let order = s.fk_order();
        let pos = |t: &str| order.iter().position(|x| x == t).unwrap();
        assert!(pos("partners") < pos("tenants"));
        assert!(pos("tenants") < pos("identities"));
        assert!(pos("identities") < pos("addresses"));
        assert_eq!(order.len(), s.tables.len());
    }
}
