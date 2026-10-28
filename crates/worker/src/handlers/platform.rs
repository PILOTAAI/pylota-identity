//! `POST /v1/platform/keys/{purpose}/rotate` (Security § 6.2): `platform:ops`, audit-logged.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde_json::json;

use crate::ctx::{Ctx, Reply};
use crate::db::audit;
use crate::errors::ApiError;
use crate::keyring::{self, Purpose};

pub async fn rotate_signing_key<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let purpose = match c.route.param("purpose") {
        "web_bot_auth" => {
            return Err(ApiError::new(
                ErrorCode::WebBotAuthDisabled,
                "Signed HTTP requests arrive with M25.",
            )
            .details(json!({ "purpose": "web_bot_auth" })));
        }
        p => Purpose::parse(p).ok_or_else(|| {
            ApiError::invalid(
                "purpose",
                "purpose is thread, link, cursor or web_bot_auth.",
            )
        })?,
    };
    let revoke = match c.query("revoke_previous") {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => return Err(ApiError::invalid("revoke_previous", "true or false.")),
    };
    let r = keyring::rotate(c.p.as_ref(), purpose, revoke).await?;
    c.p.db()
        .execute(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            None,
            Some(&c.key().key_id),
            "signing_key.rotate",
            "signing_key",
            &format!("{}:{}", purpose.as_str(), r.kid),
            &json!({ "revoke_previous": revoke }),
            &c.request_id,
            c.now,
        ))
        .await?;
    Ok(Reply::json(
        200,
        json!({
            "purpose": purpose.as_str(), "kid": r.kid, "created_at": rfc3339(r.created_at),
            "previous": { "kid": r.previous_kid, "verify_until": rfc3339(r.previous_verify_until), "revoked": r.revoked },
        }),
    ))
}
