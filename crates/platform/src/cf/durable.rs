//! Durable Objects: the client (`id_from_string`, `fetch` with the RPC envelope), and inside an object
//! its SQL, alarm and hibernating WebSockets.

use std::cell::RefCell;

use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;

use super::externs::DurableObjectStorageExt;
use super::js;
use crate::app::{HttpRequestIn, HttpResponseOut, UpgradeResponse};
use crate::config::Jurisdiction;
use crate::d1::{ExecMeta, SqlValue, check_params};
use crate::durable::{
    DoClass, Hibernation, ObjectClient, ObjectState, RPC_HEADER, RPC_URL, RpcEnvelope, RpcResult,
    STREAM_URL, Sql, refuses_statement,
};
use crate::{PResult, PlatformError, PlatformErrorKind};

#[derive(Clone)]
pub struct CfObjects {
    pub(crate) env: worker::Env,
    pub(crate) jurisdiction: Jurisdiction,
}

impl CfObjects {
    fn stub(&self, class: DoClass, object_id: &str) -> PResult<worker::Stub> {
        let binding = class.binding();
        let ns = self
            .env
            .durable_object(binding)
            .map_err(|e| js::worker_error(binding, e))?;
        ns.id_from_string(object_id)
            .and_then(|id| id.get_stub())
            .map_err(|e| PlatformError::invalid(binding, e.to_string()))
    }
}

impl ObjectClient for CfObjects {
    fn new_object_id(&self, class: DoClass) -> PResult<String> {
        let binding = class.binding();
        let ns = self
            .env
            .durable_object(binding)
            .map_err(|e| js::worker_error(binding, e))?;
        let id = match self.jurisdiction {
            Jurisdiction::Eu => ns.unique_id_with_jurisdiction("eu"),
            Jurisdiction::Default => ns.unique_id(),
        }
        .map_err(|e| js::worker_error(binding, e))?;
        Ok(id.to_string())
    }

    async fn call<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        class: DoClass,
        object_id: &str,
        envelope: &RpcEnvelope<Req>,
    ) -> PResult<RpcResult<Resp>> {
        let binding = class.binding();
        // A fault armed for `{binding}:{object_id}` slows or fails that one object; one armed for the
        // binding, any of its objects.
        #[cfg(feature = "itest-hooks")]
        let fault =
            match super::itest::take_fault("do.call", &format!("{binding}:{object_id}")).await {
                Some(m) => Some(m),
                None => super::itest::take_fault("do.call", binding).await,
            };
        #[cfg(feature = "itest-hooks")]
        match fault {
            // `delay:{ms}`: a slow object (F15), then the call as usual.
            Some(mode) if mode.starts_with("delay:") => {
                let ms: u64 = mode["delay:".len()..].parse().unwrap_or(2_000);
                worker::Delay::from(std::time::Duration::from_millis(ms)).await;
            }
            Some(_) => {
                return Err(PlatformError::unavailable(
                    binding,
                    "fault injected by the test",
                ));
            }
            None => {}
        }
        let body = serde_json::to_vec(envelope)
            .map_err(|e| PlatformError::invalid(binding, format!("envelope: {e}")))?;
        let headers = worker::Headers::new();
        let _ = headers.set("content-type", "application/json");
        let mut init = worker::RequestInit::new();
        init.with_method(worker::Method::Post)
            .with_headers(headers)
            .with_body(Some(js_sys::Uint8Array::from(body.as_slice()).into()));
        let req = worker::Request::new_with_init(RPC_URL, &init)
            .map_err(|e| js::worker_error(binding, e))?;
        let mut resp = self
            .stub(class, object_id)?
            .fetch_with_request(req)
            .await
            .map_err(|e| js::worker_error(binding, e))?;
        if resp.status_code() != 200 {
            return Err(PlatformError::unavailable(
                binding,
                format!("object answered {}", resp.status_code()),
            ));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| js::worker_error(binding, e))?;
        serde_json::from_slice(&bytes).map_err(|e| {
            PlatformError::new(PlatformErrorKind::Corrupt, binding, format!("result: {e}"))
        })
    }

    async fn upgrade<Req: Serialize>(
        &self,
        class: DoClass,
        object_id: &str,
        req: HttpRequestIn,
        envelope: &RpcEnvelope<Req>,
    ) -> PResult<HttpResponseOut> {
        let binding = class.binding();
        let rpc = serde_json::to_vec(envelope)
            .map_err(|e| PlatformError::invalid(binding, format!("envelope: {e}")))?;
        let headers = worker::Headers::new();
        for (name, value) in &req.headers {
            let _ = headers.append(name, value);
        }
        let _ = headers.set(RPC_HEADER, &crate::base64::encode_url(&rpc));
        let mut init = worker::RequestInit::new();
        init.with_method(worker::Method::Get).with_headers(headers);
        let forwarded = worker::Request::new_with_init(STREAM_URL, &init)
            .map_err(|e| js::worker_error(binding, e))?;
        let resp = self
            .stub(class, object_id)?
            .fetch_with_request(forwarded)
            .await
            .map_err(|e| js::worker_error(binding, e))?;
        if resp.status_code() == 101 {
            Ok(HttpResponseOut::Upgrade(UpgradeResponse {
                response: resp.into(),
            }))
        } else {
            let status = resp.status_code();
            let headers = resp.headers().entries().collect();
            let mut resp = resp;
            let body = resp.bytes().await.unwrap_or_default();
            Ok(HttpResponseOut::Http {
                status,
                headers,
                body,
            })
        }
    }
}

