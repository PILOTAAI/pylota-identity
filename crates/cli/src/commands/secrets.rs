//! `idvy secrets rotate-master` ([CLI and setup §12.1], Security § 6.2): a new key into the inactive
//! slot, then the slot switched, then the Worker's re-seal sweep polled to the end. The key value goes
//! to Wrangler on stdin only and is never printed; the CLI makes no D1 query.
//!
//! [CLI and setup §12.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#121-secrets-rotate-master

use std::time::Duration;

use pylota_identity_core::days::{parse_rfc3339, rfc3339};
use serde_json::{Value, json};

use super::setup::{Deployment, secret32};
use crate::bundle::{release, render};
use crate::cli::{RotateMasterArgs, SecretsCmd};
use crate::wrangler::Wrangler;
use crate::{CliError, Ctx, Output, VERSION};

/// A rotation within this long of the previous re-seal needs `--discard-previous` (D1 Time Travel's
/// 30-day window).
const RESTORE_WINDOW_MS: i64 = 30 * 86_400_000;
/// The Worker's sweep re-seals this many values a minute.
const PER_MINUTE: u64 = 500;

pub async fn secrets(ctx: &Ctx, c: SecretsCmd) -> Result<Output, CliError> {
    match c {
        SecretsCmd::RotateMaster(a) => rotate_master(ctx, a).await,
    }
}

async fn master_key(ctx: &Ctx) -> Result<Value, CliError> {
    let v = ctx.api()?.get(ctx, "/v1/platform/status", &[]).await?;
    Ok(v["master_key"].clone())
}

fn slot_of<'v>(mk: &'v Value, slot: &str) -> &'v Value {
    mk["slots"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["slot"] == slot))
        .unwrap_or(&Value::Null)
}

fn precondition(m: impl Into<String>) -> CliError {
    CliError::Precondition(crate::error::Local {
        message: m.into(),
        fix: None,
        details: None,
    })
}

async fn rotate_master(ctx: &Ctx, a: RotateMasterArgs) -> Result<Output, CliError> {
    // Step 1.
    let mk = master_key(ctx).await?;
    let active = mk["active_slot"].as_str().unwrap_or("a").to_owned();
    let remaining = mk["remaining"].as_u64().unwrap_or(0);
    let target = if a.resume {
        active.clone()
    } else {
        // Step 2.
        if remaining > 0 {
            return Err(precondition(format!(
                "A master-key rotation is unfinished: {remaining} values are not re-sealed yet."
            ))
            .with_fix("idvy secrets rotate-master --resume"));
        }
        // Step 3.
        let inactive = if active == "a" { "b" } else { "a" };
        let resealed = mk["resealed_at"].as_str().and_then(parse_rfc3339);
        if slot_of(&mk, inactive)["present"] == true
            && let Some(at) = resealed
            && ctx.now_ms() - at < RESTORE_WINDOW_MS
        {
            let allowed = rfc3339(at + RESTORE_WINDOW_MS);
            if !a.discard_previous {
                return Err(precondition(format!(
                    "Slot {inactive} still holds the key that sealed values before {}; a D1 restore inside the 30-day window needs it. A rotation is allowed from {allowed}.",
                    rfc3339(at)
                ))
                .with_fix("Wait, or after a suspected leak of the previous key: idvy secrets rotate-master --discard-previous")
                .with_details(json!({ "allowed_from": allowed })));
            }
            ctx.note(&format!(
                "Overwriting slot {inactive}: a D1 restore to before {} may no longer open sealed values.",
                rfc3339(at)
            ));
            ctx.confirm("Discard the previous master key?")?;
        }

        // Step 4: the new key into the inactive slot, then the selector.
        let d = Deployment::open(ctx, &a.dir.dir)?;
        let version = d
            .rendered
            .as_deref()
            .and_then(|r| {
                r.lines()
                    .find_map(|l| l.strip_prefix(render::MARKER))
                    .and_then(|v| v.split(' ').next())
                    .map(|v| v.trim_start_matches('v').to_owned())
            })
            .unwrap_or_else(|| VERSION.to_owned());
        let bundle = release::fetch(ctx, &d.dir, &version).await?;
        let wrangler = Wrangler::install(ctx, &d.dir, &bundle.dir, d.token(), &d.cf.account)?;
        let (value, bytes) = secret32()?;
        let new_kid = pylota_identity_core::crypto::kid(&bytes);
        let name = if inactive == "b" {
            "PM_MASTER_KEY_B"
        } else {
            "PM_MASTER_KEY"
        };
        wrangler.secret_put(&d.name, name, &value)?;
        drop(value);
        wrangler.secret_put(&d.name, "PM_MASTER_KEY_ACTIVE", inactive)?;
        ctx.note(&format!(
            "Slot {inactive} now holds key {new_kid} and is active; the Worker re-seals every value under it."
        ));
        inactive.to_owned()
    };

    // Step 5: poll until the Worker reports the target slot active with nothing left.
    let interval = Duration::from_millis(ctx.poll_ms.unwrap_or(60_000));
    let mk = loop {
        let mk = master_key(ctx).await?;
        let left = mk["remaining"].as_u64().unwrap_or(0);
        if mk["active_slot"] == target.as_str() && left == 0 && mk["resealed_at"].is_string() {
            break mk;
        }
        if mk["active_slot"] == target.as_str() {
            ctx.note(&format!(
                "remaining {left} (about {} min)",
                left.div_ceil(PER_MINUTE)
            ));
        } else {
            ctx.note("waiting for the new version to serve the switched slot");
        }
        tokio::time::sleep(interval).await;
    };

    // Step 6.
    let kid = slot_of(&mk, &target)["kid"].clone();
    let resealed = mk["resealed_at"].as_str().and_then(parse_rfc3339);
    let next = resealed.map(|t| rfc3339(t + RESTORE_WINDOW_MS));
    let human = format!(
        "Active slot {target}, key {}. Every sealed value is re-sealed. The next rotation without --discard-previous is allowed from {}.\n",
        kid.as_str().unwrap_or("?"),
        next.as_deref().unwrap_or("?")
    );
    Ok(Output::object(json!({
        "active_slot": target, "kid": kid, "resealed_at": mk["resealed_at"], "next_rotation_from": next,
    }))
    .human(human))
}
