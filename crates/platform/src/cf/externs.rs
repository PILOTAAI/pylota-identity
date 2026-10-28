//! Runtime APIs that `workers-rs` 0.8.7 does not wrap ([Rust workspace §7]): `transactionSync`, the
//! Vectorize binding, `AI.run` options and `AI.toMarkdown`. S1 and S6 ran each against the runtime.
//!
//! [Rust workspace §7]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// `ctx.storage`, seen through the methods `worker` lacks.
    #[wasm_bindgen(extends = js_sys::Object)]
    pub type DurableObjectStorageExt;

    /// Runs `closure` synchronously in one SQLite transaction; a throw rolls it back and is re-thrown.
    /// The closure is a stack closure, valid only for this call, which is all `transactionSync` needs:
    /// the runtime calls it before returning.
    #[wasm_bindgen(method, catch, js_name = transactionSync)]
    pub fn transaction_sync(
        this: &DurableObjectStorageExt,
        closure: &mut dyn FnMut() -> Result<JsValue, JsValue>,
    ) -> Result<JsValue, JsValue>;
}

#[wasm_bindgen]
extern "C" {
    /// A Vectorize V2 index binding (`env.VECTORS`).
    #[wasm_bindgen(extends = js_sys::Object)]
    pub type VectorizeIndex;
    #[wasm_bindgen(method, catch)]
    pub fn upsert(
        this: &VectorizeIndex,
        vectors: &js_sys::Array,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    pub fn query(
        this: &VectorizeIndex,
        vector: &js_sys::Float32Array,
        options: &JsValue,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method, catch, js_name = queryById)]
    pub fn query_by_id(
        this: &VectorizeIndex,
        id: &str,
        options: &JsValue,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method, catch, js_name = deleteByIds)]
    pub fn delete_by_ids(
        this: &VectorizeIndex,
        ids: &js_sys::Array,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getByIds)]
    pub fn get_by_ids(
        this: &VectorizeIndex,
        ids: &js_sys::Array,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    pub fn describe(this: &VectorizeIndex) -> Result<js_sys::Promise, JsValue>;

    /// The Workers AI binding (`env.AI`), with the options and `toMarkdown` that `worker` 0.8.7 lacks.
    #[wasm_bindgen(extends = js_sys::Object)]
    pub type AiBinding;
    #[wasm_bindgen(method, catch, js_name = run)]
    pub fn run_with_options(
        this: &AiBinding,
        model: &str,
        inputs: &JsValue,
        options: &JsValue,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method, catch, js_name = toMarkdown)]
    pub fn to_markdown(
        this: &AiBinding,
        files: &js_sys::Array,
        options: &JsValue,
    ) -> Result<js_sys::Promise, JsValue>;
}