/// `ctx.storage.sql`.
pub struct CfSql {
    pub(crate) sql: worker::SqlStorage,
    pub(crate) storage: worker::Storage,
}

fn sql_value(v: &SqlValue) -> worker::SqlStorageValue {
    match v {
        SqlValue::Null => worker::SqlStorageValue::Null,
        SqlValue::Int(i) => worker::SqlStorageValue::Integer(*i),
        SqlValue::Real(r) => worker::SqlStorageValue::Float(*r),
        SqlValue::Text(t) => worker::SqlStorageValue::String(t.clone()),
        SqlValue::Blob(b) => worker::SqlStorageValue::Blob(b.clone()),
    }
}

fn sql_err(e: worker::Error) -> PlatformError {
    crate::d1::classify("sql", &e.to_string())
}

impl CfSql {
    fn cursor(&self, sql: &str, params: &[SqlValue]) -> PResult<worker::SqlCursor> {
        check_params("sql", params, usize::MAX)?;
        let bindings: Vec<worker::SqlStorageValue> = params.iter().map(sql_value).collect();
        self.sql.exec(sql, bindings).map_err(sql_err)
    }
}

impl Sql for CfSql {
    fn exec(&self, sql: &str, params: &[SqlValue]) -> PResult<ExecMeta> {
        if refuses_statement(sql) {
            return Err(PlatformError::invalid(
                "sql",
                "transactions go through transaction_sync",
            ));
        }
        let cursor = self.cursor(sql, params)?;
        // Drain the cursor so every statement runs; never `one()` (it throws on zero or several rows).
        let _: Vec<serde_json::Value> = cursor.to_array().map_err(sql_err)?;
        Ok(ExecMeta {
            size_after: None,
            changes: cursor.rows_written() as u64,
            last_row_id: None,
        })
    }

    fn query<T: DeserializeOwned>(&self, sql: &str, params: &[SqlValue]) -> PResult<Vec<T>> {
        self.cursor(sql, params)?.to_array().map_err(sql_err)
    }

    fn transaction_sync<R, E: From<PlatformError>>(
        &self,
        f: impl FnOnce(&Self) -> Result<R, E>,
    ) -> Result<R, E> {
        let outcome: RefCell<Option<Result<R, E>>> = RefCell::new(None);
        let mut f = Some(f);
        let mut body = || -> Result<JsValue, JsValue> {
            let f = f
                .take()
                .ok_or_else(|| JsValue::from_str("transaction body ran twice"))?;
            let result = f(self);
            let failed = result.is_err();
            *outcome.borrow_mut() = Some(result);
            if failed {
                // Thrown inside the callback, so the runtime rolls back.
                Err(js_sys::Error::new("rollback").into())
            } else {
                Ok(JsValue::UNDEFINED)
            }
        };
        let ext: &DurableObjectStorageExt = self.storage.as_raw().unchecked_ref();
        let ran = ext.transaction_sync(&mut body);
        match (ran, outcome.into_inner()) {
            (Ok(_), Some(result)) => result,
            (Err(_), Some(Err(e))) => Err(e),
            (Err(e), _) => Err(E::from(js::error("sql", &e))),
            (Ok(_), None) => Err(E::from(PlatformError::internal(
                "sql",
                "transactionSync returned without running the callback",
            ))),
        }
    }

    fn database_size(&self) -> u64 {
        self.sql.database_size() as u64
    }
}

