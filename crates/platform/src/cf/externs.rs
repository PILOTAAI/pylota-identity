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
