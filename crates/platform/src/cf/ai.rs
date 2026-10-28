//! Workers AI and Vectorize through `cf::externs` (S6: every call, the `gateway` option and the
//! result shapes).

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::externs::{AiBinding, VectorizeIndex};
use super::js;
use crate::ai::{Ai, AiOptions, MarkdownInput, MarkdownOutput};
use crate::vectorize::{
    IndexInfo, VectorIndex, VectorMatch, VectorRecord, check_upsert, query_options, read_info,
    read_matches, read_mutation,
};
use crate::{PResult, PlatformError};

#[derive(Clone)]
pub struct CfAi {
    pub(crate) env: worker::Env,
}

const AI: &str = "AI";

async fn wait(binding: &'static str, p: Result<js_sys::Promise, JsValue>) -> PResult<JsValue> {
    let p = p.map_err(|e| js::error(binding, &e))?;
    JsFuture::from(p).await.map_err(|e| js::error(binding, &e))
}

impl CfAi {
    fn binding(&self) -> PResult<AiBinding> {
        Ok(js::binding(&self.env, AI)?.unchecked_into())
    }

    async fn raw<I: Serialize>(
        &self,
        model: &str,
        input: &I,
        opts: &AiOptions,
    ) -> PResult<JsValue> {
        let ai = self.binding()?;
        let input = js::from_serialize(AI, input)?;
        let options = js::from_json(&opts.to_json()).map_err(|e| js::error(AI, &e))?;
        wait(AI, ai.run_with_options(model, &input, &options)).await
    }
}

impl Ai for CfAi {
    async fn run<I: Serialize, O: DeserializeOwned>(
        &self,
        model: &str,
        input: &I,
        opts: &AiOptions,
    ) -> PResult<O> {
        #[cfg(feature = "itest-hooks")]
        if let Some(r) = super::itest::fake_call(
            AI,
            "/__ai/run",
            serde_json::json!({ "model": model, "input": serde_json::to_value(input).unwrap_or(Value::Null) }),
        )
        .await
        {
            return serde_json::from_value(r?).map_err(|e| PlatformError::invalid(AI, e.to_string()));
        }
        let out = self.raw(model, input, opts).await?;
        js::to_value(AI, &out)
    }

    async fn run_bytes<I: Serialize>(
        &self,
        model: &str,
        input: &I,
        opts: &AiOptions,
    ) -> PResult<Vec<u8>> {
        let out = self.raw(model, input, opts).await?;
        if let Some(bytes) = out.dyn_ref::<js_sys::Uint8Array>() {
            return Ok(bytes.to_vec());
        }
        // A stream (audio, images): read it whole through a Response.
        let response = js::construct("Response", &[&out]).map_err(|e| js::error(AI, &e))?;
        let buffer = js::call_async(&response, "arrayBuffer", &[])
            .await
            .map_err(|e| js::error(AI, &e))?;
        Ok(js_sys::Uint8Array::new(&buffer).to_vec())
    }

    async fn to_markdown(&self, docs: &[MarkdownInput]) -> PResult<Vec<MarkdownOutput>> {
        let ai = self.binding()?;
        let files = js_sys::Array::new();
        for d in docs {
            let options = js::object();
            js::set(&options, "type", &JsValue::from_str(&d.mime_type))
                .map_err(|e| js::error(AI, &e))?;
            let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(d.bytes.as_slice()));
            let blob =
                js::construct("Blob", &[&parts.into(), &options]).map_err(|e| js::error(AI, &e))?;
            let file = js::object();
            js::set(&file, "name", &JsValue::from_str(&d.name)).map_err(|e| js::error(AI, &e))?;
            js::set(&file, "blob", &blob).map_err(|e| js::error(AI, &e))?;
            files.push(&file);
        }
        // PDFs without the metadata section (Inbound › Attachment text extraction).
        let options = js::object();
        let pdf = js::object();
        js::set(&pdf, "metadata", &JsValue::FALSE).map_err(|e| js::error(AI, &e))?;
        let conversion = js::object();
        js::set(&conversion, "pdf", &pdf).map_err(|e| js::error(AI, &e))?;
        js::set(&options, "conversionOptions", &conversion).map_err(|e| js::error(AI, &e))?;
        let out = wait(AI, ai.to_markdown(&files, &options)).await?;
        js::to_value(AI, &out)
    }
}

/// `VECTORS` or `VECTORS_NEXT`.
#[derive(Clone)]
pub struct CfVectors {
    pub(crate) env: worker::Env,
    pub(crate) binding: &'static str,
}

