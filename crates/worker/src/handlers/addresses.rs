//! Addresses ([Identities and domains › Addresses]): the list (M6); adding an alias on a tenant domain
//! with its literal rule on a zone subdomain (H6, A11), promotion and rollback (FR-ADR-2, FR-ADR-4,
//! A14), retirement (FR-ADR-3) and deleting a pending address (M13).
//!
//! [Identities and domains › Addresses]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::{DomainClass, address_hash, validate_username};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use super::identity_in_scope;
use super::messages::only_fields;
use crate::ctx::{Ctx, Reply};
use crate::db::{addresses as adb, domains as ddb, identities as idb};
use crate::domains::cloudflare_api::Cf;
use crate::domains::monitor::{self, DomainRequest};
use crate::errors::ApiError;

/// At most this many addresses per identity, in any state.
pub const MAX_ADDRESSES: usize = 20;
/// Literal rules per domain, and the longest literal matcher value.
pub const MAX_RULES: i64 = 200;
pub const MAX_MATCHER: usize = 90;
const DAY_MS: i64 = 86_400_000;

/// Every address of the identity, in every status.
pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (row, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let limit = c.limit()?;
    let after = c.after(&[]).await?;
    let rows: Vec<adb::Row> = c.p.db().query(adb::page(&row.id, after, limit)).await?;
    c.page(
        rows,
        limit,
        &[],
        |r| (r.created_at, r.id.clone()),
        adb::Row::json,
    )
    .await
}

/// Appends an identity event (`identity.address_*`) to the identity's mailbox; its ID is derived from
/// `seed`, so a retried call stores one event. A mailbox that does not answer is logged: the change is
/// already committed in D1.
pub async fn emit_identity_event<P: Platform>(
    p: &P,
    tenant_id: &str,
    identity_id: &str,
    event_type: &str,
    data: &serde_json::Value,
    seed: &str,
    now: i64,
) {
    use pylota_identity_api_types::ids::IdPrefix;
    use pylota_identity_platform::ObjectClient as _;
    use pylota_identity_platform::durable::DoClass;
    let Ok(Some(idn)) = p
        .db()
        .first::<crate::db::identities::Row>(crate::db::identities::get(identity_id))
        .await
    else {
        return;
    };
    if idn.mailbox_do_id.is_empty() {
        return;
    }
    let event_id = pylota_identity_core::ids::derived(
        IdPrefix::Event,
        now,
        &p.secrets().hash_key,
        &format!("{event_type}:{identity_id}:{seed}"),
    );
    let env = crate::rpc::envelope_for(
        p,
        Some(tenant_id),
        Some(identity_id),
        &crate::request_id::new(p),
        None,
        crate::mailbox::MailboxRequest::EmitEvent(crate::mailbox::EmitEvent {
            event_id,
            event_type: event_type.to_owned(),
            occurred_at: now,
            data: data.clone(),
        }),
    );
    if p.objects()
        .call::<_, serde_json::Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
        .await
        .is_err()
    {
        crate::log::warn(
            "identity_event_unsent",
            &[
                ("event_type", event_type.into()),
                ("identity_id", identity_id.into()),
            ],
        );
    }
}

#[derive(Deserialize)]
struct N {
    n: i64,
}

fn not_found() -> ApiError {
    ApiError::not_found(ErrorCode::AddressNotFound)
}

fn cf_token_required() -> ApiError {
    ApiError::new(
        ErrorCode::CfTokenRequired,
        "A literal routing rule needs the Worker's Cloudflare API token.",
    )
}

/// Creates the literal rule of an address on a zone subdomain; `None` when the call failed (H6).
pub async fn create_rule<P: Platform>(p: &P, d: &ddb::Row, a: &adb::Row) -> Option<String> {
    let cf = Cf::new(p)?;
    let zone = d.zone_id.as_deref()?;
    match cf.create_rule(zone, &a.address, &a.id).await {
        Ok(id) if !id.is_empty() => {
            let _ = p
                .db()
                .execute(Stmt::new(
                    "UPDATE addresses SET routing_rule_id = ?2 WHERE id = ?1",
                    vec![a.id.clone().into(), id.clone().into()],
                ))
                .await;
            Some(id)
        }
        _ => None,
    }
}

