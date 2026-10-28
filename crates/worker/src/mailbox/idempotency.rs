//! The send ledger ([Outbound › Reservation inside the mailbox], FR-OUT-1, G1): the `idempotency` table
//! keyed by the SHA-256 of the `Idempotency-Key`, and the in-memory set of keys being processed.
//!
//! [Outbound › Reservation inside the mailbox]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use std::cell::RefCell;
use std::collections::HashSet;

use pylota_identity_platform::PResult;
use pylota_identity_platform::durable::Sql;
use serde::Deserialize;

/// A ledger row is kept this long.
pub const TTL_MS: i64 = 2_592_000_000;

thread_local! {
    /// `{object id}:{key hash}` of the sends running in this isolate. A Durable Object has one live
    /// instance, and the set is lost only with every request it was running.
    static IN_FLIGHT: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Removes its key from the set on every exit path.
pub struct InFlight(String);

impl InFlight {
    /// `None` when the key is already being processed (`409 request_in_progress`).
    pub fn enter(object_id: &str, key_hash: &str) -> Option<Self> {
        let k = format!("{object_id}:{key_hash}");
        IN_FLIGHT
            .with(|s| s.borrow_mut().insert(k.clone()))
            .then_some(Self(k))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT.with(|s| {
            s.borrow_mut().remove(&self.0);
        });
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    pub fingerprint: String,
    pub message_id: Option<String>,
    pub response_json: String,
}

/// The unexpired row of a key.
pub fn lookup<S: Sql>(s: &S, key_hash: &str, now: i64) -> PResult<Option<Entry>> {
    s.first(
        "SELECT fingerprint, message_id, response_json FROM idempotency WHERE key_hash = ?1 AND expires_at > ?2",
        &[key_hash.into(), now.into()],
    )
}

/// Records an accepted send (inside the storage transaction).
pub fn record<S: Sql>(
    tx: &S,
    key_hash: &str,
    fingerprint: &str,
    operation: &str,
    message_id: &str,
    response_json: &str,
    now: i64,
) -> PResult<()> {
    tx.exec(
        "INSERT INTO idempotency (key_hash, fingerprint, operation, message_id, response_json, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        &[
            key_hash.into(),
            fingerprint.into(),
            operation.into(),
            message_id.into(),
            response_json.into(),
            now.into(),
            (now + TTL_MS).into(),
        ],
    )?;
    Ok(())
}
