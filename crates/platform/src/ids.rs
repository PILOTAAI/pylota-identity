//! IDs: `{prefix}_{ULID}`, generated only here and monotonic within an isolate
//! ([Design › Time, randomness and IDs]). If the clock has not passed the last ID's millisecond, the
//! last millisecond is reused and the 80-bit random part incremented by one, moving to the next
//! millisecond on overflow.
//!
//! [Design › Time, randomness and IDs]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/index.md

use std::cell::Cell;

pub use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_api_types::ids::{MAX_TIME_MS, format};

pub trait Ids {
    fn new_id(&self, prefix: IdPrefix) -> String;
}

const RANDOM_MAX: u128 = (1 << 80) - 1;

/// The monotonic rule, shared by the Cloudflare generator (one per isolate, in a `thread_local`) and
/// the fake.
#[derive(Debug, Default)]
pub struct Monotonic {
    last: Cell<Option<(u64, u128)>>,
}

impl Monotonic {
    pub const fn new() -> Self {
        Self {
            last: Cell::new(None),
        }
    }

    /// The `(time_ms, random)` of the next ID, from the clock and 80 fresh random bits.
    pub fn next(&self, now_ms: i64, fresh: u128) -> (u64, u128) {
        let now = now_ms.clamp(0, MAX_TIME_MS as i64) as u64;
        let fresh = fresh & RANDOM_MAX;
        let next = match self.last.get() {
            Some((ms, random)) if now <= ms => {
                if random < RANDOM_MAX {
                    (ms, random + 1)
                } else {
                    ((ms + 1).min(MAX_TIME_MS), fresh)
                }
            }
            _ => (now, fresh),
        };
        self.last.set(Some(next));
        next
    }

    /// The next ID with this prefix.
    pub fn id(&self, prefix: IdPrefix, now_ms: i64, fresh: u128) -> String {
        let (ms, random) = self.next(now_ms, fresh);
        format(prefix, ms, random)
    }
}

/// 80 random bits from the platform's RNG.
pub fn random80(rng: &impl crate::Rng) -> u128 {
    let mut b = [0u8; 16];
    rng.fill(&mut b[..10]);
    u128::from_le_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IDs generated in one millisecond sort strictly; random overflow moves to the next millisecond;
    /// a clock that goes back keeps the order.
    #[test]
    fn monotonic_within_ms() {
        let m = Monotonic::new();
        let now = 1_791_540_000_000;
        let ids: Vec<String> = (0..1000u128)
            .map(|i| m.id(IdPrefix::Message, now, (i * 7919) % 1000))
            .collect();
        for pair in ids.windows(2) {
            assert!(pair[0] < pair[1], "{} !< {}", pair[0], pair[1]);
            assert_eq!(pair[0][..14], pair[1][..14], "same millisecond");
        }

        let m = Monotonic::new();
        assert_eq!(m.next(now, RANDOM_MAX), (now as u64, RANDOM_MAX));
        assert_eq!(m.next(now, 5), (now as u64 + 1, 5), "overflow moves on");
        assert_eq!(m.next(now - 10, 9), (now as u64 + 1, 6), "clock went back");
        assert_eq!(m.next(now + 5, 3), (now as u64 + 5, 3), "clock advanced");

        let a = m.id(IdPrefix::Message, now + 5, 0);
        let b = m.id(IdPrefix::Message, now + 5, 0);
        assert!(a < b);
        assert!(a.starts_with("msg_") && a.len() == 30);
    }
}
