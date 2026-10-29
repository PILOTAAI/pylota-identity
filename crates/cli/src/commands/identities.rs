//! `identities` and `addresses` ([CLI and setup §19]).
//!
//! [CLI and setup §19]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#19-command-to-endpoint-map

use serde_json::{Value, json};

use super::{Obj, metadata, paging, q_opt};
use crate::cli::{AddressesCmd, IdentitiesCmd, IdentityFields};
use crate::http::{Idem, seg};
use crate::output::{Col, col};
use crate::{CliError, Ctx, Output, resolve};

const IDENTITY_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("ADDRESS", "primary_address", 40),
    col("NAME", "display_name", 28),
    col("STATUS", "status", 8),
];

/// The body fields shared by create and update.
fn fields(f: IdentityFields, mut o: Obj) -> Result<Obj, CliError> {
    let owner = match (f.owner_name, f.owner_email) {
        (None, None) => None,
        (n, e) => Some(Obj::new().opt("name", n).opt("email", e).value()),
    };
    if let Some(h) = f.message_hours {
        if !(1..=8760).contains(&h) {
            return Err(CliError::usage("--message-hours is 1 to 8760."));
        }
        o = o.set("retention", json!({ "message_hours": h }));
    } else if f.keep_messages {
        o = o.set("retention", json!({ "message_hours": null }));
    }
    Ok(o.opt("display_name", f.display_name)
        .opt("purpose", f.purpose)
        .opt("owner", owner)
        .opt("signature", f.signature_text.map(|t| json!({ "text": t })))
        .opt("metadata", metadata(&f.metadata)?))
}

