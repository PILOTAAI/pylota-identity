//! Workspace members ([Console › Members]): the list, removal and leaving, role changes and ownership
//! transfers, shared by the REST routes and the console. Each change is one D1 batch with its audit
//! rows and `member.*` platform events; the keys and webhook endpoints a person created follow them
//! out (W36, W47).
//!
//! [Console › Members]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#members

pub mod invitations;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::console::Role;
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform, QueueProducer as _};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::{KeyScope, Level};
use crate::db::{audit, tenants as tdb};
use crate::errors::ApiError;
use crate::webhooks::envelope::{OwnerKind, WebhookJob, envelope};

/// Who acts: a key, or a signed-in person (whose `usr_` ID stands where a key ID would).
pub struct Actor<'a> {
    pub id: &'a str,
    pub request_id: &'a str,
}

impl Actor<'_> {
    /// The person, when a signed-in person acts.
    pub fn user(&self) -> Option<&str> {
        self.id.starts_with("usr_").then_some(self.id)
    }
}

/// A `member.*` event: a platform event of the workspace (Console › Audit and events). Answers the
/// `event_index` insert for the change's batch and the `Fanout` to queue after it.
pub fn event<P: Platform>(
    p: &P,
    t: &tdb::Row,
    event_type: &str,
    data: &Value,
    now: i64,
) -> (Stmt, WebhookJob) {
    let event_id = p.ids().new_id(IdPrefix::Event);
    let body = envelope(&event_id, event_type, now, Some(&t.id), None, None, data);
    (
        Stmt::new(
            "INSERT OR IGNORE INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, partner_id, payload_json, occurred_at) VALUES (?1, ?2, NULL, ?3, 'platform', 'platform', ?4, ?5, ?6)",
            vec![
                event_id.clone().into(),
                t.id.clone().into(),
                event_type.into(),
                t.partner_id.clone().into(),
                body.into(),
                now.into(),
            ],
        ),
        WebhookJob::Fanout {
            v: 1,
            event_id,
            event_type: event_type.to_owned(),
            tenant_id: Some(t.id.clone()),
            identity_id: None,
            owner_kind: OwnerKind::Platform,
            owner_id: "platform".into(),
            occurred_at: now,
        },
    )
}

/// Queues fan-outs after their batch committed; a lost send is repaired by the every-minute sweep of
/// unfanned platform events.
pub async fn fan<P: Platform>(p: &P, jobs: Vec<WebhookJob>) {
    for j in jobs {
        let _ = p.queues().send(QueueName::Webhooks, &j, 0).await;
    }
}

/// A member row joined with its person.
#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    pub user_id: String,
    pub email: String,
    pub name: Option<String>,
    pub role: String,
    pub last_login_at: Option<i64>,
    pub created_at: i64,
}

impl Row {
    pub fn json(&self) -> Value {
        json!({
            "user_id": self.user_id, "email": self.email, "name": self.name, "role": self.role,
            "last_login_at": self.last_login_at.map(rfc3339), "created_at": rfc3339(self.created_at),
        })
    }

    pub fn role(&self) -> Role {
        Role::parse(&self.role).unwrap_or(Role::Viewer)
    }
}

pub fn of_tenant(tenant_id: &str) -> Stmt {
    Stmt::new(
        "SELECT m.user_id, u.email, u.name, m.role, u.last_login_at, m.created_at FROM members m JOIN users u ON u.id = m.user_id WHERE m.tenant_id = ?1 ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 WHEN 'member' THEN 2 ELSE 3 END, u.email",
        vec![tenant_id.into()],
    )
}

pub async fn get<P: Platform>(p: &P, tenant_id: &str, user_id: &str) -> Result<Row, ApiError> {
    p.db()
        .first::<Row>(Stmt::new(
            "SELECT m.user_id, u.email, u.name, m.role, u.last_login_at, m.created_at FROM members m JOIN users u ON u.id = m.user_id WHERE m.tenant_id = ?1 AND m.user_id = ?2",
            vec![tenant_id.into(), user_id.into()],
        ))
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::MemberNotFound))
}

