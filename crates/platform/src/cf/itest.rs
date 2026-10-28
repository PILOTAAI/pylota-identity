//! `itest-hooks` only: the fake clock and the fault plan of the test process's fake server (Testing
//! § 6.5–6.6). Each invocation reads `GET {fakes}/state` first; a fault is taken with
//! `POST {fakes}/faults/take`, so the server keeps the counts across isolates and objects.

use std::cell::Cell;

use serde_json::{Value, json};

use super::net::CfHttp;
use crate::config::Config;
use crate::http::{HttpClient as _, HttpRequest};

thread_local! {
    static OFFSET: Cell<i64> = const { Cell::new(0) };
    static FAKES: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// Whether the fake server holds any armed fault (from the last sync), so an unarmed run never
    /// asks it on each call.
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

/// The fake clock's offset from `Date.now()`.
pub fn offset() -> i64 {
    OFFSET.with(Cell::get)
}

/// Reads the fake server's state into the isolate (no-op without `PM_ITEST_FAKES_URL`).
pub async fn sync(cfg: &Config) {
    let Some(url) = &cfg.itest_fakes_url else {
        return;
    };
    FAKES.with(|f| *f.borrow_mut() = Some(url.clone()));
    let mut req = HttpRequest::get(format!("{url}/state"));
    req.timeout_ms = 2_000;
    if let Ok(r) = CfHttp.send(req).await
        && let Ok(v) = serde_json::from_slice::<Value>(&r.body)
    {
        OFFSET.with(|o| o.set(v["clock_offset_ms"].as_i64().unwrap_or(0)));
        ARMED.with(|a| a.set(v["faults_armed"].as_bool().unwrap_or(true)));
    }
}

/// Whether an armed fault matches `target` and `binding` (and takes one of its count).
pub async fn take_fault(target: &str, binding: &str) -> Option<String> {
    if !ARMED.with(Cell::get) {
        return None;
    }
    let url = FAKES.with(|f| f.borrow().clone())?;
    let mut req = HttpRequest::get(format!("{url}/faults/take"));
    req.method = "POST";
    req.timeout_ms = 2_000;
    req.headers
        .push(("content-type".into(), "application/json".into()));
    req.body = Some(
        json!({ "target": target, "binding": binding })
            .to_string()
            .into_bytes(),
    );
    let r = CfHttp.send(req).await.ok()?;
    let v: Value = serde_json::from_slice(&r.body).ok()?;
    v["mode"].as_str().map(str::to_owned)
}

/// The `PlatformError` an armed fault answers with.
pub fn fault_error(binding: &'static str) -> crate::PlatformError {
    crate::PlatformError::unavailable(binding, "fault injected by the test")
}
