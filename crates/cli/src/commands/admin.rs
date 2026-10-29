//! `tenants`, `partners`, `keys` and `audit` ([CLI and setup §18, §19]).
//!
//! [CLI and setup §18, §19]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#19-command-to-endpoint-map

use serde_json::{Value, json};

use super::{Obj, json_arg, paging, q_opt, time};
use crate::cli::{AuditArgs, KeysCmd, PartnersCmd, TenantsCmd};
use crate::http::{Idem, seg};
use crate::output::{Col, col};
use crate::{CliError, Ctx, Output, config, resolve};

const TENANT_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("SLUG", "slug", 24),
    col("NAME", "name", 30),
    col("MODE", "mode", 5),
    col("STATUS", "status", 10),
];

pub async fn tenants(ctx: &Ctx, c: TenantsCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        TenantsCmd::Create {
            slug,
            name,
            mode,
            timezone,
            address_suffix,
            policy_file,
            owner_email,
            owner_name,
            billing_mode,
        } => {
            let owner =
                owner_email.map(|e| Obj::new().set("email", e).opt("name", owner_name).value());
            let body = Obj::new()
                .set("slug", slug)
                .set("name", name)
                .set("mode", mode.unwrap_or_else(|| "live".into()))
                .opt("timezone", timezone)
                .opt("address_suffix", address_suffix)
                .opt("policy", json_arg(ctx, None, policy_file.as_deref())?)
                .opt("owner", owner)
                .opt("billing", billing_mode.map(|m| json!({ "mode": m })))
                .value();
            let t = api
                .call(
                    ctx,
                    "POST",
                    "/v1/tenants",
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?;
            Ok(Output::object(t))
        }
        TenantsCmd::List {
            status,
            mode,
            partner,
            paging: p,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "status", status);
            q_opt(&mut q, "mode", mode);
            q_opt(&mut q, "partner_id", partner);
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, "/v1/tenants", &q, p.all).await?,
                TENANT_COLS,
            ))
        }
        TenantsCmd::Get { tenant } => {
            let id = resolve::tenant_arg(ctx, &tenant).await?;
            Ok(Output::object(
                api.get(ctx, &format!("/v1/tenants/{}", seg(&id)), &[])
                    .await?,
            ))
        }
        TenantsCmd::Update {
            tenant,
            name,
            timezone,
            policy_file,
            policy,
        } => {
            let id = resolve::tenant_arg(ctx, &tenant).await?;
            let body = Obj::new().opt("name", name).opt("timezone", timezone).opt(
                "policy",
                json_arg(ctx, policy.as_deref(), policy_file.as_deref())?,
            );
            if body.is_empty() {
                return Err(CliError::usage(
                    "Nothing to change: pass --name, --timezone, --policy or --policy-file.",
                ));
            }
            let t = api
                .call(
                    ctx,
                    "PATCH",
                    &format!("/v1/tenants/{}", seg(&id)),
                    &[],
                    Some(&body.value()),
                    Idem::None,
                )
                .await?;
            Ok(Output::object(t))
        }
        TenantsCmd::Policy {
            tenant,
            set,
            set_file,
        } => {
            let id = resolve::tenant_arg(ctx, &tenant).await?;
            let path = format!("/v1/tenants/{}/policy", seg(&id));
            match json_arg(ctx, set.as_deref(), set_file.as_deref())? {
                None => Ok(Output::object(api.get(ctx, &path, &[]).await?)),
                Some(body) => Ok(Output::object(
                    api.call(ctx, "PATCH", &path, &[], Some(&body), Idem::None)
                        .await?,
                )),
            }
        }
        TenantsCmd::Suspend { tenant } => tenant_status(ctx, &tenant, "suspended").await,
        TenantsCmd::Resume { tenant } => tenant_status(ctx, &tenant, "active").await,
    }
}

/// `tenants suspend|resume`: `PATCH /v1/tenants/{id}` with the status.
async fn tenant_status(ctx: &Ctx, tenant: &str, status: &str) -> Result<Output, CliError> {
    let id = resolve::tenant_arg(ctx, tenant).await?;
    let t = ctx
        .api()?
        .call(
            ctx,
            "PATCH",
            &format!("/v1/tenants/{}", seg(&id)),
            &[],
            Some(&json!({ "status": status })),
            Idem::None,
        )
        .await?;
    Ok(Output::object(t))
}

const PARTNER_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("NAME", "name", 30),
    col("STATUS", "status", 10),
    col("BILLING", "default_billing_mode", 8),
    col("MAX", "max_tenants", 5),
];

