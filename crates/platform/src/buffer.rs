//! Bytes that stay in the JS heap until Rust needs them ([Rust workspace §6.2] `BlobBody::Js`): a raw
//! inbound message or a request body goes to R2 without being copied into wasm memory. Natively the
//! buffer is a `Vec<u8>`.
//!
//! [Rust workspace §6.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

/// An opaque handle to bytes in the JS heap (a `Uint8Array` on wasm32).
#[derive(Clone, Debug)]
pub struct JsBuffer {
    #[cfg(target_arch = "wasm32")]
    pub(crate) array: js_sys::Uint8Array,
    #[cfg(not(target_arch = "wasm32"))]
    bytes: Vec<u8>,
}

impl JsBuffer {
    /// Copies `bytes` into a new buffer.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                array: js_sys::Uint8Array::from(bytes),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                bytes: bytes.to_vec(),
            }
        }
    }

    pub fn len(&self) -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            u64::from(self.array.length())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.bytes.len() as u64
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copies the bytes into wasm memory.
    pub fn to_vec(&self) -> Vec<u8> {
        #[cfg(target_arch = "wasm32")]
        {
            self.array.to_vec()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.bytes.clone()
        }
    }
}

/// A request body streamed through to R2 (`BlobStore::put_stream`): the runtime's `ReadableStream` on
/// wasm32, the whole body natively.
pub struct BodyStream {
    #[cfg(target_arch = "wasm32")]
    pub(crate) stream: web_sys::ReadableStream,
    #[cfg(not(target_arch = "wasm32"))]
    pub bytes: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
impl BodyStream {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}
