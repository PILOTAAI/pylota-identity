//! D1, the control-plane database (binding `DB`). The tenant-scoped data-access layer that makes
//! `tenant_id` a required parameter lives in `worker::db`, not here.

use serde::de::DeserializeOwned;

use crate::{PResult, PlatformError};

/// A bound parameter or a column value.
#[derive(Clone, Debug, PartialEq)]
pub enum SqlValue {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl From<i64> for SqlValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}
impl From<u32> for SqlValue {
    fn from(v: u32) -> Self {
        Self::Int(i64::from(v))
    }
}
impl From<i32> for SqlValue {
    fn from(v: i32) -> Self {
        Self::Int(i64::from(v))
    }
}
impl From<bool> for SqlValue {
    fn from(v: bool) -> Self {
        Self::Int(i64::from(v))
    }
}
impl From<f64> for SqlValue {
    fn from(v: f64) -> Self {
        Self::Real(v)
    }
}
impl From<&str> for SqlValue {
    fn from(v: &str) -> Self {
        Self::Text(v.to_owned())
    }
}
impl From<String> for SqlValue {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}
impl From<&String> for SqlValue {
    fn from(v: &String) -> Self {
        Self::Text(v.clone())
    }
}
impl From<Vec<u8>> for SqlValue {
    fn from(v: Vec<u8>) -> Self {
        Self::Blob(v)
    }
}
impl<T: Into<SqlValue>> From<Option<T>> for SqlValue {
    fn from(v: Option<T>) -> Self {
        v.map_or(Self::Null, Into::into)
    }
}

/// One statement. The SQL is a literal: values are always bound, never formatted in.
#[derive(Clone, Debug)]
pub struct Stmt {
    pub sql: &'static str,
    pub params: Vec<SqlValue>,
}

impl Stmt {
    pub fn new(sql: &'static str, params: Vec<SqlValue>) -> Self {
        Self { sql, params }
    }
}

/// What a write did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecMeta {
    pub changes: u64,
    pub last_row_id: Option<i64>,
}

pub trait ControlDb {
    async fn query<T: DeserializeOwned>(&self, stmt: Stmt) -> PResult<Vec<T>>;
    async fn first<T: DeserializeOwned>(&self, stmt: Stmt) -> PResult<Option<T>>;
    async fn execute(&self, stmt: Stmt) -> PResult<ExecMeta>;
    /// One SQL transaction: all statements apply or none do.
    async fn batch(&self, stmts: Vec<Stmt>) -> PResult<Vec<ExecMeta>>;
}

/// The largest integer a JS number holds exactly; D1 and Durable Object SQL bind through JS numbers.
pub const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

/// D1's limit on bound parameters per statement.
pub const MAX_D1_PARAMS: usize = 100;

/// Refuses parameters a JS number cannot carry exactly (`Invalid`), and more than `max` of them.
pub fn check_params(binding: &'static str, params: &[SqlValue], max: usize) -> PResult<()> {
    if params.len() > max {
        return Err(PlatformError::invalid(
            binding,
            format!("{} bound parameters; at most {max}", params.len()),
        ));
    }
    for (i, p) in params.iter().enumerate() {
        if let SqlValue::Int(v) = p
            && !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(v)
        {
            return Err(PlatformError::invalid(
                binding,
                format!("parameter {} is outside ±2^53−1", i + 1),
            ));
        }
    }
    Ok(())
}

/// The kind of a database error from its message, as D1 and SQLite word it: a constraint failure is a
/// `Conflict`, a statement the schema refuses (`no such …`, a syntax error) `Internal`, anything else
/// `Unavailable`.
pub fn classify(binding: &'static str, message: &str) -> PlatformError {
    let kind = if message.contains("constraint failed") {
        crate::PlatformErrorKind::Conflict
    } else if message.contains("no such ") || message.contains("syntax error") {
        crate::PlatformErrorKind::Internal
    } else {
        crate::PlatformErrorKind::Unavailable
    };
    // The message is SQLite's text: table and column names, never values.
    PlatformError::new(kind, binding, message.chars().take(300).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_past_2_53_are_refused() {
        assert!(check_params("DB", &[SqlValue::Int(MAX_SAFE_INTEGER)], 100).is_ok());
        let e = check_params("DB", &[1.into(), SqlValue::Int(1 << 53)], 100).unwrap_err();
        assert_eq!(e.kind, crate::PlatformErrorKind::Invalid);
        assert!(e.detail.contains("parameter 2"));
        assert!(check_params("DB", &vec![SqlValue::Null; 101], MAX_D1_PARAMS).is_err());
    }

    #[test]
    fn constraint_failures_are_conflicts() {
        let e = classify(
            "DB",
            "D1_ERROR: UNIQUE constraint failed: tenants.slug: SQLITE_CONSTRAINT",
        );
        assert_eq!(e.kind, crate::PlatformErrorKind::Conflict);
        assert_eq!(
            classify("DB", "D1_ERROR: no such table: x").kind,
            crate::PlatformErrorKind::Internal
        );
        assert_eq!(
            classify("DB", "D1_ERROR: Network connection lost.").kind,
            crate::PlatformErrorKind::Unavailable
        );
    }
}
