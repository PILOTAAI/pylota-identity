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

/// `DomainMonitor`.
pub struct MonitorObject {
    platform: CfPlatform,
    sql: CfSql,
    storage: CfStorage,
}

impl MonitorObject {
    fn monitor(&self) -> crate::domains::monitor::Monitor<'_, CfPlatform, CfSql, CfStorage> {
        crate::domains::monitor::Monitor {
            p: &self.platform,
            sql: &self.sql,
            state: &self.storage,
        }
    }
}

impl ObjectApp for MonitorObject {
    fn new(state: CfObjectState, platform: CfPlatform) -> Self {
        Self {
            sql: state.sql(),
            storage: state.storage(),
            platform,
        }
    }

    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        json(200, &self.monitor().handle(&body_of(&req)).await)
    }

    async fn alarm(&self) {
        self.monitor().alarm().await;
    }
}

/// `SesControl`: the deployment's SES token bucket.
pub struct SesControlObject {
    platform: CfPlatform,
    sql: CfSql,
}

impl ObjectApp for SesControlObject {
    fn new(state: CfObjectState, platform: CfPlatform) -> Self {
        Self {
            sql: state.sql(),
            platform,
        }
    }

    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        let c = crate::domains::ses_control::SesControl {
            p: &self.platform,
            sql: &self.sql,
        };
        json(200, &c.handle(&body_of(&req)))
    }
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

/// `JobRunner`.
pub struct RunnerObject {
    platform: CfPlatform,
    sql: CfSql,
    storage: CfStorage,
}

impl RunnerObject {
    fn runner(&self) -> crate::jobs::Runner<'_, CfPlatform, CfSql, CfStorage> {
        crate::jobs::Runner {
            p: &self.platform,
            sql: &self.sql,
            state: &self.storage,
        }
    }
}

impl ObjectApp for RunnerObject {
    fn new(state: CfObjectState, platform: CfPlatform) -> Self {
        Self {
            sql: state.sql(),
            storage: state.storage(),
            platform,
        }
    }

    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        json(200, &self.runner().handle(&body_of(&req)).await)
    }

    async fn alarm(&self) {
        self.runner().alarm().await;
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

shell!(NotifierObject, "Notifier");
shell!(EventsObject, "TenantEvents");
