//! Identity keys ([Agent signing keys § 2], § 7): one `active` Ed25519 key per identity, created on first
//! signing or with `POST …/keys`, rotated with an overlap and revoked at once. The seed is sealed under
//! the master key and opened only to sign; no response, log line or event carries it.
//!
//! [Agent signing keys § 2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::{crypto, jwk, jwt};
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform, Rng as _};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::identities::event_intent;
use super::{Count, identity_in_scope};
use crate::ctx::{Ctx, Reply};
use crate::db::identities as idb;
use crate::db::identity_keys as kdb;
use crate::db::{audit, intents};
use crate::errors::ApiError;

const DAY_MS: i64 = 86_400_000;

fn aad(kid: &str) -> String {
    crypto::aad("identity_keys", "private_enc", kid)
}

/// An opened active key, for one signature.
pub struct Signer {
    pub kid: String,
    pub seed: Zeroizing<[u8; 32]>,
}

/// Opens a key's seed with whichever master-key slot matches its envelope.
pub fn open<P: Platform>(p: &P, row: &kdb::Row) -> Result<Signer, ApiError> {
    let s = p.secrets();
    let slots: Vec<&[u8; 32]> = [s.master_key_a.as_ref(), s.master_key_b.as_ref()]
        .into_iter()
        .flatten()
        .collect();
    let envelope = std::str::from_utf8(&row.private_enc).map_err(|_| ApiError::internal())?;
    let seed = Zeroizing::new(crypto::open(&slots, &aad(&row.id), envelope).map_err(|e| {
        crate::log::error(
            "identity_key_open_failed",
            &[("reason", format!("{e:?}").into())],
        );
        ApiError::internal()
    })?);
    let seed: [u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| ApiError::internal())?;
    Ok(Signer {
        kid: row.id.clone(),
        seed: Zeroizing::new(seed),
    })
}

/// A fresh key whose thumbprint was never published: a tombstoned or existing kid draws a new seed (O7).
async fn generate<P: Platform>(p: &P) -> Result<(String, String, Vec<u8>), ApiError> {
    for _ in 0..4 {
        let seed = Zeroizing::new(p.rng().bytes::<32>());
        let x = jwt::public_key(&seed);
        let kid = jwk::thumbprint(&x);
        let used: Option<Count> = p.db().first(kdb::kid_used(&kid)).await?;
        if used.is_some_and(|c| c.n > 0) {
            continue;
        }
        let nonce: [u8; 12] = p.rng().bytes();
        let sealed = crypto::seal(
            p.secrets().active_master_key(),
            nonce,
            &aad(&kid),
            &seed[..],
        );
        return Ok((kid, jwt::published_jwk(&x).to_string(), sealed.into_bytes()));
    }
    Err(ApiError::internal())
}

/// Who asked for a key change, for the audit row.
pub struct Actor<'a> {
    pub key_id: &'a str,
    pub request_id: &'a str,
}

/// Creates the identity's active key if it has none. Returns the active key and whether this call
/// created it. Racing creations insert one row each; `identity_keys_one_active` keeps one, the loser
/// reads it, and only the winner's event (`event_type`) and audit row are written (O28).
pub async fn ensure_active<P: Platform>(
    p: &P,
    idn: &idb::Row,
    actor: &Actor<'_>,
    event: Option<(&str, Value)>,
    audit_action: Option<&str>,
    now: i64,
) -> Result<(kdb::Row, bool), ApiError> {
    if let Some(row) = p.db().first::<kdb::Row>(kdb::active(&idn.id)).await? {
        return Ok((row, false));
    }
    let (kid, public_jwk, sealed) = generate(p).await?;
    let mut batch = vec![kdb::insert_active(
        &kid,
        &idn.id,
        &idn.tenant_id,
        &public_jwk,
        sealed,
        now,
    )];
    if let Some(action) = audit_action {
        batch.push(audit::when_key(audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            Some(&idn.tenant_id),
            Some(actor.key_id),
            action,
            "identity_key",
            &kid,
            &json!({ "identity_id": idn.id }),
            actor.request_id,
            now,
        )));
    }
    let has_mailbox = !idn.mailbox_do_id.is_empty();
    let event_type = event
        .as_ref()
        .map(|(t, _)| *t)
        .unwrap_or("identity.key_created");
    if has_mailbox {
        let data = event
            .map(|(_, d)| d)
            .unwrap_or_else(|| json!({ "identity_id": idn.id, "kid": kid }));
        batch.push(intents::when_key(
            event_intent(actor.request_id, idn, event_type, &data, now),
            &kid,
        ));
    }
    let metas = p.db().batch(batch).await?;
    let won = metas.first().is_some_and(|m| m.changes == 1);
    if won && has_mailbox {
        crate::intents::flush_object(p, actor.request_id, &idn.mailbox_do_id).await;
    }
    let row = p
        .db()
        .first::<kdb::Row>(kdb::active(&idn.id))
        .await?
        .ok_or_else(ApiError::internal)?;
    let created = won && row.id == kid;
    Ok((row, created))
}

