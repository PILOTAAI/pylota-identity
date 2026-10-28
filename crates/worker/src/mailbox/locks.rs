//! The thread send lock ([Outbound › Thread lock], C4, FR-OUT-9): taken in the lock transaction of submit
//! step 5, held while the message is `queued`, released with the state change that leaves `queued` or
//! enters a quota back-off. A crashed holder's lock expires with its lease, so nothing sweeps them.
//!
//! [Outbound › Thread lock]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_platform::PResult;
use pylota_identity_platform::durable::Sql;
use serde::Deserialize;

/// The lease a submit takes.
pub const LEASE_MS: i64 = 120_000;
/// The lease `BeginTransport` refreshes to.
pub const TRANSPORT_LEASE_MS: i64 = 60_000;
/// How long a submit waits for a held lock, and between attempts.
pub const WAIT_MS: i64 = 10_000;
pub const RETRY_MS: u32 = 250;

/// Takes the lock of thread `seq` for `owner` (a message ID); `true` when taken.
pub fn acquire<S: Sql>(tx: &S, seq: i64, owner: &str, now: i64, lease: i64) -> PResult<bool> {
    let r = tx.exec(
        "UPDATE threads SET lock_owner = ?1, lock_until = ?2 WHERE seq = ?3 AND (lock_owner IS NULL OR lock_until < ?4 OR lock_owner = ?1)",
        &[owner.into(), (now + lease).into(), seq.into(), now.into()],
    )?;
    Ok(r.changes == 1)
}

/// Releases the lock `owner` holds on its thread, if any.
pub fn release<S: Sql>(tx: &S, owner: &str) -> PResult<()> {
    tx.exec(
        "UPDATE threads SET lock_owner = NULL, lock_until = NULL WHERE lock_owner = ?1",
        &[owner.into()],
    )?;
    Ok(())
}

#[derive(Deserialize)]
struct Until {
    lock_until: Option<i64>,
}

/// `details.retry_after` of `thread_busy`: the rest of the holder's lease, 1 to 60 seconds.
pub fn retry_after<S: Sql>(s: &S, seq: i64, now: i64) -> i64 {
    let until = s
        .first::<Until>(
            "SELECT lock_until FROM threads WHERE seq = ?1",
            &[seq.into()],
        )
        .ok()
        .flatten()
        .and_then(|u| u.lock_until)
        .unwrap_or(now);
    ((until - now + 999) / 1000).clamp(1, 60)
}
