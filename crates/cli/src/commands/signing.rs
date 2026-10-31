//! `identity-keys`, `assertions` and `http-sign` ([CLI and setup §18.5]).
//!
//! [CLI and setup §18.5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#185-identity-keys-assertions-and-http-sign

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pylota_identity::assertions::{AssertionError, Verifier, VerifierConfig};
use serde_json::{Value, json};

use super::{Obj, paging, q_opt, read_file};
use crate::cli::{AssertionsCmd, HttpSignArgs, IdentityKeysCmd};
use crate::http::{Idem, seg};
use crate::output::{Col, col};
use crate::{CliError, Ctx, Output, config, resolve};

const KEY_COLS: &[Col] = &[
    col("KID", "kid", 44),
    col("STATUS", "status", 9),
    col("CREATED", "created_at", 20),
    col("VERIFY UNTIL", "verify_until", 20),
    col("RETIRED", "retired_at", 20),
];

pub async fn identity_keys(ctx: &Ctx, c: IdentityKeysCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let idn = resolve::identity(ctx).await?;
    let base = format!("/v1/identities/{}/keys", seg(&idn));
    match c {
        IdentityKeysCmd::List { status, paging: p } => {
            let mut q = Vec::new();
            q_opt(&mut q, "status", status);
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, &base, &q, p.all).await?,
                KEY_COLS,
            ))
        }
        IdentityKeysCmd::Create => {
            let r = api
                .send(ctx, "POST", &base, &[], Some(&json!({})), Idem::Generated)
                .await?;
            let key: Value = serde_json::from_slice(&r.body)
                .map_err(|e| CliError::internal(format!("unexpected response: {e}")))?;
            let kid = key["kid"].as_str().unwrap_or("").to_owned();
            let line = if r.status == 201 {
                format!("Created key {kid}\n")
            } else {
                format!("Active key {kid} already exists\n")
            };
            Ok(Output::object(key).human(line).quiet(format!("{kid}\n")))
        }
        IdentityKeysCmd::Rotate => {
            let r = api
                .call(
                    ctx,
                    "POST",
                    &format!("{base}/rotate"),
                    &[],
                    Some(&json!({})),
                    Idem::Generated,
                )
                .await?;
            let kid = r["key"]["kid"].as_str().unwrap_or("").to_owned();
            let mut line = format!("New key {kid}\n");
            if let Some(prev) = r["previous"]["kid"].as_str() {
                line.push_str(&format!(
                    "Previous key {prev} verifies until {}\n",
                    r["previous"]["verify_until"].as_str().unwrap_or("")
                ));
            }
            Ok(Output::object(r).human(line).quiet(format!("{kid}\n")))
        }
        IdentityKeysCmd::Revoke { kid } => {
            let path = format!("{base}/{}/revoke", seg(&kid));
            ctx.confirm(&format!(
                "Revoke key {kid}? Assertions signed with it stop verifying as soon as verifiers fetch the key set again (it is cached for up to 5 minutes)."
            ))?;
            let r = api
                .call(ctx, "POST", &path, &[], Some(&json!({})), Idem::Generated)
                .await?;
            Ok(Output::object(r).human(format!("Revoked key {kid}\n")))
        }
    }
}

pub async fn assertions(ctx: &Ctx, c: AssertionsCmd) -> Result<Output, CliError> {
    match c {
        AssertionsCmd::Create {
            audience,
            expires_in,
            nonce,
            ext,
            ext_file,
        } => {
            let ext = match (ext, ext_file) {
                (Some(t), _) => Some(t.into_bytes()),
                (None, Some(f)) => Some(read_file(ctx, &f)?),
                (None, None) => None,
            };
            let ext = match ext {
                Some(b) => match serde_json::from_slice::<Value>(&b) {
                    Ok(v @ Value::Object(_)) => Some(v),
                    _ => return Err(CliError::usage("--ext is a JSON object.")),
                },
                None => None,
            };
            let idn = resolve::identity(ctx).await?;
            let body = Obj::new()
                .set("audience", audience)
                .set("expires_in", expires_in)
                .opt("nonce", nonce)
                .opt("ext", ext)
                .value();
            let r = ctx
                .api()?
                .call(
                    ctx,
                    "POST",
                    &format!("/v1/identities/{}/assertions", seg(&idn)),
                    &[],
                    Some(&body),
                    Idem::None,
                )
                .await?;
            let token = r["assertion"].as_str().unwrap_or("").to_owned();
            Ok(Output::object(r).quiet(format!("{token}\n")))
        }
        AssertionsCmd::Verify {
            token,
            audience,
            issuer,
            now,
        } => verify(ctx, token, audience, issuer, now).await,
    }
}

