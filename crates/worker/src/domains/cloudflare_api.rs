//! The Cloudflare API ([Outbound › Provider suppressions and resending], G4): the Email Sending
//! suppression lookup, `GET /accounts/{PM_CF_ACCOUNT_ID}/email/sending/suppressions?email={address}`
//! with `PM_CF_API_TOKEN`. M13 adds the zone, routing and sending calls.
//!
//! [Outbound › Provider suppressions and resending]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_platform::http::HttpRequest;
use pylota_identity_platform::{HttpClient as _, Platform};
use serde_json::Value;

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
