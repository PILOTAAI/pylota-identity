//! Re-sends the `rpc_intents` rows left by a call that failed after its D1 change (Design conventions
//! § 9, J21): the call is idempotent at the object (`Init` stores the owner once), so a re-send of one
//! that did arrive changes nothing.

use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{Clock as _, ControlDb as _, ObjectClient as _, Platform};
use serde_json::Value;

use crate::db::intents;
use crate::rpc::{RpcResult, envelope_for};

/// At most this many intents per run.
pub const BATCH: u32 = 50;

pub async fn resend<P: Platform>(p: &P, request_id: &str) {
    let now = p.clock().now_ms();
    let due: Vec<intents::Row> = match p.db().query(intents::due(now, BATCH)).await {
        Ok(rows) => rows,
        Err(_) => {
            crate::log::warn("intents_read_failed", &[]);
            return;
        }
    };
    for row in due {
        let ok = match (row.class.as_str(), row.op.as_str()) {
            ("quota", "init") => {
                let body: Value = serde_json::from_str(&row.body_json).unwrap_or(Value::Null);
                let req: Result<crate::quota::QuotaRequest, _> = serde_json::from_value(body);
                match req {
                    Ok(req) => {
                        let env = envelope_for(
                            p,
                            Some(&row.tenant_id),
                            row.identity_id.as_deref(),
                            request_id,
                            None,
                            req,
                        );
                        matches!(
                            p.objects()
                                .call::<_, Value>(DoClass::Quota, &row.object_id, &env)
                                .await,
                            Ok(RpcResult::Ok(_))
                        )
                    }
                    Err(_) => false,
                }
            }
            // Mailbox intents arrive with M6.
            _ => continue,
        };
        let stmt = if ok {
            intents::done(&row.id)
        } else {
            intents::retry(&row.id, row.attempts, now)
        };
        if p.db().execute(stmt).await.is_err() {
            crate::log::warn("intent_update_failed", &[("intent", row.id.clone().into())]);
        }
        if !ok {
            crate::log::warn(
                "intent_resend_failed",
                &[("intent", row.id.into()), ("attempts", row.attempts.into())],
            );
        }
    }
}