/// `GET /v1/identities/{identity_id}/keys`: every key, `retired` ones included.
pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let status = c.query("status");
    if let Some(s) = status
        && !matches!(s, "active" | "retiring" | "retired")
    {
        return Err(ApiError::invalid(
            "status",
            "status is active, retiring or retired.",
        ));
    }
    let limit = c.limit()?;
    let after = c.after(&["status"]).await?;
    let rows: Vec<kdb::Row> =
        c.p.db()
            .query(kdb::page(&idn.id, status, after, limit))
            .await?;
    c.page(
        rows,
        limit,
        &["status"],
        |r| (r.created_at, r.id.clone()),
        kdb::Row::json,
    )
    .await
}

fn actor<P: Platform>(c: &Ctx<P>) -> Actor<'_> {
    Actor {
        key_id: &c.key().key_id,
        request_id: &c.request_id,
    }
}

/// `POST /v1/identities/{identity_id}/keys`: `201` with a new key, `200` with the active one.
pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let (row, created) = ensure_active(
        c.p.as_ref(),
        &idn,
        &actor(c),
        None,
        Some("identity_key.create"),
        c.now,
    )
    .await?;
    Ok(Reply::json(if created { 201 } else { 200 }, row.json()))
}

/// `POST /v1/identities/{identity_id}/keys/rotate`: a new active key; the previous one is `retiring`
/// until now + `PM_IDENTITY_KEY_OVERLAP_DAYS`. With no active key the rotation creates the first one
/// (`previous: null`, `identity.key_created`).
pub async fn rotate<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let p = c.p.as_ref();
    let now = c.now;
    let Some(prev) = p.db().first::<kdb::Row>(kdb::active(&idn.id)).await? else {
        let (row, _) =
            ensure_active(p, &idn, &actor(c), None, Some("identity_key.rotate"), now).await?;
        return Ok(Reply::json(
            200,
            json!({ "key": row.json(), "previous": null }),
        ));
    };
    let verify_until = now + i64::from(p.config().identity_key_overlap_days) * DAY_MS;
    let (kid, public_jwk, sealed) = generate(p).await?;
    let mut batch = vec![
        kdb::retire_active(&idn.id, &prev.id, verify_until),
        kdb::insert_active(&kid, &idn.id, &idn.tenant_id, &public_jwk, sealed, now),
        audit::when_key(audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            Some(&idn.tenant_id),
            Some(&c.key().key_id),
            "identity_key.rotate",
            "identity_key",
            &kid,
            &json!({ "identity_id": idn.id, "previous_kid": prev.id }),
            &c.request_id,
            now,
        )),
    ];
    if !idn.mailbox_do_id.is_empty() {
        batch.push(intents::when_key(
            event_intent(
                &c.request_id,
                &idn,
                "identity.key_rotated",
                &json!({ "identity_id": idn.id, "kid": kid, "previous_kid": prev.id }),
                now,
            ),
            &kid,
        ));
    }
    let metas = p.db().batch(batch).await?;
    let won = metas.get(1).is_some_and(|m| m.changes == 1);
    if won && !idn.mailbox_do_id.is_empty() {
        crate::intents::flush_object(p, &c.request_id, &idn.mailbox_do_id).await;
    }
    // A concurrent rotation that made its key active first answers for both: this key, its audit row
    // and its event were not written.
    let key: kdb::Row = p
        .db()
        .first(kdb::active(&idn.id))
        .await?
        .ok_or_else(ApiError::internal)?;
    let previous: Option<kdb::Row> = p.db().first(kdb::get(&idn.id, &prev.id)).await?;
    Ok(Reply::json(
        200,
        json!({ "key": key.json(), "previous": previous.map(|r| r.json()) }),
    ))
}

/// `POST /v1/identities/{identity_id}/keys/{kid}/revoke`: the key is `retired` at once and leaves the
/// JWKS (O3). Revoking a retired key answers it unchanged.
pub async fn revoke<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let p = c.p.as_ref();
    let kid = c.route.param("kid");
    let not_found = || ApiError::not_found(ErrorCode::KeyNotFound);
    let row: kdb::Row = p
        .db()
        .first(kdb::get(&idn.id, kid))
        .await?
        .ok_or_else(not_found)?;
    if row.status == "retired" {
        return Ok(Reply::json(200, row.json()));
    }
    let now = c.now;
    let mut batch = vec![
        kdb::revoke(&idn.id, kid, now),
        audit::insert(
            &p.ids().new_id(IdPrefix::Audit),
            Some(&idn.tenant_id),
            Some(&c.key().key_id),
            "identity_key.revoke",
            "identity_key",
            kid,
            &json!({ "identity_id": idn.id, "previous_status": row.status }),
            &c.request_id,
            now,
        ),
    ];
    if !idn.mailbox_do_id.is_empty() {
        batch.push(event_intent(
            &c.request_id,
            &idn,
            "identity.key_revoked",
            &json!({ "identity_id": idn.id, "kid": kid }),
            now,
        ));
    }
    p.db().batch(batch).await?;
    if !idn.mailbox_do_id.is_empty() {
        crate::intents::flush_object(p, &c.request_id, &idn.mailbox_do_id).await;
    }
    let row: kdb::Row = p
        .db()
        .first(kdb::get(&idn.id, kid))
        .await?
        .ok_or_else(not_found)?;
    Ok(Reply::json(200, row.json()))
}
