//! Runtime APIs that `workers-rs` 0.8.7 does not wrap ([Rust workspace §7]). S1 adds `transactionSync`;
//! S6 adds Vectorize, `AI.run` options and `AI.toMarkdown`.
//!
//! [Rust workspace §7]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// `ctx.storage`, seen through the methods `worker` lacks.
    #[wasm_bindgen(extends = js_sys::Object)]
    pub type DurableObjectStorageExt;

    /// Runs `closure` synchronously in one SQLite transaction; a throw rolls it back and is re-thrown.
    #[wasm_bindgen(method, catch, js_name = transactionSync)]
    pub fn transaction_sync(
        this: &DurableObjectStorageExt,
        closure: &js_sys::Function,
    ) -> Result<JsValue, JsValue>;
}

/// Runs `body` inside `ctx.storage.transactionSync`. An `Err` from `body` is thrown inside the
/// callback, so the runtime rolls the transaction back, and comes back here as the same `Err`.
pub fn transaction_sync<T>(
    storage: &worker::Storage,
    body: impl FnOnce() -> Result<T, String> + 'static,
) -> Result<T, String>
where
    T: 'static,
{
    let result = std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot = result.clone();
    let mut body = Some(body);
    let callback = Closure::<dyn FnMut() -> Result<JsValue, JsValue>>::new(move || {
        let body = body
            .take()
            .ok_or_else(|| JsValue::from_str("called twice"))?;
        match body() {
            Ok(value) => {
                *slot.borrow_mut() = Some(value);
                Ok(JsValue::UNDEFINED)
            }
            Err(e) => Err(js_sys::Error::new(&e).into()),
        }
    });
    let ext: &DurableObjectStorageExt = storage.as_raw().unchecked_ref();
    match ext.transaction_sync(callback.as_ref().unchecked_ref()) {
        Ok(_) => result
            .borrow_mut()
            .take()
            .ok_or_else(|| "transactionSync returned without running the callback".to_owned()),
        Err(e) => Err(js_sys::Error::from(e)
            .message()
            .as_string()
            .unwrap_or_else(|| "transaction failed".to_owned())),
    }
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

/// A binding from the `Env` JS object, unchecked: the runtime's binding objects have no constructor name
/// to check against.
pub fn binding<T: wasm_bindgen::JsCast>(env: &worker::Env, name: &str) -> Result<T, String> {
    let value =
        js_sys::Reflect::get(env, &name.into()).map_err(|_| format!("no binding {name}"))?;
    if value.is_undefined() {
        return Err(format!("binding {name} is undefined"));
    }
    Ok(value.unchecked_into())
}
