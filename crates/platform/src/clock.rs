//! Time ([Design › Time, randomness and IDs]): Unix milliseconds from `Date.now()`, which in Workers
//! advances only across I/O. Nothing else reads the clock.
//!
//! [Design › Time, randomness and IDs]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/index.md

pub trait Clock {
    fn now_ms(&self) -> i64;

    /// Waits `ms` milliseconds (the retry pauses of the `email()` handler, the `wait` polls). The fake
    /// clock advances by `ms` and returns at once.
    fn sleep(&self, ms: u32) -> impl std::future::Future<Output = ()> {
        let _ = ms;
        async {}
    }
}
