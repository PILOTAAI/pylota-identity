//! Invitations ([Console › Invitations]): create or re-send, revoke, accept and expire. A pending
//! invitation holds a seat; its token is stored only as a keyed hash under the current `link` key; the
//! email goes from the system identity after the batch commits, also with `PM_CONSOLE=off`.
//!
//! [Console › Invitations]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#invitations

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::console::{self as cc, Role};
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{ControlDb as _, Ids as _, ObjectClient as _, Platform, Rng as _};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Actor, event, fan};
use crate::db::{audit, tenants as tdb};
use crate::errors::ApiError;
use crate::handlers::Count;
use crate::keyring::{self, Purpose};
use crate::quota::{Feature, HoldAnswer, QuotaRequest};

const DAY_MS: i64 = 86_400_000;
/// Invitation emails (new and re-sent) per workspace per UTC day.
pub const DAILY: i64 = 50;

#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub id: String,
    pub tenant_id: String,
    pub email: String,
    pub role: String,
    pub invited_by: Option<String>,
    pub status: String,
    pub expires_at: i64,
    pub created_at: i64,
}

impl Row {
    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "email": self.email, "role": self.role, "invited_by": self.invited_by,
            "expires_at": rfc3339(self.expires_at),
        })
    }
}

pub fn pending(tenant_id: &str, now: i64) -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, email, role, invited_by, status, expires_at, created_at FROM invitations WHERE tenant_id = ?1 AND status = 'pending' AND expires_at > ?2 ORDER BY created_at",
        vec![tenant_id.into(), now.into()],
    )
}

/// The signed-in person's pending invitations, in every workspace.
pub fn for_email(email: &str, now: i64) -> Stmt {
    Stmt::new(
        "SELECT id, tenant_id, email, role, invited_by, status, expires_at, created_at FROM invitations WHERE email = ?1 AND status = 'pending' AND expires_at > ?2 ORDER BY created_at",
        vec![email.into(), now.into()],
    )
}

pub async fn get<P: Platform>(p: &P, tenant_id: &str, id: &str) -> Result<Option<Row>, ApiError> {
    Ok(p
        .db()
        .first(Stmt::new(
            "SELECT id, tenant_id, email, role, invited_by, status, expires_at, created_at FROM invitations WHERE tenant_id = ?1 AND id = ?2",
            vec![tenant_id.into(), id.into()],
        ))
        .await?)
}

/// A sign-in or invitation address in stored form: lower case, ASCII, `local@domain`.
pub fn email(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    let (local, domain) = s.rsplit_once('@')?;
    let ok = !local.is_empty()
        && local.len() <= 64
        && s.len() <= 254
        && domain.contains('.')
        && s.is_ascii()
        && !s.contains(char::is_whitespace)
        && !s.contains(['<', '>', '"', ',', ';'])
        && domain.split('.').all(|l| !l.is_empty());
    ok.then_some(s)
}

/// Whether this deployment receives mail for `email` (W45): the platform domain, or an identity's
/// `pending`, `active` or `retiring` address.
pub async fn hosted<P: Platform>(p: &P, email: &str) -> Result<bool, ApiError> {
    let domain = email.rsplit_once('@').map_or("", |(_, d)| d);
    if domain == p.config().platform_domain {
        return Ok(true);
    }
    let n: Option<Count> = p
        .db()
        .first(Stmt::new(
            "SELECT COUNT(*) AS n FROM addresses WHERE address = ?1 AND status IN ('pending','active','retiring')",
            vec![email.into()],
        ))
        .await?;
    Ok(n.is_some_and(|n| n.n > 0))
}

/// A new token and its keyed hash under the current `link` key.
async fn new_token<P: Platform>(p: &P) -> Result<(String, String, String), ApiError> {
    let key = keyring::current(p, Purpose::Link).await?;
    let token = cc::token(&key.kid, &p.rng().bytes::<32>());
    let hash = cc::keyed_hash(&key.secret, &token);
    Ok((token, hash, key.kid))
}

/// The keyed hash of a presented token, under the key its kid names (any key still in its window).
pub async fn hash_of<P: Platform>(p: &P, token: &str) -> Result<Option<String>, ApiError> {
    let Some(kid) = cc::token_kid(token) else {
        return Ok(None);
    };
    let keys = keyring::keys(p, Purpose::Link).await?;
    Ok(keys
        .iter()
        .find(|k| k.kid == kid)
        .map(|k| cc::keyed_hash(&k.secret, token)))
}

