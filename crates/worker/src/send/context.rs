//! `send::context::load` (Outbound › Handler, step 6) and the recipient facts the mailbox reads.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::address::address_hash;
use pylota_identity_core::lists::candidates;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Platform};
use serde::Deserialize;

use super::{AddressCtx, IdentityCtx, RecipientFacts, SendContext, TenantCtx};
use crate::db::identities as idb;
use crate::db::tenants as tdb;
use crate::errors::ApiError;

#[derive(Deserialize)]
struct Extra {
    sending_pause_reason: Option<String>,
    partner_status: Option<String>,
}

/// The context of a send from `idn` in `t`.
pub async fn load<P: Platform>(
    p: &P,
    idn: &idb::Row,
    t: &tdb::Row,
) -> Result<SendContext, ApiError> {
    let db = p.db();
    let extra: Option<Extra> = db
        .first(Stmt::new(
            "SELECT t.sending_pause_reason, p.status AS partner_status FROM tenants t LEFT JOIN partners p ON p.id = t.partner_id WHERE t.id = ?1",
            vec![t.id.clone().into()],
        ))
        .await
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "D1 did not answer."))?;
    let addresses: Vec<AddressCtx> = db
        .query(Stmt::new(
            "SELECT a.address, a.status, a.role, a.domain_id, d.name AS domain, d.kind AS domain_kind, d.state, d.transport, d.reply_token, d.sending_paused_at, CASE WHEN d.transport = 'cloudflare' AND d.kind <> 'platform' AND d.event_subscription_id IS NULL THEN 'manual' ELSE NULL END AS delivery_events FROM addresses a JOIN domains d ON d.id = a.domain_id WHERE a.identity_id = ?1 ORDER BY a.created_at, a.id",
            vec![idn.id.clone().into()],
        ))
        .await
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "D1 did not answer."))?;
    let deployment_quota = match p.config().daily_send_quota {
        Some(quota) => db
            .first::<DefaultTenant>(Stmt::new(
                "SELECT id, quota_do_id FROM tenants WHERE address_suffix = '' AND quota_do_id <> '' LIMIT 1",
                vec![],
            ))
            .await
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "D1 did not answer."))?
            .map(|d| super::DeploymentQuota {
                tenant_id: d.id,
                quota_do_id: d.quota_do_id,
                quota,
            }),
        None => None,
    };
    let extra = extra.unwrap_or(Extra {
        sending_pause_reason: None,
        partner_status: None,
    });
    Ok(SendContext {
        identity: IdentityCtx {
            id: idn.id.clone(),
            display_name: idn.display_name.clone(),
            status: idn.status.clone(),
            pause_reason: idn.pause_reason.clone(),
            owner_name: idn.owner_name.clone(),
            owner_email: idn.owner_email.clone(),
            signature_text: idn.signature_text.clone(),
            signature_html: idn.signature_html.clone(),
            send_policy: serde_json::from_str(&idn.send_policy_json)
                .unwrap_or_else(|_| serde_json::json!({})),
            is_system: idn.is_system == 1,
        },
        tenant: TenantCtx {
            id: t.id.clone(),
            status: t.status.clone(),
            mode: t.mode.clone(),
            timezone: t.timezone.clone(),
            sending_paused_at: t.sending_paused_at,
            sending_pause_reason: extra.sending_pause_reason,
            partner_id: t.partner_id.clone(),
            partner_status: extra.partner_status,
            quota_do_id: t.quota_do_id.clone(),
        },
        policy: crate::policy::effective(p, t),
        addresses,
        platform_domain: p.config().platform_domain.clone(),
        ses_configured: p.secrets().ses.is_some(),
        deployment_quota,
    })
}

#[derive(Deserialize)]
struct DefaultTenant {
    id: String,
    quota_do_id: String,
}

#[derive(Deserialize)]
struct Supp {
    address_hash: String,
    reason: String,
}

#[derive(Deserialize)]
struct ListHit {
    identity_id: String,
    kind: String,
    entry: String,
}

#[derive(Deserialize)]
struct Dir {
    address: String,
    tenant_id: String,
    mode: String,
    partner_id: Option<String>,
    identity_id: String,
}

/// Suppressions, send-list entries and (for a test tenant) directory rows of `recipients`, each list
/// bound as one JSON array.
pub async fn recipient_facts<P: Platform>(
    p: &P,
    tenant_id: &str,
    identity_id: &str,
    recipients: &[String],
    test: bool,
    now: i64,
) -> Result<RecipientFacts, ApiError> {
    let key = &p.secrets().hash_key;
    let hashes: Vec<(String, String)> = recipients
        .iter()
        .map(|a| (address_hash(key, a), a.clone()))
        .collect();
    let hash_json = serde_json::to_string(&hashes.iter().map(|(h, _)| h).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut entries: Vec<String> = recipients.iter().flat_map(|a| candidates(a)).collect();
    entries.sort();
    entries.dedup();
    let entries_json = serde_json::to_string(&entries).unwrap_or_default();
    let unavailable = |_| ApiError::new(ErrorCode::Unavailable, "D1 did not answer.");
    let db = p.db();
    let supp: Vec<Supp> = db
        .query(Stmt::new(
            "SELECT address_hash, reason FROM suppressions WHERE tenant_id = ?1 AND address_hash IN (SELECT value FROM json_each(?2)) AND (expires_at IS NULL OR expires_at > ?3)",
            vec![tenant_id.into(), hash_json.into(), now.into()],
        ))
        .await
        .map_err(unavailable)?;
    let lists: Vec<ListHit> = db
        .query(Stmt::new(
            "SELECT identity_id, kind, entry FROM sender_lists WHERE tenant_id = ?1 AND direction = 'send' AND identity_id IN ('', ?2) AND entry IN (SELECT value FROM json_each(?3))",
            vec![tenant_id.into(), identity_id.into(), entries_json.into()],
        ))
        .await
        .map_err(unavailable)?;
    let directory: Vec<Dir> = if test {
        db.query(Stmt::new(
            "SELECT a.address, a.tenant_id, t.mode, t.partner_id, a.identity_id FROM addresses a JOIN tenants t ON t.id = a.tenant_id WHERE a.address IN (SELECT value FROM json_each(?1)) AND a.status IN ('active','retiring')",
            vec![serde_json::to_string(recipients).unwrap_or_default().into()],
        ))
        .await
        .map_err(unavailable)?
    } else {
        Vec::new()
    };
    let mut facts = RecipientFacts::default();
    for s in supp {
        if let Some((_, a)) = hashes.iter().find(|(h, _)| *h == s.address_hash) {
            facts.suppressed.push((a.clone(), s.reason));
        }
    }
    for r in recipients {
        let c = candidates(r);
        for l in lists.iter().filter(|l| c.contains(&l.entry)) {
            facts.send_lists.push((
                r.clone(),
                !l.identity_id.is_empty(),
                l.kind == "block",
                l.entry.clone(),
            ));
        }
    }
    facts.directory = directory
        .into_iter()
        .map(|d| (d.address, d.tenant_id, d.mode, d.partner_id, d.identity_id))
        .collect();
    Ok(facts)
}