/// `GET /v1/tenants/{tenant_id}/members`: members, pending invitations and seats.
pub async fn list<P: Platform>(p: &P, tenant_id: &str, now: i64) -> Result<Value, ApiError> {
    let members: Vec<Row> = p.db().query(of_tenant(tenant_id)).await?;
    let pending: Vec<invitations::Row> = p.db().query(invitations::pending(tenant_id, now)).await?;
    Ok(json!({
        "data": members.iter().map(Row::json).collect::<Vec<_>>(),
        "invitations": pending.iter().map(invitations::Row::json).collect::<Vec<_>>(),
        // Seat allowances arrive with plans (M22); until then seats are unlimited.
        "seats": { "granted": null, "used": members.len() + pending.len() },
    }))
}

#[derive(Deserialize)]
struct KeyRow {
    id: String,
    permissions_json: String,
}

/// The statements that take a person's keys and webhook endpoints with them (W36, W47): on removal
/// every key traced to them is revoked and every endpoint they created disabled; on a role change only
/// keys holding a permission the new role lacks are revoked (the others record the new role), and the
/// endpoints only when the new role lacks `webhooks:manage`.
async fn follow<P: Platform>(
    p: &P,
    tenant_id: &str,
    user_id: &str,
    new_role: Option<Role>,
    actor: &Actor<'_>,
    now: i64,
) -> Result<Vec<Stmt>, ApiError> {
    let keys: Vec<KeyRow> = p
        .db()
        .query(Stmt::new(
            "SELECT id, permissions_json FROM api_keys WHERE tenant_id = ?1 AND created_by_user_id = ?2 AND revoked_at IS NULL",
            vec![tenant_id.into(), user_id.into()],
        ))
        .await?;
    let allowed = new_role.map(Role::permissions);
    let reason = if new_role.is_some() {
        "creator_role_changed"
    } else {
        "creator_removed"
    };
    let mut out = Vec::new();
    for k in keys {
        let perms: Vec<String> = serde_json::from_str(&k.permissions_json).unwrap_or_default();
        let keep = allowed
            .as_ref()
            .is_some_and(|a| perms.iter().all(|x| a.contains(&x.as_str())));
        if keep {
            out.push(Stmt::new(
                "UPDATE api_keys SET created_by_role = ?2 WHERE id = ?1",
                vec![
                    k.id.clone().into(),
                    new_role.map(Role::as_str).unwrap_or("").into(),
                ],
            ));
            continue;
        }
        out.push(crate::db::keys::revoke(&k.id, now));
        out.push(audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            Some(tenant_id),
            Some(actor.id),
            "key.revoke",
            "key",
            &k.id,
            &json!({ "reason": reason, "user_id": user_id }),
            actor.request_id,
            now,
        ));
    }
    let endpoints_go = allowed
        .as_ref()
        .is_none_or(|a| !a.contains(&"webhooks:manage"));
    if endpoints_go {
        #[derive(Deserialize)]
        struct E {
            id: String,
        }
        let eps: Vec<E> = p
            .db()
            .query(Stmt::new(
                "SELECT id FROM webhook_endpoints WHERE tenant_id = ?1 AND created_by_user_id = ?2 AND enabled = 1",
                vec![tenant_id.into(), user_id.into()],
            ))
            .await?;
        for e in eps {
            out.push(Stmt::new(
                "UPDATE webhook_endpoints SET enabled = 0, disabled_reason = 'creator_removed', updated_at = ?2 WHERE id = ?1 AND enabled = 1",
                vec![e.id.clone().into(), now.into()],
            ));
            out.push(audit::insert(
                &p.ids().new_id(IdPrefix::Audit),
                Some(tenant_id),
                Some(actor.id),
                "webhook.disable",
                "webhook",
                &e.id,
                &json!({ "reason": "creator_removed", "user_id": user_id }),
                actor.request_id,
                now,
            ));
        }
    }
    Ok(out)
}

