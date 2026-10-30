//! Durable Objects: calling them ([`ObjectClient`]), and inside one ([`Sql`], [`ObjectState`],
//! [`Hibernation`]). Objects are called only with an [`RpcEnvelope`] posted to
//! `https://do.internal/rpc`; a handled result, success or error, comes back as HTTP 200 with an
//! [`RpcResult`] ([Design › Durable Object RPC]).
//!
//! [Design › Durable Object RPC]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/index.md

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::app::{HttpRequestIn, HttpResponseOut};
use crate::d1::{ExecMeta, SqlValue};
use crate::{PResult, PlatformError};

/// The seven classes, by binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DoClass {
    Mailbox,
    Domain,
    Job,
    Quota,
    SesControl,
    Notifier,
    Events,
}

impl DoClass {
    pub const ALL: [DoClass; 7] = [
        DoClass::Mailbox,
        DoClass::Domain,
        DoClass::Job,
        DoClass::Quota,
        DoClass::SesControl,
        DoClass::Notifier,
        DoClass::Events,
    ];

    /// The namespace binding (Configuration › Bindings).
    pub const fn binding(self) -> &'static str {
        match self {
            DoClass::Mailbox => "MAILBOX",
            DoClass::Domain => "DOMAINS",
            DoClass::Job => "JOBS",
            DoClass::Quota => "QUOTA",
            DoClass::SesControl => "SES_CONTROL",
            DoClass::Notifier => "NOTIFY",
            DoClass::Events => "EVENTS",
        }
    }

    /// The exported class name (`new_sqlite_classes`).
    pub const fn class_name(self) -> &'static str {
        match self {
            DoClass::Mailbox => "IdentityMailbox",
            DoClass::Domain => "DomainMonitor",
            DoClass::Job => "JobRunner",
            DoClass::Quota => "TenantQuota",
            DoClass::SesControl => "SesControl",
            DoClass::Notifier => "Notifier",
            DoClass::Events => "TenantEvents",
        }
    }
}

/// The URL of every RPC call; the host is never resolved.
pub const RPC_URL: &str = "https://do.internal/rpc";
/// The URL of the event-stream upgrade (`TenantEvents` only, Event feed § 7.1).
pub const STREAM_URL: &str = "https://do.internal/stream";
/// The header carrying the envelope of an upgrade: base64url of its JSON.
pub const RPC_HEADER: &str = "X-PM-Rpc";

/// Every call into an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcEnvelope<T> {
    /// `1`.
    pub v: u8,
    /// `None` only for platform-level `DomainMonitor` and `SesControl` calls.
    pub tenant_id: Option<String>,
    /// Required for `IdentityMailbox`.
    pub identity_id: Option<String>,
    /// `req_…`.
    pub request_id: String,
    /// For audit.
    pub actor_key_id: Option<String>,
    /// Absolute, in Unix milliseconds; the object refuses work past it.
    pub deadline_ms: i64,
    pub req: T,
}

/// The API error body an object answers with (`{"error": {…}}`).
pub type ApiErrorBody = pylota_identity_api_types::errors::Error;

/// A handled result: `{"ok": …}` or `{"err": {"error": …}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcResult<T> {
    Ok(T),
    Err(Box<ApiErrorBody>),
}

pub trait ObjectClient {
    /// A new object's ID, in the EU jurisdiction when `PM_JURISDICTION = eu` (§6.4): hex, stored in D1
    /// with the owning row and always re-addressed with `id_from_string`.
    fn new_object_id(&self, class: DoClass) -> PResult<String>;
    async fn call<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        class: DoClass,
        object_id: &str,
        envelope: &RpcEnvelope<Req>,
    ) -> PResult<RpcResult<Resp>>;
    /// Forwards a WebSocket upgrade to the object; the runtime's 101 comes back unchanged
    /// (Event feed § 7.1).
    async fn upgrade<Req: Serialize>(
        &self,
        class: DoClass,
        object_id: &str,
        req: HttpRequestIn,
        envelope: &RpcEnvelope<Req>,
    ) -> PResult<HttpResponseOut>;
}

