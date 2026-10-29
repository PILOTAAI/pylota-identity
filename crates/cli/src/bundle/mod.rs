//! Release bundles ([CLI and setup §7, §8]): download and verification, safe extraction, the
//! `wrangler.toml` renderer and the D1 migration runner.
//!
//! [CLI and setup §7, §8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#8-deploy

pub mod extract;
pub mod migrate;
pub mod release;
pub mod render;
pub mod verify;
