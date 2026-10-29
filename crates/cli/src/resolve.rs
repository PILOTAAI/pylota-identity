//! Names to IDs ([CLI and setup §2.4]): an identity by its address, a tenant by its slug, a domain by
//! its name; and the defaults when none is given.
//!
//! [CLI and setup §2.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#24-resolving-names-to-ids

use serde_json::Value;

use crate::config;
use crate::http::seg;
use crate::{CliError, Ctx};

/// `GET /v1/me`, called at most once per invocation.
pub async fn me(ctx: &Ctx) -> Result<Value, CliError> {
    if let Some(m) = ctx.me.borrow().clone() {
        return Ok(m);
    }
    let m = ctx.api()?.get(ctx, "/v1/me", &[]).await?;
    *ctx.me.borrow_mut() = Some(m.clone());
    Ok(m)
}

/// The key's level.
pub async fn level(ctx: &Ctx) -> Result<String, CliError> {
    Ok(me(ctx).await?["level"].as_str().unwrap_or("").to_owned())
}

/// An address in lookup form: the domain as an A-label.
fn ascii_address(a: &str) -> String {
    match a.rsplit_once('@') {
        Some((local, domain)) => {
            let d = pylota_identity_core::address::normalise_domain(domain)
                .unwrap_or_else(|| domain.to_ascii_lowercase());
            format!("{local}@{d}")
        }
        None => a.to_owned(),
    }
}

/// An identity argument (`idn_…` or an address) as an ID.
pub async fn identity_arg(ctx: &Ctx, arg: &str) -> Result<String, CliError> {
    if arg.starts_with("idn_") {
        return Ok(arg.to_owned());
    }
    if !arg.contains('@') {
        return Err(CliError::usage(format!(
            "{arg} is neither an identity ID nor an address."
        )));
    }
    let r = ctx
        .api()?
        .get(
            ctx,
            "/v1/identities/lookup",
            &[("address", ascii_address(arg))],
        )
        .await?;
    r["id"]
        .as_str()
        .or_else(|| r["identity_id"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| CliError::internal("the lookup answered without an id"))
}

/// The identity of a mail command: `--identity`, else the profile's, else an identity key's own.
pub async fn identity(ctx: &Ctx) -> Result<String, CliError> {
    let resolved = config::resolve(ctx)?;
    if let Some(i) = resolved.identity {
        return identity_arg(ctx, &i.value).await;
    }
    let m = me(ctx).await?;
    if let (true, Some(i)) = (m["level"] == "identity", m["identity_id"].as_str()) {
        return Ok(i.to_owned());
    }
    Err(CliError::usage("Pass --identity."))
}

/// A tenant argument (`ten_…` or a slug) as an ID.
pub async fn tenant_arg(ctx: &Ctx, arg: &str) -> Result<String, CliError> {
    if arg.starts_with("ten_") {
        return Ok(arg.to_owned());
    }
    let m = me(ctx).await?;
    let api = ctx.api()?;
    if matches!(m["level"].as_str(), Some("tenant" | "identity")) {
        let own = m["tenant_id"].as_str().unwrap_or("");
        let t = api
            .get(ctx, &format!("/v1/tenants/{}", seg(own)), &[])
            .await?;
        if t["slug"] == arg {
            return Ok(own.to_owned());
        }
        return Err(CliError::usage(format!(
            "This key may only name its own tenant, not {arg}."
        )));
    }
    let all = api
        .list(ctx, "/v1/tenants", &[("limit", "100".into())], true)
        .await?;
    all["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["slug"] == arg)
        .and_then(|t| t["id"].as_str().map(str::to_owned))
        .ok_or_else(|| CliError::usage(format!("No tenant with the slug {arg}.")))
}

/// The tenant of a tenant-scoped command: `--tenant`, else a tenant or identity key's own, else the
/// profile's, else (platform keys only) the default tenant, the one with an empty `address_suffix`.
pub async fn tenant(ctx: &Ctx) -> Result<String, CliError> {
    let resolved = config::resolve(ctx)?;
    if let Some(t) = resolved.tenant.filter(|t| t.source == config::Source::Flag) {
        return tenant_arg(ctx, &t.value).await;
    }
    let m = me(ctx).await?;
    if let (Some("tenant" | "identity"), Some(t)) = (m["level"].as_str(), m["tenant_id"].as_str()) {
        return Ok(t.to_owned());
    }
    if let Some(t) = config::resolve(ctx)?.tenant {
        return tenant_arg(ctx, &t.value).await;
    }
    match m["level"].as_str() {
        Some("platform") => default_tenant(ctx).await,
        _ => Err(CliError::usage("Pass --tenant.")),
    }
}

/// The default tenant: the one tenant whose addresses have no suffix.
pub async fn default_tenant(ctx: &Ctx) -> Result<String, CliError> {
    let all = ctx
        .api()?
        .list(ctx, "/v1/tenants", &[("limit", "100".into())], true)
        .await?;
    all["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["address_suffix"].as_str() == Some(""))
        .and_then(|t| t["id"].as_str().map(str::to_owned))
        .ok_or_else(|| CliError::usage("No default tenant exists; pass --tenant."))
}

/// A domain argument (`dom_…` or a name) as an ID, by listing the tenant's domains.
pub async fn domain_arg(ctx: &Ctx, arg: &str) -> Result<String, CliError> {
    if arg.starts_with("dom_") {
        return Ok(arg.to_owned());
    }
    let name = pylota_identity_core::address::normalise_domain(arg).ok_or_else(|| {
        CliError::usage(format!("{arg} is neither a domain ID nor a domain name."))
    })?;
    let t = tenant(ctx).await?;
    let all = ctx
        .api()?
        .list(
            ctx,
            &format!("/v1/tenants/{}/domains", seg(&t)),
            &[("limit", "100".into())],
            true,
        )
        .await?;
    all["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|d| d["name"] == name.as_str())
        .and_then(|d| d["id"].as_str().map(str::to_owned))
        .ok_or_else(|| CliError::usage(format!("No domain {name} in tenant {t}.")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn idn_addresses_become_a_labels() {
        assert_eq!(
            super::ascii_address("bookings@bücher.example"),
            "bookings@xn--bcher-kva.example"
        );
        assert_eq!(super::ascii_address("a@B.example"), "a@b.example");
    }
}
