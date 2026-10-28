//! `GET /v1/links/{token}` ([Security § 7.3]): a large attachment of an outbound message replaced by a
//! link (G5). No key: the MAC authenticates the link. A bad, expired or unknown link, or a missing
//! target, is `404 attachment_not_found`.
//!
//! [Security § 7.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::links;
use pylota_identity_platform::{BlobStore as _, Platform};

use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;
use crate::keyring::{self, Purpose};

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let not_found = || ApiError::not_found(ErrorCode::AttachmentNotFound);
    let token = c.route.param("token");
    let kid = links::kid(token).ok_or_else(not_found)?;
    let keys = keyring::keys(c.p.as_ref(), Purpose::Link).await?;
    let key = keys
        .iter()
        .find(|k| k.kid.starts_with(kid) && k.kid.chars().count() == 1)
        .filter(|k| k.verify_until.is_none_or(|u| u > c.now))
        .ok_or_else(not_found)?;
    let link = links::verify(&key.secret, token, c.now / 1000).ok_or_else(not_found)?;
    let k = format!(
        "t/{}/i/{}/out/{}/a/{}",
        link.tenant_id, link.identity_id, link.message_id, link.attachment_id
    );
    let blob = c.p.blobs().get(&k).await?.ok_or_else(not_found)?;
    Ok(Reply {
        status: 200,
        body: None,
        raw: Some(("application/octet-stream", blob.body.to_vec())),
        headers: vec![(
            "content-disposition".into(),
            format!("attachment; filename=\"{}\"", link.attachment_id),
        )],
        stream: None,
    })
}
