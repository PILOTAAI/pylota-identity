//! Durable Object calls after a D1 change (Design conventions § 9): the calls written to `rpc_intents`
//! in the change's batch, sent after commit by the handler and repaired by the every-minute cron.
//! Calls to one object go oldest first, so a mailbox appends an identity's events in the order of
//! their D1 changes.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{Clock as _, ControlDb as _, ObjectClient as _, Platform};
use serde_json::Value;

use crate::db::intents::{self, Row};
use crate::rpc::{RpcResult, envelope_for};

/// What one call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    Ok,
    /// The object answered that its target is erased: the intent is dropped.
    Gone,
    Failed,
}

/// Sends one intent's call.
pub async fn send<P: Platform>(p: &P, request_id: &str, row: &Row) -> Sent {
    let mut body: Value = serde_json::from_str(&row.body_json).unwrap_or(Value::Null);
    // The event's ID is derived from the intent, so every attempt sends the same one (step 1).
    let event_id =
        pylota_identity_core::ids::intent_event_id(&p.secrets().hash_key, &row.id, row.occurred_at);
    match row.op.as_str() {
        "emit_event" => body["event_id"] = event_id.into(),
        "init" if row.class == "mailbox" => body["created_event"]["event_id"] = event_id.into(),
        _ => {}
    }
    let class = match row.class.as_str() {
        "quota" => DoClass::Quota,
        "mailbox" => DoClass::Mailbox,
        _ => return Sent::Failed,
    };
    let env = envelope_for(
        p,
        Some(&row.tenant_id),
        row.identity_id.as_deref(),
        request_id,
        None,
        body,
    );
    match p
        .objects()
        .call::<_, Value>(class, &row.object_id, &env)
        .await
    {
        Ok(RpcResult::Ok(_)) => Sent::Ok,
        Ok(RpcResult::Err(e)) if e.error.code == ErrorCode::IdentityNotFound => Sent::Gone,
        _ => Sent::Failed,
    }
}

/// Records the outcome of a call: deletes the row, or backs it off.
async fn settle<P: Platform>(p: &P, row: &Row, sent: Sent, now: i64) {
    let stmt = match sent {
        Sent::Ok | Sent::Gone => intents::done(&row.id),
        Sent::Failed => intents::retry(&row.id, row.attempts, now),
    };
    if p.db().execute(stmt).await.is_err() {
        crate::log::warn("intent_update_failed", &[("intent", row.id.clone().into())]);
    }
    if sent == Sent::Failed {
        let stuck = now - row.occurred_at > 3_600_000;
        crate::log::warn(
            if stuck {
                "rpc_intent_stuck"
            } else {
                "intent_resend_failed"
            },
            &[
                ("intent", row.id.clone().into()),
                ("attempts", row.attempts.into()),
            ],
        );
    }
}

/// Sends every pending intent of one object, oldest first, stopping at the first failure so a later
/// call never overtakes an earlier one. The handler calls it after its commit.
pub async fn flush_object<P: Platform>(p: &P, request_id: &str, object_id: &str) {
    let rows: Vec<Row> = match p.db().query(intents::of_object(object_id)).await {
        Ok(rows) => rows,
        Err(_) => return,
    };
    let now = p.clock().now_ms();
    for row in rows {
        let sent = send(p, request_id, &row).await;
        settle(p, &row, sent, now).await;
        if sent == Sent::Failed {
            return;
        }
    }
}

/// The every-minute repair (step 3): the objects of up to 100 due rows, oldest `occurred_at` first;
/// each object's pending rows are sent in order, so an earlier row that is not yet due is never
/// overtaken by a later one.
pub async fn resend<P: Platform>(p: &P, request_id: &str) {
    let now = p.clock().now_ms();
    let due: Vec<Row> = match p.db().query(intents::due(now, intents::BATCH)).await {
        Ok(rows) => rows,
        Err(_) => {
            crate::log::warn("intents_read_failed", &[]);
            return;
        }
    };
    let mut seen: Vec<&str> = Vec::new();
    for row in &due {
        if !seen.contains(&row.object_id.as_str()) {
            seen.push(&row.object_id);
            flush_object(p, request_id, &row.object_id).await;
        }
    }
}