/// `POST /v1/identities/{identity_id}/addresses`: an alias on a tenant domain.
pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let p = c.p.as_ref();
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let body = c.json()?;
    only_fields(&body, &["local_part", "domain_id"])?;
    let domain_id = body["domain_id"]
        .as_str()
        .ok_or_else(|| ApiError::invalid("domain_id", "A domain of this tenant."))?;
    let d: ddb::Row = p
        .db()
        .first::<ddb::Row>(ddb::get(domain_id))
        .await?
        .filter(|d| {
            d.tenant_id.as_deref() == Some(idn.tenant_id.as_str())
                && !matches!(d.state.as_str(), "removing" | "removed")
        })
        .ok_or_else(|| ApiError::not_found(ErrorCode::DomainNotFound))?;
    let local = validate_username(
        body["local_part"].as_str().unwrap_or(""),
        DomainClass::Tenant,
        40,
        "",
        &[],
    )
    .map_err(|e| {
        let mut err = super::identities::address_error(e);
        err.details = Some(json!({ "field": "local_part" }));
        err
    })?;
    let address = format!("{local}@{}", d.name);
    if d.routing_mode == "literal" && address.len() > MAX_MATCHER {
        return Err(ApiError::new(
            ErrorCode::AddressInvalid,
            "An address on this domain is at most 90 characters.",
        )
        .details(json!({ "field": "local_part" })));
    }
    let existing: Vec<adb::Row> = p.db().query(adb::of_identity(&idn.id)).await?;
    // A11: a newer pending address on the same domain replaces the older one.
    let replaced: Vec<adb::Row> = existing
        .iter()
        .filter(|a| a.domain_id == d.id && a.status == "pending" && a.address != address)
        .cloned()
        .collect();
    if existing.len() - replaced.len() >= MAX_ADDRESSES {
        return Err(ApiError::new(
            ErrorCode::AddressLimitReached,
            "An identity has at most 20 addresses.",
        )
        .details(json!({ "limit": MAX_ADDRESSES })));
    }
    let taken: Option<adb::Row> = p.db().first(adb::by_address(&address)).await?;
    let tomb: Option<adb::Tombstone> = p
        .db()
        .first(adb::tombstone(&address_hash(
            &p.secrets().hash_key,
            &address,
        )))
        .await?;
    if taken.is_some() || tomb.is_some_and(|t| t.identity_id.as_deref() != Some(idn.id.as_str())) {
        return Err(ApiError::new(
            ErrorCode::AddressTaken,
            "This address is taken.",
        ));
    }
    let literal = d.routing_mode == "literal";
    let cf = Cf::new(p);
    if literal {
        if cf.is_none() {
            return Err(cf_token_required());
        }
        let rules: Option<N> = p
            .db()
            .first(Stmt::new(
                "SELECT COUNT(*) AS n FROM addresses WHERE domain_id = ?1 AND routing_rule_id IS NOT NULL",
                vec![d.id.clone().into()],
            ))
            .await?;
        if rules.map_or(0, |n| n.n) >= MAX_RULES {
            return Err(ApiError::new(
                ErrorCode::DomainInUse,
                "This domain holds 200 routing rules.",
            )
            .details(json!({ "reason": "routing_rule_limit" })));
        }
    }
    let now = c.now;
    let id = p.ids().new_id(IdPrefix::Address);
    let mut batch = vec![adb::insert(
        &id,
        &address,
        &local,
        &d.id,
        &idn.tenant_id,
        &idn.id,
        "alias",
        "pending",
        now,
    )];
    for r in &replaced {
        batch.push(Stmt::new(
            "DELETE FROM addresses WHERE id = ?1 AND status = 'pending'",
            vec![r.id.clone().into()],
        ));
    }
    p.db().batch(batch).await?;
    if let (Some(cf), Some(z)) = (&cf, &d.zone_id) {
        for r in replaced.iter().filter(|r| r.routing_rule_id.is_some()) {
            let _ = cf
                .delete_rule(z, r.routing_rule_id.as_deref().unwrap_or(""))
                .await;
        }
    }
    let mut row: adb::Row = p
        .db()
        .first(adb::get(&id))
        .await?
        .ok_or_else(ApiError::internal)?;
    let routed = !literal || create_rule(p, &d, &row).await.is_some();
    if routed && matches!(d.state.as_str(), "healthy" | "degraded") {
        p.db()
            .execute(Stmt::new(
                "UPDATE addresses SET status = 'active', updated_at = ?2 WHERE id = ?1 AND status = 'pending'",
                vec![id.clone().into(), now.into()],
            ))
            .await?;
    }
    row = p
        .db()
        .first(adb::get(&id))
        .await?
        .ok_or_else(ApiError::internal)?;
    emit_identity_event(
        p,
        &idn.tenant_id,
        &idn.id,
        "identity.address_added",
        &json!({ "identity_id": idn.id, "address": row.json() }),
        &format!("added:{id}"),
        now,
    )
    .await;
    // The monitor retries a failed rule (H6) and activates the address with its domain.
    let _ = monitor::call(
        p,
        &d.monitor_do_id,
        DomainRequest::Check {
            domain_id: d.id.clone(),
        },
    )
    .await;
    Ok(Reply::json(201, row.json()))
}

