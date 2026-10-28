//! The Cloudflare API ([Outbound › Provider suppressions and resending], G4): the Email Sending
//! suppression lookup, `GET /accounts/{PM_CF_ACCOUNT_ID}/email/sending/suppressions?email={address}`
//! with `PM_CF_API_TOKEN`. M13 adds the zone, routing and sending calls.
//!
//! [Outbound › Provider suppressions and resending]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_platform::http::HttpRequest;
use pylota_identity_platform::{HttpClient as _, Platform};
use serde_json::{Value, json};

/// One matching suppression of an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suppression {
    /// `account`, or the sending domain's name.
    pub scope: String,
    pub reason: String,
}

/// The suppressions of `address` that count for `sending_domain`: an account item or one scoped to
/// that domain. `None` when the lookup cannot be made (no token, an error).
pub async fn suppressions<P: Platform>(
    p: &P,
    address: &str,
    sending_domain: &str,
) -> Option<Vec<Suppression>> {
    let account = p.config().cf_account_id.clone()?;
    let token = p.secrets().cf_api_token.clone()?;
    let mut req = HttpRequest::get(format!(
        "https://api.cloudflare.com/client/v4/accounts/{account}/email/sending/suppressions?email={}",
        address.replace('+', "%2B").replace('@', "%40")
    ));
    req.timeout_ms = 10_000;
    req.headers
        .push(("authorization".into(), format!("Bearer {token}")));
    let r = p.http().send(req).await.ok()?;
    if !(200..300).contains(&r.status) {
        return None;
    }
    let v: Value = serde_json::from_slice(&r.body).ok()?;
    let items = v["result"].as_array()?;
    Some(
        items
            .iter()
            .filter_map(|i| {
                let scope = match i["scope"].as_str() {
                    Some("account") => "account".to_owned(),
                    _ => i["domain"]
                        .as_str()
                        .or(i["sending_domain"].as_str())?
                        .to_ascii_lowercase(),
                };
                (scope == "account" || scope == sending_domain).then(|| Suppression {
                    scope,
                    reason: i["reason"].as_str().unwrap_or("provider").to_owned(),
                })
            })
            .collect(),
    )
}

// ---- Zones, DNS records, Email Routing, Email Sending and event subscriptions (M13) ---------------

const BASE: &str = "https://api.cloudflare.com/client/v4";

/// A failed Cloudflare call: the HTTP status (`0` when no response came) and the error codes of the
/// response's `errors`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CfError {
    pub status: u16,
    pub codes: Vec<i64>,
}

impl CfError {
    /// `429`, a `5xx` or no response: the call may succeed later.
    pub fn transient(&self) -> bool {
        self.status == 0 || self.status == 429 || self.status >= 500
    }
}

/// A zone of the account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub status: String,
    pub name_servers: Vec<String>,
}

/// A DNS record of a zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub rtype: String,
    pub name: String,
    pub content: String,
    pub priority: Option<u64>,
}

fn record(v: &Value) -> Record {
    Record {
        id: v["id"].as_str().unwrap_or("").to_owned(),
        rtype: v["type"].as_str().unwrap_or("").to_owned(),
        name: v["name"].as_str().unwrap_or("").to_ascii_lowercase(),
        content: v["content"].as_str().unwrap_or("").to_owned(),
        priority: v["priority"].as_u64(),
    }
}

