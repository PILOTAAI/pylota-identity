//! `domains` ([CLI and setup §18.2–§18.4]): `add --method` builds the create body from its flags, the
//! records print with both `name` and `host`, and `subscribe` creates a `manual` domain's event
//! subscription with the operator's own Cloudflare token.
//!
//! [CLI and setup §18.2–§18.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#182-domains-add---method

use serde_json::{Value, json};

use super::{Obj, paging};
use crate::cli::DomainsCmd;
use crate::http::{Idem, seg};
use crate::output::{Col, col, table};
use crate::{CliError, Ctx, Output, resolve};

const DOMAIN_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("NAME", "name", 36),
    col("METHOD", "method", 16),
    col("STATE", "state", 10),
    col("EVENTS", "delivery_events", 8),
];

const RECORD_COLS: &[Col] = &[
    col("TYPE", "type", 6),
    col("NAME", "name", 50),
    col("HOST", "host", 34),
    col("VALUE", "value", 50),
    col("PRIORITY", "priority", 8),
    col("PURPOSE", "purpose", 12),
    col("REQUIRED", "required", 8),
    col("STATUS", "status", 9),
];

/// Which `domains add` flags each method allows ([CLI and setup §18.2]).
///
/// [CLI and setup §18.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#182-domains-add---method
fn allowed(flag: &str, method: &str) -> bool {
    match flag {
        "--replace-mx" => matches!(method, "cloudflare_zone" | "dns_records"),
        "--confirm-dedicated" => method == "nameservers",
        "--inbound"
        | "--smtp-host"
        | "--smtp-port"
        | "--smtp-username"
        | "--smtp-password-stdin"
        | "--probe-from" => method == "smtp_relay",
        _ => true,
    }
}

/// The domain and its records, for people.
fn domain_human(d: &Value) -> String {
    let mut s = format!(
        "{}  {}  {}  {}\n",
        d["id"].as_str().unwrap_or(""),
        crate::output::terminal_safe(d["name"].as_str().unwrap_or("")),
        d["method"].as_str().unwrap_or(""),
        d["state"].as_str().unwrap_or(""),
    );
    if let Some(r) = d["records"].as_array().filter(|r| !r.is_empty()) {
        s.push('\n');
        s.push_str(&table(r, RECORD_COLS));
    }
    if let Some(a) = d["details"]["action"].as_str() {
        s.push_str(&format!("\nnext: {}\n", crate::output::terminal_safe(a)));
    }
    s
}