/// The alarm and `deleteAll`.
pub struct CfStorage {
    pub(crate) storage: worker::Storage,
    /// The raw `DurableObjectState`, for the calls `workers-rs` 0.8.7 does not wrap.
    pub(crate) raw_state: JsValue,
}

impl ObjectState for CfStorage {
    async fn get_alarm(&self) -> PResult<Option<i64>> {
        self.storage
            .get_alarm()
            .await
            .map_err(|e| js::worker_error("storage", e))
    }

    async fn set_alarm(&self, at_ms: i64) -> PResult<()> {
        let at = js_sys::Date::new(&JsValue::from_f64(at_ms as f64));
        self.storage
            .set_alarm(worker::ScheduledTime::new(at))
            .await
            .map_err(|e| js::worker_error("storage", e))
    }

    async fn delete_all(&self) -> PResult<()> {
        self.storage
            .delete_all()
            .await
            .map_err(|e| js::worker_error("storage", e))
    }

    async fn restore_on_next_session(
        &self,
        at_ms: Option<i64>,
        bookmark: Option<&str>,
    ) -> PResult<String> {
        let storage = js::get(&self.raw_state, "storage").map_err(|e| js::error("storage", &e))?;
        let bookmark = match (at_ms, bookmark) {
            (_, Some(b)) => JsValue::from_str(b),
            (Some(at), None) => {
                let at = js_sys::Date::new(&JsValue::from_f64(at as f64));
                js::call_async(&storage, "getBookmarkForTime", &[&at])
                    .await
                    .map_err(|e| js::error("storage", &e))?
            }
            (None, None) => {
                return Err(crate::PlatformError::invalid(
                    "storage",
                    "a time or a bookmark",
                ));
            }
        };
        let undo = js::call_async(&storage, "onNextSessionRestoreBookmark", &[&bookmark])
            .await
            .map_err(|e| js::error("storage", &e))?;
        undo.as_string()
            .ok_or_else(|| crate::PlatformError::invalid("storage", "no undo bookmark"))
    }

    fn abort(&self) {
        let _ = js::call(&self.raw_state, "abort", &[]);
    }
}

/// A WebSocket accepted by a hibernating object (`TenantEvents`).
#[derive(Clone)]
pub struct CfWebSocket(pub(crate) worker::WebSocket);

impl CfWebSocket {
    pub fn send_text(&self, text: &str) -> PResult<()> {
        self.0
            .send_with_str(text)
            .map_err(|e| js::worker_error("websocket", e))
    }

    pub fn send_bytes(&self, bytes: &[u8]) -> PResult<()> {
        self.0
            .send_with_bytes(bytes)
            .map_err(|e| js::worker_error("websocket", e))
    }

    pub fn close(&self, code: u16, reason: &str) -> PResult<()> {
        self.0
            .close(Some(code), Some(reason))
            .map_err(|e| js::worker_error("websocket", e))
    }

    /// `serializeAttachment`: survives hibernation (at most 2,048 bytes).
    pub fn set_attachment<T: Serialize>(&self, value: &T) -> PResult<()> {
        self.0
            .serialize_attachment(value)
            .map_err(|e| js::worker_error("websocket", e))
    }

    pub fn attachment<T: DeserializeOwned>(&self) -> PResult<Option<T>> {
        self.0
            .deserialize_attachment()
            .map_err(|e| js::worker_error("websocket", e))
    }
}

/// `ctx.acceptWebSocket` and friends.
pub struct CfHibernation {
    pub(crate) state: worker::State,
}

impl CfHibernation {
    pub(crate) fn new(raw: &worker::worker_sys::DurableObjectState) -> Self {
        Self {
            state: worker::State::from(super::basic::dup_state(raw)),
        }
    }
}

impl Hibernation for CfHibernation {
    type Socket = CfWebSocket;

    fn accept(&self, ws: &CfWebSocket, tags: &[&str]) {
        self.state.accept_websocket_with_tags(&ws.0, tags);
    }

    fn sockets(&self) -> Vec<CfWebSocket> {
        self.state
            .get_websockets()
            .into_iter()
            .map(CfWebSocket)
            .collect()
    }

    fn sockets_with_tag(&self, tag: &str) -> Vec<CfWebSocket> {
        self.state
            .get_websockets_with_tag(tag)
            .into_iter()
            .map(CfWebSocket)
            .collect()
    }

    fn set_auto_response(&self, request: &str, response: &str) {
        if let Ok(pair) = worker::worker_sys::WebSocketRequestResponsePair::new(request, response) {
            self.state.set_websocket_auto_response(&pair);
        }
    }
}
