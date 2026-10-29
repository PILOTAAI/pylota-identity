//! The nightly `backup` job ([Privacy and erasure § 5.4]): every object under `t/` that `BACKUP`
//! lacks is copied with its custom metadata, except uploads (`…/up/`). The job never deletes an
//! object that still exists in `BLOBS`; after each copy it checks `BLOBS` again and removes its copy
//! when an erasure deleted the key meanwhile (I11).
//!
//! [Privacy and erasure § 5.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/privacy.md

use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::r2::{BlobBody, BlobMeta};
use pylota_identity_platform::{BlobStore as _, Platform};
use serde_json::json;

use super::{Ctx, Slice, StepResult};

pub const STEPS: &[&str] = &["copy"];

pub async fn run<P: Platform, S: Sql>(
    c: &Ctx<'_, P, S>,
    _step: &str,
    cursor: Option<&str>,
) -> StepResult {
    let Some(backup) = c.p.backup() else {
        return Ok(Slice::done());
    };
    let page = c.p.blobs().list("t/", cursor, 200).await?;
    let (mut copied, mut skipped, mut revoked) = (0u32, 0u32, 0u32);
    for (key, _) in &page.keys {
        if key.contains("/up/") || backup.head(key).await?.is_some() {
            skipped += 1;
            continue;
        }
        let Some(blob) = c.p.blobs().get(key).await? else {
            continue;
        };
        backup
            .put(
                key,
                BlobBody::Bytes(blob.body.to_vec()),
                &BlobMeta {
                    content_type: None,
                    custom: blob.custom.clone(),
                },
            )
            .await?;
        if c.p.blobs().head(key).await?.is_none() {
            backup.delete(std::slice::from_ref(key)).await?;
            revoked += 1;
        } else {
            copied += 1;
        }
    }
    for (result, n) in [
        ("copied", copied),
        ("skipped", skipped),
        ("revoked", revoked),
    ] {
        if n > 0 {
            crate::log::info(
                "backup_objects",
                &[("result", result.into()), ("n", i64::from(n).into())],
            );
            crate::metrics::count(c.p, "backup_objects_total", None, n);
        }
    }
    let counts = json!({ "copied": copied, "skipped": skipped, "revoked": revoked });
    Ok(match page.cursor {
        Some(next) => Slice::More {
            cursor: next,
            counts,
        },
        None => Slice::Done { counts },
    })
}