/// The pending, unexpired invitation a token names.
pub async fn by_token<P: Platform>(p: &P, token: &str, now: i64) -> Result<Option<Row>, ApiError> {
    let Some(hash) = hash_of(p, token).await? else {
        return Ok(None);
    };
    Ok(p
        .db()
        .first(Stmt::new(
            "SELECT id, tenant_id, email, role, invited_by, status, expires_at, created_at FROM invitations WHERE token_hash = ?1 AND status = 'pending' AND expires_at > ?2",
            vec![hash.into(), now.into()],
        ))
        .await?)
}

async fn sent_today<P: Platform>(p: &P, tenant_id: &str, now: i64) -> Result<i64, ApiError> {
    let n: Option<Count> = p
        .db()
        .first(Stmt::new(
            "SELECT COUNT(*) AS n FROM audit_log WHERE tenant_id = ?1 AND action IN ('member.invite','member.invite_resend') AND created_at >= ?2",
            vec![tenant_id.into(), (now.div_euclid(DAY_MS) * DAY_MS).into()],
        ))
        .await?;
    Ok(n.map_or(0, |n| n.n))
}

async fn quota<P: Platform>(
    p: &P,
    t: &tdb::Row,
    req: QuotaRequest,
    request_id: &str,
) -> Result<Value, ApiError> {
    let env = crate::rpc::envelope_for(p, Some(&t.id), None, request_id, None, req);
    match p
        .objects()
        .call::<_, Value>(DoClass::Quota, &t.quota_do_id, &env)
        .await?
    {
        pylota_identity_platform::durable::RpcResult::Ok(v) => Ok(v),
        pylota_identity_platform::durable::RpcResult::Err(_) => Err(ApiError::new(
            ErrorCode::Unavailable,
            "The workspace's quota is not ready; retry.",
        )),
    }
}

/// Invites `email` as `role`, or re-sends a pending invitation to it (Console › Invitations, steps
/// 1–5). `inviter` is the signed-in person, `None` for an API key.
pub async fn create<P: Platform>(
    p: &P,
    t: &tdb::Row,
    email_in: &str,
    role_in: &str,
    inviter: Option<&str>,
    actor: &Actor<'_>,
    now: i64,
) -> Result<(u16, Row), ApiError> {
    let address = email(email_in)
        .ok_or_else(|| ApiError::invalid("email", "email is not a valid address."))?;
    let role = match Role::parse(role_in) {
        Some(r) if r != Role::Owner => r,
        _ => {
            return Err(ApiError::invalid(
                "role",
                "role is admin, member or viewer.",
            ));
        }
    };
    let member: Option<Count> = p
        .db()
        .first(Stmt::new(
            "SELECT COUNT(*) AS n FROM members m JOIN users u ON u.id = m.user_id WHERE m.tenant_id = ?1 AND u.email = ?2",
            vec![t.id.clone().into(), address.clone().into()],
        ))
        .await?;
    if member.is_some_and(|m| m.n > 0) {
        return Err(
            ApiError::invalid("email", "This address is already a member.")
                .details(json!({ "errors": [{ "path": "email", "message": "already a member" }], "reason": "already_member" })),
        );
    }
    if hosted(p, &address).await? {
        return Err(ApiError::invalid(
            "email",
            "An address this deployment hosts cannot be invited.",
        )
        .details(json!({ "errors": [{ "path": "email", "message": "hosted address" }], "reason": "hosted_address" })));
    }
    if sent_today(p, &t.id, now).await? >= DAILY {
        let resets = now.div_euclid(DAY_MS) * DAY_MS + DAY_MS;
        return Err(ApiError::new(
            ErrorCode::DailyCapReached,
            "The workspace sent its invitations for today.",
        )
        .details(json!({ "cap": "invitations", "resets_at": rfc3339(resets) })));
    }
    let (token, hash, kid) = new_token(p).await?;
    let expires = now + 7 * DAY_MS;
    let existing: Option<Row> = p
        .db()
        .first(Stmt::new(
            "SELECT id, tenant_id, email, role, invited_by, status, expires_at, created_at FROM invitations WHERE tenant_id = ?1 AND email = ?2 AND status = 'pending'",
            vec![t.id.clone().into(), address.clone().into()],
        ))
        .await?;
    let (status, id) = if let Some(inv) = existing {
        // Re-sent: a new token and expiry, no new seat.
        p.db()
            .batch(vec![
                Stmt::new(
                    "UPDATE invitations SET token_hash = ?2, key_kid = ?3, expires_at = ?4, role = ?5 WHERE id = ?1 AND status = 'pending'",
                    vec![
                        inv.id.clone().into(),
                        hash.into(),
                        kid.into(),
                        expires.into(),
                        role.as_str().into(),
                    ],
                ),
                audit::insert(
                    &p.ids().new_id(IdPrefix::Audit),
                    Some(&t.id),
                    Some(actor.id),
                    "member.invite_resend",
                    "invitation",
                    &inv.id,
                    &json!({ "role": role.as_str() }),
                    actor.request_id,
                    now,
                ),
            ])
            .await?;
        (200, inv.id)
    } else {
        let id = p.ids().new_id(IdPrefix::Invitation);
        let hold = quota(
            p,
            t,
            QuotaRequest::Hold {
                feature: Feature::Seats,
                units: 1,
                r#ref: id.clone(),
                gates: vec![],
            },
            actor.request_id,
        )
        .await?;
        if let Ok(HoldAnswer::Denied(d)) = serde_json::from_value::<HoldAnswer>(hold) {
            return Err(
                ApiError::new(ErrorCode::BillingLimit, "The plan's seats are all used.")
                    .details(json!({ "feature": "seats", "granted": d.granted, "used": d.used })),
            );
        }
        let (ev, job) = event(
            p,
            t,
            "member.invited",
            &json!({ "invitation_id": id, "email_hint": cc::email_hint(&address), "role": role.as_str() }),
            now,
        );
        let r = p
            .db()
            .batch(vec![
                Stmt::new(
                    "INSERT INTO invitations (id, tenant_id, email, role, token_hash, key_kid, invited_by, status, expires_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9)",
                    vec![
                        id.clone().into(),
                        t.id.clone().into(),
                        address.clone().into(),
                        role.as_str().into(),
                        hash.into(),
                        kid.into(),
                        inviter.map(str::to_owned).into(),
                        expires.into(),
                        now.into(),
                    ],
                ),
                audit::insert(
                    &p.ids().new_id(IdPrefix::Audit),
                    Some(&t.id),
                    Some(actor.id),
                    "member.invite",
                    "invitation",
                    &id,
                    &json!({ "role": role.as_str() }),
                    actor.request_id,
                    now,
                ),
                ev,
            ])
            .await;
        let settle = |consume| QuotaRequest::Settle {
            feature: Feature::Seats,
            r#ref: id.clone(),
            consume,
            keep: 0,
        };
        if let Err(e) = r {
            let _ = quota(p, t, settle(0), actor.request_id).await;
            return Err(e.into());
        }
        let _ = quota(p, t, settle(1), actor.request_id).await;
        fan(p, vec![job]).await;
        (201, id)
    };
    send_email(p, t, &address, role, inviter, &token, &id, actor.request_id).await;
    let row = get(p, &t.id, &id).await?.ok_or_else(ApiError::internal)?;
    Ok((status, row))
}

