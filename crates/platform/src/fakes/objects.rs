//! Durable Objects: an [`ObjectClient`] that routes calls to handlers the test registers per class
//! (usually a logic module over its own [`FakeState`]), the storage of one object, and hibernating
//! WebSockets.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::sqlite::FakeSql;
use crate::app::{HttpRequestIn, HttpResponseOut, UpgradeResponse, WsMessage};
use crate::durable::{DoClass, Hibernation, ObjectClient, ObjectState, RpcEnvelope, RpcResult};
use crate::{PResult, PlatformError, PlatformErrorKind};

type Handler = Rc<dyn Fn(String, Value) -> Pin<Box<dyn Future<Output = PResult<Value>>>>>;

/// Calls recorded as `(class, object ID, envelope JSON)`.
#[derive(Default)]
pub struct FakeObjects {
    next: Cell<u64>,
    handlers: RefCell<HashMap<DoClass, Handler>>,
    calls: RefCell<Vec<(DoClass, String, Value)>>,
    upgrades: RefCell<Vec<(DoClass, String, Value)>>,
}

impl FakeObjects {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers every call to `class` with `f(object_id, envelope)`, whose value is the JSON of an
    /// `RpcResult`.
    pub fn on<F, Fut>(&self, class: DoClass, f: F)
    where
        F: Fn(String, Value) -> Fut + 'static,
        Fut: Future<Output = PResult<Value>> + 'static,
    {
        self.handlers
            .borrow_mut()
            .insert(class, Rc::new(move |id, env| Box::pin(f(id, env))));
    }

    pub fn calls(&self) -> Vec<(DoClass, String, Value)> {
        self.calls.borrow().clone()
    }

    pub fn upgrades(&self) -> Vec<(DoClass, String, Value)> {
        self.upgrades.borrow().clone()
    }
}

impl ObjectClient for FakeObjects {
    fn new_object_id(&self, _class: DoClass) -> PResult<String> {
        let n = self.next.get() + 1;
        self.next.set(n);
        Ok(format!("{n:064x}"))
    }

    async fn call<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        class: DoClass,
        object_id: &str,
        envelope: &RpcEnvelope<Req>,
    ) -> PResult<RpcResult<Resp>> {
        let binding = class.binding();
        let env = serde_json::to_value(envelope)
            .map_err(|e| PlatformError::invalid(binding, format!("envelope: {e}")))?;
        self.calls
            .borrow_mut()
            .push((class, object_id.to_owned(), env.clone()));
        let handler =
            self.handlers.borrow().get(&class).cloned().ok_or_else(|| {
                PlatformError::unavailable(binding, "no fake handler for this class")
            })?;
        let out = handler(object_id.to_owned(), env).await?;
        serde_json::from_value(out).map_err(|e| {
            PlatformError::new(PlatformErrorKind::Corrupt, binding, format!("result: {e}"))
        })
    }

    async fn upgrade<Req: Serialize>(
        &self,
        class: DoClass,
        object_id: &str,
        _req: HttpRequestIn,
        envelope: &RpcEnvelope<Req>,
    ) -> PResult<HttpResponseOut> {
        let env = serde_json::to_value(envelope)
            .map_err(|e| PlatformError::invalid(class.binding(), format!("envelope: {e}")))?;
        self.upgrades
            .borrow_mut()
            .push((class, object_id.to_owned(), env));
        Ok(HttpResponseOut::Upgrade(UpgradeResponse {
            object_id: object_id.to_owned(),
        }))
    }
}

/// One object's storage: SQL, the alarm, `deleteAll`.
#[derive(Clone, Default)]
pub struct FakeState {
    pub sql: FakeSql,
    alarm: Rc<Cell<Option<i64>>>,
}

impl FakeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The alarm, if set; tests fire it by calling the object's `alarm()` after advancing the clock.
    pub fn alarm(&self) -> Option<i64> {
        self.alarm.get()
    }
}

impl ObjectState for FakeState {
    async fn get_alarm(&self) -> PResult<Option<i64>> {
        Ok(self.alarm.get())
    }

    async fn set_alarm(&self, at_ms: i64) -> PResult<()> {
        self.alarm.set(Some(at_ms));
        Ok(())
    }

    async fn delete_all(&self) -> PResult<()> {
        self.alarm.set(None);
        self.sql.drop_everything()
    }

    async fn restore_on_next_session(
        &self,
        _at_ms: Option<i64>,
        _bookmark: Option<&str>,
    ) -> PResult<String> {
        Err(crate::PlatformError::unavailable(
            "storage",
            "point-in-time recovery is not available here",
        ))
    }

    fn abort(&self) {}
}

/// A WebSocket accepted by a hibernating object: what the object sent, its attachment and how it closed.
#[derive(Clone, Debug)]
pub struct FakeSocket(Rc<SocketInner>);

#[derive(Debug)]
struct SocketInner {
    id: u64,
    attachment: RefCell<Option<Value>>,
    sent: RefCell<Vec<WsMessage>>,
    closed: RefCell<Option<(u16, String)>>,
}

impl FakeSocket {
    pub fn new(id: u64) -> Self {
        Self(Rc::new(SocketInner {
            id,
            attachment: RefCell::new(None),
            sent: RefCell::new(Vec::new()),
            closed: RefCell::new(None),
        }))
    }

    pub fn id(&self) -> u64 {
        self.0.id
    }

