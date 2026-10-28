//! The domain steps of the crons ([Identities and domains]): every minute, a `DomainMonitor` for each row
//! setup or `idvy domains add --local-token` wrote without one (The platform domain), and the retirement
//! of addresses whose `retire_at` passed (Retirement); in the `*/15` run whose minute is below 15, the
//! cleanup of failed adds (Cleanup after a failed add, H16).
//!
//! [Identities and domains]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{Clock as _, ControlDb as _, ObjectClient as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::domains as ddb;
use crate::domains::cloudflare_api::Cf;
use crate::domains::monitor::{self, DomainRequest};
use crate::domains::ses_api::Ses;
use crate::domains::ses_control::SesCaller;

#[derive(Deserialize)]
struct Bare {
    id: String,
}

/// Rows with `monitor_do_id = ''`: the ID is written only if the row still has none, then `Init` and,
/// for a tenant's row, `domain.created`.
pub async fn mint_monitors<P: Platform>(p: &P) {
    let Ok(rows) = p
        .db()
        .query::<Bare>(Stmt::new(
            "SELECT id FROM domains WHERE monitor_do_id = '' AND state <> 'removed' LIMIT 20",
            vec![],
        ))
        .await
    else {
        return;
    };
    let now = p.clock().now_ms();
    for r in rows {
        let Ok(id) = p.objects().new_object_id(DoClass::Domain) else {
            continue;
        };
        let won = p
            .db()
            .batch(vec![Stmt::new(
                "UPDATE domains SET monitor_do_id = ?1, updated_at = ?2 WHERE id = ?3 AND monitor_do_id = ''",
                vec![id.clone().into(), now.into(), r.id.clone().into()],
            )])
            .await
            .is_ok_and(|m| m.first().is_some_and(|m| m.changes == 1));
        if !won {
            continue;
        }
        let _ = monitor::call(
            p,
            &id,
            DomainRequest::Init {
                domain_id: r.id.clone(),
            },
        )
        .await;
        if let Ok(Some(row)) = p.db().first::<ddb::Row>(ddb::get(&r.id)).await
            && row.tenant_id.is_some()
        {
            let _ = crate::domains::emit(
                p,
                &row,
                "domain.created",
                &crate::domains::domain_data(&row),
                "created",
                now,
            )
            .await;
        }
    }
}

#[derive(Deserialize)]
struct Retiring {
    id: String,
    identity_id: String,
    tenant_id: String,
}

/// Addresses whose `retire_at` passed become `retired`, with `identity.address_retired`.
pub async fn retire<P: Platform>(p: &P) {
    let now = p.clock().now_ms();
    let Ok(rows) = p
        .db()
        .query::<Retiring>(Stmt::new(
            "SELECT id, identity_id, tenant_id FROM addresses WHERE status = 'retiring' AND retire_at <= ?1 ORDER BY retire_at LIMIT 100",
            vec![now.into()],
        ))
        .await
    else {
        return;
    };
    for r in rows {
        let changed = p
            .db()
            .batch(vec![Stmt::new(
                "UPDATE addresses SET status = 'retired', retired_at = ?1, retire_at = NULL, updated_at = ?1 WHERE id = ?2 AND status = 'retiring' AND retire_at <= ?1",
                vec![now.into(), r.id.clone().into()],
            )])
            .await
            .is_ok_and(|m| m.first().is_some_and(|m| m.changes == 1));
        if !changed {
            continue;
        }
        if let Ok(Some(a)) = p
            .db()
            .first::<crate::db::addresses::Row>(crate::db::addresses::get(&r.id))
            .await
        {
            crate::handlers::addresses::emit_identity_event(
                p,
                &r.tenant_id,
                &r.identity_id,
                "identity.address_retired",
                &json!({ "identity_id": r.identity_id, "address": a.json() }),
                &format!("retired:{}", r.id),
                now,
            )
            .await;
        }
    }
}

#[derive(Deserialize)]
struct Journal {
    name: String,
    zone_id: Option<String>,
    objects_json: String,
}

/// Journal rows untouched for an hour: undo exactly their objects, then delete the row (H16).
pub async fn cleanup_failed_adds<P: Platform>(p: &P) {
    let now = p.clock().now_ms();
    let Ok(rows) = p
        .db()
        .query::<Journal>(Stmt::new(
            "SELECT name, zone_id, objects_json FROM domain_onboarding WHERE updated_at < ?1 LIMIT 20",
            vec![(now - 3_600_000).into()],
        ))
        .await
    else {
        return;
    };
    for j in rows {
        let o: Value = serde_json::from_str(&j.objects_json).unwrap_or_default();
        let mut done = true;
        if let (Some(cf), Some(z)) = (Cf::new(p), j.zone_id.as_deref()) {
            if let Some(tag) = o["sending"]["tag"]
                .as_str()
                .filter(|_| o["sending"]["created"] == true)
            {
                done &= cf.delete_sending(z, tag).await.is_ok();
            }
            if o["catch_all"]["created"] == true {
                done &= cf.put_catch_all(z, false).await.is_ok();
            }
            if o["routing"]["created"] == true {
                let _ = cf.unlock_routing(z, &j.name).await;
                for id in o["routing"]["record_ids"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                {
                    if let Some(id) = id.as_str() {
                        done &= cf.delete_record(z, id).await.is_ok()
                            || cf.record(z, id).await.is_ok_and(|r| r.is_none());
                    }
                }
            }
            for id in o["dns_records"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .chain(
                    o["ses_identity"]["dkim_record_ids"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                )
            {
                if let Some(id) = id.as_str() {
                    done &= cf.delete_record(z, id).await.is_ok()
                        || cf.record(z, id).await.is_ok_and(|r| r.is_none());
                }
            }
            if let Some(sid) = o["subscription"].as_str() {
                done &= cf.unsubscribe(sid).await.is_ok();
            }
        }
        if o["ses_identity"]["created"] == true
            && let Some(ses) = Ses::new(p, SesCaller::Removal)
        {
            done &= ses.delete_identity(&j.name).await.is_ok();
        }
        if done {
            let _ = p
                .db()
                .batch(vec![
                    Stmt::new(
                        "DELETE FROM domain_onboarding WHERE name = ?1",
                        vec![j.name.clone().into()],
                    ),
                    Stmt::new(
                        "DELETE FROM zone_claims WHERE zone_name = ?1 AND state = 'pending'",
                        vec![j.name.clone().into()],
                    ),
                ])
                .await;
        }
    }
}
