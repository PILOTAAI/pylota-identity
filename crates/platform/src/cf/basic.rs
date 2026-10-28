//! Time, randomness, IDs, rate limits, Analytics Engine and `waitUntil`.

use std::cell::Cell;

use wasm_bindgen::JsValue;

use super::js;
use crate::background::Background;
use crate::ids::{IdPrefix, Ids, Monotonic, random80};
use crate::metrics::{DataPoint, Metrics};
use crate::ratelimit::{RateLimiter, RlBucket};
use crate::{Clock, PResult, PlatformError, Rng};

/// `Date.now()`.
#[derive(Clone, Copy, Default)]
pub struct CfClock;

impl Clock for CfClock {
    fn now_ms(&self) -> i64 {
        #[cfg(feature = "itest-hooks")]
        return js_sys::Date::now() as i64 + super::itest::offset();
        #[cfg(not(feature = "itest-hooks"))]
        return js_sys::Date::now() as i64;
    }
}

/// `crypto.getRandomValues` through `getrandom` (`wasm_js`).
#[derive(Clone, Copy, Default)]
pub struct CfRng;

impl Rng for CfRng {
    fn fill(&self, buf: &mut [u8]) {
        // getRandomValues fails only on a buffer over 65,536 bytes; getrandom splits it.
        getrandom::fill(buf).expect("crypto.getRandomValues");
    }
}

thread_local! {
    /// One generator per isolate (the Worker is single-threaded).
    static MONOTONIC: Monotonic = const { Monotonic::new() };
}

#[derive(Clone, Copy, Default)]
pub struct CfIds;

impl Ids for CfIds {
    fn new_id(&self, prefix: IdPrefix) -> String {
        let random = random80(&CfRng);
        MONOTONIC.with(|m| m.id(prefix, CfClock.now_ms(), random))
    }
}

/// The `RL_*` bindings: `limit({ key })` resolves to `{ success }`.
#[derive(Clone)]
pub struct CfLimits {
    pub(crate) env: worker::Env,
}

impl RateLimiter for CfLimits {
    async fn allow(&self, bucket: RlBucket, key: &str) -> PResult<bool> {
        let name = bucket.binding();
        let rl = js::binding(&self.env, name)?;
        let arg = js::object();
        js::set(&arg, "key", &JsValue::from_str(key)).map_err(|e| js::error(name, &e))?;
        let out = js::call_async(&rl, "limit", &[&arg])
            .await
            .map_err(|e| js::error(name, &e))?;
        js::get(&out, "success")
            .map_err(|e| js::error(name, &e))?
            .as_bool()
            .ok_or_else(|| {
                PlatformError::new(crate::PlatformErrorKind::Corrupt, name, "no success field")
            })
    }
}

thread_local! {
    static DROPPED: Cell<u64> = const { Cell::new(0) };
}

/// Data points refused by `METRICS` in this isolate (`metrics_dropped_total`).
pub fn metrics_dropped() -> u64 {
    DROPPED.with(Cell::get)
}

#[derive(Clone)]
pub struct CfMetrics {
    pub(crate) env: worker::Env,
}

impl Metrics for CfMetrics {
    fn write(&self, point: DataPoint) {
        let written = (|| -> Result<(), JsValue> {
            let ds = js::get(&self.env, "METRICS")?;
            if ds.is_undefined() {
                return Err(JsValue::from_str("METRICS is not bound"));
            }
            let p = js::object();
            let strings = |v: &[String]| {
                v.iter()
                    .map(|s| JsValue::from_str(s))
                    .collect::<js_sys::Array>()
            };
            js::set(&p, "indexes", &strings(&point.indexes).into())?;
            js::set(&p, "blobs", &strings(&point.blobs).into())?;
            let doubles: js_sys::Array = point
                .doubles
                .iter()
                .map(|d| JsValue::from_f64(*d))
                .collect();
            js::set(&p, "doubles", &doubles.into())?;
            js::call(&ds, "writeDataPoint", &[&p]).map(|_| ())
        })();
        if written.is_err() {
            DROPPED.with(|d| d.set(d.get() + 1));
        }
    }
}

/// `ctx.waitUntil` of the invocation, or of the Durable Object's state.
pub enum CfBackground {
    Worker(worker::Context),
    Object(worker::worker_sys::DurableObjectState),
}

impl Background for CfBackground {
    fn wait_until(&self, fut: impl core::future::Future<Output = ()> + 'static) {
        match self {
            CfBackground::Worker(ctx) => ctx.wait_until(fut),
            CfBackground::Object(raw) => worker::State::from(dup_state(raw)).wait_until(fut),
        }
    }
}

/// A second handle to the same `DurableObjectState` (the extern type is not `Clone`).
pub(crate) fn dup_state(
    raw: &worker::worker_sys::DurableObjectState,
) -> worker::worker_sys::DurableObjectState {
    use wasm_bindgen::JsCast as _;
    let value: &JsValue = raw.as_ref();
    value.clone().unchecked_into()
}
