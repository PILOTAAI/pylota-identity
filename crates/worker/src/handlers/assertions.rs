//! Agent assertions ([Agent signing keys § 4]): a short-lived EdDSA JWT signed with the identity's
//! active key, minted per call and never stored or logged; only a count is kept. The signing checks,
//! the vouched address and the usage count are shared with signed HTTP requests.
//!
//! [Agent signing keys § 4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::days::rfc3339;
use pylota_identity_core::jwt;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{
    Background as _, ControlDb as _, Ids as _, ObjectClient as _, Platform,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::identity_in_scope;
use super::identity_keys::{self, Actor, Signer};
use crate::ctx::{Ctx, Reply};
use crate::db::identities as idb;
use crate::db::tenants as tdb;
use crate::errors::ApiError;
use crate::quota::{QuotaRequest, UsageMetric};
use crate::rpc::envelope_for;

/// Claims the service sets, which `ext` may not name (O6).
const RESERVED: &[&str] = &[
    "iss",
    "sub",
    "aud",
    "exp",
    "nbf",
    "iat",
    "jti",
    "email",
    "email_verified",
    "unverified",
    "ai_agent",
    "nonce",
    "ext",
];

/// The identity and tenant of a signing call, after the checks every signature shares: scope, then
/// `403 tenant_suspended`, `409 identity_paused` and `404` for a deleting identity, as on sends (O1),
/// then `RL_SIGN` per identity.
pub async fn signer_identity<P: Platform>(c: &Ctx<P>) -> Result<(idb::Row, tdb::Row), ApiError> {
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    if matches!(idn.status.as_str(), "deleting" | "deleted") {
        return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
    }
    #[derive(Deserialize)]
    struct S {
        status: String,
    }
    let partner_suspended = match &t.partner_id {
        Some(pid) => {
            c.p.db()
                .first::<S>(Stmt::new(
                    "SELECT status FROM partners WHERE id = ?1",
                    vec![pid.clone().into()],
                ))
                .await?
                .is_some_and(|s| s.status == "suspended")
        }
        None => false,
    };
    if t.status == "suspended" || partner_suspended {
        return Err(ApiError::new(
            ErrorCode::TenantSuspended,
            "The tenant is suspended.",
        ));
    }
    if idn.status == "paused" {
        return Err(
            ApiError::new(ErrorCode::IdentityPaused, "The identity is paused.")
                .details(json!({ "reason": idn.pause_reason })),
        );
    }
    crate::ratelimit::check(c.p.as_ref(), RlBucket::Sign, &idn.id).await?;
    Ok((idn, t))
}

/// The identity's active key, created lazily on first use (`identity.key_created`).
pub async fn active_signer<P: Platform>(c: &Ctx<P>, idn: &idb::Row) -> Result<Signer, ApiError> {
    let actor = Actor {
        key_id: &c.key().key_id,
        request_id: &c.request_id,
    };
    let (row, _) =
        identity_keys::ensure_active(c.p.as_ref(), idn, &actor, None, None, c.now).await?;
    identity_keys::open(c.p.as_ref(), &row)
}

/// The vouched address (O29): the primary address when mail to it reaches this deployment (the
/// platform domain, or a `healthy` or `degraded` tenant domain receiving through routing or SES, or
/// forwarding with a passed check); otherwise the platform address.
pub async fn vouched_address<P: Platform>(p: &P, identity_id: &str) -> Result<String, ApiError> {
    #[derive(Deserialize)]
    struct A {
        address: String,
        role: String,
        status: String,
        forwarding: Option<String>,
        kind: String,
        state: String,
        inbound: String,
    }
    let rows: Vec<A> = p
        .db()
        .query(Stmt::new(
            "SELECT a.address, a.role, a.status, a.forwarding, d.kind, d.state, d.inbound FROM addresses a JOIN domains d ON d.id = a.domain_id WHERE a.identity_id = ?1 AND a.status <> 'retired' ORDER BY a.created_at, a.id",
            vec![identity_id.into()],
        ))
        .await?;
    let reaches = |a: &A| {
        a.kind == "platform"
            || (matches!(a.state.as_str(), "healthy" | "degraded")
                && (matches!(a.inbound.as_str(), "routing" | "ses")
                    || (a.inbound == "forward" && a.forwarding.as_deref() == Some("ok"))))
    };
    if let Some(primary) = rows
        .iter()
        .find(|a| a.role == "primary" && a.status == "active")
        && reaches(primary)
    {
        return Ok(primary.address.clone());
    }
    rows.iter()
        .find(|a| a.kind == "platform")
        .map(|a| a.address.clone())
        .ok_or_else(ApiError::internal)
}

/// Counts one signature on the tenant's quota after the response; a lost count never fails the call.
pub fn record_usage<P: Platform>(c: &Ctx<P>, t: &tdb::Row, metric: UsageMetric) {
    let env = envelope_for(
        c.p.as_ref(),
        Some(&t.id),
        None,
        &c.request_id,
        None,
        QuotaRequest::RecordUsage { metric, n: 1 },
    );
    let owner = c.p.clone();
    let quota_do = t.quota_do_id.clone();
    c.p.background().wait_until(async move {
        let _ = owner
            .objects()
            .call::<_, Value>(DoClass::Quota, &quota_do, &env)
            .await;
    });
}

fn printable(s: &str) -> bool {
    s.bytes().all(|b| (0x20..0x7f).contains(&b))
}

/// An integer field within bounds, or its default.
pub fn seconds(
    body: &Value,
    field: &str,
    min: i64,
    max: i64,
    default: i64,
) -> Result<i64, ApiError> {
    match body.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => match v.as_i64() {
            Some(n) if (min..=max).contains(&n) => Ok(n),
            _ => Err(ApiError::invalid(
                field,
                format!("{field} is {min} to {max} seconds."),
            )),
        },
    }
}

