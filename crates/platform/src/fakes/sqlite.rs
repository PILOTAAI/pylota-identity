//! D1 and Durable Object SQLite over `rusqlite` (bundled SQLite, built with FTS5). Rows come back as
//! JSON objects keyed by column name, as the runtime hands them to `serde`; BLOBs as arrays of bytes.

use std::cell::RefCell;
use std::rc::Rc;

use rusqlite::types::{Value as Sv, ValueRef};
use rusqlite::{Connection, params_from_iter};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::d1::{ControlDb, ExecMeta, MAX_D1_PARAMS, SqlValue, Stmt, check_params, classify};
use crate::durable::{Sql, refuses_statement};
use crate::{PResult, PlatformError, PlatformErrorKind};

fn to_sql(v: &SqlValue) -> Sv {
    match v {
        SqlValue::Null => Sv::Null,
        SqlValue::Int(i) => Sv::Integer(*i),
        SqlValue::Real(r) => Sv::Real(*r),
        SqlValue::Text(t) => Sv::Text(t.clone()),
        SqlValue::Blob(b) => Sv::Blob(b.clone()),
    }
}

fn to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(r) => Value::from(r),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::Array(b.iter().map(|&x| Value::from(x)).collect()),
    }
}

fn err(binding: &'static str, e: rusqlite::Error) -> PlatformError {
    classify(binding, &e.to_string())
}

fn rows<T: DeserializeOwned>(
    conn: &Connection,
    binding: &'static str,
    sql: &str,
    params: &[SqlValue],
) -> PResult<Vec<T>> {
    let mut stmt = conn.prepare(sql).map_err(|e| err(binding, e))?;
    let names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let mut cursor = stmt
        .query(params_from_iter(params.iter().map(to_sql)))
        .map_err(|e| err(binding, e))?;
    let mut out = Vec::new();
    while let Some(row) = cursor.next().map_err(|e| err(binding, e))? {
        let mut obj = Map::new();
        for (i, name) in names.iter().enumerate() {
            obj.insert(
                name.clone(),
                to_json(row.get_ref(i).map_err(|e| err(binding, e))?),
            );
        }
        out.push(serde_json::from_value(Value::Object(obj)).map_err(|e| {
            PlatformError::new(PlatformErrorKind::Corrupt, binding, format!("row: {e}"))
        })?);
    }
    Ok(out)
}

fn execute(
    conn: &Connection,
    binding: &'static str,
    sql: &str,
    params: &[SqlValue],
) -> PResult<ExecMeta> {
    let changes = conn
        .execute(sql, params_from_iter(params.iter().map(to_sql)))
        .map_err(|e| err(binding, e))?;
    Ok(ExecMeta {
        size_after: None,
        changes: changes as u64,
        last_row_id: Some(conn.last_insert_rowid()),
    })
}

/// D1 (`DB`): one in-memory database. [`FakeD1::exec_script`] applies migrations.
#[derive(Clone)]
pub struct FakeD1 {
    conn: Rc<RefCell<Connection>>,
}

/// An in-memory SQLite with the limits workerd applies that bundled SQLite does not: a `LIKE` or
/// `GLOB` pattern over 50 bytes is refused (`LIKE or GLOB pattern too complex`; workerd refused a
/// 61-byte pattern in the M16 SDK suite, 2026-10-29), so native tests catch such a query first.
fn workerd_like_connection() -> Connection {
    let c = Connection::open_in_memory().expect("in-memory SQLite");
    c.set_limit(
        rusqlite::limits::Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH,
        50,
    )
    .expect("a LIKE pattern limit");
    c
}

impl FakeD1 {
    pub fn new() -> Self {
        Self {
            conn: Rc::new(RefCell::new(workerd_like_connection())),
        }
    }

    /// Runs a script of statements without parameters (a migration).
    pub fn exec_script(&self, sql: &str) -> PResult<()> {
        self.conn
            .borrow()
            .execute_batch(sql)
            .map_err(|e| err("DB", e))
    }

    /// The connection, for assertions.
    pub fn conn(&self) -> std::cell::Ref<'_, Connection> {
        self.conn.borrow()
    }
}

