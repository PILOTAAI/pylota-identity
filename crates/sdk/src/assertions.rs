//! Verifying an agent assertion ([Agent signing keys §4.3]), for a service that receives one. It needs
//! no API key.
//!
//! ```rust,no_run
//! use pylota_identity::assertions::{Verifier, VerifierConfig};
//!
//! # async fn run(token: &str) -> Result<(), pylota_identity::assertions::AssertionError> {
//! let verifier = Verifier::new(VerifierConfig::new(
//!     ["https://api.pylotamail.com"],
//!     "https://portal.supplier.example",
//! ));
//! let agent = verifier.verify_assertion(token).await?;
//! println!("{} ({})", agent.email, agent.sub);
//! # Ok(())
//! # }
//! ```
//!
//! [Agent signing keys §4.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use serde_json::Value;

use crate::Error;

const TYP: &str = "agent-assertion+jwt";
const JWKS_TTL: Duration = Duration::from_secs(300);

pub struct VerifierConfig {
    /// The issuers to trust, e.g. `https://api.pylotamail.com`; never taken from the token.
    pub trusted_issuers: Vec<String>,
    /// Must equal the token's `aud`.
    pub audience: String,
    /// Clock skew allowed for `nbf` and `exp`.
    pub leeway: Duration,
}

impl VerifierConfig {
    /// A configuration with the default 60-second leeway.
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(issuers: I, audience: &str) -> Self {
        Self {
            trusted_issuers: issuers.into_iter().map(Into::into).collect(),
            audience: audience.to_owned(),
            leeway: Duration::from_secs(60),
        }
    }
}

/// Where seen `jti` values are kept until their `exp`; the default is in memory, per verifier.
pub trait ReplayStore: Send + Sync {
    /// Records `jti` until `exp` (Unix seconds); `false` when it was already recorded.
    fn record(&self, jti: &str, exp: i64, now: i64) -> bool;
}

#[derive(Default)]
struct MemoryReplay(Mutex<HashMap<String, i64>>);

impl ReplayStore for MemoryReplay {
    fn record(&self, jti: &str, exp: i64, now: i64) -> bool {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        m.retain(|_, e| *e >= now);
        m.insert(jti.to_owned(), exp).is_none()
    }
}

/// The claims of a verified assertion (Agent signing keys §4.2). Only `iss`, `sub`, `email` and
/// `email_verified` are vouched for by the issuer; `unverified` is what the workspace says.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AgentAssertion {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub jti: String,
    pub email: String,
    pub email_verified: bool,
    #[serde(default)]
    pub unverified: Value,
    #[serde(default)]
    pub ai_agent: bool,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub ext: Option<Value>,
}

#[derive(Debug)]
pub enum AssertionError {
    Malformed,
    Algorithm,
    UntrustedIssuer,
    UnknownKey,
    BadSignature,
    Audience,
    NotYetValid,
    Expired,
    Replayed,
    Jwks(Error),
}

impl std::fmt::Display for AssertionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssertionError::Jwks(e) => write!(f, "the key set could not be fetched: {e:?}"),
            other => write!(f, "{other:?}"),
        }
    }
}

impl std::error::Error for AssertionError {}

pub struct Verifier {
    config: VerifierConfig,
    http: reqwest::Client,
    jwks: Mutex<HashMap<String, (Instant, Vec<Value>)>>,
    replay: Box<dyn ReplayStore>,
}

/// `^idn_[0-9A-HJKMNP-TV-Z]{26}$`: checked before `sub` goes into a URL (O27).
fn identity_id(s: &str) -> bool {
    s.strip_prefix("idn_").is_some_and(|u| {
        u.len() == 26
            && u.bytes()
                .all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b)))
    })
}

fn b64(s: &str) -> Result<Vec<u8>, AssertionError> {
    URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| AssertionError::Malformed)
}

