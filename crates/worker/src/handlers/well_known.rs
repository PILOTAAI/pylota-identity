//! Key publication ([Agent signing keys § 3]): an identity's JWKS and the deployment's Web Bot Auth key
//! directory, both without authentication.
//!
//! [Agent signing keys § 3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::httpsig::{self, Param};
use pylota_identity_core::jwt;
use pylota_identity_platform::{ControlDb as _, Platform};
use serde_json::{Value, json};

use super::http_signatures::nonce;
use crate::ctx::{Ctx, Reply};
use crate::db::identities as idb;
use crate::db::identity_keys as kdb;
use crate::db::tenants as tdb;
use crate::errors::ApiError;
use crate::keyring::web_bot_auth;

fn raw(content_type: &'static str, body: &Value, cache: &str) -> Reply {
    Reply {
        status: 200,
        body: None,
        raw: Some((content_type, body.to_string().into_bytes())),
        headers: vec![("cache-control".into(), cache.into())],
        stream: None,
    }
}

/// `GET /.well-known/jwks/{identity_id}.json`: the active key and the retiring keys inside their
/// overlap. An unknown, deleting, deleted, paused or suspended identity is the same `404` (O1).
pub async fn jwks<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let p = c.p.as_ref();
    let not_found = || ApiError::not_found(ErrorCode::IdentityNotFound);
    let idn: idb::Row = p
        .db()
        .first(idb::get(c.route.param("identity_id")))
        .await?
        .ok_or_else(not_found)?;
    if idn.status != "active" || idn.is_system == 1 {
        return Err(not_found());
    }
    let t: tdb::Row = p
        .db()
        .first(tdb::get(&idn.tenant_id))
        .await?
        .ok_or_else(not_found)?;
    if t.status != "active" {
        return Err(not_found());
    }
    let rows: Vec<kdb::Row> = p.db().query(kdb::published(&idn.id, c.now)).await?;
    let keys: Vec<Value> = rows
        .iter()
        .filter_map(|r| serde_json::from_str(&r.public_jwk).ok())
        .collect();
    Ok(raw(
        "application/jwk-set+json",
        &json!({ "keys": keys }),
        "public, max-age=300",
    ))
}

/// `GET /.well-known/http-message-signatures-directory`: the deployment keys as a JWKS, the response
/// signed once per listed key over the request's `@authority` (O12). `404 key_not_found` while
/// `PM_WEB_BOT_AUTH=off`.
pub async fn directory<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let p = c.p.as_ref();
    if !p.config().web_bot_auth {
        return Err(ApiError::not_found(ErrorCode::KeyNotFound));
    }
    let keys = web_bot_auth::keys(p).await?;
    let authority = p.config().api_host.to_ascii_lowercase();
    let created = c.now.div_euclid(1000) as u64;
    let mut inputs = Vec::new();
    let mut sigs = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        let params = [
            Param::Created(created),
            Param::Expires(created + 300),
            Param::KeyId(k.kid.clone()),
            Param::Alg("ed25519".into()),
            Param::Nonce(nonce(p)),
            Param::Tag("http-message-signatures-directory".into()),
        ];
        let base = httpsig::signature_base(&[("@authority;req", &authority)], &params)
            .map_err(|_| ApiError::internal())?;
        let input = httpsig::signature_params(&["@authority;req"], &params)
            .map_err(|_| ApiError::internal())?;
        let sig = jwt::ed25519(&k.seed, base.as_bytes());
        inputs.push(format!("sig{}={input}", i + 1));
        sigs.push(format!("sig{}={}", i + 1, httpsig::sf_bytes(&sig)));
    }
    let body = json!({ "keys": keys.iter().map(|k| k.public_jwk.clone()).collect::<Vec<_>>() });
    let mut reply = raw(
        "application/http-message-signatures-directory+json",
        &body,
        "max-age=86400",
    );
    reply
        .headers
        .push(("signature-input".into(), inputs.join(", ")));
    reply.headers.push(("signature".into(), sigs.join(", ")));
    Ok(reply)
}