/// Removes a member, or lets one leave (`leaving`): one batch deletes the membership, ends their
/// sessions in this workspace, takes their keys, endpoints and pending invitations with them, and
/// writes the audit row and `member.removed` (W9, W36, W47). The owner is refused (W10).
pub async fn remove<P: Platform>(
    p: &P,
    t: &tdb::Row,
    user_id: &str,
    leaving: bool,
    actor: &Actor<'_>,
    now: i64,
) -> Result<(), ApiError> {
    let m = get(p, &t.id, user_id).await?;
    if m.role == "owner" {
        return Err(owner_required());
    }
    let mut batch = vec![
        Stmt::new(
            "DELETE FROM members WHERE tenant_id = ?1 AND user_id = ?2 AND role <> 'owner'",
            vec![t.id.clone().into(), user_id.into()],
        ),
        Stmt::new(
            "UPDATE sessions SET revoked_at = ?3 WHERE user_id = ?1 AND tenant_id = ?2 AND revoked_at IS NULL",
            vec![user_id.into(), t.id.clone().into(), now.into()],
        ),
    ];
    batch.extend(follow(p, &t.id, user_id, None, actor, now).await?);
    #[derive(Deserialize)]
    struct I {
        id: String,
    }
    let sent: Vec<I> = p
        .db()
        .query(Stmt::new(
            "SELECT id FROM invitations WHERE tenant_id = ?1 AND invited_by = ?2 AND status = 'pending'",
            vec![t.id.clone().into(), user_id.into()],
        ))
        .await?;
    for i in &sent {
        batch.push(Stmt::new(
            "UPDATE invitations SET status = 'revoked' WHERE id = ?1 AND status = 'pending'",
            vec![i.id.clone().into()],
        ));
        batch.push(audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            Some(&t.id),
            Some(actor.id),
            "member.invite_revoke",
            "invitation",
            &i.id,
            &json!({ "reason": "inviter_removed" }),
            actor.request_id,
            now,
        ));
    }
    batch.push(audit::insert(
        &p.ids().new_id(IdPrefix::Audit),
        Some(&t.id),
        Some(actor.id),
        if leaving {
            "member.leave"
        } else {
            "member.remove"
        },
        "user",
        user_id,
        &json!({ "role": m.role }),
        actor.request_id,
        now,
    ));
    let (ev, job) = event(p, t, "member.removed", &json!({ "user_id": user_id }), now);
    batch.push(ev);
    let metas = p.db().batch(batch).await?;
    if metas.first().is_none_or(|m| m.changes == 0) {
        return Err(ApiError::not_found(ErrorCode::MemberNotFound));
    }
    seats(p, t, -(1 + sent.len() as i64), user_id, actor).await;
    fan(p, vec![job]).await;
    Ok(())
}

pub fn owner_required() -> ApiError {
    ApiError::new(
        ErrorCode::OwnerRequired,
        "The owner cannot leave, be removed or change role: transfer ownership to an admin first.",
    )
}

/// Releases (or takes) seats in the workspace's quota; a lost call is corrected by the hourly
/// reconciliation.
pub async fn seats<P: Platform>(p: &P, t: &tdb::Row, delta: i64, r#ref: &str, actor: &Actor<'_>) {
    use pylota_identity_platform::ObjectClient as _;
    use pylota_identity_platform::durable::DoClass;
    if delta == 0 {
        return;
    }
    let env = crate::rpc::envelope_for(
        p,
        Some(&t.id),
        None,
        actor.request_id,
        None,
        crate::quota::QuotaRequest::Adjust {
            feature: crate::quota::Feature::Seats,
            delta,
            r#ref: r#ref.to_owned(),
        },
    );
    let _ = p
        .objects()
        .call::<_, Value>(DoClass::Quota, &t.quota_do_id, &env)
        .await;
}

/// Changes a member's role (owner or admin; an admin never touches the owner nor makes an owner). The
/// person's keys and endpoints follow the new role (W36, W47).
pub async fn change_role<P: Platform>(
    p: &P,
    t: &tdb::Row,
    user_id: &str,
    to: Role,
    by: Role,
    actor: &Actor<'_>,
    now: i64,
) -> Result<(), ApiError> {
    let m = get(p, &t.id, user_id).await?;
    if m.role == "owner" || to == Role::Owner {
        return Err(owner_required());
    }
    if by < Role::Admin {
        return Err(ApiError::permission_denied(&["members:manage"]));
    }
    if m.role == to.as_str() {
        return Ok(());
    }
    let mut batch = vec![Stmt::new(
        "UPDATE members SET role = ?3 WHERE tenant_id = ?1 AND user_id = ?2 AND role <> 'owner'",
        vec![t.id.clone().into(), user_id.into(), to.as_str().into()],
    )];
    batch.extend(follow(p, &t.id, user_id, Some(to), actor, now).await?);
    batch.push(audit::insert(
        &p.ids().new_id(IdPrefix::Audit),
        Some(&t.id),
        Some(actor.id),
        "member.role_change",
        "user",
        user_id,
        &json!({ "from": m.role, "to": to.as_str() }),
        actor.request_id,
        now,
    ));
    let (ev, job) = event(
        p,
        t,
        "member.role_changed",
        &json!({ "user_id": user_id, "from": m.role, "to": to.as_str() }),
        now,
    );
    batch.push(ev);
    p.db().batch(batch).await?;
    fan(p, vec![job]).await;
    Ok(())
}