/// `ctx.storage.sql` of the current object.
pub trait Sql {
    fn exec(&self, sql: &str, params: &[SqlValue]) -> PResult<ExecMeta>;
    fn query<T: DeserializeOwned>(&self, sql: &str, params: &[SqlValue]) -> PResult<Vec<T>>;
    /// `ctx.storage.transactionSync`: commits if `f` returns `Ok`, rolls back if it returns `Err`.
    /// `f` is synchronous: no `.await`, no subrequests (Design › Durable Object transactions).
    fn transaction_sync<R, E: From<PlatformError>>(
        &self,
        f: impl FnOnce(&Self) -> Result<R, E>,
    ) -> Result<R, E>;
    fn database_size(&self) -> u64;

    /// At most one row.
    fn first<T: DeserializeOwned>(&self, sql: &str, params: &[SqlValue]) -> PResult<Option<T>> {
        let mut rows = self.query(sql, params)?;
        match rows.len() {
            0 | 1 => Ok(rows.pop()),
            n => Err(PlatformError::internal(
                "sql",
                format!("{n} rows where at most one was expected"),
            )),
        }
    }
}

/// The SQL statements `ctx.storage.sql.exec()` refuses: transactions go through `transaction_sync`.
pub fn refuses_statement(sql: &str) -> bool {
    let first = sql
        .trim_start()
        .split(|c: char| c.is_whitespace() || c == ';')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(
        first.as_str(),
        "BEGIN" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "END"
    )
}

/// The rest of an object's storage.
pub trait ObjectState {
    async fn get_alarm(&self) -> PResult<Option<i64>>;
    /// An absolute time in Unix milliseconds.
    async fn set_alarm(&self, at_ms: i64) -> PResult<()>;
    /// Deletes every row and the alarm (compatibility date ≥ 2026-02-24).
    async fn delete_all(&self) -> PResult<()>;
    /// Point-in-time recovery of the object's SQLite storage (Observability § 9, Restore from PITR):
    /// the bookmark for `at_ms` (`getBookmarkForTime`), or `bookmark` itself, is restored when the
    /// object next starts (`onNextSessionRestoreBookmark`); answers the bookmark that undoes it. The
    /// caller then ends the session ([`ObjectState::abort`]). Not available in local development.
    async fn restore_on_next_session(
        &self,
        at_ms: Option<i64>,
        bookmark: Option<&str>,
    ) -> PResult<String>;
    /// Ends the object's session (`ctx.abort()`), so it restarts with restored storage.
    fn abort(&self);
}

/// `ctx.acceptWebSocket` and friends (Event feed § 7.9). Natively, `fakes::FakeHibernation` simulates
/// eviction: in-memory state goes, attachments stay.
pub trait Hibernation {
    type Socket: Clone;
    fn accept(&self, ws: &Self::Socket, tags: &[&str]);
    fn sockets(&self) -> Vec<Self::Socket>;
    fn sockets_with_tag(&self, tag: &str) -> Vec<Self::Socket>;
    fn set_auto_response(&self, request: &str, response: &str);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_result_wire_form() {
        let ok: RpcResult<u32> = RpcResult::Ok(3);
        assert_eq!(
            serde_json::to_value(&ok).unwrap(),
            serde_json::json!({ "ok": 3 })
        );
        let back: RpcResult<u32> = serde_json::from_value(serde_json::json!({
            "err": { "error": { "code": "internal_error", "message": "m", "retryable": true, "fix": "f",
                                "request_id": "req_01ARYZ6S410000000000000000",
                                "docs": "https://pylotamail.com/docs/reference/errors#code-internal_error" } }
        }))
        .unwrap();
        assert!(matches!(back, RpcResult::Err(_)));
    }

    #[test]
    fn transaction_statements_are_refused() {
        for sql in [
            "BEGIN",
            "  begin transaction",
            "SAVEPOINT a",
            "COMMIT;",
            "release x",
        ] {
            assert!(refuses_statement(sql), "{sql}");
        }
        for sql in [
            "SELECT 1",
            "INSERT INTO beginnings VALUES (1)",
            "UPDATE t SET end_at = 1",
        ] {
            assert!(!refuses_statement(sql), "{sql}");
        }
    }

    #[test]
    fn classes_and_bindings() {
        assert_eq!(DoClass::ALL.len(), 7);
        assert_eq!(DoClass::Events.class_name(), "TenantEvents");
        assert_eq!(DoClass::Notifier.binding(), "NOTIFY");
    }
}
