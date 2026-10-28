//! Request, response, object and event types of the Pylota Identity API, the `ErrorCode` enum and the
//! OpenAPI description (build plan M3). The contract is `docs/src/reference/openapi.yaml`; the types
//! here generate `openapi.json` through `utoipa`, and `openapi::tests` compares the two.
#![forbid(unsafe_code)]

pub mod errors;
pub mod events;
pub mod ids;
pub mod internal;
pub mod nullable;
pub mod objects;
pub mod openapi;
pub mod requests;

pub use errors::*;
pub use events::*;
pub use objects::*;
pub use requests::*;
