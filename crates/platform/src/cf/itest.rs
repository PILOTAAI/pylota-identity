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
    /// The fake server's configuration overrides (a JSON object), when it has any.
    static OVERRIDES: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// The D1 size every statement reports instead of the real one (J30), when set.
    static D1_SIZE: Cell<Option<u64>> = const { Cell::new(None) };
    /// The configuration last read with an override set.
    static OVERRIDDEN: std::cell::RefCell<Option<(String, std::rc::Rc<Config>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The D1 size the fake server set, if any.
pub fn d1_size() -> Option<u64> {
    D1_SIZE.with(Cell::get)
}

/// The override set of the last sync, if any.
pub fn config_overrides() -> Option<String> {
    OVERRIDES.with(|o| o.borrow().clone())
}

pub fn cached_config(overrides: &str) -> Option<std::rc::Rc<Config>> {
    OVERRIDDEN.with(|o| {
        o.borrow()
            .as_ref()
            .filter(|(k, _)| k == overrides)
            .map(|(_, c)| c.clone())
    })
}

pub fn cache_config(overrides: &str, c: std::rc::Rc<Config>) {
    OVERRIDDEN.with(|o| *o.borrow_mut() = Some((overrides.to_owned(), c)));
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
        let overrides = v["config_overrides"]
            .as_object()
            .filter(|m| !m.is_empty())
            .map(|m| Value::Object(m.clone()).to_string());
        OVERRIDES.with(|o| *o.borrow_mut() = overrides);
        D1_SIZE.with(|d| d.set(v["d1_size_after"].as_u64()));
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

/// Testing § 6.3: every outbound request goes to the fake server instead, as
/// `{fakes}/__http/{host}{path}`; the server routes it by host (DoH, providers, receivers). Requests to
/// the fake server itself pass unchanged.
pub fn reroute(url: &str) -> String {
    let Some(fakes) = FAKES.with(|f| f.borrow().clone()) else {
        return url.to_owned();
    };
    if url.starts_with(&fakes) {
        return url.to_owned();
    }
    match url.split_once("://") {
        Some((_, rest)) => format!("{fakes}/__http/{rest}"),
        None => url.to_owned(),
    }
}

/// The `PlatformError` an armed fault answers with.
pub fn fault_error(binding: &'static str) -> crate::PlatformError {
    crate::PlatformError::unavailable(binding, "fault injected by the test")
}

/// Testing § 6.3: the Email Sending binding goes to the fake server, `POST {fakes}/__mail/send` with
/// the structured message; the server answers `{messageId}`, `{code, message}`, `{exception}` or
/// `{timeout: true}` from the test's script. `None` without a fake server.
pub async fn mail_send(
    msg: &crate::email::StructuredEmail,
) -> Option<Result<String, crate::email::SendError>> {
    use crate::email::SendError;
    let url = FAKES.with(|f| f.borrow().clone())?;
    let mut req = HttpRequest::get(format!("{url}/__mail/send"));
    req.method = "POST";
    req.timeout_ms = 5_000;
    req.headers
        .push(("content-type".into(), "application/json".into()));
    let attachments: Vec<Value> = msg
        .attachments
        .iter()
        .map(|a| json!({ "filename": a.filename, "content_type": a.content_type, "size": a.bytes.len(), "content_id": a.inline_content_id }))
        .collect();
    req.body = Some(
        json!({
            "from": msg.from.0, "from_name": msg.from.1, "to": msg.to, "cc": msg.cc, "bcc": msg.bcc,
            "reply_to": msg.reply_to, "subject": msg.subject, "text": msg.text, "html": msg.html,
            "headers": msg.headers, "attachments": attachments,
        })
        .to_string()
        .into_bytes(),
    );
    let r = match CfHttp.send(req).await {
        Ok(r) => r,
        Err(_) => return Some(Err(SendError::Timeout)),
    };
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    Some(if let Some(id) = v["messageId"].as_str() {
        Ok(id.to_owned())
    } else if let Some(code) = v["code"].as_str() {
        Err(SendError::Coded {
            code: code.to_owned(),
            message: v["message"].as_str().unwrap_or("").to_owned(),
        })
    } else if v["timeout"].as_bool() == Some(true) {
        Err(SendError::Timeout)
    } else {
        Err(SendError::Exception {
            message: v["exception"].as_str().unwrap_or("no answer").to_owned(),
        })
    })
}

/// Testing § 6.3: Workers AI and Vectorize go to the fake server, `POST {fakes}/__{service}/{op}`
/// with a JSON body. `None` without a fake server; an answer of 429 or 5xx is an error of the binding
/// (the detail names the status, so a caller can tell a rate limit).
pub async fn fake_call(
    binding: &'static str,
    path: &str,
    body: Value,
) -> Option<crate::PResult<Value>> {
    let url = FAKES.with(|f| f.borrow().clone())?;
    let mut req = HttpRequest::get(format!("{url}{path}"));
    req.method = "POST";
    req.timeout_ms = 10_000;
    req.max_body_bytes = 32 << 20;
    req.headers
        .push(("content-type".into(), "application/json".into()));
    req.body = Some(body.to_string().into_bytes());
    Some(match CfHttp.send(req).await {
        Ok(r) if r.status >= 400 => Err(crate::PlatformError::unavailable(
            binding,
            format!("fake answered {}", r.status),
        )),
        Ok(r) => serde_json::from_slice(&r.body)
            .map_err(|e| crate::PlatformError::invalid(binding, e.to_string())),
        Err(_) => Err(crate::PlatformError::unavailable(
            binding,
            "fake unreachable",
        )),
    })
}
