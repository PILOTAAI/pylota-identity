//! Addresses (Identities and domains › Addresses). M6 has the list; adding, promoting and retiring an
//! address need a tenant domain and arrive with M13.

use pylota_identity_platform::{ControlDb as _, Platform};

use super::identity_in_scope;
use crate::ctx::{Ctx, Reply};
use crate::db::addresses as adb;
use crate::errors::ApiError;

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
