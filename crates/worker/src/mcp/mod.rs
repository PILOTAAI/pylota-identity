//! The MCP server at `/mcp` ([MCP server]): Streamable HTTP inside the Worker's `fetch`, dual-era
//! (`2026-07-28` per-request `_meta`, and the `initialize` handshake of `2025-11-25` and `2025-06-18`),
//! stateless, authenticated by the REST API keys. `transport.rs` handles the request, `tools.rs` the
//! tools, `prompts.rs` the prompt and instructions, `schemas.rs` the protocol types.
//!
//! [MCP server]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/mcp.md

pub mod prompts;
pub mod schemas;
pub mod tools;
mod transport;

pub use transport::serve;

/// The revision served statelessly with per-request `_meta`.
pub const MODERN: &str = "2026-07-28";
/// The revisions served with the `initialize` handshake.
pub const LEGACY: [&str; 2] = ["2025-11-25", "2025-06-18"];
/// Every revision served, newest first.
pub const SUPPORTED_VERSIONS: [&str; 3] = [MODERN, LEGACY[0], LEGACY[1]];

/// The largest request body: the REST limit for a send with attachments (MCP § 2.1).
pub const MAX_BODY: u64 = 7 << 20;