/// The invitation email, from the system identity. It names the inviter, or the workspace alone when
/// an API key invited.
#[allow(clippy::too_many_arguments)]
async fn send_email<P: Platform>(
    p: &P,
    t: &tdb::Row,
    to: &str,
    role: Role,
    inviter: Option<&str>,
    token: &str,
    id: &str,
    request_id: &str,
) {
    #[derive(Deserialize)]
    struct U {
        name: Option<String>,
        email: String,
    }
    let who = match inviter {
        Some(u) => p
            .db()
            .first::<U>(Stmt::new(
                "SELECT name, email FROM users WHERE id = ?1",
                vec![u.into()],
            ))
            .await
            .ok()
            .flatten()
            .map(|u| u.name.unwrap_or(u.email)),
        None => None,
    };
    let host = &p.config().console_host;
    let lead = match who {
        Some(w) => format!(
            "{w} invited you to the workspace {} as {}.",
            t.name,
            role.as_str()
        ),
        None => format!(
            "You are invited to the workspace {} as {}.",
            t.name,
            role.as_str()
        ),
    };
    let text = format!(
        "{lead}\n\nAccept the invitation:\nhttps://{host}/console/invitations/accept?t={token}\n\nThe link works for 7 days. If you did not expect this, ignore this email.\n"
    );
    let key = format!(
        "invite:{id}:{}",
        &pylota_identity_core::ses::sha256_hex(token.as_bytes())[..16]
    );
    let _ = crate::system_mail::send(
        p,
        to,
        &format!("Invitation to {}", t.name),
        &text,
        &key,
        request_id,
    )
    .await;
}

