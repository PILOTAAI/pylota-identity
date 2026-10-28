//! The SES v2 control-plane calls ([Identities and domains › Kind `external`], [Domains on any DNS host
//! § 4.8]): `CreateEmailIdentity`, `GetEmailIdentity` and `DeleteEmailIdentity`, signed with SigV4, each
//! behind a token of `SesControl`. A throttled call acquires again after 2 seconds, at most three times.
//!
//! [Identities and domains › Kind `external`]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md
//! [Domains on any DNS host § 4.8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/domain-connections.md

use pylota_identity_core::ses::{Credentials, Request, sha256_hex, sigv4};
use pylota_identity_platform::http::HttpRequest;
use pylota_identity_platform::{Clock as _, HttpClient as _, Platform};
use serde_json::{Value, json};

use super::ses_control::{SesCaller, acquire};

/// Tag keys that mark an identity as this deployment's, for one domain.
pub const TAG_HOST: &str = "pylota-identity:api-host";
pub const TAG_DOMAIN: &str = "pylota-identity:domain-id";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SesError {
    /// The token bucket stayed busy past the caller's deadline.
    Busy {
        retry_after_ms: i64,
    },
    AlreadyExists,
    NotFound,
    /// Any other failure (status `0`: no response).
    Failed {
        status: u16,
    },
}

fn error_type(status: u16, body: &Value, raw: &[u8]) -> String {
    let t = body["__type"]
        .as_str()
        .or(body["code"].as_str())
        .or(body["Code"].as_str())
        .unwrap_or("");
    if !t.is_empty() {
        return t.rsplit('#').next().unwrap_or(t).to_owned();
    }
    let text = String::from_utf8_lossy(raw);
    for k in [
        "AlreadyExistsException",
        "NotFoundException",
        "ThrottlingException",
        "TooManyRequestsException",
    ] {
        if text.contains(k) {
            return k.to_owned();
        }
    }
    if status == 404 {
        "NotFoundException".into()
    } else {
        String::new()
    }
}

/// The SES client of the configured region; `None` when SES is not configured.
pub struct Ses<'a, P: Platform> {
    p: &'a P,
    region: String,
    caller: SesCaller,
}

impl<'a, P: Platform> Ses<'a, P> {
    pub fn new(p: &'a P, caller: SesCaller) -> Option<Self> {
        p.secrets().ses.as_ref()?;
        Some(Self {
            region: p.config().ses_region.clone()?,
            p,
            caller,
        })
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    async fn send(
        &self,
        method: &'static str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, SesError> {
        let creds = self
            .p
            .secrets()
            .ses
            .clone()
            .ok_or(SesError::Failed { status: 0 })?;
        let host = format!("email.{}.amazonaws.com", self.region);
        let payload = body.map(Value::to_string).unwrap_or_default();
        let hash = sha256_hex(payload.as_bytes());
        for attempt in 0..4 {
            acquire(self.p, self.caller)
                .await
                .map_err(|ms| SesError::Busy { retry_after_ms: ms })?;
            let signed = sigv4(
                &Request {
                    method,
                    host: &host,
                    path,
                    query: "",
                    headers: &[("content-type", "application/json")],
                    payload_sha256: &hash,
                },
                &Credentials {
                    key_id: &creds.access_key_id,
                    secret: &creds.secret_access_key,
                    session_token: None,
                },
                &self.region,
                "ses",
                self.p.clock().now_ms() / 1000,
            );
            let mut req = HttpRequest::get(format!("https://{host}{path}"));
            req.method = method;
            req.timeout_ms = 10_000;
            req.headers = vec![
                ("content-type".into(), "application/json".into()),
                ("x-amz-date".into(), signed.x_amz_date.clone()),
                ("x-amz-content-sha256".into(), hash.clone()),
                ("authorization".into(), signed.authorization.clone()),
            ];
            if body.is_some() {
                req.body = Some(payload.clone().into_bytes());
            }
            let r = match self.p.http().send(req).await {
                Ok(r) => r,
                Err(_) => return Err(SesError::Failed { status: 0 }),
            };
            let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            if (200..300).contains(&r.status) {
                return Ok(v);
            }
            match error_type(r.status, &v, &r.body).as_str() {
                "AlreadyExistsException" => return Err(SesError::AlreadyExists),
                "NotFoundException" => return Err(SesError::NotFound),
                "ThrottlingException" | "TooManyRequestsException" if attempt < 3 => {
                    crate::metrics::count(self.p, "ses_control_throttled_total", None, 1);
                    self.p.clock().sleep(2_000).await;
                }
                "ThrottlingException" | "TooManyRequestsException" => {
                    return Err(SesError::Busy {
                        retry_after_ms: 2_000,
                    });
                }
                _ => return Err(SesError::Failed { status: r.status }),
            }
        }
        Err(SesError::Failed { status: 0 })
    }

    /// `CreateEmailIdentity` with the two ownership tags.
    pub async fn create_identity(&self, name: &str, domain_id: &str) -> Result<Value, SesError> {
        let body = json!({
            "EmailIdentity": name,
            "Tags": [
                { "Key": TAG_HOST, "Value": self.p.config().api_host },
                { "Key": TAG_DOMAIN, "Value": domain_id },
            ],
        });
        self.send("POST", "/v2/email/identities", Some(&body)).await
    }

    pub async fn get_identity(&self, name: &str) -> Result<Value, SesError> {
        self.send("GET", &format!("/v2/email/identities/{name}"), None)
            .await
    }

    /// `NotFoundException` counts as done.
    pub async fn delete_identity(&self, name: &str) -> Result<(), SesError> {
        match self
            .send("DELETE", &format!("/v2/email/identities/{name}"), None)
            .await
        {
            Ok(_) | Err(SesError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Whether an identity's tags name this deployment and `domain_id`.
    pub fn ours(&self, identity: &Value, domain_id: &str) -> bool {
        let tag = |k: &str| {
            identity["Tags"].as_array().and_then(|a| {
                a.iter()
                    .find(|t| t["Key"] == k)
                    .and_then(|t| t["Value"].as_str())
            })
        };
        tag(TAG_HOST) == Some(self.p.config().api_host.as_str())
            && tag(TAG_DOMAIN) == Some(domain_id)
    }
}

/// The DKIM tokens and signing zone of a `GetEmailIdentity` or `CreateEmailIdentity` answer.
pub fn dkim(identity: &Value) -> (Vec<String>, String) {
    let d = &identity["DkimAttributes"];
    let tokens = d["Tokens"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let zone = d["SigningHostedZone"]
        .as_str()
        .or(identity["SigningHostedZone"].as_str())
        .unwrap_or("dkim.amazonses.com")
        .to_owned();
    (tokens, zone)
}
