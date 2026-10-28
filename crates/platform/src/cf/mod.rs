//! The Cloudflare side of the platform, compiled for wasm32 only: one implementation per trait over
//! the runtime, [`CfPlatform`] bundling them for one invocation, and the entry glue.

mod ai;
mod basic;
mod d1;
mod durable;
mod email;
pub mod externs;
pub mod glue;
#[cfg(feature = "itest-hooks")]
pub mod itest;
pub(crate) mod js;
mod net;
mod queues;
mod r2;

use std::cell::RefCell;
use std::rc::Rc;

pub use ai::{CfAi, CfVectors};
pub use basic::{CfBackground, CfClock, CfIds, CfLimits, CfMetrics, CfRng, metrics_dropped};
pub use d1::CfD1;
pub use durable::{CfHibernation, CfObjects, CfSql, CfStorage, CfWebSocket};
pub use email::CfMail;
pub use net::{CfConn, CfHttp, CfTcp};
pub use queues::CfQueues;
pub use r2::CfBlobs;

use crate::Platform;
use crate::config::{Build, Config, ConfigInvalid};
use crate::dns::DohClient;

thread_local! {
    /// `Config` is read once per isolate; so is its outcome.
    static CONFIG: RefCell<Option<Result<Rc<Config>, ConfigInvalid>>> = const { RefCell::new(None) };
}

/// The isolate's configuration, read from `env` on first use. The first read logs `config_invalid`
/// with the variable's name, or each warning (`docs_url_invalid`), once.
pub fn config(env: &worker::Env) -> Result<Rc<Config>, ConfigInvalid> {
    CONFIG.with(|cell| {
        if let Some(done) = cell.borrow().as_ref() {
            return done.clone();
        }
        let lookup = |name: &str| {
            env.secret(name)
                .map(|s| s.to_string())
                .or_else(|_| env.var(name).map(|v| v.to_string()))
                .ok()
        };
        let read = Config::read(&lookup, Build::RELEASE).map(Rc::new);
        match &read {
            Err(e) => {
                worker::console_error!("event=config_invalid name={} reason={}", e.name, e.reason)
            }
            Ok(c) => {
                for w in &c.warnings {
                    worker::console_warn!("event={w}");
                }
            }
        }
        *cell.borrow_mut() = Some(read.clone());
        read
    })
}

/// The size of the wasm linear memory, in bytes (S4's peak-memory measure: linear memory only grows).
pub fn wasm_memory_bytes() -> u64 {
    use wasm_bindgen::JsCast as _;
    wasm_bindgen::memory()
        .unchecked_into::<js_sys::WebAssembly::Memory>()
        .buffer()
        .unchecked_into::<js_sys::ArrayBuffer>()
        .byte_length() as u64
}

/// Every capability of one invocation, over its `Env`.
pub struct CfPlatform {
    config: Rc<Config>,
    db: CfD1,
    objects: CfObjects,
    blobs: CfBlobs,
    backup: Option<CfBlobs>,
    queues: CfQueues,
    ai: CfAi,
    vectors: CfVectors,
    vectors_next: Option<CfVectors>,
    mail: CfMail,
    limits: CfLimits,
    dns: DohClient<CfHttp>,
    metrics: CfMetrics,
    background: CfBackground,
}

impl CfPlatform {
    pub(crate) fn new(env: worker::Env, background: CfBackground) -> Result<Self, ConfigInvalid> {
        let config = config(&env)?;
        let bound = |name: &str| js::get(&env, name).is_ok_and(|v| !v.is_undefined());
        Ok(Self {
            db: CfD1 { env: env.clone() },
            objects: CfObjects {
                env: env.clone(),
                jurisdiction: config.jurisdiction,
            },
            blobs: CfBlobs {
                env: env.clone(),
                binding: "BLOBS",
            },
            backup: config.backup_bucket.as_ref().map(|_| CfBlobs {
                env: env.clone(),
                binding: "BACKUP",
            }),
            queues: CfQueues { env: env.clone() },
            ai: CfAi { env: env.clone() },
            vectors: CfVectors {
                env: env.clone(),
                binding: "VECTORS",
            },
            vectors_next: bound("VECTORS_NEXT").then(|| CfVectors {
                env: env.clone(),
                binding: "VECTORS_NEXT",
            }),
            mail: CfMail { env: env.clone() },
            limits: CfLimits { env: env.clone() },
            dns: DohClient {
                http: CfHttp,
                resolvers: config.doh_resolvers.clone(),
            },
            metrics: CfMetrics { env },
            background,
            config,
        })
    }
}

impl Platform for CfPlatform {
    type Clock = CfClock;
    type Rng = CfRng;
    type Ids = CfIds;
    type Db = CfD1;
    type Objects = CfObjects;
    type Blobs = CfBlobs;
    type Queues = CfQueues;
    type Ai = CfAi;
    type Vectors = CfVectors;
    type Mail = CfMail;
    type Limits = CfLimits;
    type Dns = DohClient<CfHttp>;
    type Http = CfHttp;
    type Tcp = CfTcp;
    type Metrics = CfMetrics;
    type Background = CfBackground;

    fn clock(&self) -> &CfClock {
        &CfClock
    }
    fn rng(&self) -> &CfRng {
        &CfRng
    }
    fn ids(&self) -> &CfIds {
        &CfIds
    }
    fn db(&self) -> &CfD1 {
        &self.db
    }
    fn objects(&self) -> &CfObjects {
        &self.objects
    }
    fn blobs(&self) -> &CfBlobs {
        &self.blobs
    }
    fn backup(&self) -> Option<&CfBlobs> {
        self.backup.as_ref()
    }
    fn queues(&self) -> &CfQueues {
        &self.queues
    }
    fn ai(&self) -> &CfAi {
        &self.ai
    }
    fn vectors(&self) -> &CfVectors {
        &self.vectors
    }
    fn vectors_next(&self) -> Option<&CfVectors> {
        self.vectors_next.as_ref()
    }
    fn mail(&self) -> &CfMail {
        &self.mail
    }
    fn limits(&self) -> &CfLimits {
        &self.limits
    }
    fn dns(&self) -> &DohClient<CfHttp> {
        &self.dns
    }
    fn http(&self) -> &CfHttp {
        &CfHttp
    }
    fn tcp(&self) -> &CfTcp {
        &CfTcp
    }
    fn metrics(&self) -> &CfMetrics {
        &self.metrics
    }
    fn background(&self) -> &CfBackground {
        &self.background
    }
    fn config(&self) -> &Config {
        &self.config
    }
}
