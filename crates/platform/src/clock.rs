//! Time ([Design › Time, randomness and IDs]): Unix milliseconds from `Date.now()`, which in Workers
//! advances only across I/O. Nothing else reads the clock.
//!
//! [Design › Time, randomness and IDs]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/index.md

pub trait Clock {
    fn now_ms(&self) -> i64;
}
