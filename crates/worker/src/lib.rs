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