impl Verifier {
    pub fn new(config: VerifierConfig) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            config,
            http,
            jwks: Mutex::new(HashMap::new()),
            replay: Box::new(MemoryReplay::default()),
        }
    }

    /// Fetches key sets with this client (a proxy, a private CA). It must not follow redirects:
    /// build it with `redirect(reqwest::redirect::Policy::none())`.
    pub fn with_http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    /// Shares seen `jti` values across processes.
    pub fn with_replay_store(mut self, store: Box<dyn ReplayStore>) -> Self {
        self.replay = store;
        self
    }

    /// Verifies `token` against the clock.
    pub async fn verify_assertion(&self, token: &str) -> Result<AgentAssertion, AssertionError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        self.verify_assertion_at(token, now).await
    }

    /// Verifies `token` as at `now` (Unix seconds), in the order of §4.3.
    pub async fn verify_assertion_at(
        &self,
        token: &str,
        now: i64,
    ) -> Result<AgentAssertion, AssertionError> {
        let parts: Vec<&str> = token.split('.').collect();
        let [h, p, s] = parts.as_slice() else {
            return Err(AssertionError::Malformed);
        };
        // 1. The header.
        let header: Value =
            serde_json::from_slice(&b64(h)?).map_err(|_| AssertionError::Malformed)?;
        if header["alg"] != "EdDSA" || header["typ"] != TYP {
            return Err(AssertionError::Algorithm);
        }
        let kid = header["kid"].as_str().ok_or(AssertionError::Malformed)?;
        let claims: Value =
            serde_json::from_slice(&b64(p)?).map_err(|_| AssertionError::Malformed)?;
        // 2. The issuer, from the configuration only.
        let iss = claims["iss"].as_str().ok_or(AssertionError::Malformed)?;
        let Some(iss) = self.config.trusted_issuers.iter().find(|t| *t == iss) else {
            return Err(AssertionError::UntrustedIssuer);
        };
        // 3. `sub` before it reaches a URL.
        let sub = claims["sub"].as_str().ok_or(AssertionError::Malformed)?;
        if !identity_id(sub) {
            return Err(AssertionError::Malformed);
        }
        // 4. The key, refetched once on an unknown kid.
        let mut key = self.key(iss, sub, kid, false).await?;
        if key.is_none() {
            key = self.key(iss, sub, kid, true).await?;
        }
        let x = key.ok_or(AssertionError::UnknownKey)?;
        // 5. The signature.
        let sig: [u8; 64] = b64(s)?.try_into().map_err(|_| AssertionError::Malformed)?;
        let vk = VerifyingKey::from_bytes(&x).map_err(|_| AssertionError::BadSignature)?;
        vk.verify_strict(format!("{h}.{p}").as_bytes(), &Signature::from_bytes(&sig))
            .map_err(|_| AssertionError::BadSignature)?;
        // 6. Audience and time.
        let a: AgentAssertion =
            serde_json::from_value(claims).map_err(|_| AssertionError::Malformed)?;
        if a.aud != self.config.audience {
            return Err(AssertionError::Audience);
        }
        let leeway = self.config.leeway.as_secs() as i64;
        if a.nbf > now + leeway {
            return Err(AssertionError::NotYetValid);
        }
        if a.exp + leeway <= now {
            return Err(AssertionError::Expired);
        }
        // 7. Replay.
        if !self.replay.record(&a.jti, a.exp + leeway, now) {
            return Err(AssertionError::Replayed);
        }
        Ok(a)
    }

    /// The `x` of `kid` in the identity's key set, from the cache unless `refresh`.
    async fn key(
        &self,
        iss: &str,
        sub: &str,
        kid: &str,
        refresh: bool,
    ) -> Result<Option<[u8; 32]>, AssertionError> {
        let url = format!("{}/.well-known/jwks/{sub}.json", iss.trim_end_matches('/'));
        let cached = (!refresh)
            .then(|| {
                let m = self.jwks.lock().unwrap_or_else(|e| e.into_inner());
                m.get(&url)
                    .filter(|(at, _)| at.elapsed() < JWKS_TTL)
                    .map(|(_, k)| k.clone())
            })
            .flatten();
        let keys = match cached {
            Some(k) => k,
            None => {
                let k = self.fetch(&url).await?;
                self.jwks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(url, (Instant::now(), k.clone()));
                k
            }
        };
        Ok(keys
            .iter()
            .find(|k| k["kid"] == kid && k["kty"] == "OKP" && k["crv"] == "Ed25519")
            .and_then(|k| k["x"].as_str())
            .and_then(|x| URL_SAFE_NO_PAD.decode(x).ok())
            .and_then(|x| x.try_into().ok()))
    }

    /// A `200` of `application/jwk-set+json`, no redirect followed.
    async fn fetch(&self, url: &str) -> Result<Vec<Value>, AssertionError> {
        let jwks = |e: Error| AssertionError::Jwks(e);
        let res = self.http.get(url).send().await.map_err(|e| {
            jwks(Error::Transport {
                message: e.to_string(),
                idempotency_key: None,
            })
        })?;
        let status = res.status().as_u16();
        let media = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(|v| v.trim().to_ascii_lowercase());
        let body = res.bytes().await.map_err(|e| {
            jwks(Error::Transport {
                message: e.to_string(),
                idempotency_key: None,
            })
        })?;
        if status != 200 {
            return Err(jwks(Error::from_response(status, &body)));
        }
        if media.as_deref() != Some("application/jwk-set+json") {
            return Err(jwks(Error::Decode(format!(
                "the key set's content type is {}",
                media.as_deref().unwrap_or("missing")
            ))));
        }
        let v: Value =
            serde_json::from_slice(&body).map_err(|e| jwks(Error::Decode(e.to_string())))?;
        v["keys"]
            .as_array()
            .cloned()
            .ok_or_else(|| jwks(Error::Decode("no keys array".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ids_only() {
        assert!(identity_id("idn_01J9ZQ3V5K8M2N4P6R7S9T0VWX"));
        for bad in [
            "../../v1/links/abc?",
            "idn_01J9ZQ3V5K8M2N4P6R7S9T0VW",
            "idn_01J9ZQ3V5K8M2N4P6R7S9T0VWI",
            "idn_01j9zq3v5k8m2n4p6r7s9t0vwx",
            "idn_01J9ZQ3V5K8M2N4P6R7S9/../X",
        ] {
            assert!(!identity_id(bad), "{bad}");
        }
    }

    #[test]
    fn memory_replay() {
        let r = MemoryReplay::default();
        assert!(r.record("a", 100, 0));
        assert!(!r.record("a", 100, 50));
        assert!(r.record("a", 300, 200));
    }

    /// No request is made for a token whose `sub` is not an identity ID, or whose `alg` is `none`.
    #[tokio::test]
    async fn refuses_before_fetching() {
        let v = Verifier::new(VerifierConfig::new(["https://issuer.invalid"], "aud"));
        let enc = |j: &str| URL_SAFE_NO_PAD.encode(j);
        let h = enc(r#"{"alg":"EdDSA","typ":"agent-assertion+jwt","kid":"k"}"#);
        let p = enc(r#"{"iss":"https://issuer.invalid","sub":"../../v1/links/x?"}"#);
        assert!(matches!(
            v.verify_assertion_at(&format!("{h}.{p}.AA"), 0).await,
            Err(AssertionError::Malformed)
        ));
        let none = enc(r#"{"alg":"none","typ":"agent-assertion+jwt"}"#);
        assert!(matches!(
            v.verify_assertion_at(&format!("{none}.{p}."), 0).await,
            Err(AssertionError::Algorithm)
        ));
        let other =
            enc(r#"{"iss":"https://elsewhere.invalid","sub":"idn_01J9ZQ3V5K8M2N4P6R7S9T0VWX"}"#);
        assert!(matches!(
            v.verify_assertion_at(&format!("{h}.{other}.AA"), 0).await,
            Err(AssertionError::UntrustedIssuer)
        ));
    }
}