/// The identity, the address and its domain; the address must belong to the identity.
async fn load<P: Platform>(
    c: &Ctx<P>,
) -> Result<(idb::Row, adb::Row, ddb::Row, Vec<adb::Row>), ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let all: Vec<adb::Row> = c.p.db().query(adb::of_identity(&idn.id)).await?;
    let a = all
        .iter()
        .find(|a| a.id == c.route.param("address_id"))
        .cloned()
        .ok_or_else(not_found)?;
    let d: ddb::Row =
        c.p.db()
            .first(ddb::get(&a.domain_id))
            .await?
            .ok_or_else(not_found)?;
    Ok((idn, a, d, all))
}

async fn kind_of<P: Platform>(c: &Ctx<P>, domain_id: &str) -> Result<String, ApiError> {
    Ok(c.p
        .db()
        .first::<ddb::Row>(ddb::get(domain_id))
        .await?
        .map(|d| d.kind)
        .unwrap_or_default())
}

/// `POST …/addresses/{address_id}/promote`: this address becomes the primary. Promoting the platform
/// address or a retiring address is a rollback: the previous primary stays an `active` alias (A14).
pub async fn promote<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let p = c.p.as_ref();
    let (idn, a, d, all) = load(c).await?;
    let body = c.json().unwrap_or(Value::Null);
    let days = body["retire_previous_after_days"].as_i64().unwrap_or(90);
    if !(0..=365).contains(&days) {
        return Err(ApiError::invalid("retire_previous_after_days", "0 to 365."));
    }
    if a.role == "primary" {
        return Ok(Reply::json(200, idn.json(&all)));
    }
    if !matches!(a.status.as_str(), "active" | "retiring") {
        return Err(ApiError::new(
            ErrorCode::AddressNotFound,
            "Only an active or retiring address can be promoted.",
        ));
    }
    if !matches!(d.state.as_str(), "healthy" | "degraded") {
        return Err(ApiError::new(
            ErrorCode::DomainNotReady,
            "The address's domain is not verified.",
        ));
    }
    let now = c.now;
    let rollback = d.kind == "platform" || a.status == "retiring";
    let previous = all.iter().find(|x| x.role == "primary").cloned();
    // The previous primary steps down first: an identity never has two primaries.
    let mut batch = Vec::new();
    if let Some(prev) = &previous {
        let prev_platform = kind_of(c, &prev.domain_id).await? == "platform";
        let (status, retire_at, retired_at): (&str, Option<i64>, Option<i64>) =
            if rollback || (prev_platform && idn.is_system == 0) {
                ("active", None, None)
            } else if days == 0 {
                ("retired", None, Some(now))
            } else {
                ("retiring", Some(now + days * DAY_MS), None)
            };
        batch.push(Stmt::new(
            "UPDATE addresses SET role = 'alias', status = ?2, retire_at = ?3, retired_at = ?4, updated_at = ?5 WHERE id = ?1",
            vec![prev.id.clone().into(), status.into(), retire_at.into(), retired_at.into(), now.into()],
        ));
    }
    batch.push(Stmt::new(
        "UPDATE addresses SET role = 'primary', status = 'active', retire_at = NULL, updated_at = ?2 WHERE id = ?1",
        vec![a.id.clone().into(), now.into()],
    ));
    p.db().batch(batch).await?;
    let all: Vec<adb::Row> = p.db().query(adb::of_identity(&idn.id)).await?;
    let promoted = all.iter().find(|x| x.id == a.id).map(adb::Row::json);
    emit_identity_event(
        p,
        &idn.tenant_id,
        &idn.id,
        "identity.address_promoted",
        &json!({ "identity_id": idn.id, "address": promoted,
            "previous_primary": previous.as_ref().map(|x| x.address.clone()) }),
        &format!("promoted:{}:{now}", a.id),
        now,
    )
    .await;
    if let Some(prev) = previous.and_then(|x| all.iter().find(|y| y.id == x.id).cloned())
        && prev.status == "retired"
    {
        emit_identity_event(
            p,
            &idn.tenant_id,
            &idn.id,
            "identity.address_retired",
            &json!({ "identity_id": idn.id, "address": prev.json() }),
            &format!("retired:{}", prev.id),
            now,
        )
        .await;
    }
    Ok(Reply::json(200, idn.json(&all)))
}

