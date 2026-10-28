//! The Durable Object shells exported by `export_worker!` (wasm32 only).

use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::app::{HttpRequestIn, HttpResponseOut};
use pylota_identity_platform::cf::CfPlatform;
use pylota_identity_platform::cf::glue::{CfObjectState, ObjectApp};
use pylota_identity_platform::{Platform as _, cf::CfSql};
use serde_json::Value;

use crate::errors::ApiError;
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

shell!(MailboxObject, "IdentityMailbox");
shell!(MonitorObject, "DomainMonitor");
shell!(RunnerObject, "JobRunner");
shell!(SesControlObject, "SesControl");
shell!(NotifierObject, "Notifier");
shell!(EventsObject, "TenantEvents");
