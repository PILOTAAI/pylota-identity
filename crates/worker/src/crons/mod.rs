//! Cron triggers (Configuration › Cron triggers). Every minute: mint the mailboxes setup left to the
//! Worker (the system identity's, Identities and domains › The system identity), then re-send the
//! `rpc_intents` rows (Design conventions § 9), then the webhook sweep (`outbox_sweep`), the
//! master-key re-seal sweep and the state alert evaluator (Observability § 5.4). Every 15
//! minutes: the semantic index's nightly reconciliation (`index_reconcile`). The other steps arrive
//! with their milestones.

pub mod domains;
pub mod index_reconcile;
pub mod outbox_sweep;

use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{Clock as _, ControlDb as _, ObjectClient as _, Platform};
use serde_json::json;

use crate::db::{addresses as adb, identities as idb, intents};
use crate::webhooks::envelope as ev;

/// One `scheduled` invocation.
pub async fn run<P: Platform>(p: &P, cron: &str, request_id: &str) {
    // A restore freeze: the crons only log.
    if p.config().frozen {
        crate::log::info("frozen", &[("detail", cron.to_owned().into())]);
        return;
    }
    if cron == "* * * * *" {
        mint_mailboxes(p, request_id).await;
        domains::mint_monitors(p).await;
        crate::intents::resend(p, request_id).await;
        domains::retire(p).await;
        outbox_sweep::run(p).await;
        crate::jobs::restart_queued(p).await;
        crate::ops::reseal::run(p, request_id).await;
        crate::ops::alerts::evaluate(p, request_id).await;
    }
    if cron == "*/15 * * * *" {
        index_reconcile::run(p).await;
        crate::jobs::retention::schedule(p).await;
        let minute = (p.clock().now_ms() / 60_000) % 60;
        if minute < 15 {
            domains::cleanup_failed_adds(p).await;
        }
    }
}

/// Rows with `mailbox_do_id = ''` get a mailbox: the ID is written only if the row still has none
/// (an overlapping run that lost discards its ID), with the `init` intent in the same batch, so the
/// intent re-send repairs a lost `Init`.
pub async fn mint_mailboxes<P: Platform>(p: &P, request_id: &str) {
    let rows: Vec<idb::Row> = match p.db().query(idb::without_mailbox(20)).await {
        Ok(rows) => rows,
        Err(_) => return,
    };
    let now = p.clock().now_ms();
    for row in rows {
        let Ok(object_id) = p.objects().new_object_id(DoClass::Mailbox) else {
            continue;
        };
        let addrs: Vec<adb::Row> = p
            .db()
            .query(adb::of_identity(&row.id))
            .await
            .unwrap_or_default();
        let mut minted = row.clone();
        minted.mailbox_do_id = object_id.clone();
        let init = json!({
            "op": "init", "created_at": row.created_at, "message_hours": null, "is_system": row.is_system == 1,
            "created_event": { "event_id": "", "event_type": "identity.created", "occurred_at": now, "data": ev::identity_created(&minted.json(&addrs)) },
        });
        let mut intent = intents::insert(
            &format!("{request_id}:{}:init", row.id),
            "mailbox",
            &object_id,
            &row.tenant_id,
            Some(&row.id),
            "init",
            &init.to_string(),
            now,
        );
        intent = intents::when_minted(intent);
        let batch = vec![
            Stmt::new(
                "UPDATE identities SET mailbox_do_id = ?1, updated_at = ?2 WHERE id = ?3 AND mailbox_do_id = ''",
                vec![object_id.clone().into(), now.into(), row.id.clone().into()],
            ),
            intent,
        ];
        match p.db().batch(batch).await {
            Ok(m) if m.first().is_some_and(|m| m.changes == 1) => {
                crate::intents::flush_object(p, request_id, &object_id).await;
            }
            _ => {}
        }
    }
}