fn zone(v: &Value) -> Zone {
    Zone {
        id: v["id"].as_str().unwrap_or("").to_owned(),
        name: v["name"].as_str().unwrap_or("").to_ascii_lowercase(),
        status: v["status"].as_str().unwrap_or("").to_owned(),
        name_servers: v["name_servers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|n| n.as_str().map(|s| s.to_ascii_lowercase()))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Query-string encoding of one value.
fn q(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The Worker's Cloudflare API client: `PM_CF_API_TOKEN` and `PM_CF_ACCOUNT_ID`.
pub struct Cf<'a, P: Platform> {
    p: &'a P,
    token: String,
    pub account: String,
}

impl<'a, P: Platform> Cf<'a, P> {
    /// `None` without `PM_CF_API_TOKEN` (`422 cf_token_required`) or the account ID.
    pub fn new(p: &'a P) -> Option<Self> {
        Some(Self {
            token: p.secrets().cf_api_token.clone()?,
            account: p.config().cf_account_id.clone()?,
            p,
        })
    }

    /// One call; answers the response's `result`.
    pub async fn call(
        &self,
        method: &'static str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, CfError> {
        let mut req = HttpRequest::get(format!("{BASE}{path}"));
        req.method = method;
        req.timeout_ms = 15_000;
        req.headers
            .push(("authorization".into(), format!("Bearer {}", self.token)));
        if let Some(b) = body {
            req.headers
                .push(("content-type".into(), "application/json".into()));
            req.body = Some(b.to_string().into_bytes());
        }
        let r = self.p.http().send(req).await.map_err(|_| CfError {
            status: 0,
            codes: Vec::new(),
        })?;
        let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
        if (200..300).contains(&r.status) && v["success"] != false {
            return Ok(v["result"].clone());
        }
        Err(CfError {
            status: r.status,
            codes: v["errors"]
                .as_array()
                .map(|a| a.iter().filter_map(|e| e["code"].as_i64()).collect())
                .unwrap_or_default(),
        })
    }

    /// Zones of the account with exactly this name, any status.
    pub async fn zones_named(&self, name: &str) -> Result<Vec<Zone>, CfError> {
        let v = self
            .call(
                "GET",
                &format!("/zones?name={}&account.id={}", q(name), q(&self.account)),
                None,
            )
            .await?;
        Ok(v.as_array()
            .map(|a| a.iter().map(zone).collect())
            .unwrap_or_default())
    }

    /// The most specific zone of the account containing `name`: the name, then each parent label
    /// down to two labels.
    pub async fn find_zone(&self, name: &str) -> Result<Option<Zone>, CfError> {
        let labels: Vec<&str> = name.split('.').collect();
        for i in 0..labels.len().saturating_sub(1) {
            let candidate = labels[i..].join(".");
            if let Some(z) = self
                .zones_named(&candidate)
                .await?
                .into_iter()
                .find(|z| z.name == candidate)
            {
                return Ok(Some(z));
            }
        }
        Ok(None)
    }

    pub async fn get_zone(&self, id: &str) -> Result<Zone, CfError> {
        Ok(zone(
            &self.call("GET", &format!("/zones/{id}"), None).await?,
        ))
    }

    /// Records at `name`, of `rtype` when given.
    pub async fn records(
        &self,
        zone_id: &str,
        name: &str,
        rtype: Option<&str>,
    ) -> Result<Vec<Record>, CfError> {
        let mut path = format!("/zones/{zone_id}/dns_records?name.exact={}", q(name));
        if let Some(t) = rtype {
            path.push_str(&format!("&type={t}"));
        }
        let v = self.call("GET", &path, None).await?;
        Ok(v.as_array()
            .map(|a| a.iter().map(record).collect())
            .unwrap_or_default())
    }

    /// One record by ID; `None` when the read-back no longer finds it.
    pub async fn record(&self, zone_id: &str, id: &str) -> Result<Option<Record>, CfError> {
        match self
            .call("GET", &format!("/zones/{zone_id}/dns_records/{id}"), None)
            .await
        {
            Ok(v) => Ok(Some(record(&v))),
            Err(e) if !e.transient() => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn create_record(
        &self,
        zone_id: &str,
        rtype: &str,
        name: &str,
        content: &str,
    ) -> Result<Record, CfError> {
        let body = json!({ "type": rtype, "name": name, "content": content, "ttl": 1 });
        Ok(record(
            &self
                .call(
                    "POST",
                    &format!("/zones/{zone_id}/dns_records"),
                    Some(&body),
                )
                .await?,
        ))
    }

    pub async fn delete_record(&self, zone_id: &str, id: &str) -> Result<(), CfError> {
        self.call(
            "DELETE",
            &format!("/zones/{zone_id}/dns_records/{id}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Turns Email Routing on for `name` (`None` for the apex, whose name the API refuses: S9).
    pub async fn enable_routing(&self, zone_id: &str, name: Option<&str>) -> Result<(), CfError> {
        let body = name.map_or_else(|| json!({}), |n| json!({ "name": n }));
        self.call(
            "POST",
            &format!("/zones/{zone_id}/email/routing/dns"),
            Some(&body),
        )
        .await
        .map(|_| ())
    }

    /// The records Email Routing needs at `name` (`None`: the apex).
    pub async fn routing_dns(
        &self,
        zone_id: &str,
        name: Option<&str>,
    ) -> Result<Vec<Record>, CfError> {
        let path = match name {
            Some(n) => format!("/zones/{zone_id}/email/routing/dns?subdomain={}", q(n)),
            None => format!("/zones/{zone_id}/email/routing/dns"),
        };
        let v = self.call("GET", &path, None).await?;
        let list = v
            .as_array()
            .cloned()
            .or_else(|| v["record"].as_array().cloned())
            .unwrap_or_default();
        Ok(list.iter().map(record).collect())
    }

    pub async fn routing_settings(&self, zone_id: &str, body: &Value) -> Result<(), CfError> {
        self.call(
            "PATCH",
            &format!("/zones/{zone_id}/email/routing"),
            Some(body),
        )
        .await
        .map(|_| ())
    }

    /// Unlocks the MX records Email Routing locked at `name` (removal of one name's routing).
    pub async fn unlock_routing(&self, zone_id: &str, name: &str) -> Result<(), CfError> {
        self.call(
            "PATCH",
            &format!("/zones/{zone_id}/email/routing/dns"),
            Some(&json!({ "name": name })),
        )
        .await
        .map(|_| ())
    }

    /// Disables Email Routing for the whole zone (only for the zone's last routing domain, at its
    /// apex: H11).
    pub async fn disable_zone_routing(&self, zone_id: &str) -> Result<(), CfError> {
        self.call(
            "DELETE",
            &format!("/zones/{zone_id}/email/routing/dns"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn catch_all(&self, zone_id: &str) -> Result<Value, CfError> {
        self.call(
            "GET",
            &format!("/zones/{zone_id}/email/routing/rules/catch_all"),
            None,
        )
        .await
    }

    pub async fn put_catch_all(&self, zone_id: &str, enabled: bool) -> Result<(), CfError> {
        let body = json!({
            "actions": [{ "type": "worker", "value": [WORKER_SCRIPT] }],
            "matchers": [{ "type": "all" }], "enabled": enabled, "name": "pylota-identity",
        });
        self.call(
            "PUT",
            &format!("/zones/{zone_id}/email/routing/rules/catch_all"),
            Some(&body),
        )
        .await
        .map(|_| ())
    }

    /// A literal rule sending `address` to the Worker; answers the rule ID.
    pub async fn create_rule(
        &self,
        zone_id: &str,
        address: &str,
        address_id: &str,
    ) -> Result<String, CfError> {
        let body = json!({
            "matchers": [{ "type": "literal", "field": "to", "value": address }],
            "actions": [{ "type": "worker", "value": [WORKER_SCRIPT] }],
            "enabled": true, "name": format!("pylota-identity {address_id}"), "priority": 0,
        });
        let v = self
            .call(
                "POST",
                &format!("/zones/{zone_id}/email/routing/rules"),
                Some(&body),
            )
            .await?;
        Ok(v["id"]
            .as_str()
            .or(v["tag"].as_str())
            .unwrap_or("")
            .to_owned())
    }

    /// Whether the rule still exists (the read-back of C10).
    pub async fn rule_exists(&self, zone_id: &str, rule_id: &str) -> Result<bool, CfError> {
        match self
            .call(
                "GET",
                &format!("/zones/{zone_id}/email/routing/rules/{rule_id}"),
                None,
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if !e.transient() => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub async fn delete_rule(&self, zone_id: &str, rule_id: &str) -> Result<(), CfError> {
        self.call(
            "DELETE",
            &format!("/zones/{zone_id}/email/routing/rules/{rule_id}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// The zone's Email Sending domains: `(name, tag)`.
    pub async fn sending_domains(&self, zone_id: &str) -> Result<Vec<(String, String)>, CfError> {
        let v = self
            .call(
                "GET",
                &format!("/zones/{zone_id}/email/sending/subdomains"),
                None,
            )
            .await?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .map(|s| {
                        (
                            s["name"].as_str().unwrap_or("").to_ascii_lowercase(),
                            s["tag"].as_str().unwrap_or("").to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Creates the Email Sending domain; answers its `tag`.
    pub async fn create_sending(&self, zone_id: &str, name: &str) -> Result<String, CfError> {
        let v = self
            .call(
                "POST",
                &format!("/zones/{zone_id}/email/sending/subdomains"),
                Some(&json!({ "name": name })),
            )
            .await?;
        Ok(v["tag"].as_str().unwrap_or("").to_owned())
    }

    pub async fn patch_sending(
        &self,
        zone_id: &str,
        tag: &str,
        body: &Value,
    ) -> Result<(), CfError> {
        self.call(
            "PATCH",
            &format!("/zones/{zone_id}/email/sending/subdomains/{tag}"),
            Some(body),
        )
        .await
        .map(|_| ())
    }

    pub async fn sending_dns(&self, zone_id: &str, tag: &str) -> Result<Vec<Record>, CfError> {
        let v = self
            .call(
                "GET",
                &format!("/zones/{zone_id}/email/sending/subdomains/{tag}/dns"),
                None,
            )
            .await?;
        Ok(v.as_array()
            .map(|a| a.iter().map(record).collect())
            .unwrap_or_default())
    }

    pub async fn delete_sending(&self, zone_id: &str, tag: &str) -> Result<(), CfError> {
        self.call(
            "DELETE",
            &format!("/zones/{zone_id}/email/sending/subdomains/{tag}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// The queue ID of `name`.
    pub async fn queue_id(&self, name: &str) -> Result<Option<String>, CfError> {
        let v = self
            .call("GET", &format!("/accounts/{}/queues", self.account), None)
            .await?;
        Ok(v.as_array().and_then(|a| {
            a.iter()
                .find(|x| x["queue_name"] == name)
                .and_then(|x| x["queue_id"].as_str().map(str::to_owned))
        }))
    }

    /// The domain's event subscription to `pm-delivery-events`; answers its ID.
    pub async fn subscribe(
        &self,
        zone_id: &str,
        domain: &str,
        queue_id: &str,
    ) -> Result<String, CfError> {
        let body = json!({
            "name": format!("pylota-identity {domain}"), "enabled": true,
            "source": { "type": "email.sending", "zone_id": zone_id, "domain": domain },
            "destination": { "type": "queues.queue", "queue_id": queue_id },
            "events": ["message.delivered", "message.deferred", "message.bounced",
                       "message.failed", "message.rejected", "message.complained"],
        });
        let v = self
            .call(
                "POST",
                &format!(
                    "/accounts/{}/event_subscriptions/subscriptions",
                    self.account
                ),
                Some(&body),
            )
            .await?;
        Ok(v["id"].as_str().unwrap_or("").to_owned())
    }

    pub async fn unsubscribe(&self, id: &str) -> Result<(), CfError> {
        self.call(
            "DELETE",
            &format!(
                "/accounts/{}/event_subscriptions/subscriptions/{id}",
                self.account
            ),
            None,
        )
        .await
        .map(|_| ())
    }
}

/// The Worker's script name, the value of a `worker` routing action (S9).
pub const WORKER_SCRIPT: &str = "pylota-identity";
