//! `/health`, `/openapi.json`, `/.well-known/security.txt` and `/v1/me` (REST API › Meta, Security § 14).

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::Platform;
use serde_json::{Value, json};

use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;

pub fn health<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let cfg = c.p.config();
    let mut body = json!({
        "status": if cfg.is_degraded() { "degraded" } else { "ok" },
        "version": crate::BUILD_VERSION,
        "commit": crate::BUILD_COMMIT,
        "env": match cfg.env {
            pylota_identity_platform::config::DeployEnv::Production => "production",
            pylota_identity_platform::config::DeployEnv::Staging => "staging",
            pylota_identity_platform::config::DeployEnv::Local => "local",
        },
    });
    for (feature, reason) in &cfg.degraded {
        body[*feature] = Value::from(*reason);
    }
    if cfg.ses_transport()
        && let Some(r) = &cfg.ses_region
    {
        body["ses_region"] = Value::from(r.as_str());
    }
    if cfg.frozen {
        body["frozen"] = Value::Bool(true);
    }
    Ok(Reply::json(200, body))
}

pub fn openapi<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let mut doc = pylota_identity_api_types::openapi::openapi_json();
    doc["servers"] = json!([{ "url": format!("https://{}/v1", c.p.config().api_host) }]);
    Ok(Reply::json(200, doc))
}

/// The release build date plus 365 days.
fn expires() -> String {
    let built =
        pylota_identity_core::days::parse_rfc3339(&format!("{}T00:00:00Z", crate::BUILD_DATE))
            .unwrap_or(0);
    rfc3339(built + 365 * 86_400_000)
}

pub fn security_txt<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let cfg = c.p.config();
    let Some(contact) = &cfg.security_contact else {
        return Err(ApiError::new(
            ErrorCode::RouteNotFound,
            "No security contact is configured.",
        ));
    };
    let contact = if contact.contains(':') {
        contact.clone()
    } else {
        format!("mailto:{contact}")
    };
    let text = format!(
        "Contact: {contact}\nExpires: {}\nPreferred-Languages: en\nCanonical: https://{}/.well-known/security.txt\nPolicy: https://github.com/PILOTAAI/pylota-identity/security/policy\n",
        expires(),
        cfg.api_host
    );
    Ok(Reply {
        status: 200,
        body: None,
        raw: Some(("text/plain; charset=utf-8", text.into_bytes())),
        headers: Vec::new(),
    })
}

pub fn me<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let k = c.key();
    let mut permissions: Vec<&str> = k.permissions.iter().map(String::as_str).collect();
    permissions.sort_unstable();
    permissions.dedup();
    Ok(Reply::json(
        200,
        json!({
            "key_id": k.key_id, "name": k.name, "level": k.level.as_str(), "mode": k.mode.as_str(),
            "partner_id": k.partner_id, "tenant_id": k.tenant_id, "identity_id": k.identity_id,
            "permissions": permissions, "mcp_tools": k.mcp_tools, "expires_at": k.expires_at.map(rfc3339),
        }),
    ))
}