/// Transfers ownership from `owner` to an existing admin: both updates in one batch, in the order the
/// unique owner index allows; a target that is not an admin changes nothing (`400`). The former
/// owner's keys holding `tenants:erase` are revoked (W36).
pub async fn transfer<P: Platform>(
    p: &P,
    t: &tdb::Row,
    owner: &str,
    to: &str,
    actor: &Actor<'_>,
    now: i64,
) -> Result<(), ApiError> {
    let mut batch = vec![
        Stmt::new(
            "UPDATE members SET role = 'admin' WHERE tenant_id = ?1 AND user_id = ?2 AND role = 'owner' AND EXISTS (SELECT 1 FROM members WHERE tenant_id = ?1 AND user_id = ?3 AND role = 'admin')",
            vec![t.id.clone().into(), owner.into(), to.into()],
        ),
        Stmt::new(
            "UPDATE members SET role = 'owner' WHERE tenant_id = ?1 AND user_id = ?2 AND role = 'admin' AND NOT EXISTS (SELECT 1 FROM members WHERE tenant_id = ?1 AND role = 'owner')",
            vec![t.id.clone().into(), to.into()],
        ),
    ];
    let target = get(p, &t.id, to).await?;
    if target.role != "admin" || owner == to {
        return Err(new_owner_must_be_admin());
    }
    batch.extend(follow(p, &t.id, owner, Some(Role::Admin), actor, now).await?);
    batch.push(audit::insert(
        &p.ids().new_id(IdPrefix::Audit),
        Some(&t.id),
        Some(actor.id),
        "member.ownership_transfer",
        "user",
        to,
        &json!({ "from": owner }),
        actor.request_id,
        now,
    ));
    let (e1, j1) = event(
        p,
        t,
        "member.role_changed",
        &json!({ "user_id": owner, "from": "owner", "to": "admin" }),
        now,
    );
    let (e2, j2) = event(
        p,
        t,
        "member.role_changed",
        &json!({ "user_id": to, "from": "admin", "to": "owner" }),
        now,
    );
    batch.push(e1);
    batch.push(e2);
    let metas = p.db().batch(batch).await?;
    if metas.iter().take(2).any(|m| m.changes != 1) {
        return Err(new_owner_must_be_admin());
    }
    fan(p, vec![j1, j2]).await;
    Ok(())
}

fn new_owner_must_be_admin() -> ApiError {
    ApiError::invalid(
        "user_id",
        "The new owner must be an admin of this workspace.",
    )
}

/// The principal of a signed-in person in a workspace: a tenant-level scope holding the role's
/// permissions, which the route table checks like a key's (Console › What the console is).
pub fn principal(user_id: &str, email: &str, t: &tdb::Row, role: Role) -> KeyScope {
    KeyScope {
        key_id: user_id.to_owned(),
        name: email.to_owned(),
        level: Level::Tenant,
        mode: pylota_identity_core::keys::Mode::parse(&t.mode)
            .unwrap_or(pylota_identity_core::keys::Mode::Live),
        partner_id: None,
        tenant_id: Some(t.id.clone()),
        identity_id: None,
        permissions: role.permissions().into_iter().map(str::to_owned).collect(),
        mcp_tools: None,
        expires_at: None,
        created_by_user_id: Some(user_id.to_owned()),
        created_by_role: Some(role.as_str().to_owned()),
        created_by_level: "tenant".into(),
        tenant_partner_id: t.partner_id.clone(),
        provisional: false,
        person: Some(user_id.to_owned()),
    }
}