pub async fn identities(ctx: &Ctx, c: IdentitiesCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    match c {
        IdentitiesCmd::Create {
            username,
            fields: f,
            domain,
            client_id,
        } => {
            if f.display_name.is_none() {
                return Err(CliError::usage("Pass --display-name."));
            }
            let tenant = resolve::tenant(ctx).await?;
            let domain_id = match domain {
                Some(d) => Some(resolve::domain_arg(ctx, &d).await?),
                None => None,
            };
            let body = fields(f, Obj::new().set("username", username))?
                .opt("domain_id", domain_id)
                .opt("client_id", client_id)
                .value();
            let i = api
                .call(
                    ctx,
                    "POST",
                    &format!("/v1/tenants/{}/identities", seg(&tenant)),
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?;
            Ok(Output::object(i))
        }
        IdentitiesCmd::List {
            status,
            purpose,
            client_id,
            paging: p,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "status", status);
            q_opt(&mut q, "purpose", purpose);
            q_opt(&mut q, "client_id", client_id);
            paging(&p, &mut q);
            let path = if ctx.global.tenant.is_some() {
                format!(
                    "/v1/tenants/{}/identities",
                    seg(&resolve::tenant(ctx).await?)
                )
            } else {
                "/v1/identities".to_owned()
            };
            Ok(Output::list(
                api.list(ctx, &path, &q, p.all).await?,
                IDENTITY_COLS,
            ))
        }
        IdentitiesCmd::Get { identity } => {
            let id = resolve::identity_arg(ctx, &identity).await?;
            Ok(Output::object(
                api.get(ctx, &format!("/v1/identities/{}", seg(&id)), &[])
                    .await?,
            ))
        }
        IdentitiesCmd::Update {
            identity,
            fields: f,
            daily_cap,
            auto_reply,
            require_known_recipient,
        } => {
            let id = resolve::identity_arg(ctx, &identity).await?;
            let policy = Obj::new()
                .opt("daily_cap", daily_cap)
                .opt("auto_reply", auto_reply)
                .opt("require_known_recipient", require_known_recipient);
            let mut body = fields(f, Obj::new())?;
            if !policy.is_empty() {
                body = body.set("send_policy", policy.value());
            }
            if body.is_empty() {
                return Err(CliError::usage("Nothing to change."));
            }
            patch_identity(ctx, &id, body.value()).await
        }
        IdentitiesCmd::Pause { identity } => {
            let id = resolve::identity_arg(ctx, &identity).await?;
            patch_identity(ctx, &id, json!({ "status": "paused" })).await
        }
        IdentitiesCmd::Resume { identity } => {
            let id = resolve::identity_arg(ctx, &identity).await?;
            patch_identity(ctx, &id, json!({ "status": "active" })).await
        }
        IdentitiesCmd::Delete { identity, confirm } => {
            let id = resolve::identity_arg(ctx, &identity).await?;
            let current = api
                .get(ctx, &format!("/v1/identities/{}", seg(&id)), &[])
                .await?;
            let primary = current["primary_address"].as_str().unwrap_or("").to_owned();
            let typed = match confirm {
                Some(c) => c,
                None if ctx.interactive => ctx.prompt(&format!(
                    "This erases the whole mailbox. Type {primary} to confirm: "
                ))?,
                None => {
                    return Err(CliError::usage(format!(
                        "Pass --confirm {primary} to erase this mailbox."
                    )));
                }
            };
            if !typed.trim().eq_ignore_ascii_case(&primary) {
                return Err(CliError::usage(
                    "The typed address does not match; nothing was deleted.",
                ));
            }
            Ok(Output::object(
                api.call(
                    ctx,
                    "DELETE",
                    &format!("/v1/identities/{}", seg(&id)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?,
            ))
        }
        IdentitiesCmd::Lookup { address } => {
            let id = resolve::identity_arg(ctx, &address).await?;
            Ok(Output::object(
                api.get(ctx, &format!("/v1/identities/{}", seg(&id)), &[])
                    .await?,
            ))
        }
    }
}

async fn patch_identity(ctx: &Ctx, id: &str, body: Value) -> Result<Output, CliError> {
    Ok(Output::object(
        ctx.api()?
            .call(
                ctx,
                "PATCH",
                &format!("/v1/identities/{}", seg(id)),
                &[],
                Some(&body),
                Idem::None,
            )
            .await?,
    ))
}

const ADDRESS_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("ADDRESS", "address", 44),
    col("ROLE", "role", 8),
    col("STATUS", "status", 9),
    col("RETIRES", "retire_at", 20),
];

pub async fn addresses(ctx: &Ctx, c: AddressesCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let idn = resolve::identity(ctx).await?;
    let base = format!("/v1/identities/{}/addresses", seg(&idn));
    match c {
        AddressesCmd::List => Ok(Output::list(
            api.list(ctx, &base, &[], true).await?,
            ADDRESS_COLS,
        )),
        AddressesCmd::Add { local_part, domain } => {
            let domain_id = resolve::domain_arg(ctx, &domain).await?;
            let body = json!({ "local_part": local_part, "domain_id": domain_id });
            Ok(Output::object(
                api.call(ctx, "POST", &base, &[], Some(&body), Idem::Generated)
                    .await?,
            ))
        }
        AddressesCmd::Promote {
            address,
            retire_previous_after_days,
        } => {
            let body = Obj::new()
                .opt("retire_previous_after_days", retire_previous_after_days)
                .value();
            Ok(Output::object(
                api.call(
                    ctx,
                    "POST",
                    &format!("{base}/{}/promote", seg(&address)),
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        AddressesCmd::Retire {
            address,
            after_days,
        } => {
            let body = Obj::new().opt("after_days", after_days).value();
            Ok(Output::object(
                api.call(
                    ctx,
                    "POST",
                    &format!("{base}/{}/retire", seg(&address)),
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        AddressesCmd::Delete { address } => Ok(Output::object(
            api.call(
                ctx,
                "DELETE",
                &format!("{base}/{}", seg(&address)),
                &[],
                None,
                Idem::None,
            )
            .await?,
        )),
    }
}