/// `POST …/addresses/{address_id}/retire`: an alias leaves after `after_days` (`0`: now).
pub async fn retire<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let p = c.p.as_ref();
    let (idn, a, d, _) = load(c).await?;
    let body = c.json().unwrap_or(Value::Null);
    let days = body["after_days"].as_i64().unwrap_or(30);
    if !(0..=365).contains(&days) {
        return Err(ApiError::invalid("after_days", "0 to 365."));
    }
    if a.role == "primary" {
        return Err(ApiError::new(
            ErrorCode::AddressIsPrimary,
            "Promote another address first.",
        ));
    }
    if d.kind == "platform" {
        return Err(ApiError::new(
            ErrorCode::AddressInUse,
            "The platform address is the fallback address and is never retired.",
        ));
    }
    if !matches!(a.status.as_str(), "active" | "retiring") {
        return Err(ApiError::new(
            ErrorCode::AddressInUse,
            "Only an active or retiring alias can be retired.",
        ));
    }
    let now = c.now;
    let (status, retire_at, retired_at) = if days == 0 {
        ("retired", None, Some(now))
    } else {
        ("retiring", Some(now + days * DAY_MS), None)
    };
    p.db()
        .execute(Stmt::new(
            "UPDATE addresses SET status = ?2, retire_at = ?3, retired_at = ?4, updated_at = ?5 WHERE id = ?1",
            vec![a.id.clone().into(), status.into(), retire_at.into(), retired_at.into(), now.into()],
        ))
        .await?;
    let row: adb::Row = p.db().first(adb::get(&a.id)).await?.ok_or_else(not_found)?;
    if status == "retired" {
        emit_identity_event(
            p,
            &idn.tenant_id,
            &idn.id,
            "identity.address_retired",
            &json!({ "identity_id": idn.id, "address": row.json() }),
            &format!("retired:{}", a.id),
            now,
        )
        .await;
    }
    Ok(Reply::json(200, row.json()))
}

/// `DELETE …/addresses/{address_id}`: a pending address (never routed, so it never received mail).
pub async fn delete<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let p = c.p.as_ref();
    let (_, a, d, _) = load(c).await?;
    if a.status != "pending" || d.kind == "platform" {
        return Err(ApiError::new(
            ErrorCode::AddressInUse,
            "Only a pending address can be deleted; retire an address in use.",
        ));
    }
    p.db()
        .execute(Stmt::new(
            "DELETE FROM addresses WHERE id = ?1 AND status = 'pending'",
            vec![a.id.clone().into()],
        ))
        .await?;
    if let (Some(cf), Some(z), Some(rule)) = (Cf::new(p), &d.zone_id, &a.routing_rule_id) {
        let _ = cf.delete_rule(z, rule).await;
    }
    Ok(Reply::no_content())
}
