//! Small helpers over `js_sys` for the bindings `worker` 0.8.7 wraps differently from what the traits
//! need (R2 with checksums and JS buffers, queues with plain-object bodies, Vectorize, AI options):
//! method calls by name, promises with a deadline, and JSON both ways.

use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::{PResult, PlatformError, PlatformErrorKind};

/// `target[name]`.
pub fn get(target: &JsValue, name: &str) -> Result<JsValue, JsValue> {
    js_sys::Reflect::get(target, &JsValue::from_str(name))
}

pub fn set(target: &JsValue, name: &str, value: &JsValue) -> Result<(), JsValue> {
    js_sys::Reflect::set(target, &JsValue::from_str(name), value).map(|_| ())
}

/// `target[name](...args)`.
pub fn call(target: &JsValue, name: &str, args: &[&JsValue]) -> Result<JsValue, JsValue> {
    let f: js_sys::Function = get(target, name)?.dyn_into()?;
    let array = js_sys::Array::new();
    for a in args {
        array.push(a);
    }
    f.apply(target, &array)
}

/// Awaits `value` if it is a promise.
pub async fn settle(value: JsValue) -> Result<JsValue, JsValue> {
    match value.dyn_into::<js_sys::Promise>() {
        Ok(p) => JsFuture::from(p).await,
        Err(v) => Ok(v),
    }
}

/// `target[name](...args)`, awaited.
pub async fn call_async(
    target: &JsValue,
    name: &str,
    args: &[&JsValue],
) -> Result<JsValue, JsValue> {
    settle(call(target, name, args)?).await
}

const TIMEOUT_MARK: &str = "__pylota_timeout__";

/// Races `promise` against a timer of `ms`; `Ok(None)` when the timer wins. The losing promise is
/// abandoned, not cancelled.
pub async fn with_deadline(promise: js_sys::Promise, ms: u32) -> Result<Option<JsValue>, JsValue> {
    let timer = js_sys::Promise::new(&mut |resolve, _| {
        if let Ok(f) =
            get(&js_sys::global(), "setTimeout").and_then(|f| f.dyn_into::<js_sys::Function>())
        {
            let _ = f.call3(
                &JsValue::UNDEFINED,
                &resolve,
                &JsValue::from(ms),
                &JsValue::from_str(TIMEOUT_MARK),
            );
        }
    });
    let raced = js_sys::Promise::race(&js_sys::Array::of2(&promise, &timer));
    let value = JsFuture::from(raced).await?;
    Ok((value.as_string().as_deref() != Some(TIMEOUT_MARK)).then_some(value))
}

/// A plain JS object or array from JSON text (`JSON.parse`), never a `Map`.
pub fn from_json(value: &serde_json::Value) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(&value.to_string())
}

pub fn from_serialize<T: serde::Serialize>(binding: &'static str, value: &T) -> PResult<JsValue> {
    let text = serde_json::to_string(value)
        .map_err(|e| PlatformError::invalid(binding, format!("serialise: {e}")))?;
    js_sys::JSON::parse(&text).map_err(|e| error(binding, &e))
}

/// A JS value as `T`, through `JSON.stringify`.
pub fn to_value<T: serde::de::DeserializeOwned>(
    binding: &'static str,
    value: &JsValue,
) -> PResult<T> {
    let text = if value.is_undefined() {
        "null".to_owned()
    } else {
        js_sys::JSON::stringify(value)
            .map_err(|e| error(binding, &e))?
            .as_string()
            .unwrap_or_default()
    };
    serde_json::from_str(&text).map_err(|e| {
        PlatformError::new(PlatformErrorKind::Corrupt, binding, format!("result: {e}"))
    })
}

/// The `message` of a thrown value, else its string form.
pub fn message(e: &JsValue) -> String {
    get(e, "message")
        .ok()
        .and_then(|m| m.as_string())
        .or_else(|| e.as_string())
        .unwrap_or_else(|| "unknown error".to_owned())
}

/// The `code` property of a thrown value, as text.
pub fn code(e: &JsValue) -> Option<String> {
    let c = get(e, "code").ok()?;
    c.as_string().or_else(|| c.as_f64().map(|n| format!("{n}")))
}

/// A thrown value as a platform error: `Unavailable`, with the message cut to 300 characters (it is
/// the runtime's text, never content).
pub fn error(binding: &'static str, e: &JsValue) -> PlatformError {
    PlatformError::unavailable(binding, message(e).chars().take(300).collect::<String>())
}

pub fn worker_error(binding: &'static str, e: worker::Error) -> PlatformError {
    PlatformError::unavailable(binding, e.to_string().chars().take(300).collect::<String>())
}

/// A binding from the `Env` object; missing is `Unavailable`.
pub fn binding(env: &worker::Env, name: &'static str) -> PResult<JsValue> {
    let value = get(env, name).map_err(|e| error(name, &e))?;
    if value.is_undefined() || value.is_null() {
        return Err(PlatformError::unavailable(
            name,
            "binding is not configured",
        ));
    }
    Ok(value)
}

/// `new globalThis[name](...args)`.
pub fn construct(name: &str, args: &[&JsValue]) -> Result<JsValue, JsValue> {
    let ctor: js_sys::Function = get(&js_sys::global(), name)?.dyn_into()?;
    let array = js_sys::Array::new();
    for a in args {
        array.push(a);
    }
    js_sys::Reflect::construct(&ctor, &array)
}

pub fn object() -> JsValue {
    js_sys::Object::new().into()
}