/// `idvy assertions verify`: the SDK verifier without its replay check; exit 11 when invalid.
async fn verify(
    ctx: &Ctx,
    token: String,
    audience: String,
    issuer: Option<String>,
    now: Option<i64>,
) -> Result<Output, CliError> {
    let token = if token == "-" {
        String::from_utf8_lossy(&ctx.read_stdin()?)
            .trim()
            .to_owned()
    } else {
        token
    };
    let issuer = match issuer {
        Some(i) => i,
        None => config::resolve(ctx)?.url.map(|u| u.value).ok_or_else(|| {
            CliError::config(
                "No issuer: pass --issuer, --url, set PYLOTA_IDENTITY_URL or run idvy login.",
            )
        })?,
    };
    let issuer = issuer.trim_end_matches('/').to_owned();
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|p| issuer == *p || issuer.starts_with(&format!("{p}:")));
    if !issuer.starts_with("https://") && !local {
        return Err(CliError::usage("--issuer must be an https:// URL."));
    }
    let now = now.unwrap_or(ctx.now_ms() / 1000);
    let verifier = Verifier::new(VerifierConfig::new([issuer], &audience))
        .with_replay_store(Box::new(NoReplay));
    let kid = token
        .split('.')
        .next()
        .and_then(|h| URL_SAFE_NO_PAD.decode(h).ok())
        .and_then(|h| serde_json::from_slice::<Value>(&h).ok())
        .and_then(|h| h["kid"].as_str().map(str::to_owned));
    match verifier.verify_assertion_at(&token, now).await {
        Ok(a) => {
            let claims = json!({
                "sub": a.sub, "email": a.email, "aud": a.aud, "exp": a.exp, "jti": a.jti,
                "unverified": { "name": a.unverified["name"], "org": a.unverified["org"] },
            });
            let human = format!(
                "valid\nsub: {}\nemail: {}\naud: {}\nexp: {}\njti: {}\nkid: {}\nname (unverified): {}\norg (unverified): {}\n",
                a.sub,
                a.email,
                a.aud,
                a.exp,
                a.jti,
                kid.as_deref().unwrap_or(""),
                a.unverified["name"].as_str().unwrap_or(""),
                a.unverified["org"].as_str().unwrap_or(""),
            );
            Ok(
                Output::object(json!({ "valid": true, "kid": kid, "claims": claims }))
                    .human(human)
                    .quiet("valid\n"),
            )
        }
        Err(e) => {
            let reason = match e {
                AssertionError::Malformed => "malformed",
                AssertionError::Algorithm => "unsupported_algorithm",
                AssertionError::UntrustedIssuer => "issuer_mismatch",
                AssertionError::UnknownKey => "unknown_kid",
                AssertionError::BadSignature => "bad_signature",
                AssertionError::Audience => "audience_mismatch",
                AssertionError::Expired => "expired",
                AssertionError::NotYetValid => "not_yet_valid",
                AssertionError::Replayed => "replayed",
                AssertionError::Jwks(pylota_identity::Error::Api(a)) if a.status == 404 => {
                    "identity_not_found"
                }
                AssertionError::Jwks(pylota_identity::Error::Status { status: 404, .. }) => {
                    "identity_not_found"
                }
                AssertionError::Jwks(other) => return Err(other.into()),
            };
            Err(CliError::verification(format!("invalid: {reason}"))
                .with_details(json!({ "valid": false, "reason": reason })))
        }
    }
}

/// The CLI keeps no state across calls, so it checks no replay (it prints `jti` and `exp`).
struct NoReplay;

impl pylota_identity::assertions::ReplayStore for NoReplay {
    fn record(&self, _: &str, _: i64, _: i64) -> bool {
        true
    }
}

const COMPONENTS: &[&str] = &["@method", "@path", "@query"];
const HEADERS: &[&str] = &["Signature-Agent", "From", "Signature-Input", "Signature"];

pub async fn http_sign(ctx: &Ctx, a: HttpSignArgs) -> Result<Output, CliError> {
    if let Some(bad) = a
        .components
        .iter()
        .find(|c| !COMPONENTS.contains(&c.as_str()))
    {
        return Err(CliError::usage(format!(
            "--component is @method, @path or @query, not {bad}."
        )));
    }
    let idn = resolve::identity(ctx).await?;
    let components = (!a.components.is_empty()).then(|| {
        let mut all: Vec<String> = ["@authority", "signature-agent", "from"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        all.extend(a.components.iter().cloned());
        all
    });
    let body = Obj::new()
        .set("url", a.url)
        .opt("method", a.method)
        .set("expires_in", a.expires_in)
        .opt("components", components)
        .value();
    let r = ctx
        .api()?
        .call(
            ctx,
            "POST",
            &format!("/v1/identities/{}/http-signatures", seg(&idn)),
            &[],
            Some(&body),
            Idem::None,
        )
        .await?;
    let lines: String = HEADERS
        .iter()
        .map(|h| format!("{h}: {}\n", r["headers"][h].as_str().unwrap_or("")))
        .collect();
    Ok(Output::object(r).human(lines.clone()).quiet(lines))
}
