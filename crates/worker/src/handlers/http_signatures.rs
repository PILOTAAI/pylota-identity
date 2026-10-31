//! Signed HTTP requests, Web Bot Auth ([Agent signing keys § 5]): RFC 9421 headers for the agent's own
//! HTTP client, signed with the deployment's active `web_bot_auth` key and naming the identity's vouched
//! address in a signed `From`. The Worker never makes the request; nothing is stored or logged.
//!
//! [Agent signing keys § 5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::rfc3339;
use pylota_identity_core::httpsig::{self, Param};
use pylota_identity_core::jwt;
use pylota_identity_core::policy::get_path;
use pylota_identity_platform::{Platform, Rng as _};
use serde_json::{Value, json};

use super::assertions::{record_usage, seconds, signer_identity, vouched_address};
use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;
use crate::keyring::web_bot_auth;
use crate::quota::UsageMetric;

/// Always signed, in this order.
const ALWAYS: &[&str] = &["@authority", "signature-agent", "from"];
const OPTIONAL: &[&str] = &["@method", "@path", "@query"];

/// `422 web_bot_auth_disabled` while `PM_WEB_BOT_AUTH=off` (O9).
pub fn enabled<P: Platform>(p: &P) -> Result<(), ApiError> {
    if p.config().web_bot_auth {
        Ok(())
    } else {
        Err(ApiError::new(
            ErrorCode::WebBotAuthDisabled,
            "Signed HTTP requests are off on this deployment (PM_WEB_BOT_AUTH).",
        ))
    }
}

/// A base64 nonce of 64 random bytes.
pub fn nonce<P: Platform>(p: &P) -> String {
    httpsig::base64(&p.rng().bytes::<64>())
}

fn components(body: &Value) -> Result<Vec<&'static str>, ApiError> {
    let mut out: Vec<&'static str> = ALWAYS.to_vec();
    let Some(list) = body.get("components").filter(|v| !v.is_null()) else {
        return Ok(out);
    };
    let bad = || {
        ApiError::invalid(
            "components",
            "components are @authority, signature-agent, from, @method, @path and @query.",
        )
    };
    for v in list.as_array().ok_or_else(bad)? {
        let name = v.as_str().ok_or_else(bad)?;
        let known = ALWAYS
            .iter()
            .chain(OPTIONAL)
            .find(|c| **c == name)
            .ok_or_else(bad)?;
        if !out.contains(known) {
            out.push(known);
        }
    }
    Ok(out)
}

/// `POST /v1/identities/{identity_id}/http-signatures` (`identities:sign`, `RL_SIGN`).
pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let p = c.p.as_ref();
    let (idn, t) = signer_identity(c).await?;
    enabled(p)?;
    let policy = crate::policy::effective(p, &t);
    if get_path(&policy, "web_bot_auth.allowed") != Some(&Value::Bool(true)) {
        return Err(ApiError::new(
            ErrorCode::PolicyDenied,
            "The tenant is not opted in to signed HTTP requests (policy web_bot_auth.allowed).",
        )
        .details(json!({ "policy": "web_bot_auth.allowed" })));
    }
    let body = c.json()?;
    let url = match body.get("url").and_then(Value::as_str) {
        Some(u) if u.len() <= 2048 => u,
        _ => {
            return Err(ApiError::invalid(
                "url",
                "url is an https URL of at most 2,048 characters.",
            ));
        }
    };
    let target = httpsig::https_target(url)
        .map_err(|_| ApiError::invalid("url", "url is an https URL with an ASCII or IDN host."))?;
    let expires_in = seconds(&body, "expires_in", 30, 300, 60)?;
    let covered = components(&body)?;
    let method = match body.get("method") {
        None | Some(Value::Null) => None,
        Some(Value::String(m))
            if !m.is_empty() && m.bytes().all(|b| b.is_ascii_uppercase() || b == b'-') =>
        {
            Some(m.clone())
        }
        Some(_) => {
            return Err(ApiError::invalid(
                "method",
                "method is an upper-case token.",
            ));
        }
    };
    if covered.contains(&"@method") && method.is_none() {
        return Err(ApiError::invalid(
            "method",
            "method is required when @method is signed.",
        ));
    }
    let agent = httpsig::sf_string(
        &format!("https://{}", p.config().api_host),
        "signature-agent",
    )
    .map_err(|_| ApiError::internal())?;
    let from = vouched_address(p, &idn.id).await?;
    let mut values: Vec<(&str, String)> = Vec::new();
    for name in &covered {
        let v = match *name {
            "@authority" => target.authority.clone(),
            "signature-agent" => agent.clone(),
            "from" => from.clone(),
            "@method" => method.clone().unwrap_or_default(),
            "@path" => target.path.clone(),
            _ => target.query.clone(),
        };
        values.push((name, v));
    }
    let key = web_bot_auth::keys(p).await?.swap_remove(0);
    let created = c.now.div_euclid(1000);
    let expires = created + expires_in;
    let params = [
        Param::Created(created as u64),
        Param::Expires(expires as u64),
        Param::KeyId(key.kid.clone()),
        Param::Alg("ed25519".into()),
        Param::Nonce(nonce(p)),
        Param::Tag("web-bot-auth".into()),
    ];
    let pairs: Vec<(&str, &str)> = values.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let base = httpsig::signature_base(&pairs, &params).map_err(|e| match e {
        httpsig::Error::NonAsciiValue(name) => ApiError::invalid(
            "components",
            format!("The value of {name} is not ASCII, which RFC 9421 refuses."),
        )
        .details(json!({ "field": "components", "reason": "non_ascii", "component": name })),
        _ => ApiError::invalid("url", "The URL cannot be signed."),
    })?;
    let names: Vec<&str> = covered.clone();
    let input = httpsig::signature_params(&names, &params).map_err(|_| ApiError::internal())?;
    let sig = jwt::ed25519(&key.seed, base.as_bytes());
    record_usage(c, &t, UsageMetric::HttpSignatures);
    Ok(Reply::json(
        200,
        json!({
            "headers": {
                "Signature-Agent": agent,
                "From": from,
                "Signature-Input": format!("sig1={input}"),
                "Signature": format!("sig1={}", httpsig::sf_bytes(&sig)),
            },
            "expires_at": rfc3339(expires * 1000),
        }),
    ))
}