pub async fn partners(ctx: &Ctx, c: PartnersCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        PartnersCmd::Create {
            name,
            default_billing_mode,
            max_tenants,
            ramp_exempt,
        } => {
            let body = Obj::new()
                .set("name", name)
                .opt("default_billing_mode", default_billing_mode)
                .opt("max_tenants", max_tenants)
                .opt("ramp_exempt", ramp_exempt)
                .value();
            Ok(Output::object(
                api.call(
                    ctx,
                    "POST",
                    "/v1/partners",
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        PartnersCmd::List { status, paging: p } => {
            let mut q = Vec::new();
            q_opt(&mut q, "status", status);
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, "/v1/partners", &q, p.all).await?,
                PARTNER_COLS,
            ))
        }
        PartnersCmd::Get { partner } => Ok(Output::object(
            api.get(ctx, &format!("/v1/partners/{}", seg(&partner)), &[])
                .await?,
        )),
        PartnersCmd::Update {
            partner,
            name,
            status,
            default_billing_mode,
            max_tenants,
            ramp_exempt,
        } => {
            let suspending = status.as_deref() == Some("suspended");
            let body = Obj::new()
                .opt("name", name)
                .opt("status", status)
                .opt("default_billing_mode", default_billing_mode)
                .opt("max_tenants", max_tenants)
                .opt("ramp_exempt", ramp_exempt);
            if body.is_empty() {
                return Err(CliError::usage("Nothing to change."));
            }
            let p = api
                .call(
                    ctx,
                    "PATCH",
                    &format!("/v1/partners/{}", seg(&partner)),
                    &[],
                    Some(&body.value()),
                    Idem::None,
                )
                .await?;
            let mut out = Output::object(p);
            if suspending {
                out = out.note("warning: every key of the partner and of its tenants is now refused, and their webhook deliveries are held; their inbound mail is still stored.");
            }
            Ok(out)
        }
        PartnersCmd::Delete { partner } => {
            ctx.confirm(&format!(
                "Delete partner {partner}? Its partner keys and webhook endpoints are deleted."
            ))?;
            let r = api
                .call(
                    ctx,
                    "DELETE",
                    &format!("/v1/partners/{}", seg(&partner)),
                    &[],
                    None,
                    Idem::None,
                )
                .await;
            match r {
                Err(CliError::Api(e)) if e.code == "partner_has_tenants" => {
                    Err(CliError::Api(Box::new(pylota_identity::ApiError {
                        fix: Some("Erase those tenants first: idvy erasure create --scope tenant --tenant <tenant> --reason <text>".into()),
                        ..*e
                    })))
                }
                r => Ok(Output::object(r?)),
            }
        }
    }
}

const KEY_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("NAME", "name", 24),
    col("LEVEL", "level", 8),
    col("TENANT", "tenant_id", 30),
    col("STATUS", "status", 8),
    col("EXPIRES", "expires_at", 20),
];

/// `90d`, `12h`, `30m` or seconds as milliseconds.
fn duration_ms(s: &str) -> Result<i64, CliError> {
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: i64 = n
        .parse()
        .map_err(|_| CliError::usage(format!("{s} is not a duration (90d, 12h, 30m).")))?;
    let unit_ms = match unit {
        "d" => 86_400_000,
        "h" => 3_600_000,
        "m" => 60_000,
        "s" | "" => 1_000,
        _ => {
            return Err(CliError::usage(format!(
                "{s} is not a duration (90d, 12h, 30m)."
            )));
        }
    };
    Ok(n * unit_ms)
}

/// Every permission a platform key may hold.
pub fn platform_permissions() -> Vec<&'static str> {
    use pylota_identity_core::keys::{Level, PERMISSIONS, level_can_hold};
    PERMISSIONS
        .iter()
        .copied()
        .filter(|p| level_can_hold(Level::Platform, p))
        .collect()
}

