//! `GuardedHttp` ([Security § 9]): every request to a host a user chose goes through here. The URL rules
//! and the resolved addresses are checked by `core::ssrf` before each request, resolution runs through
//! the configured DoH resolvers, redirects are never followed, and only 4 KB of the answer is read.
//!
//! [Security § 9]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use std::net::IpAddr;

use pylota_identity_core::ssrf::{self, Refusal};
use pylota_identity_platform::dns::{DnsStatus, RType, Resolver};
use pylota_identity_platform::http::{HttpError, HttpRequest, HttpResponse, Redirect};
use pylota_identity_platform::{Dns as _, HttpClient as _, Platform};

/// Webhook deadline per attempt.
pub const TIMEOUT_MS: u32 = 15_000;
/// Response bytes read.
pub const MAX_BODY_BYTES: usize = 4096;

/// How a guarded request ended without an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuardError {
    /// The URL failed rules 1–4.
    InvalidUrl(Refusal),
    /// Resolution refused (no address, a blocked address).
    Blocked(Refusal),
    /// Neither resolver answered.
    Dns,
    Http(HttpError),
}

/// The names refused for this deployment besides the fixed ones (rule 3).
pub fn refused_hosts<P: Platform>(p: &P) -> Vec<String> {
    let c = p.config();
    vec![
        c.api_host.clone(),
        c.console_host.clone(),
        c.platform_domain.clone(),
    ]
}

/// Rules 1–4 at create and update.
pub fn check_url<P: Platform>(p: &P, url: &str) -> Result<ssrf::Target, Refusal> {
    let refused = refused_hosts(p);
    let refs: Vec<&str> = refused.iter().map(String::as_str).collect();
    ssrf::check_url(url, &refs)
}

async fn resolve<P: Platform>(p: &P, host: &str) -> Result<Vec<IpAddr>, GuardError> {
    let mut out = Vec::new();
    for rtype in [RType::A, RType::Aaaa] {
        let mut answered = false;
        for r in [Resolver::First, Resolver::Second] {
            match p.dns().query(r, host, rtype).await {
                Ok(a) if matches!(a.status, DnsStatus::NoError | DnsStatus::NxDomain) => {
                    out.extend(
                        a.records
                            .iter()
                            .filter(|rec| rec.rtype == rtype)
                            .filter_map(|rec| rec.data.parse::<IpAddr>().ok()),
                    );
                    answered = true;
                    break;
                }
                _ => {}
            }
        }
        if !answered {
            return Err(GuardError::Dns);
        }
    }
    Ok(out)
}

/// One guarded `POST`: rules 1–5, then the request with `redirect: manual`, the deadline and the body
/// cap. A `3xx` comes back as a response; the caller treats it as a failure.
pub async fn post<P: Platform>(
    p: &P,
    url: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> Result<HttpResponse, GuardError> {
    let target = check_url(p, url).map_err(GuardError::InvalidUrl)?;
    let addresses = resolve(p, &target.host).await?;
    ssrf::check_addresses(&addresses).map_err(GuardError::Blocked)?;
    let req = HttpRequest {
        method: "POST",
        url: url.to_owned(),
        headers,
        body: Some(body),
        timeout_ms: TIMEOUT_MS,
        redirect: Redirect::Manual,
        max_body_bytes: MAX_BODY_BYTES,
    };
    p.http().send(req).await.map_err(GuardError::Http)
}
