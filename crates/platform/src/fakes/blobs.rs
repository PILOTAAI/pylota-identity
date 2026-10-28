//! R2 as a sorted map. `put_stream` and `copy` check the length and SHA-256 as R2 does, and
//! [`FakeBlobs::fail_next`] makes the next write fail with an R2 code (`10013`, `10037`, `10058`).

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};

use sha2::{Digest, Sha256};

use crate::PResult;
use crate::buffer::BodyStream;
use crate::r2::{
    Blob, BlobBody, BlobMeta, BlobPage, BlobStore, R2_BAD_DIGEST, R2_INCOMPLETE_BODY, check_delete,
    check_list, check_meta, r2_error,
};

#[derive(Clone, Debug)]
struct Stored {
    bytes: Vec<u8>,
    meta: BlobMeta,
    sha256: Option<[u8; 32]>,
}

#[derive(Default)]
pub struct FakeBlobs {
    objects: RefCell<BTreeMap<String, Stored>>,
    failures: RefCell<VecDeque<u32>>,
}

const BINDING: &str = "BLOBS";

impl FakeBlobs {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next write (`put`, `put_stream`, `copy`, `delete`) fails with this R2 code.
    pub fn fail_next(&self, code: u32) {
        self.failures.borrow_mut().push_back(code);
    }

    pub fn keys(&self) -> Vec<String> {
        self.objects.borrow().keys().cloned().collect()
    }

    pub fn bytes(&self, key: &str) -> Option<Vec<u8>> {
        self.objects.borrow().get(key).map(|s| s.bytes.clone())
    }

    pub fn content_type(&self, key: &str) -> Option<String> {
        self.objects.borrow().get(key)?.meta.content_type.clone()
    }

    fn injected(&self) -> PResult<()> {
        match self.failures.borrow_mut().pop_front() {
            Some(code) => Err(r2_error(BINDING, code, "injected by the test")),
            None => Ok(()),
        }
    }

