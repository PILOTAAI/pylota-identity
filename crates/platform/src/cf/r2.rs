//! R2 through the binding object itself: `worker` 0.8.7's `Bucket` copies every body into wasm memory
//! and cannot take a `Uint8Array` or a `FixedLengthStream`. R2 errors carry their code in the message
//! (`… (10037)`), which becomes `r2 {code}: …`.

use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;

use super::js;
use crate::buffer::{BodyStream, JsBuffer};
use crate::r2::{
    Blob, BlobBody, BlobMeta, BlobPage, BlobStore, check_delete, check_list, check_meta, r2_error,
};
use crate::{PResult, PlatformError, PlatformErrorKind};

#[derive(Clone)]
pub struct CfBlobs {
    pub(crate) env: worker::Env,
    /// `BLOBS` or `BACKUP`.
    pub(crate) binding: &'static str,
}

/// The five-digit R2 code in a thrown error's message.
fn r2_code(message: &str) -> Option<u32> {
    message
        .split(|c: char| !c.is_ascii_digit())
        .filter(|w| w.len() == 5 && w.starts_with('1'))
        .find_map(|w| w.parse().ok())
}

impl CfBlobs {
    fn bucket(&self) -> PResult<JsValue> {
        js::binding(&self.env, self.binding)
    }

    fn err(&self, e: &JsValue) -> PlatformError {
        let message = js::message(e);
        match r2_code(&message) {
            Some(code) => r2_error(
                self.binding,
                code,
                &message.chars().take(200).collect::<String>(),
            ),
            None => js::error(self.binding, e),
        }
    }

    async fn call(&self, method: &str, args: &[&JsValue]) -> PResult<JsValue> {
        let bucket = self.bucket()?;
        js::call_async(&bucket, method, args)
            .await
            .map_err(|e| self.err(&e))
    }

    fn options(&self, meta: &BlobMeta, sha256: Option<&[u8; 32]>) -> PResult<JsValue> {
        let o = js::object();
        let wrap = |r: Result<(), JsValue>| r.map_err(|e| js::error(self.binding, &e));
        if let Some(ct) = &meta.content_type {
            let http = js::object();
            wrap(js::set(&http, "contentType", &JsValue::from_str(ct)))?;
            wrap(js::set(&o, "httpMetadata", &http))?;
        }
        if !meta.custom.is_empty() {
            let custom = js::object();
            for (k, v) in &meta.custom {
                wrap(js::set(&custom, k, &JsValue::from_str(v)))?;
            }
            wrap(js::set(&o, "customMetadata", &custom))?;
        }
        if let Some(sha) = sha256 {
            wrap(js::set(
                &o,
                "sha256",
                &js_sys::Uint8Array::from(&sha[..]).buffer().into(),
            ))?;
        }
        Ok(o)
    }

    /// `stream` piped through a `FixedLengthStream` of `len`, so R2 knows the length and refuses a short
    /// body (`10013`).
    fn fixed_length(&self, stream: &JsValue, len: u64) -> PResult<JsValue> {
        let fixed = js::construct("FixedLengthStream", &[&JsValue::from_f64(len as f64)])
            .map_err(|e| js::error(self.binding, &e))?;
        let writable = js::get(&fixed, "writable").map_err(|e| js::error(self.binding, &e))?;
        // The pipe's own failure surfaces as the put's; its promise is not awaited separately.
        let piped =
            js::call(stream, "pipeTo", &[&writable]).map_err(|e| js::error(self.binding, &e))?;
        let ignore: JsValue =
            wasm_bindgen::closure::Closure::<dyn FnMut(JsValue)>::once_into_js(|_: JsValue| {});
        let _ = js::call(&piped, "catch", &[&ignore]);
        js::get(&fixed, "readable").map_err(|e| js::error(self.binding, &e))
    }

    fn custom(object: &JsValue) -> Vec<(String, String)> {
        let Ok(custom) = js::get(object, "customMetadata") else {
            return Vec::new();
        };
        let Ok(entries) =
            js_sys::Object::entries(custom.unchecked_ref()).dyn_into::<js_sys::Array>()
        else {
            return Vec::new();
        };
        entries
            .iter()
            .filter_map(|pair| {
                let pair: js_sys::Array = pair.dyn_into().ok()?;
                Some((pair.get(0).as_string()?, pair.get(1).as_string()?))
            })
            .collect()
    }
}

fn size(object: &JsValue) -> u64 {
    js::get(object, "size")
        .ok()
        .and_then(|s| s.as_f64())
        .unwrap_or(0.0) as u64
}