impl Default for FakeD1 {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlDb for FakeD1 {
    async fn query<T: DeserializeOwned>(&self, stmt: Stmt) -> PResult<Vec<T>> {
        check_params("DB", &stmt.params, MAX_D1_PARAMS)?;
        rows(&self.conn.borrow(), "DB", stmt.sql, &stmt.params)
    }

    async fn first<T: DeserializeOwned>(&self, stmt: Stmt) -> PResult<Option<T>> {
        Ok(self.query(stmt).await?.into_iter().next())
    }

    async fn execute(&self, stmt: Stmt) -> PResult<ExecMeta> {
        check_params("DB", &stmt.params, MAX_D1_PARAMS)?;
        execute(&self.conn.borrow(), "DB", stmt.sql, &stmt.params)
    }

    async fn batch(&self, stmts: Vec<Stmt>) -> PResult<Vec<ExecMeta>> {
        for s in &stmts {
            check_params("DB", &s.params, MAX_D1_PARAMS)?;
        }
        let mut conn = self.conn.borrow_mut();
        let tx = conn.transaction().map_err(|e| err("DB", e))?;
        let mut out = Vec::with_capacity(stmts.len());
        for s in &stmts {
            out.push(execute(&tx, "DB", s.sql, &s.params)?); // dropping `tx` rolls back
        }
        tx.commit().map_err(|e| err("DB", e))?;
        Ok(out)
    }
}

/// A Durable Object's `ctx.storage.sql`. `exec` refuses `BEGIN`, `SAVEPOINT` and the rest, as the
/// runtime does; [`Sql::transaction_sync`] uses a savepoint.
#[derive(Clone)]
pub struct FakeSql {
    conn: Rc<RefCell<Connection>>,
    depth: Rc<std::cell::Cell<u32>>,
}

impl FakeSql {
    pub fn new() -> Self {
        Self {
            conn: Rc::new(RefCell::new(workerd_like_connection())),
            depth: Rc::default(),
        }
    }

    pub(crate) fn drop_everything(&self) -> PResult<()> {
        let conn = self.conn.borrow();
        let names: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND type IN ('table', 'view')")
                .map_err(|e| err("sql", e))?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| err("sql", e))?
                .collect::<Result<_, _>>()
                .map_err(|e| err("sql", e))?
        };
        for (kind, name) in names {
            // An FTS5 table's shadow tables go with it; ignore the ones already gone.
            let _ = conn.execute_batch(&format!(
                "DROP {} IF EXISTS \"{}\"",
                kind.to_ascii_uppercase(),
                name.replace('"', "\"\"")
            ));
        }
        Ok(())
    }
}

impl Default for FakeSql {
    fn default() -> Self {
        Self::new()
    }
}

impl Sql for FakeSql {
    fn exec(&self, sql: &str, params: &[SqlValue]) -> PResult<ExecMeta> {
        if refuses_statement(sql) {
            return Err(PlatformError::invalid(
                "sql",
                "transactions go through transaction_sync",
            ));
        }
        check_params("sql", params, usize::MAX)?;
        let conn = self.conn.borrow();
        if params.is_empty() && sql.trim().trim_end_matches(';').contains(';') {
            conn.execute_batch(sql).map_err(|e| err("sql", e))?;
            return Ok(ExecMeta {
                size_after: None,
                changes: conn.changes(),
                last_row_id: None,
            });
        }
        let meta = execute(&conn, "sql", sql, params)?;
        Ok(ExecMeta {
            last_row_id: None,
            ..meta
        })
    }

    fn query<T: DeserializeOwned>(&self, sql: &str, params: &[SqlValue]) -> PResult<Vec<T>> {
        check_params("sql", params, usize::MAX)?;
        rows(&self.conn.borrow(), "sql", sql, params)
    }

    fn transaction_sync<R, E: From<PlatformError>>(
        &self,
        f: impl FnOnce(&Self) -> Result<R, E>,
    ) -> Result<R, E> {
        let name = format!("txn{}", self.depth.get());
        self.conn
            .borrow()
            .execute_batch(&format!("SAVEPOINT {name}"))
            .map_err(|e| E::from(err("sql", e)))?;
        self.depth.set(self.depth.get() + 1);
        let result = f(self);
        self.depth.set(self.depth.get() - 1);
        let end = match &result {
            Ok(_) => format!("RELEASE {name}"),
            Err(_) => format!("ROLLBACK TO {name}; RELEASE {name}"),
        };
        self.conn
            .borrow()
            .execute_batch(&end)
            .map_err(|e| E::from(err("sql", e)))?;
        result
    }

