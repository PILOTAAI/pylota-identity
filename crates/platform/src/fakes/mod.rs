//! In-memory fakes of every platform trait, for native tests of `worker` logic modules (Testing § 3):
//! a clock that tests advance, a seeded RNG, `rusqlite` for D1 and Durable Object SQLite (with FTS5),
//! maps for R2 that can fail with R2's codes, captured queue sends, scripted AI and Vectorize, a DNS
//! zone map per resolver, a recording HTTP client, a scripted SMTP peer and a recording mail sender.
//!
//! The fakes apply the same limits as the Cloudflare implementations (they call the same `check_*`
//! functions), so a test that passes here does not depend on a limit only workerd enforces. Every
//! future a fake returns is ready on its first poll; [`block_on`] runs one.

mod blobs;
mod net;
mod objects;
mod services;
mod sqlite;

use std::cell::Cell;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

pub use blobs::FakeBlobs;
pub use net::{FakeConn, FakeDns, FakeHttp, FakeTcp, ScriptedResponse, SmtpPeer};
pub use objects::{FakeHibernation, FakeObjects, FakeSocket, FakeState};
pub use services::{
    FakeAi, FakeBackground, FakeLimits, FakeMail, FakeMetrics, FakeQueues, FakeVectors, SentMail,
    Settled, incoming,
};
pub use sqlite::{FakeD1, FakeSql};

use crate::config::{Build, Config};
use crate::ids::{IdPrefix, Monotonic, random80};
use crate::{Clock, Ids, Platform, Rng};

/// Runs a future that is ready on its first poll, as every fake's is. A future that waits on anything
/// else is a test bug, and panics.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = pin!(fut);
    match fut.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("a future under block_on waited: fakes never wait"),
    }
}

/// `now_ms = 1_791_540_000_000` (Testing § 3) unless set; tests advance it.
#[derive(Clone, Debug)]
pub struct FakeClock(Rc<Cell<i64>>);

impl FakeClock {
    pub const START_MS: i64 = 1_791_540_000_000;

    pub fn new(now_ms: i64) -> Self {
        Self(Rc::new(Cell::new(now_ms)))
    }

    pub fn set(&self, now_ms: i64) {
        self.0.set(now_ms);
    }

    pub fn advance(&self, ms: i64) {
        self.0.set(self.0.get() + ms);
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new(Self::START_MS)
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.get()
    }
}

/// A seeded generator (SplitMix64): the same seed gives the same bytes. Not for anything but tests.
#[derive(Clone, Debug)]
pub struct FakeRng(Rc<Cell<u64>>);

impl FakeRng {
    pub fn seeded(seed: u64) -> Self {
        Self(Rc::new(Cell::new(seed)))
    }