impl BlobStore for CfBlobs {
    async fn put(&self, key: &str, body: BlobBody, meta: &BlobMeta) -> PResult<()> {
        check_meta(self.binding, meta)?;
        #[cfg(feature = "itest-hooks")]
        if super::itest::take_fault("r2.put", self.binding)
            .await
            .is_some()
        {
            return Err(super::itest::fault_error(self.binding));
        }
        let value: JsValue = match &body {
            BlobBody::Bytes(b) => js_sys::Uint8Array::from(b.as_slice()).into(),
            BlobBody::Js(b) => b.array.clone().into(),
        };
        let options = self.options(meta, None)?;
        self.call("put", &[&JsValue::from_str(key), &value, &options])
            .await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> PResult<Option<Blob>> {
        let object = self.call("get", &[&JsValue::from_str(key)]).await?;
        if object.is_null() || object.is_undefined() {
            return Ok(None);
        }
        let buffer = js::call_async(&object, "arrayBuffer", &[])
            .await
            .map_err(|e| self.err(&e))?;
        Ok(Some(Blob {
            size: size(&object),
            custom: Self::custom(&object),
            body: BlobBody::Js(JsBuffer {
                array: js_sys::Uint8Array::new(&buffer),
            }),
        }))
    }

    async fn get_range(&self, key: &str, offset: u64, len: u64) -> PResult<Option<Vec<u8>>> {
        let range = js::object();
        let options = js::object();
        let wrap = |r: Result<(), JsValue>| r.map_err(|e| js::error(self.binding, &e));
        wrap(js::set(&range, "offset", &JsValue::from_f64(offset as f64)))?;
        wrap(js::set(&range, "length", &JsValue::from_f64(len as f64)))?;
        wrap(js::set(&options, "range", &range))?;
        let object = self
            .call("get", &[&JsValue::from_str(key), &options])
            .await?;
        if object.is_null() || object.is_undefined() {
            return Ok(None);
        }
        let buffer = js::call_async(&object, "arrayBuffer", &[])
            .await
            .map_err(|e| self.err(&e))?;
        Ok(Some(js_sys::Uint8Array::new(&buffer).to_vec()))
    }

    async fn head(&self, key: &str) -> PResult<Option<u64>> {
        let object = self.call("head", &[&JsValue::from_str(key)]).await?;
        Ok((!object.is_null() && !object.is_undefined()).then(|| size(&object)))
    }

    async fn delete(&self, keys: &[String]) -> PResult<()> {
        check_delete(self.binding, keys)?;
        if keys.is_empty() {
            return Ok(());
        }
        let array: js_sys::Array = keys.iter().map(|k| JsValue::from_str(k)).collect();
        self.call("delete", &[&array.into()]).await?;
        Ok(())
    }

    async fn list(&self, prefix: &str, cursor: Option<&str>, limit: u32) -> PResult<BlobPage> {
        check_list(self.binding, limit)?;
        let options = js::object();
        let wrap = |r: Result<(), JsValue>| r.map_err(|e| js::error(self.binding, &e));
        wrap(js::set(&options, "prefix", &JsValue::from_str(prefix)))?;
        wrap(js::set(&options, "limit", &JsValue::from(limit)))?;
        if let Some(c) = cursor {
            wrap(js::set(&options, "cursor", &JsValue::from_str(c)))?;
        }
        let page = self.call("list", &[&options]).await?;
        let objects: js_sys::Array = js::get(&page, "objects")
            .ok()
            .and_then(|o| o.dyn_into().ok())
            .unwrap_or_default();
        let keys = objects
            .iter()
            .filter_map(|o| Some((js::get(&o, "key").ok()?.as_string()?, size(&o))))
            .collect();
        let truncated = js::get(&page, "truncated").ok().and_then(|t| t.as_bool()) == Some(true);
        Ok(BlobPage {
            keys,
            cursor: if truncated {
                js::get(&page, "cursor").ok().and_then(|c| c.as_string())
            } else {
                None
            },
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
        check_meta(self.binding, meta)?;
        let readable = self.fixed_length(&body.stream.into(), len)?;
        let options = self.options(meta, Some(&sha256))?;
        self.call("put", &[&JsValue::from_str(key), &readable, &options])
            .await?;
        Ok(())
    }

    async fn head_checksum(&self, key: &str) -> PResult<Option<(u64, Option<[u8; 32]>)>> {
        let object = self.call("head", &[&JsValue::from_str(key)]).await?;
        if object.is_null() || object.is_undefined() {
            return Ok(None);
        }
        let sha = js::get(&object, "checksums")
            .and_then(|c| js::get(&c, "sha256"))
            .ok()
            .filter(|s| !s.is_undefined() && !s.is_null())
            .and_then(|s| <[u8; 32]>::try_from(js_sys::Uint8Array::new(&s).to_vec()).ok());
        Ok(Some((size(&object), sha)))
    }

    async fn copy(&self, from: &str, to: &str, len: u64, sha256: [u8; 32]) -> PResult<()> {
        // The binding has no server-side copy: the source's body streams into a new put, which R2
        // checks against the length and digest again.
        let object = self.call("get", &[&JsValue::from_str(from)]).await?;
        if object.is_null() || object.is_undefined() {
            return Err(PlatformError::new(
                PlatformErrorKind::NotFound,
                self.binding,
                "copy source missing",
            ));
        }
        let body = js::get(&object, "body").map_err(|e| js::error(self.binding, &e))?;
        let readable = self.fixed_length(&body, len)?;
        let content_type = js::get(&object, "httpMetadata")
            .and_then(|h| js::get(&h, "contentType"))
            .ok()
            .and_then(|c| c.as_string());
        let meta = BlobMeta {
            content_type,
            custom: Self::custom(&object),
        };
        let options = self.options(&meta, Some(&sha256))?;
        self.call("put", &[&JsValue::from_str(to), &readable, &options])
            .await?;
        Ok(())
    }
}