struct Request {
    audience: String,
    expires_in: i64,
    nonce: Option<String>,
    ext: Option<Map<String, Value>>,
}

fn validate(body: &Value) -> Result<Request, ApiError> {
    let audience = match body.get("audience").and_then(Value::as_str) {
        Some(a) if (1..=256).contains(&a.len()) && printable(a) => a.to_owned(),
        _ => {
            return Err(ApiError::invalid(
                "audience",
                "audience is 1 to 256 characters of printable ASCII.",
            ));
        }
    };
    let expires_in = seconds(body, "expires_in", 60, 600, 300)?;
    let nonce = match body.get("nonce") {
        None | Some(Value::Null) => None,
        Some(Value::String(n)) if (1..=128).contains(&n.len()) && printable(n) => Some(n.clone()),
        Some(_) => {
            return Err(ApiError::invalid(
                "nonce",
                "nonce is 1 to 128 characters of printable ASCII.",
            ));
        }
    };
    let ext = match body.get("ext") {
        None | Some(Value::Null) => None,
        Some(Value::Object(m)) => {
            if Value::Object(m.clone()).to_string().len() > 2048 {
                return Err(ApiError::invalid("ext", "ext is at most 2 KB as JSON."));
            }
            if let Some(k) = m.keys().find(|k| RESERVED.contains(&k.as_str())) {
                return Err(
                    ApiError::invalid("ext", format!("ext may not set the {k} claim."))
                        .details(json!({ "field": "ext", "reason": "reserved_claim", "claim": k })),
                );
            }
            Some(m.clone())
        }
        Some(_) => return Err(ApiError::invalid("ext", "ext is an object.")),
    };
    Ok(Request {
        audience,
        expires_in,
        nonce,
        ext,
    })
}

/// `POST /v1/identities/{identity_id}/assertions` (`identities:sign`, `RL_SIGN`). `Idempotency-Key` is
/// ignored: a replay record would hold the token.
pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, t) = signer_identity(c).await?;
    let req = validate(&c.json()?)?;
    let signer = active_signer(c, &idn).await?;
    let p = c.p.as_ref();
    let cfg = p.config();
    let iss = format!("https://{}", cfg.api_host);
    let now_s = c.now.div_euclid(1000);
    let exp = now_s + req.expires_in;
    let jti = p.ids().new_id(IdPrefix::Request);
    let jti = jti.split_once('_').map_or(jti.as_str(), |(_, u)| u);
    let mut claims = json!({
        "iss": iss, "sub": idn.id, "aud": req.audience, "iat": now_s, "nbf": now_s, "exp": exp,
        "jti": jti, "email": vouched_address(p, &idn.id).await?, "email_verified": true,
        "unverified": {
            "name": idn.display_name, "org": t.name,
            "accountable_human": idn.owner_name.is_some() && idn.owner_email.is_some(),
        },
        "ai_agent": true,
    });
    if let Some(n) = req.nonce {
        claims["nonce"] = n.into();
    }
    if let Some(e) = req.ext {
        claims["ext"] = Value::Object(e);
    }
    let token = jwt::sign(
        &jwt::assertion_header(&signer.kid),
        claims.to_string().as_bytes(),
        &signer.seed,
    );
    record_usage(c, &t, UsageMetric::Assertions);
    Ok(Reply::json(
        201,
        json!({
            "assertion": token, "kid": signer.kid, "expires_at": rfc3339(exp * 1000),
            "jwks_uri": format!("{iss}/.well-known/jwks/{}.json", idn.id),
        }),
    ))
}