/// Revokes a pending invitation and frees its seat.
pub async fn revoke<P: Platform>(
    p: &P,
    t: &tdb::Row,
    id: &str,
    actor: &Actor<'_>,
    now: i64,
) -> Result<(), ApiError> {
    if get(p, &t.id, id)
        .await?
        .is_none_or(|i| i.status != "pending")
    {
        return Err(ApiError::not_found(ErrorCode::InvitationNotFound));
    }
    let metas = p
        .db()
        .batch(vec![
            Stmt::new(
                "UPDATE invitations SET status = 'revoked' WHERE tenant_id = ?1 AND id = ?2 AND status = 'pending'",
                vec![t.id.clone().into(), id.into()],
            ),
            audit::insert(
                &p.ids().new_id(IdPrefix::Audit),
                Some(&t.id),
                Some(actor.id),
                "member.invite_revoke",
                "invitation",
                id,
                &json!({}),
                actor.request_id,
                now,
            ),
        ])
        .await?;
    if metas.first().is_none_or(|m| m.changes == 0) {
        return Err(ApiError::not_found(ErrorCode::InvitationNotFound));
    }
    super::seats(p, t, -1, id, actor).await;
    Ok(())
}

/// Accepts an invitation for `email` (its address): one batch creates the person if needed, inserts
/// the membership, marks the invitation accepted and writes `member.join` and `member.joined`.
/// Answers the person's `usr_` ID. The seat of the invitation becomes the member's.
pub async fn accept<P: Platform>(
    p: &P,
    inv: &Row,
    request_id: &str,
    now: i64,
) -> Result<String, ApiError> {
    let t: tdb::Row = p
        .db()
        .first(tdb::get(&inv.tenant_id))
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::InvitationNotFound))?;
    let new_user = p.ids().new_id(IdPrefix::ConsoleUser);
    p.db()
        .execute(Stmt::new(
            "INSERT INTO users (id, email, status, created_at) VALUES (?1, ?2, 'active', ?3) ON CONFLICT (email) DO NOTHING",
            vec![new_user.into(), inv.email.clone().into(), now.into()],
        ))
        .await?;
    #[derive(Deserialize)]
    struct U {
        id: String,
        status: String,
    }
    let u: U = p
        .db()
        .first(Stmt::new(
            "SELECT id, status FROM users WHERE email = ?1",
            vec![inv.email.clone().into()],
        ))
        .await?
        .ok_or_else(ApiError::internal)?;
    if u.status != "active" {
        return Err(ApiError::not_found(ErrorCode::InvitationNotFound));
    }
    let (ev, job) = event(
        p,
        &t,
        "member.joined",
        &json!({ "user_id": u.id, "role": inv.role }),
        now,
    );
    let metas = p
        .db()
        .batch(vec![
            Stmt::new(
                "UPDATE invitations SET status = 'accepted' WHERE id = ?1 AND status = 'pending' AND expires_at > ?2",
                vec![inv.id.clone().into(), now.into()],
            ),
            Stmt::new(
                "INSERT INTO members (tenant_id, user_id, role, created_at) SELECT ?1, ?2, ?3, ?4 WHERE EXISTS (SELECT 1 FROM invitations WHERE id = ?5 AND status = 'accepted') AND NOT EXISTS (SELECT 1 FROM members WHERE tenant_id = ?1 AND user_id = ?2)",
                vec![
                    inv.tenant_id.clone().into(),
                    u.id.clone().into(),
                    inv.role.clone().into(),
                    now.into(),
                    inv.id.clone().into(),
                ],
            ),
            audit::insert(
                &p.ids().new_id(IdPrefix::Audit),
                Some(&inv.tenant_id),
                Some(&u.id),
                "member.join",
                "user",
                &u.id,
                &json!({ "invitation_id": inv.id, "role": inv.role }),
                request_id,
                now,
            ),
            ev,
        ])
        .await?;
    if metas.first().is_none_or(|m| m.changes == 0) {
        return Err(ApiError::not_found(ErrorCode::InvitationNotFound));
    }
    fan(p, vec![job]).await;
    Ok(u.id)
}

/// The hourly roll-up: pending invitations past `expires_at` become `expired` and free their seats.
pub async fn expire<P: Platform>(p: &P, now: i64, request_id: &str) -> Result<u32, ApiError> {
    #[derive(Deserialize)]
    struct E {
        id: String,
        tenant_id: String,
    }
    let due: Vec<E> = p
        .db()
        .query(Stmt::new(
            "SELECT id, tenant_id FROM invitations WHERE status = 'pending' AND expires_at <= ?1 LIMIT 500",
            vec![now.into()],
        ))
        .await?;
    let mut n = 0;
    for e in due {
        let m = p
            .db()
            .execute(Stmt::new(
                "UPDATE invitations SET status = 'expired' WHERE id = ?1 AND status = 'pending'",
                vec![e.id.clone().into()],
            ))
            .await?;
        if m.changes == 0 {
            continue;
        }
        n += 1;
        if let Some(t) = p.db().first::<tdb::Row>(tdb::get(&e.tenant_id)).await? {
            let actor = Actor {
                id: "system",
                request_id,
            };
            super::seats(p, &t, -1, &e.id, &actor).await;
        }
    }
    Ok(n)
}