impl CfVectors {
    fn index(&self) -> PResult<VectorizeIndex> {
        Ok(js::binding(&self.env, self.binding)?.unchecked_into())
    }

    fn ids(ids: &[String]) -> js_sys::Array {
        ids.iter().map(|i| JsValue::from_str(i)).collect()
    }

    fn json(&self, v: &JsValue) -> PResult<Value> {
        js::to_value(self.binding, v)
    }

    #[cfg(feature = "itest-hooks")]
    async fn fake(&self, op: &str, body: Value) -> Option<PResult<Value>> {
        super::itest::fake_call(self.binding, &format!("/__vec/{}/{op}", self.binding), body).await
    }
}

impl VectorIndex for CfVectors {
    async fn upsert(&self, vectors: &[VectorRecord]) -> PResult<String> {
        check_upsert(self.binding, vectors)?;
        #[cfg(feature = "itest-hooks")]
        if let Some(r) = self
            .fake(
                "upsert",
                serde_json::json!({ "vectors": vectors.iter().map(|v| serde_json::json!({
                    "id": v.id, "namespace": v.namespace, "values": v.values, "metadata": v.metadata })).collect::<Vec<_>>() }),
            )
            .await
        {
            return read_mutation(self.binding, &r?);
        }
        let b = self.binding;
        let array = js_sys::Array::new();
        for v in vectors {
            let item = js::object();
            let set = |k: &str, val: &JsValue| js::set(&item, k, val).map_err(|e| js::error(b, &e));
            set("id", &JsValue::from_str(&v.id))?;
            set("namespace", &JsValue::from_str(&v.namespace))?;
            set(
                "values",
                &js_sys::Float32Array::from(v.values.as_slice()).into(),
            )?;
            let metadata =
                js::from_json(&Value::Object(v.metadata.clone())).map_err(|e| js::error(b, &e))?;
            set("metadata", &metadata)?;
            array.push(&item);
        }
        let out = wait(b, self.index()?.upsert(&array)).await?;
        read_mutation(b, &self.json(&out)?)
    }

    async fn query(
        &self,
        namespace: &str,
        vector: &[f32],
        top_k: u32,
        filter: &Value,
    ) -> PResult<Vec<VectorMatch>> {
        let b = self.binding;
        let options = query_options(b, namespace, top_k, filter)?;
        #[cfg(feature = "itest-hooks")]
        if let Some(r) = self
            .fake(
                "query",
                serde_json::json!({ "vector": vector, "options": options }),
            )
            .await
        {
            return read_matches(b, &r?);
        }
        let options = js::from_json(&options).map_err(|e| js::error(b, &e))?;
        let out = wait(
            b,
            self.index()?
                .query(&js_sys::Float32Array::from(vector), &options),
        )
        .await?;
        read_matches(b, &self.json(&out)?)
    }

    async fn delete_by_ids(&self, ids: &[String]) -> PResult<String> {
        #[cfg(feature = "itest-hooks")]
        if let Some(r) = self.fake("delete", serde_json::json!({ "ids": ids })).await {
            return read_mutation(self.binding, &r?);
        }
        let out = wait(self.binding, self.index()?.delete_by_ids(&Self::ids(ids))).await?;
        read_mutation(self.binding, &self.json(&out)?)
    }

    async fn get_by_ids(&self, ids: &[String]) -> PResult<Vec<String>> {
        #[cfg(feature = "itest-hooks")]
        if let Some(r) = self.fake("get", serde_json::json!({ "ids": ids })).await {
            return Ok(r?
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v["id"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default());
        }
        let out = wait(self.binding, self.index()?.get_by_ids(&Self::ids(ids))).await?;
        let found = self.json(&out)?;
        let list = found.as_array().ok_or_else(|| {
            PlatformError::new(
                crate::PlatformErrorKind::Corrupt,
                self.binding,
                "getByIds: not an array",
            )
        })?;
        Ok(list
            .iter()
            .filter_map(|v| v["id"].as_str().map(str::to_owned))
            .collect())
    }

    async fn describe(&self) -> PResult<IndexInfo> {
        #[cfg(feature = "itest-hooks")]
        if let Some(r) = self.fake("describe", serde_json::json!({})).await {
            return read_info(self.binding, &r?);
        }
        let out = wait(self.binding, self.index()?.describe()).await?;
        read_info(self.binding, &self.json(&out)?)
    }
}