    fn store(&self, key: &str, bytes: Vec<u8>, meta: &BlobMeta, sha256: Option<[u8; 32]>) {
        self.objects.borrow_mut().insert(
            key.to_owned(),
            Stored {
                bytes,
                meta: meta.clone(),
                sha256,
            },
        );
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

impl BlobStore for FakeBlobs {
    async fn put(&self, key: &str, body: BlobBody, meta: &BlobMeta) -> PResult<()> {
        check_meta(BINDING, meta)?;
        self.injected()?;
        self.store(key, body.to_vec(), meta, None);
        Ok(())
    }

    async fn get(&self, key: &str) -> PResult<Option<Blob>> {
        Ok(self.objects.borrow().get(key).map(|s| Blob {
            size: s.bytes.len() as u64,
            body: BlobBody::Bytes(s.bytes.clone()),
            custom: s.meta.custom.clone(),
        }))
    }

    async fn get_range(&self, key: &str, offset: u64, len: u64) -> PResult<Option<Vec<u8>>> {
        Ok(self.objects.borrow().get(key).map(|s| {
            let start = (offset as usize).min(s.bytes.len());
            let end = start.saturating_add(len as usize).min(s.bytes.len());
            s.bytes[start..end].to_vec()
        }))
    }

    async fn head(&self, key: &str) -> PResult<Option<u64>> {
        Ok(self.objects.borrow().get(key).map(|s| s.bytes.len() as u64))
    }

    async fn delete(&self, keys: &[String]) -> PResult<()> {
        check_delete(BINDING, keys)?;
        self.injected()?;
        let mut objects = self.objects.borrow_mut();
        for k in keys {
            objects.remove(k);
        }
        Ok(())
    }

    async fn list(&self, prefix: &str, cursor: Option<&str>, limit: u32) -> PResult<BlobPage> {
        check_list(BINDING, limit)?;
        let objects = self.objects.borrow();
        let mut keys: Vec<(String, u64)> = objects
            .iter()
            .filter(|(k, _)| k.starts_with(prefix) && cursor.is_none_or(|c| k.as_str() > c))
            .take(limit as usize + 1)
            .map(|(k, s)| (k.clone(), s.bytes.len() as u64))
            .collect();
        let more = keys.len() > limit as usize;
        keys.truncate(limit as usize);
        Ok(BlobPage {
            cursor: if more {
                keys.last().map(|(k, _)| k.clone())
            } else {
                None
            },
            keys,
        })
    }

    async fn put_stream(
        &self,
        key: &str,
        body: BodyStream,
        len: u64,
        sha256: [u8; 32],
        meta: &BlobMeta,
    ) -> PResult<()> {
        check_meta(BINDING, meta)?;
        self.injected()?;
        if body.bytes.len() as u64 != len {
            return Err(r2_error(BINDING, R2_INCOMPLETE_BODY, "IncompleteBody"));
        }
        if digest(&body.bytes) != sha256 {
            return Err(r2_error(BINDING, R2_BAD_DIGEST, "BadDigest"));
        }
        self.store(key, body.bytes, meta, Some(sha256));
        Ok(())
    }

    async fn head_checksum(&self, key: &str) -> PResult<Option<(u64, Option<[u8; 32]>)>> {
        Ok(self
            .objects
            .borrow()
            .get(key)
            .map(|s| (s.bytes.len() as u64, s.sha256)))
    }

    async fn copy(&self, from: &str, to: &str, len: u64, sha256: [u8; 32]) -> PResult<()> {
        self.injected()?;
        let source = self.objects.borrow().get(from).cloned();
        let Some(source) = source else {
            return Err(crate::PlatformError::new(
                crate::PlatformErrorKind::NotFound,
                BINDING,
                "copy source missing",
            ));
        };
        if source.bytes.len() as u64 != len {
            return Err(r2_error(BINDING, R2_INCOMPLETE_BODY, "IncompleteBody"));
        }
        if digest(&source.bytes) != sha256 {
            return Err(r2_error(BINDING, R2_BAD_DIGEST, "BadDigest"));
        }
        self.store(to, source.bytes, &source.meta, Some(sha256));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::block_on;
    use crate::r2::R2_TOO_MANY_REQUESTS;

    #[test]
    fn uploads_check_length_and_digest() {
        let r2 = FakeBlobs::new();
        let body = b"hello upload".to_vec();
        let sha = digest(&body);
        let meta = BlobMeta::default();
        let short = block_on(r2.put_stream(
            "u/1",
            BodyStream::from_bytes(body[..5].to_vec()),
            12,
            sha,
            &meta,
        ));
        assert_eq!(short.unwrap_err().r2_code(), Some(R2_INCOMPLETE_BODY));
        let wrong = block_on(r2.put_stream(
            "u/1",
            BodyStream::from_bytes(body.clone()),
            12,
            [0; 32],
            &meta,
        ));
        assert_eq!(wrong.unwrap_err().r2_code(), Some(R2_BAD_DIGEST));
        assert!(r2.keys().is_empty(), "nothing stored on a refused write");
        block_on(r2.put_stream("u/1", BodyStream::from_bytes(body.clone()), 12, sha, &meta))
            .unwrap();
        assert_eq!(
            block_on(r2.head_checksum("u/1")).unwrap(),
            Some((12, Some(sha)))
        );
        block_on(r2.copy("u/1", "t/1", 12, sha)).unwrap();
        assert_eq!(r2.bytes("t/1").unwrap(), body);
        assert_eq!(
            block_on(r2.copy("u/1", "t/2", 12, [1; 32]))
                .unwrap_err()
                .r2_code(),
            Some(R2_BAD_DIGEST)
        );
        r2.fail_next(R2_TOO_MANY_REQUESTS);
        let busy = block_on(r2.put("k", BlobBody::Bytes(vec![1]), &meta)).unwrap_err();
        assert_eq!(busy.r2_code(), Some(R2_TOO_MANY_REQUESTS));
        assert_eq!(busy.kind, crate::PlatformErrorKind::Unavailable);
    }

    #[test]
    fn list_pages_and_ranges() {
        let r2 = FakeBlobs::new();
        for i in 0..5 {
            block_on(r2.put(
                &format!("t/{i}"),
                BlobBody::Bytes(vec![i; 3]),
                &BlobMeta::default(),
            ))
            .unwrap();
        }
        block_on(r2.put("u/x", BlobBody::Bytes(vec![]), &BlobMeta::default())).unwrap();
        let page = block_on(r2.list("t/", None, 2)).unwrap();
        assert_eq!(page.keys, [("t/0".to_owned(), 3), ("t/1".to_owned(), 3)]);
        let rest = block_on(r2.list("t/", page.cursor.as_deref(), 10)).unwrap();
        assert_eq!(rest.keys.len(), 3);
        assert_eq!(rest.cursor, None);
        assert_eq!(
            block_on(r2.get_range("t/1", 1, 10)).unwrap(),
            Some(vec![1, 1])
        );
        block_on(r2.delete(&["t/0".to_owned(), "t/9".to_owned()])).unwrap();
        assert_eq!(block_on(r2.head("t/0")).unwrap(), None);
    }
}