pub async fn keys(ctx: &Ctx, c: KeysCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        KeysCmd::Create {
            level,
            name,
            partner,
            permissions,
            expires_at,
            expires_in,
            mcp_tools,
            save_profile,
        } => {
            let Some(permissions) = permissions else {
                return Err(CliError::usage(format!(
                    "Pass --permissions: least privilege is the default.{}",
                    if level == "platform" {
                        format!(
                            " A platform key may hold: {}",
                            platform_permissions().join(",")
                        )
                    } else {
                        String::new()
                    }
                )));
            };
            match (level.as_str(), &partner) {
                ("partner", None) => {
                    return Err(CliError::usage("--level partner needs --partner <ptn_…>."));
                }
                ("partner", Some(_))
                    if ctx.global.tenant.is_some() || ctx.global.identity.is_some() =>
                {
                    return Err(CliError::usage(
                        "A partner key belongs to no tenant or identity: drop --tenant and --identity.",
                    ));
                }
                (l, Some(_)) if l != "partner" => {
                    return Err(CliError::usage("--partner goes with --level partner only."));
                }
                _ => {}
            }
            let mut body = Obj::new()
                .set("name", name)
                .set("level", level.clone())
                .set("permissions", super::csv(&permissions))
                .opt("partner_id", partner);
            match level.as_str() {
                "tenant" => body = body.set("tenant_id", resolve::tenant(ctx).await?),
                "identity" => body = body.set("identity_id", resolve::identity(ctx).await?),
                _ => {}
            }
            let expires = match (expires_at, expires_in) {
                (Some(t), _) => Some(time(&t)?),
                (None, Some(d)) => Some(pylota_identity_core::days::rfc3339(
                    ctx.now_ms() + duration_ms(&d)?,
                )),
                _ => None,
            };
            body = body.opt("expires_at", expires);
            if let Some(t) = mcp_tools {
                body = body.set("mcp_tools", super::csv(&t));
            }
            let k = api
                .call(
                    ctx,
                    "POST",
                    "/v1/keys",
                    &[],
                    Some(&body.value()),
                    Idem::Generated,
                )
                .await?;
            let secret = k["secret"]
                .as_str()
                .or_else(|| k["key"].as_str())
                .unwrap_or("")
                .to_owned();
            let mut out = Output::object(k.clone())
                .human(format!(
                    "{}{secret}\n",
                    crate::output::pretty(&strip(&k, &["secret", "key"]))
                ))
                .quiet(format!("{secret}\n"))
                .note("The secret above is shown only once.");
            if let Some(p) = save_profile {
                let url = api.url().to_owned();
                config::update_profile(ctx, &p, |prof| {
                    prof.url = Some(url);
                    prof.key = Some(secret.clone());
                    prof.key_env = None;
                    prof.key_command = None;
                })?;
                out = out.note(format!("Saved the key in profile {p}."));
            }
            Ok(out)
        }
        KeysCmd::List { paging: p } => {
            let mut q = Vec::new();
            if ctx.global.tenant.is_some() {
                q.push(("tenant_id", resolve::tenant(ctx).await?));
            }
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, "/v1/keys", &q, p.all).await?,
                KEY_COLS,
            ))
        }
        KeysCmd::Get { key } => Ok(Output::object(
            api.get(ctx, &format!("/v1/keys/{}", seg(&key)), &[])
                .await?,
        )),
        KeysCmd::Revoke { key } => {
            ctx.confirm(&format!("Revoke {key}? It stops working at once."))?;
            Ok(Output::object(
                api.call(
                    ctx,
                    "DELETE",
                    &format!("/v1/keys/{}", seg(&key)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?,
            ))
        }
        KeysCmd::Rotate {
            key,
            overlap_hours,
            revoke_previous,
        } => {
            if key.starts_with("key_") {
                if revoke_previous {
                    return Err(CliError::usage(
                        "--revoke-previous goes with a signing key (thread, link, cursor, web_bot_auth).",
                    ));
                }
                let body = Obj::new().opt("overlap_hours", overlap_hours).value();
                let k = api
                    .call(
                        ctx,
                        "POST",
                        &format!("/v1/keys/{}/rotate", seg(&key)),
                        &[],
                        Some(&body),
                        Idem::Generated,
                    )
                    .await?;
                let secret = k["secret"].as_str().unwrap_or("").to_owned();
                Ok(Output::object(k.clone())
                    .human(format!(
                        "{}{secret}\n",
                        crate::output::pretty(&strip(&k, &["secret"]))
                    ))
                    .quiet(format!("{secret}\n"))
                    .note("The new secret above is shown only once."))
            } else if ["thread", "link", "cursor", "web_bot_auth"].contains(&key.as_str()) {
                if overlap_hours.is_some() {
                    return Err(CliError::usage(
                        "--overlap-hours goes with an API key (key_…).",
                    ));
                }
                let mut q = Vec::new();
                if revoke_previous {
                    q.push(("revoke_previous", "true".to_owned()));
                }
                Ok(Output::object(
                    api.call(
                        ctx,
                        "POST",
                        &format!("/v1/platform/keys/{key}/rotate"),
                        &q,
                        None,
                        Idem::Generated,
                    )
                    .await?,
                ))
            } else {
                Err(CliError::usage(
                    "Rotate a key_… ID, or thread, link, cursor or web_bot_auth.",
                ))
            }
        }
    }
}

/// A copy of an object without some members.
fn strip(v: &Value, keys: &[&str]) -> Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        for k in keys {
            o.remove(*k);
        }
    }
    v
}

const AUDIT_COLS: &[Col] = &[
    col("TIME", "created_at", 20),
    col("ACTION", "action", 28),
    col("TARGET", "target_id", 30),
    col("KEY", "actor_key_id", 30),
];

pub async fn audit(ctx: &Ctx, a: AuditArgs) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let mut q = Vec::new();
    if ctx.global.tenant.is_some() {
        q.push(("tenant_id", resolve::tenant(ctx).await?));
    }
    q_opt(&mut q, "action", a.action);
    q_opt(&mut q, "target_id", a.target);
    if let Some(t) = a.after {
        q.push(("after", time(&t)?));
    }
    paging(&a.paging, &mut q);
    Ok(Output::list(
        api.list(ctx, "/v1/audit-events", &q, a.paging.all).await?,
        AUDIT_COLS,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn durations() {
        assert_eq!(super::duration_ms("90d").unwrap(), 90 * 86_400_000);
        assert_eq!(super::duration_ms("12h").unwrap(), 12 * 3_600_000);
        assert!(super::duration_ms("soon").is_err());
    }

    #[test]
    fn platform_permissions_exclude_sign() {
        let p = super::platform_permissions();
        assert!(p.contains(&"platform:ops") && !p.contains(&"identities:sign"));
    }
}