    fn next(&self) -> u64 {
        let s = self.0.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
        self.0.set(s);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Rng for FakeRng {
    fn fill(&self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let bytes = self.next().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }
}

/// The monotonic generator over the fake clock and RNG.
#[derive(Debug)]
pub struct FakeIds {
    clock: FakeClock,
    rng: FakeRng,
    monotonic: Monotonic,
}

impl FakeIds {
    pub fn new(clock: FakeClock, rng: FakeRng) -> Self {
        Self {
            clock,
            rng,
            monotonic: Monotonic::new(),
        }
    }
}

impl Ids for FakeIds {
    fn new_id(&self, prefix: IdPrefix) -> String {
        self.monotonic
            .id(prefix, self.clock.now_ms(), random80(&self.rng))
    }
}

/// The variables of [`FakePlatform::new`]: the required ones, with fixed test keys.
pub const TEST_VARS: &[(&str, &str)] = &[
    ("PM_PLATFORM_DOMAIN", "pylotamail.test"),
    ("PM_API_HOST", "api.pylotamail.test"),
    ("PM_ENV", "local"),
    (
        "PM_MASTER_KEY",
        "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
    ),
    (
        "PM_KEY_PEPPER",
        "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=",
    ),
    (
        "PM_HASH_KEY",
        "AgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4fICE=",
    ),
];

/// Every fake, as one [`Platform`]. Fields are public so tests script and inspect them.
pub struct FakePlatform {
    pub clock: FakeClock,
    pub rng: FakeRng,
    pub ids: FakeIds,
    pub db: FakeD1,
    pub objects: FakeObjects,
    pub blobs: FakeBlobs,
    pub backup: Option<FakeBlobs>,
    pub queues: FakeQueues,
    pub ai: FakeAi,
    pub vectors: FakeVectors,
    pub vectors_next: Option<FakeVectors>,
    pub mail: FakeMail,
    pub limits: FakeLimits,
    pub dns: FakeDns,
    pub http: FakeHttp,
    pub tcp: FakeTcp,
    pub metrics: FakeMetrics,
    pub background: FakeBackground,
    pub config: Config,
}

impl FakePlatform {
    /// [`TEST_VARS`] and nothing else.
    pub fn new() -> Self {
        Self::with_vars(&[])
    }

    /// [`TEST_VARS`] overridden and extended by `vars`.
    pub fn with_vars(vars: &[(&str, &str)]) -> Self {
        let lookup = |name: &str| {
            vars.iter()
                .chain(TEST_VARS)
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        };
        let config = Config::read(&lookup, Build::RELEASE).expect("test configuration");
        let clock = FakeClock::default();
        let rng = FakeRng::seeded(1);
        Self {
            ids: FakeIds::new(clock.clone(), rng.clone()),
            http: FakeHttp::with_clock(clock.clone()),
            backup: config.backup_bucket.as_ref().map(|_| FakeBlobs::new()),
            clock,
            rng,
            db: FakeD1::new(),
            objects: FakeObjects::new(),
            blobs: FakeBlobs::new(),
            queues: FakeQueues::new(),
            ai: FakeAi::new(),
            vectors: FakeVectors::new(),
            vectors_next: config
                .embed_model_previous
                .as_ref()
                .map(|_| FakeVectors::new()),
            mail: FakeMail::new(),
            limits: FakeLimits::new(),
            dns: FakeDns::new(),
            tcp: FakeTcp::new(),
            metrics: FakeMetrics::new(),
            background: FakeBackground::new(),
            config,
        }
    }
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for FakePlatform {
    type Clock = FakeClock;
    type Rng = FakeRng;
    type Ids = FakeIds;
    type Db = FakeD1;
    type Objects = FakeObjects;
    type Blobs = FakeBlobs;
    type Queues = FakeQueues;
    type Ai = FakeAi;
    type Vectors = FakeVectors;
    type Mail = FakeMail;
    type Limits = FakeLimits;
    type Dns = FakeDns;
    type Http = FakeHttp;
    type Tcp = FakeTcp;
    type Metrics = FakeMetrics;
    type Background = FakeBackground;

    fn clock(&self) -> &FakeClock {
        &self.clock
    }
    fn rng(&self) -> &FakeRng {
        &self.rng
    }
    fn ids(&self) -> &FakeIds {
        &self.ids
    }
    fn db(&self) -> &FakeD1 {
        &self.db
    }
    fn objects(&self) -> &FakeObjects {
        &self.objects
    }
    fn blobs(&self) -> &FakeBlobs {
        &self.blobs
    }
    fn backup(&self) -> Option<&FakeBlobs> {
        self.backup.as_ref()
    }
    fn queues(&self) -> &FakeQueues {
        &self.queues
    }
    fn ai(&self) -> &FakeAi {
        &self.ai
    }
    fn vectors(&self) -> &FakeVectors {
        &self.vectors
    }
    fn vectors_next(&self) -> Option<&FakeVectors> {
        self.vectors_next.as_ref()
    }
    fn mail(&self) -> &FakeMail {
        &self.mail
    }
    fn limits(&self) -> &FakeLimits {
        &self.limits
    }
    fn dns(&self) -> &FakeDns {
        &self.dns
    }
    fn http(&self) -> &FakeHttp {
        &self.http
    }
    fn tcp(&self) -> &FakeTcp {
        &self.tcp
    }
    fn metrics(&self) -> &FakeMetrics {
        &self.metrics
    }
    fn background(&self) -> &FakeBackground {
        &self.background
    }
    fn config(&self) -> &Config {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_seeded_and_ids_follow_the_clock() {
        let (a, b) = (FakeRng::seeded(7), FakeRng::seeded(7));
        assert_eq!(a.bytes::<13>(), b.bytes::<13>());
        assert_ne!(a.bytes::<8>(), FakeRng::seeded(8).bytes::<8>());

        let p = FakePlatform::new();
        let first = p.ids().new_id(IdPrefix::Message);
        let second = p.ids().new_id(IdPrefix::Message);
        assert!(first < second);
        p.clock.advance(1);
        let later = p.ids().new_id(IdPrefix::Message);
        assert_ne!(later[..14], second[..14]);
        assert_eq!(p.config().platform_domain, "pylotamail.test");
        assert!(p.backup().is_none() && p.vectors_next().is_none());
        assert!(
            FakePlatform::with_vars(&[("PM_BACKUP_BUCKET", "b")])
                .backup()
                .is_some()
        );
    }
}