    fn database_size(&self) -> u64 {
        let conn = self.conn.borrow();
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap_or(0);
        let size: i64 = conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .unwrap_or(0);
        (pages * size) as u64
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::fakes::block_on;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        k: String,
        v: i64,
        b: Option<Vec<u8>>,
    }

    #[test]
    fn d1_batch_is_one_transaction() {
        let db = FakeD1::new();
        db.exec_script("CREATE TABLE t (k TEXT PRIMARY KEY, v INTEGER, b BLOB);")
            .unwrap();
        let ins = |k: &str, v: i64| {
            Stmt::new(
                "INSERT INTO t (k, v) VALUES (?1, ?2)",
                vec![k.into(), v.into()],
            )
        };
        block_on(db.batch(vec![ins("a", 1), ins("b", 2)])).unwrap();
        let e = block_on(db.batch(vec![ins("c", 3), ins("a", 9)])).unwrap_err();
        assert_eq!(e.kind, PlatformErrorKind::Conflict);
        let rows: Vec<Row> =
            block_on(db.query(Stmt::new("SELECT k, v, b FROM t ORDER BY k", vec![]))).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.k.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        let meta = block_on(db.execute(Stmt::new(
            "UPDATE t SET b = ?1 WHERE k = 'a'",
            vec![vec![1u8, 2].into()],
        )))
        .unwrap();
        assert_eq!(meta.changes, 1);
        let a: Option<Row> =
            block_on(db.first(Stmt::new("SELECT k, v, b FROM t WHERE k = 'a'", vec![]))).unwrap();
        assert_eq!(a.unwrap().b, Some(vec![1, 2]));
        let big = Stmt::new("SELECT ?1", vec![SqlValue::Int(1 << 53)]);
        assert_eq!(
            block_on(db.query::<Value>(big)).unwrap_err().kind,
            PlatformErrorKind::Invalid
        );
    }

    /// The fake behaves as `platform::cf::sql::transaction_rolls_back` checks the runtime does.
    #[test]
    fn sql_transaction_rolls_back_and_fts5_works() {
        let sql = FakeSql::new();
        sql.exec(
            "CREATE TABLE t (x INTEGER); CREATE VIRTUAL TABLE f USING fts5(body);",
            &[],
        )
        .unwrap();
        let r: Result<(), PlatformError> = sql.transaction_sync(|s| {
            s.exec("INSERT INTO t (x) VALUES (1)", &[])?;
            Err(PlatformError::internal("test", "deliberate"))
        });
        assert!(r.is_err());
        let n: Vec<Value> = sql.query("SELECT COUNT(*) AS n FROM t", &[]).unwrap();
        assert_eq!(n[0]["n"], 0);
        let ok: Result<u32, PlatformError> = sql.transaction_sync(|s| {
            s.exec("INSERT INTO t (x) VALUES (2)", &[])?;
            s.transaction_sync(|s| s.exec("INSERT INTO t (x) VALUES (3)", &[]).map(|_| 0))?;
            Ok(2)
        });
        assert_eq!(ok, Ok(2));
        let n: Option<Value> = sql.first("SELECT COUNT(*) AS n FROM t", &[]).unwrap();
        assert_eq!(n.unwrap()["n"], 2);
        assert_eq!(
            sql.exec("BEGIN", &[]).unwrap_err().kind,
            PlatformErrorKind::Invalid
        );
        sql.exec(
            "INSERT INTO f (body) VALUES (?1)",
            &["the quick brown fox".into()],
        )
        .unwrap();
        let hits: Vec<Value> = sql
            .query("SELECT body FROM f WHERE f MATCH ?1", &["\"brown\"".into()])
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(sql.database_size() > 0);
        sql.drop_everything().unwrap();
        assert!(sql.query::<Value>("SELECT * FROM t", &[]).is_err());
    }
}
