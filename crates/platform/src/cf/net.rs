//! Outbound HTTP over the runtime's `fetch` (deadline through an `AbortController` raced against a
//! timer, body read chunk by chunk up to the cap, then cancelled) and TCP over `connect()` (the
//! runtime's streams; `worker::Socket` reads through tokio's traits, which `platform` must not use).

use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;

use super::js;
use crate::http::{HttpClient, HttpError, HttpRequest, HttpResponse, Redirect, take_capped};
use crate::tcp::{SocketError, TcpConn, TcpConnect, TlsMode, check_target};

#[derive(Clone, Copy, Default)]
pub struct CfHttp;

fn http_error(e: &JsValue) -> HttpError {
    let m = js::message(e);
    let lower = m.to_ascii_lowercase();
    if lower.contains("dns") || lower.contains("resolve") {
        HttpError::Dns
    } else if lower.contains("tls") || lower.contains("ssl") || lower.contains("certificate") {
        HttpError::Tls
    } else if lower.contains("connect") || lower.contains("network connection lost") {
        HttpError::Connect
    } else {
        HttpError::Other(m.chars().take(200).collect())
    }
}

fn remaining(deadline: f64) -> u32 {
    (deadline - js_sys::Date::now()).max(0.0) as u32
}

impl HttpClient for CfHttp {
    async fn send(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        let deadline = js_sys::Date::now() + f64::from(req.timeout_ms);
        let fail = |e: JsValue| http_error(&e);
        let controller = js::construct("AbortController", &[]).map_err(fail)?;
        let headers = js::construct("Headers", &[]).map_err(fail)?;
        for (name, value) in &req.headers {
            js::call(
                &headers,
                "append",
                &[&JsValue::from_str(name), &JsValue::from_str(value)],
            )
            .map_err(fail)?;
        }
        let init = js::object();
        js::set(&init, "method", &JsValue::from_str(req.method)).map_err(fail)?;
        js::set(&init, "headers", &headers).map_err(fail)?;
        let redirect = match req.redirect {
            Redirect::Manual => "manual",
            Redirect::Follow => "follow",
        };
        js::set(&init, "redirect", &JsValue::from_str(redirect)).map_err(fail)?;
        js::set(
            &init,
            "signal",
            &js::get(&controller, "signal").map_err(fail)?,
        )
        .map_err(fail)?;
        if let Some(body) = &req.body {
            js::set(
                &init,
                "body",
                &js_sys::Uint8Array::from(body.as_slice()).into(),
            )
            .map_err(fail)?;
        }
        #[cfg(feature = "itest-hooks")]
        let url = super::itest::reroute(&req.url);
        #[cfg(not(feature = "itest-hooks"))]
        let url = req.url.clone();
        let fetch: js_sys::Promise = js::call(
            &js_sys::global(),
            "fetch",
            &[&JsValue::from_str(&url), &init],
        )
        .map_err(fail)?
        .dyn_into()
        .map_err(fail)?;
        let abort = || {
            let _ = js::call(&controller, "abort", &[]);
            HttpError::Timeout
        };
        let response = match js::with_deadline(fetch, req.timeout_ms).await {
            Ok(Some(r)) => r,
            Ok(None) => return Err(abort()),
            Err(e) => return Err(http_error(&e)),
        };
        let status = js::get(&response, "status")
            .map_err(fail)?
            .as_f64()
            .unwrap_or(0.0) as u16;
        let raw_headers: worker::worker_sys::web_sys::Headers = js::get(&response, "headers")
            .map_err(fail)?
            .unchecked_into();
        let headers = worker::Headers(raw_headers).entries().collect();

        let mut body = Vec::new();
        let mut body_truncated = false;
        let stream = js::get(&response, "body").map_err(fail)?;
        if !stream.is_null() && !stream.is_undefined() {
            let reader = js::call(&stream, "getReader", &[]).map_err(fail)?;
            loop {
                let read: js_sys::Promise = js::call(&reader, "read", &[])
                    .map_err(fail)?
                    .dyn_into()
                    .map_err(fail)?;
                let chunk = match js::with_deadline(read, remaining(deadline)).await {
                    Ok(Some(c)) => c,
                    Ok(None) => return Err(abort()),
                    Err(e) => return Err(http_error(&e)),
                };
                if js::get(&chunk, "done").map_err(fail)?.is_truthy() {
                    break;
                }
                let value = js::get(&chunk, "value").map_err(fail)?;
                let bytes = js_sys::Uint8Array::new(&value).to_vec();
                if take_capped(&mut body, &bytes, req.max_body_bytes) {
                    body_truncated = true;
                    let _ = js::call(&reader, "cancel", &[]);
                    break;
                }
            }
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
            body_truncated,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub struct CfTcp;

/// One connection over the socket's reader and writer.
pub struct CfConn {
    sock: worker::worker_sys::Socket,
    reader: JsValue,
    writer: JsValue,
    pending: Vec<u8>,
    tls: TlsMode,
    upgraded: bool,
}

fn socket_error(e: &JsValue) -> SocketError {
    let lower = js::message(e).to_ascii_lowercase();
    if lower.contains("tls") || lower.contains("certificate") || lower.contains("handshake") {
        SocketError::Tls
    } else if lower.contains("closed") || lower.contains("reset") {
        SocketError::Closed
    } else if lower.contains("connect") || lower.contains("refused") {
        SocketError::Connect
    } else {
        SocketError::Other(js::message(e).chars().take(200).collect())
    }
}

async fn opened(sock: &worker::worker_sys::Socket, timeout_ms: u32) -> Result<(), SocketError> {
    let p = sock.opened().map_err(|e| socket_error(&e))?;
    match js::with_deadline(p, timeout_ms).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(SocketError::Timeout),
        Err(e) => Err(socket_error(&e)),
    }
}

impl CfConn {
    fn attach(
        sock: worker::worker_sys::Socket,
        tls: TlsMode,
        upgraded: bool,
    ) -> Result<Self, SocketError> {
        let fail = |e: JsValue| socket_error(&e);
        let readable: JsValue = sock.readable().map_err(fail)?.into();
        let writable: JsValue = sock.writable().map_err(fail)?.into();
        Ok(Self {
            reader: js::call(&readable, "getReader", &[]).map_err(fail)?,
            writer: js::call(&writable, "getWriter", &[]).map_err(fail)?,
            sock,
            pending: Vec::new(),
            tls,
            upgraded,
        })
    }
}

impl TcpConnect for CfTcp {
    type Conn = CfConn;

    async fn connect(
        &self,
        host: &str,
        port: u16,
        tls: TlsMode,
        timeout_ms: u32,
    ) -> Result<CfConn, SocketError> {
        check_target(port)?;
        let fail = |e: JsValue| socket_error(&e);
        let address = js::object();
        js::set(&address, "hostname", &JsValue::from_str(host)).map_err(fail)?;
        js::set(&address, "port", &JsValue::from(port)).map_err(fail)?;
        let options = js::object();
        let transport = match tls {
            TlsMode::Implicit => "on",
            TlsMode::StartTls => "starttls",
        };
        js::set(&options, "secureTransport", &JsValue::from_str(transport)).map_err(fail)?;
        let sock = worker::worker_sys::connect(address, options).map_err(|e| socket_error(&e))?;
        opened(&sock, timeout_ms).await?;
        CfConn::attach(sock, tls, false)
    }
}

impl TcpConn for CfConn {
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, SocketError> {
        if self.pending.is_empty() {
            let fail = |e: JsValue| socket_error(&e);
            let read: js_sys::Promise = js::call(&self.reader, "read", &[])
                .map_err(fail)?
                .dyn_into()
                .map_err(fail)?;
            let chunk = match js::with_deadline(read, timeout_ms).await {
                Ok(Some(c)) => c,
                Ok(None) => return Err(SocketError::Timeout),
                Err(e) => return Err(socket_error(&e)),
            };
            if js::get(&chunk, "done").map_err(fail)?.is_truthy() {
                return Ok(0);
            }
            self.pending =
                js_sys::Uint8Array::new(&js::get(&chunk, "value").map_err(fail)?).to_vec();
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }

    async fn write_all(&mut self, bytes: &[u8], timeout_ms: u32) -> Result<(), SocketError> {
        let fail = |e: JsValue| socket_error(&e);
        let write: js_sys::Promise = js::call(
            &self.writer,
            "write",
            &[&js_sys::Uint8Array::from(bytes).into()],
        )
        .map_err(fail)?
        .dyn_into()
        .map_err(fail)?;
        match js::with_deadline(write, timeout_ms).await {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(SocketError::Timeout),
            Err(e) => Err(socket_error(&e)),
        }
    }

    /// `startTls()` panics on a socket not opened with `starttls` in 0.8.7's wrapper, and may be called
    /// once; both are refused here first.
    async fn start_tls(self) -> Result<Self, SocketError> {
        if self.tls != TlsMode::StartTls || self.upgraded {
            return Err(SocketError::Tls);
        }
        let fail = |e: JsValue| socket_error(&e);
        js::call(&self.reader, "releaseLock", &[]).map_err(fail)?;
        js::call(&self.writer, "releaseLock", &[]).map_err(fail)?;
        let upgraded = self.sock.start_tls().map_err(fail)?;
        opened(&upgraded, 10_000).await?;
        CfConn::attach(upgraded, self.tls, true)
    }

    async fn close(self) {
        let _ = js::call(&self.writer, "close", &[]);
        if let Ok(p) = self.sock.close() {
            let _ = js::with_deadline(p, 1_000).await;
        }
    }
}