    /// `serializeAttachment`: survives hibernation.
    pub fn set_attachment(&self, value: Value) {
        *self.0.attachment.borrow_mut() = Some(value);
    }

    pub fn attachment(&self) -> Option<Value> {
        self.0.attachment.borrow().clone()
    }

    pub fn send(&self, msg: WsMessage) {
        self.0.sent.borrow_mut().push(msg);
    }

    pub fn close(&self, code: u16, reason: &str) {
        *self.0.closed.borrow_mut() = Some((code, reason.to_owned()));
    }

    pub fn sent(&self) -> Vec<WsMessage> {
        self.0.sent.borrow().clone()
    }

    pub fn closed(&self) -> Option<(u16, String)> {
        self.0.closed.borrow().clone()
    }
}

/// An accepted socket with its tags.
type Tagged = (FakeSocket, Vec<String>);

/// The runtime's side of hibernation. It outlives the object: to simulate eviction, drop the object
/// (its in-memory state goes) and build a new one over the same `FakeHibernation` (sockets, tags and
/// attachments stay).
#[derive(Clone, Default)]
pub struct FakeHibernation {
    sockets: Rc<RefCell<Vec<Tagged>>>,
    auto_response: Rc<RefCell<Option<(String, String)>>>,
}

impl FakeHibernation {
    pub fn new() -> Self {
        Self::default()
    }

    /// The auto-response pair, answered by the runtime without waking the object.
    pub fn auto_response(&self) -> Option<(String, String)> {
        self.auto_response.borrow().clone()
    }
}

impl Hibernation for FakeHibernation {
    type Socket = FakeSocket;

    fn accept(&self, ws: &FakeSocket, tags: &[&str]) {
        self.sockets
            .borrow_mut()
            .push((ws.clone(), tags.iter().map(|t| (*t).to_owned()).collect()));
    }

    fn sockets(&self) -> Vec<FakeSocket> {
        self.sockets
            .borrow()
            .iter()
            .filter(|(s, _)| s.closed().is_none())
            .map(|(s, _)| s.clone())
            .collect()
    }

    fn sockets_with_tag(&self, tag: &str) -> Vec<FakeSocket> {
        self.sockets
            .borrow()
            .iter()
            .filter(|(s, tags)| s.closed().is_none() && tags.iter().any(|t| t == tag))
            .map(|(s, _)| s.clone())
            .collect()
    }

    fn set_auto_response(&self, request: &str, response: &str) {
        *self.auto_response.borrow_mut() = Some((request.to_owned(), response.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Sql;
    use crate::fakes::block_on;

    fn envelope(req: Value) -> RpcEnvelope<Value> {
        RpcEnvelope {
            v: 1,
            tenant_id: Some("ten_1".into()),
            identity_id: None,
            request_id: "req_1".into(),
            actor_key_id: None,
            deadline_ms: 0,
            req,
        }
    }

    #[test]
    fn calls_route_to_the_class_handler() {
        let objects = FakeObjects::new();
        let id = objects.new_object_id(DoClass::Quota).unwrap();
        assert_eq!(id.len(), 64);
        assert_ne!(id, objects.new_object_id(DoClass::Quota).unwrap());
        objects.on(DoClass::Quota, |object_id, env| async move {
            Ok(json!({ "ok": { "object": object_id, "op": env["req"]["op"] } }))
        });
        let r: RpcResult<Value> =
            block_on(objects.call(DoClass::Quota, &id, &envelope(json!({ "op": "init" }))))
                .unwrap();
        assert_eq!(r, RpcResult::Ok(json!({ "object": id, "op": "init" })));
        assert_eq!(objects.calls()[0].2["tenant_id"], "ten_1");
        let missing =
            block_on(objects.call::<_, Value>(DoClass::Mailbox, &id, &envelope(json!({}))));
        assert_eq!(missing.unwrap_err().kind, PlatformErrorKind::Unavailable);
    }

    #[test]
    fn state_alarm_and_delete_all() {
        let state = FakeState::new();
        state.sql.exec("CREATE TABLE meta (k TEXT)", &[]).unwrap();
        block_on(state.set_alarm(5)).unwrap();
        assert_eq!(block_on(state.get_alarm()).unwrap(), Some(5));
        block_on(state.delete_all()).unwrap();
        assert_eq!(state.alarm(), None);
        assert!(state.sql.query::<Value>("SELECT * FROM meta", &[]).is_err());
    }

    #[test]
    fn hibernation_keeps_sockets_and_attachments_across_eviction() {
        let hib = FakeHibernation::new();
        let ws = FakeSocket::new(1);
        ws.set_attachment(json!({ "consumer": "evc_1" }));
        {
            // The first object instance accepts the socket, then is evicted (dropped).
            let object_view = hib.clone();
            object_view.accept(&ws, &["tenant:ten_1"]);
            object_view.set_auto_response("ping", "pong");
        }
        let revived = hib.clone();
        let found = revived.sockets_with_tag("tenant:ten_1");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].attachment(), Some(json!({ "consumer": "evc_1" })));
        found[0].send(WsMessage::Text("hello".into()));
        assert_eq!(ws.sent(), [WsMessage::Text("hello".into())]);
        ws.close(1000, "done");
        assert!(revived.sockets().is_empty());
        assert_eq!(
            revived.auto_response(),
            Some(("ping".into(), "pong".into()))
        );
    }
}
