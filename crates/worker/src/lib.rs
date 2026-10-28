//! The Pylota Identity Worker. M0 is a "hello" Worker: its only export, `fetch`, answers `200`. The
//! router, handlers, consumers and Durable Object classes arrive from M5 on.
//!
//! The `worker` attribute macros are never used here; the entry points come from
//! `pylota_identity_platform::export_worker!` ([Rust workspace §2]).
//!
//! [Rust workspace §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
#![forbid(unsafe_code)]

/// The workspace version, or the release tag without its `v` (`cargo xtask build-worker`).
pub const BUILD_VERSION: &str = env!("PM_BUILD_VERSION");
/// `git rev-parse --short=7 HEAD` at build time, `unknown` in native builds.
pub const BUILD_COMMIT: &str = env!("PM_BUILD_COMMIT");
/// The UTC build date, `1970-01-01` in native builds.
pub const BUILD_DATE: &str = env!("PM_BUILD_DATE");

/// The Worker application.
pub struct App;

/// A response before the platform converts it: status, headers and body.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl App {
    /// The answer to every request until M5 adds the router.
    pub fn hello() -> Reply {
        Reply {
            status: 200,
            headers: vec![(
                "content-type".to_owned(),
                "text/plain; charset=utf-8".to_owned(),
            )],
            body: b"hello\n".to_vec(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod entry {
    use pylota_identity_platform::cf::glue::{
        CfPlatform, HttpRequestIn, HttpResponseOut, WorkerApp,
    };

    impl WorkerApp for super::App {
        fn new(_platform: CfPlatform) -> Self {
            super::App
        }

        async fn fetch(&self, _req: HttpRequestIn) -> HttpResponseOut {
            let reply = super::App::hello();
            HttpResponseOut::Http {
                status: reply.status,
                headers: reply.headers,
                body: reply.body,
            }
        }
    }

    pylota_identity_platform::export_worker! {
        app: crate::App,
    }
}

/// Worker logic is generic over one `Platform` bundle; native tests run it on `platform::fakes`. Until the
/// logic modules arrive (M5 on), this exercise touches every trait of the bundle through the bound, as
/// they will.
#[cfg(test)]
mod platform_bundle {
    use pylota_identity_platform::ai::AiOptions;
    use pylota_identity_platform::d1::Stmt;
    use pylota_identity_platform::dns::{RType, Resolver};
    use pylota_identity_platform::durable::DoClass;
    use pylota_identity_platform::email::StructuredEmail;
    use pylota_identity_platform::http::HttpRequest;
    use pylota_identity_platform::ids::IdPrefix;
    use pylota_identity_platform::metrics::DataPoint;
    use pylota_identity_platform::queues::QueueName;
    use pylota_identity_platform::r2::{BlobBody, BlobMeta};
    use pylota_identity_platform::ratelimit::RlBucket;
    use pylota_identity_platform::tcp::TlsMode;
    use pylota_identity_platform::vectorize::VectorRecord;
    use pylota_identity_platform::{
        Ai, Background, BlobStore, Clock, ControlDb, Dns, HttpClient, Ids, MailSender, Metrics,
        ObjectClient, Platform, QueueProducer, RateLimiter, Rng, TcpConnect, VectorIndex,
    };

    /// One call through each trait; returns what a handler would act on.
    async fn touch_everything<P: Platform>(p: &P) -> Vec<String> {
        let mut seen = Vec::new();
        let id = p.ids().new_id(IdPrefix::Request);
        seen.push(format!("{}@{}", &id[..4], p.clock().now_ms()));
        let mut nonce = [0u8; 16];
        p.rng().fill(&mut nonce);
        let n: Vec<serde_json::Value> = p
            .db()
            .query(Stmt::new("SELECT 1 AS one", vec![]))
            .await
            .unwrap();
        seen.push(format!("db {}", n[0]["one"]));
        seen.push(format!(
            "do {}",
            p.objects().new_object_id(DoClass::Quota).unwrap().len()
        ));
        p.blobs()
            .put(
                "t/x",
                BlobBody::Bytes(b"raw".to_vec()),
                &BlobMeta::default(),
            )
            .await
            .unwrap();
        seen.push(format!("r2 {:?}", p.blobs().head("t/x").await.unwrap()));
        p.queues()
            .send(QueueName::Index, &serde_json::json!({ "op": "embed" }), 0)
            .await
            .unwrap();
        let ai: Result<serde_json::Value, _> = p
            .ai()
            .run("@cf/baai/bge-m3", &1, &AiOptions::default())
            .await;
        seen.push(format!("ai {}", ai.is_err()));
        let v = VectorRecord {
            id: "c1".into(),
            namespace: "idn".into(),
            values: vec![1.0],
            metadata: Default::default(),
        };
        p.vectors().upsert(&[v]).await.unwrap();
        seen.push(format!(
            "vectors {}",
            p.vectors().describe().await.unwrap().vector_count
        ));
        seen.push(format!(
            "mail {}",
            p.mail()
                .send(&StructuredEmail::default(), 30_000)
                .await
                .is_ok()
        ));
        seen.push(format!(
            "rl {}",
            p.limits().allow(RlBucket::Api, "key_1").await.unwrap()
        ));
        let a = p
            .dns()
            .query(Resolver::First, "example.test", RType::Txt)
            .await
            .unwrap();
        seen.push(format!("dns {:?}", a.status));
        seen.push(format!(
            "http {:?}",
            p.http()
                .send(HttpRequest::get("https://x.test/"))
                .await
                .is_err()
        ));
        seen.push(format!(
            "tcp {}",
            p.tcp()
                .connect("relay.test", 465, TlsMode::Implicit, 1_000)
                .await
                .is_err()
        ));
        p.metrics().write(DataPoint {
            indexes: ["ten_1".into()],
            blobs: vec![],
            doubles: vec![1.0],
        });
        p.background().wait_until(async {});
        seen.push(format!("config {}", p.config().platform_domain));
        seen
    }

    #[test]
    fn every_trait_runs_on_the_fakes() {
        let p = pylota_identity_platform::fakes::FakePlatform::new();
        let seen = pylota_identity_platform::fakes::block_on(touch_everything(&p));
        assert_eq!(seen[0], "req_@1791540000000");
        assert_eq!(
            seen[1..],
            [
                "db 1",
                "do 64",
                "r2 Some(3)",
                "ai true",
                "vectors 1",
                "mail true",
                "rl true",
                "dns NxDomain",
                "http true",
                "tcp true",
                "config pylotamail.test",
            ]
        );
        assert_eq!(p.queues.sent().len(), 1);
        assert_eq!(p.metrics.points().len(), 1);
        assert_eq!(p.background.pending(), 1);
    }
}

#[cfg(test)]
mod tests {
    /// The M0 Worker answers `200` (build plan M0).
    #[test]
    fn hello_answers_200() {
        let reply = super::App::hello();
        assert_eq!(reply.status, 200);
        assert_eq!(reply.body, b"hello\n");
    }

    /// Native builds get the build-time defaults from `build.rs`.
    #[test]
    fn build_values_default_natively() {
        assert!(!super::BUILD_VERSION.is_empty());
        assert!(!super::BUILD_COMMIT.is_empty());
        assert_eq!(super::BUILD_DATE.len(), "1970-01-01".len());
    }
}
