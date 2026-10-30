//! D1 over `worker::D1Database`. Integers are bound as JS numbers, so values outside ±2^53−1 are
//! refused before the call (`d1::check_params`).

use serde::de::DeserializeOwned;
use wasm_bindgen::JsValue;

use crate::d1::{ControlDb, ExecMeta, MAX_D1_PARAMS, SqlValue, Stmt, check_params, classify};
use crate::{PResult, PlatformError};

const BINDING: &str = "DB";

#[derive(Clone)]
pub struct CfD1 {
    pub(crate) env: worker::Env,
}

fn js(v: &SqlValue) -> JsValue {
    match v {
        SqlValue::Null => JsValue::NULL,
        SqlValue::Int(i) => JsValue::from_f64(*i as f64),
        SqlValue::Real(r) => JsValue::from_f64(*r),
        SqlValue::Text(t) => JsValue::from_str(t),
        SqlValue::Blob(b) => js_sys::Uint8Array::from(b.as_slice()).into(),
    }
}

fn err(e: worker::Error) -> PlatformError {
    classify(BINDING, &e.to_string())
}

impl CfD1 {
    fn db(&self) -> PResult<worker::D1Database> {
        self.env
            .d1(BINDING)
            .map_err(|e| PlatformError::unavailable(BINDING, e.to_string()))
    }

    fn prepare(
        &self,
        db: &worker::D1Database,
        stmt: &Stmt,
    ) -> PResult<worker::D1PreparedStatement> {
        check_params(BINDING, &stmt.params, MAX_D1_PARAMS)?;
        let params: Vec<JsValue> = stmt.params.iter().map(js).collect();
        db.prepare(stmt.sql).bind(&params).map_err(err)
    }
}

fn meta(result: &worker::D1Result) -> PResult<ExecMeta> {
    let m = result.meta().map_err(err)?;
    #[allow(unused_mut)]
    let mut size_after = m.as_ref().and_then(|m| m.size_after).map(|s| s as u64);
    #[cfg(feature = "itest-hooks")]
    if let Some(s) = super::itest::d1_size() {
        size_after = Some(s);
    }
    Ok(ExecMeta {
        changes: m.as_ref().and_then(|m| m.changes).unwrap_or(0) as u64,
        last_row_id: m.and_then(|m| m.last_row_id),
        size_after,
    })
}

impl ControlDb for CfD1 {
    async fn query<T: DeserializeOwned>(&self, stmt: Stmt) -> PResult<Vec<T>> {
        #[cfg(feature = "itest-hooks")]
        if super::itest::take_fault("d1", BINDING).await.is_some() {
            return Err(super::itest::fault_error(BINDING));
        }
        let db = self.db()?;
        let result = self.prepare(&db, &stmt)?.all().await.map_err(err)?;
        if let Some(e) = result.error() {
            return Err(classify(BINDING, &e));
        }
        result.results::<T>().map_err(|e| {
            PlatformError::new(
                crate::PlatformErrorKind::Corrupt,
                BINDING,
                format!("row: {e}"),
            )
        })
    }

    async fn first<T: DeserializeOwned>(&self, stmt: Stmt) -> PResult<Option<T>> {
        Ok(self.query(stmt).await?.into_iter().next())
    }

    async fn execute(&self, stmt: Stmt) -> PResult<ExecMeta> {
        let db = self.db()?;
        let result = self.prepare(&db, &stmt)?.run().await.map_err(err)?;
        if let Some(e) = result.error() {
            return Err(classify(BINDING, &e));
        }
        meta(&result)
    }

    async fn batch(&self, stmts: Vec<Stmt>) -> PResult<Vec<ExecMeta>> {
        let db = self.db()?;
        let prepared = stmts
            .iter()
            .map(|s| self.prepare(&db, s))
            .collect::<PResult<Vec<_>>>()?;
        let results = db.batch(prepared).await.map_err(err)?;
        results.iter().map(meta).collect()
    }
}
