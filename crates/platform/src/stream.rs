//! A response body written after the response is returned (server-sent events, Search § 11.11): the
//! handler returns the [`StreamBody`] and keeps the [`StreamSink`] in a `waitUntil` future. On wasm32 the
//! pair is one `TransformStream`; natively a shared buffer the tests read.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

/// The readable side, handed to the runtime as the response body.
pub struct StreamBody {
    #[cfg(target_arch = "wasm32")]
    pub(crate) readable: JsValue,
    #[cfg(not(target_arch = "wasm32"))]
    pub shared: std::rc::Rc<std::cell::RefCell<Shared>>,
}

/// The native buffer: what was written, whether the writer closed, and whether the reader went away.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct Shared {
    pub bytes: Vec<u8>,
    pub closed: bool,
    pub cancelled: bool,
}

/// The writable side.
pub struct StreamSink {
    #[cfg(target_arch = "wasm32")]
    writer: JsValue,
    #[cfg(not(target_arch = "wasm32"))]
    shared: std::rc::Rc<std::cell::RefCell<Shared>>,
}

impl core::fmt::Debug for StreamBody {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("StreamBody")
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl StreamBody {
    /// Everything written so far, as text.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.shared.borrow().bytes).into_owned()
    }

    /// The reader goes away: the next write fails, as when a client disconnects.
    pub fn cancel(&self) {
        self.shared.borrow_mut().cancelled = true;
    }
}

/// A new body and its writer.
pub fn channel() -> crate::PResult<(StreamBody, StreamSink)> {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::cf::js;
        let ts = js::construct("TransformStream", &[]).map_err(|e| js::error("STREAM", &e))?;
        let readable = js::get(&ts, "readable").map_err(|e| js::error("STREAM", &e))?;
        let writable = js::get(&ts, "writable").map_err(|e| js::error("STREAM", &e))?;
        let writer = js::call(&writable, "getWriter", &[]).map_err(|e| js::error("STREAM", &e))?;
        Ok((StreamBody { readable }, StreamSink { writer }))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let shared = std::rc::Rc::new(std::cell::RefCell::new(Shared::default()));
        Ok((
            StreamBody {
                shared: shared.clone(),
            },
            StreamSink { shared },
        ))
    }
}

impl StreamSink {
    /// Writes one chunk; `false` when the reader is gone (the client disconnected) or the stream is
    /// closed.
    pub async fn write(&self, bytes: &[u8]) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::cf::js;
            let chunk = js_sys::Uint8Array::from(bytes);
            js::call_async(&self.writer, "write", &[&chunk.into()])
                .await
                .is_ok()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut s = self.shared.borrow_mut();
            if s.cancelled || s.closed {
                return false;
            }
            s.bytes.extend_from_slice(bytes);
            true
        }
    }

    /// Ends the body.
    pub async fn close(&self) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = crate::cf::js::call_async(&self.writer, "close", &[]).await;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.shared.borrow_mut().closed = true;
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::fakes::block_on;

    #[test]
    fn writes_until_cancelled_or_closed() {
        let (body, sink) = channel().unwrap();
        assert!(block_on(sink.write(b"a")));
        body.cancel();
        assert!(!block_on(sink.write(b"b")));
        assert_eq!(body.text(), "a");
        let (body, sink) = channel().unwrap();
        block_on(sink.close());
        assert!(!block_on(sink.write(b"c")));
        assert!(body.shared.borrow().closed);
    }
}
