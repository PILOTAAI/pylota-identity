//! The Durable Object shells exported by `export_worker!` (wasm32 only).

use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::Platform as _;
use pylota_identity_platform::app::{HttpRequestIn, HttpResponseOut};
use pylota_identity_platform::cf::CfPlatform;
use pylota_identity_platform::cf::glue::{CfObjectState, ObjectApp};
use pylota_identity_platform::cf::{CfSql, CfStorage};
use serde_json::Value;

use crate::errors::ApiError;
use crate::mailbox::Mailbox;
use crate::quota::Quota;
use crate::rpc::error_body;

fn json(status: u16, v: &Value) -> HttpResponseOut {
    HttpResponseOut::Http {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: v.to_string().into_bytes(),
    }
}

/// `TenantQuota`.
pub struct QuotaObject {
    platform: Rc<CfPlatform>,
    sql: CfSql,
}

impl ObjectApp for QuotaObject {
    fn new(state: CfObjectState, platform: CfPlatform) -> Self {
        Self {
            sql: state.sql(),
            platform: Rc::new(platform),
        }
    }

    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        let body: Value = req
            .body
            .as_ref()
            .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
            .unwrap_or(Value::Null);
        let q = Quota {
            p: self.platform.as_ref(),
            sql: &self.sql,
        };
        json(200, &q.handle(&body))
    }
}

fn body_of(req: &HttpRequestIn) -> Value {
    req.body
        .as_ref()
        .and_then(|b| serde_json::from_slice(&b.to_vec()).ok())
        .unwrap_or(Value::Null)
}

/// `IdentityMailbox`.
pub struct MailboxObject {
    platform: Rc<CfPlatform>,
    sql: CfSql,
    storage: CfStorage,
    id: String,
}

impl MailboxObject {
    fn mailbox(&self) -> Mailbox<'_, CfPlatform, CfSql, CfStorage> {
        Mailbox {
            p: self.platform.as_ref(),
            sql: &self.sql,
            state: &self.storage,
            object_id: &self.id,
        }
    }
}

impl ObjectApp for MailboxObject {
    fn new(state: CfObjectState, platform: CfPlatform) -> Self {
        Self {
            sql: state.sql(),
            storage: state.storage(),
            id: state.id(),
            platform: Rc::new(platform),
        }
    }

    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        json(200, &self.mailbox().handle(&body_of(&req)).await)
    }

    async fn alarm(&self) {
        self.mailbox().alarm().await;
    }
}

/// `JobRunner`, a stub until M14: `Start` is accepted and runs nothing.
pub struct RunnerObject {
    platform: CfPlatform,
}

impl ObjectApp for RunnerObject {
    fn new(_state: CfObjectState, platform: CfPlatform) -> Self {
        Self { platform }
    }

    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        let env = body_of(&req);
        let out = match crate::jobs::stub(&env) {
            Ok(v) => serde_json::json!({ "ok": v }),
            Err(e) => {
                let rid = env["request_id"].as_str().unwrap_or("");
                serde_json::to_value(error_body::<()>(&e, rid, &self.platform.config().docs_url))
                    .unwrap_or(Value::Null)
            }
        };
        json(200, &out)
    }
}

/// A class whose milestone has not built it yet: every request is `unavailable`.
macro_rules! shell {
    ($name:ident, $class:literal) => {
        #[doc = concat!("`", $class, "` until its milestone.")]
        pub struct $name {
            platform: CfPlatform,
        }

        impl ObjectApp for $name {
            fn new(_state: CfObjectState, platform: CfPlatform) -> Self {
                Self { platform }
            }

            async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
                let request_id = req
                    .body
                    .as_ref()
                    .and_then(|b| serde_json::from_slice::<Value>(&b.to_vec()).ok())
                    .and_then(|v| v["request_id"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                let e = ApiError::new(
                    ErrorCode::Unavailable,
                    concat!($class, " is not built yet."),
                );
                let out = error_body::<()>(&e, &request_id, &self.platform.config().docs_url);
                json(200, &serde_json::to_value(out).unwrap_or(Value::Null))
            }
        }
    };
}

shell!(MonitorObject, "DomainMonitor");
shell!(SesControlObject, "SesControl");
shell!(NotifierObject, "Notifier");
shell!(EventsObject, "TenantEvents");
