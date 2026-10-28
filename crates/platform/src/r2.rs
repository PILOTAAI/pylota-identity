//! R2 (bindings `BLOBS`, and `BACKUP` when `PM_BACKUP_BUCKET` is set). Uploads stream a request body
//! to R2 with its exact length and SHA-256, so R2 itself refuses a short body (`10013`) or a wrong
//! digest (`10037`) ([Uploads]); the error carries the code ([`crate::PlatformError::r2_code`]).
//!
//! [Uploads]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/uploads.md

use crate::buffer::{BodyStream, JsBuffer};
use crate::{PResult, PlatformError};

pub enum BlobBody {
    Bytes(Vec<u8>),
    /// Passed to R2 without a copy into wasm memory (the `email()` handler's raw messages).
    Js(JsBuffer),
}

impl BlobBody {
    pub fn len(&self) -> u64 {
        match self {
            BlobBody::Bytes(b) => b.len() as u64,
            BlobBody::Js(b) => b.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        match self {
            BlobBody::Bytes(b) => b.clone(),
            BlobBody::Js(b) => b.to_vec(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlobMeta {
    pub content_type: Option<String>,
    /// At most [`MAX_CUSTOM_METADATA`] bytes, names and values together.
    pub custom: Vec<(String, String)>,
}

pub struct Blob {
    pub body: BlobBody,
    pub size: u64,
    pub custom: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlobPage {
    /// Keys with their sizes, in key order.
    pub keys: Vec<(String, u64)>,
    pub cursor: Option<String>,
}

pub const MAX_CUSTOM_METADATA: usize = 8_192;
pub const MAX_DELETE_KEYS: usize = 1_000;
pub const MAX_LIST_LIMIT: u32 = 1_000;

/// R2's codes the designs act on.
pub const R2_INCOMPLETE_BODY: u32 = 10013;
pub const R2_BAD_DIGEST: u32 = 10037;
pub const R2_TOO_MANY_REQUESTS: u32 = 10058;

pub trait BlobStore {
    async fn put(&self, key: &str, body: BlobBody, meta: &BlobMeta) -> PResult<()>;
    async fn get(&self, key: &str) -> PResult<Option<Blob>>;
    async fn get_range(&self, key: &str, offset: u64, len: u64) -> PResult<Option<Vec<u8>>>;
    /// The size, if the object exists.
    async fn head(&self, key: &str) -> PResult<Option<u64>>;
    /// At most [`MAX_DELETE_KEYS`] keys per call.
    async fn delete(&self, keys: &[String]) -> PResult<()>;
    /// `limit` at most [`MAX_LIST_LIMIT`].
    async fn list(&self, prefix: &str, cursor: Option<&str>, limit: u32) -> PResult<BlobPage>;
    /// A request body streamed to R2 with its exact length and SHA-256.
    async fn put_stream(
        &self,
        key: &str,
        body: BodyStream,
        len: u64,
        sha256: [u8; 32],
        meta: &BlobMeta,
    ) -> PResult<()>;
    /// The size and the stored SHA-256, if any.
    async fn head_checksum(&self, key: &str) -> PResult<Option<(u64, Option<[u8; 32]>)>>;
    /// A server-side copy of a verified object; R2 checks the digest again.
    async fn copy(&self, from: &str, to: &str, len: u64, sha256: [u8; 32]) -> PResult<()>;
}

/// The limits every implementation applies before calling R2.
pub fn check_meta(binding: &'static str, meta: &BlobMeta) -> PResult<()> {
    let size: usize = meta.custom.iter().map(|(k, v)| k.len() + v.len()).sum();
    if size > MAX_CUSTOM_METADATA {
        return Err(PlatformError::invalid(
            binding,
            format!("custom metadata of {size} bytes; at most {MAX_CUSTOM_METADATA}"),
        ));
    }
    Ok(())
}

pub fn check_delete(binding: &'static str, keys: &[String]) -> PResult<()> {
    if keys.len() > MAX_DELETE_KEYS {
        return Err(PlatformError::invalid(
            binding,
            format!("{} keys; at most {MAX_DELETE_KEYS} per delete", keys.len()),
        ));
    }
    Ok(())
}

pub fn check_list(binding: &'static str, limit: u32) -> PResult<()> {
    if limit == 0 || limit > MAX_LIST_LIMIT {
        return Err(PlatformError::invalid(
            binding,
            format!("list limit {limit}; 1 to {MAX_LIST_LIMIT}"),
        ));
    }
    Ok(())
}

/// The error for an R2 code, worded so that [`PlatformError::r2_code`] reads it back.
pub fn r2_error(binding: &'static str, code: u32, text: &str) -> PlatformError {
    let kind = match code {
        R2_TOO_MANY_REQUESTS => crate::PlatformErrorKind::Unavailable,
        R2_BAD_DIGEST | R2_INCOMPLETE_BODY => crate::PlatformErrorKind::Invalid,
        _ => crate::PlatformErrorKind::Upstream,
    };
    PlatformError::new(kind, binding, format!("r2 {code}: {text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits() {
        let mut meta = BlobMeta::default();
        meta.custom.push(("k".into(), "v".repeat(8_191)));
        assert!(check_meta("BLOBS", &meta).is_ok());
        meta.custom.push(("x".into(), String::new()));
        assert!(check_meta("BLOBS", &meta).is_err());
        assert!(check_delete("BLOBS", &vec![String::new(); 1_001]).is_err());
        assert!(check_list("BLOBS", 0).is_err() && check_list("BLOBS", 1_000).is_ok());
        assert_eq!(r2_error("BLOBS", 10037, "BadDigest").r2_code(), Some(10037));
    }
}