pub async fn run(ctx: &Ctx, c: DomainsCmd) -> Result<Output, CliError> {
    if let DomainsCmd::Subscribe { domain, dir } = c {
        return crate::commands::setup::domains_subscribe(ctx, &domain, &dir.dir).await;
    }
    let api = ctx.api()?;
    match c {
        DomainsCmd::Add {
            name,
            method,
            no_receiving,
            no_sending,
            replace_mx,
            confirm_dedicated,
            claim,
            inbound,
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password_stdin,
            probe_from,
        } => {
            let given = [
                ("--replace-mx", replace_mx),
                ("--confirm-dedicated", confirm_dedicated),
                ("--inbound", inbound.is_some()),
                ("--smtp-host", smtp_host.is_some()),
                ("--smtp-port", smtp_port.is_some()),
                ("--smtp-username", smtp_username.is_some()),
                ("--smtp-password-stdin", smtp_password_stdin),
                ("--probe-from", probe_from.is_some()),
            ];
            for (flag, on) in given {
                if on && !allowed(flag, &method) {
                    return Err(CliError::usage(format!(
                        "{flag} does not go with --method {method}."
                    )));
                }
            }
            let mut body = Obj::new()
                .set("name", name)
                .set("method", method.clone())
                .flag("replace_mx", replace_mx)
                .flag("confirm_dedicated", confirm_dedicated)
                .flag("claim", claim);
            if no_receiving {
                body = body.set("receiving", false);
            }
            if no_sending {
                body = body.set("sending", false);
            }
            if method == "smtp_relay" {
                let (Some(host), Some(port), Some(user), Some(inb)) =
                    (smtp_host, smtp_port, smtp_username, inbound)
                else {
                    return Err(CliError::usage(
                        "smtp_relay needs --inbound, --smtp-host, --smtp-port, --smtp-username and --smtp-password-stdin.",
                    ));
                };
                if !smtp_password_stdin {
                    return Err(CliError::usage("smtp_relay needs --smtp-password-stdin."));
                }
                if port != 465 && port != 587 {
                    return Err(CliError::usage("--smtp-port is 465 or 587."));
                }
                let password = read_password(ctx)?;
                body = body.set("inbound", inb).set(
                    "smtp",
                    Obj::new()
                        .set("host", host)
                        .set("port", port)
                        .set("username", user)
                        .set("password", password)
                        .opt("probe_from", probe_from)
                        .value(),
                );
            }
            let tenant = resolve::tenant(ctx).await?;
            let r = api
                .call(
                    ctx,
                    "POST",
                    &format!("/v1/tenants/{}/domains", seg(&tenant)),
                    &[],
                    Some(&body.value()),
                    Idem::Generated,
                )
                .await;
            match r {
                Ok(d) => Ok(Output::object(d.clone()).human(domain_human(&d))),
                Err(CliError::Api(e))
                    if e.status == 422
                        && matches!(
                            e.code.as_str(),
                            "transport_unavailable" | "cf_token_required"
                        ) =>
                {
                    let reason = e
                        .details
                        .as_ref()
                        .and_then(|d| d["reason"].as_str())
                        .unwrap_or("");
                    let fix = match (e.code.as_str(), reason) {
                        ("cf_token_required", _) => {
                            "Set PM_CF_API_TOKEN on the deployment (idvy wrangler secret put PM_CF_API_TOKEN)."
                        }
                        (_, "ses_not_configured" | "ses_receiving_not_configured") => {
                            "idvy setup ses"
                        }
                        (_, "subdomain_setup_disabled") => {
                            "Set PM_CF_SUBDOMAIN_SETUP = \"on\" and idvy deploy."
                        }
                        (_, "zone_creation_not_allowed") => {
                            "Allow it in the tenant policy (domains.allow_create_zone)."
                        }
                        (_, "ses_identity_limit") => {
                            "Raise the SES identity limit in the AWS console."
                        }
                        _ => "Use another --method.",
                    };
                    Err(CliError::Config(crate::error::Local {
                        message: format!("{}: {} {}", e.code, e.message, reason)
                            .trim()
                            .to_owned(),
                        fix: Some(fix.to_owned()),
                        details: e.details.clone(),
                    }))
                }
                Err(e) => Err(e),
            }
        }
        DomainsCmd::List { paging: p } => {
            let tenant = resolve::tenant(ctx).await?;
            let mut q = Vec::new();
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(
                    ctx,
                    &format!("/v1/tenants/{}/domains", seg(&tenant)),
                    &q,
                    p.all,
                )
                .await?,
                DOMAIN_COLS,
            ))
        }
        DomainsCmd::Get { domain } => {
            let id = resolve::domain_arg(ctx, &domain).await?;
            let d = api
                .get(ctx, &format!("/v1/domains/{}", seg(&id)), &[])
                .await?;
            Ok(Output::object(d.clone()).human(crate::output::pretty(&d)))
        }
        DomainsCmd::Records { domain, format } => {
            let id = resolve::domain_arg(ctx, &domain).await?;
            let path = format!("/v1/domains/{}/records", seg(&id));
            if format == "bind" {
                let r = api
                    .send(
                        ctx,
                        "GET",
                        &path,
                        &[("format", "bind".to_owned())],
                        None,
                        Idem::None,
                    )
                    .await?;
                let text = String::from_utf8_lossy(&r.body).into_owned();
                return Ok(
                    Output::object(json!({ "format": "bind", "zone": text.clone() }))
                        .human(text.clone())
                        .quiet(text),
                );
            }
            let r = api.get(ctx, &path, &[]).await?;
            let rows = r["data"].as_array().cloned().unwrap_or_default();
            Ok(Output::object(r).human(table(&rows, RECORD_COLS)))
        }
        DomainsCmd::Verify { domain } => post(ctx, &domain, "verify").await,
        DomainsCmd::Reprove { domain } => post(ctx, &domain, "reprove").await,
        DomainsCmd::Probe { domain } => {
            let o = post(ctx, &domain, "probe").await?;
            let id = o.doc["probe_id"].as_str().unwrap_or("").to_owned();
            Ok(o.human(format!(
                "probe {id} started; see idvy domains health {domain}\n"
            ))
            .quiet(format!("{id}\n")))
        }
        DomainsCmd::Health { domain } => {
            let id = resolve::domain_arg(ctx, &domain).await?;
            Ok(Output::object(
                api.get(ctx, &format!("/v1/domains/{}/health", seg(&id)), &[])
                    .await?,
            ))
        }
        DomainsCmd::Remove { domain } => {
            let id = resolve::domain_arg(ctx, &domain).await?;
            ctx.confirm(&format!(
                "Remove {domain}? Routing, sending and its event subscription are removed."
            ))?;
            Ok(Output::object(
                api.call(
                    ctx,
                    "DELETE",
                    &format!("/v1/domains/{}", seg(&id)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?,
            ))
        }
        DomainsCmd::Subscribe { .. } => unreachable!("handled above"),
    }
}

async fn post(ctx: &Ctx, domain: &str, action: &str) -> Result<Output, CliError> {
    let id = resolve::domain_arg(ctx, domain).await?;
    Ok(Output::object(
        ctx.api()?
            .call(
                ctx,
                "POST",
                &format!("/v1/domains/{}/{action}", seg(&id)),
                &[],
                None,
                Idem::Generated,
            )
            .await?,
    ))
}

/// The SMTP password: stdin up to the first newline, asked without echo on a terminal. Never from the
/// command line, the environment or the config file, and never printed.
fn read_password(ctx: &Ctx) -> Result<String, CliError> {
    let line = if ctx.stdin_tty {
        ctx.prompt("SMTP password: ")?
    } else {
        let bytes = ctx.read_stdin()?;
        let s =
            String::from_utf8(bytes).map_err(|_| CliError::usage("The password is not UTF-8."))?;
        s.split('\n')
            .next()
            .unwrap_or("")
            .trim_end_matches('\r')
            .to_owned()
    };
    if line.is_empty() {
        return Err(CliError::usage("No SMTP password on stdin."));
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    #[test]
    fn method_flags() {
        assert!(super::allowed("--replace-mx", "dns_records"));
        assert!(!super::allowed("--replace-mx", "send_only"));
        assert!(!super::allowed("--smtp-host", "cloudflare_zone"));
        assert!(super::allowed("--claim", "nameservers"));
    }
}
