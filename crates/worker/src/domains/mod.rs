//! Domains ([Identities and domains › Domains]): the Cloudflare and SES clients, records and their
//! checks, onboarding of `cloudflare_zone` domains, the zone permission of tenant keys (H8, H18), claim
//! tokens (H14) and the domain events.
//!
//! [Identities and domains › Domains]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

pub mod cloudflare_api;
pub mod monitor;
pub mod onboard;
pub mod records;
pub mod ses_api;
pub mod ses_control;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::org_domain;
use pylota_identity_core::policy::get_path;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{ControlDb as _, Platform, QueueProducer as _, Rng as _};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::domains as ddb;
use crate::errors::ApiError;
use crate::webhooks::envelope::{OwnerKind, WebhookJob, envelope};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

fn encode(bytes: &[u8], alphabet: &[u8; 32]) -> String {
    let mut out = String::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(alphabet[((acc >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(alphabet[((acc << (5 - bits)) & 31) as usize]));
    }
    out
}

/// `ownership_token`: 16 random bytes, Crockford base32.
pub fn ownership_token<P: Platform>(p: &P) -> String {
    encode(&p.rng().bytes::<16>(), CROCKFORD)
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len() / 2)
        .filter_map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok())
        .collect()
}

/// The claim token of a name for a tenant (S18): the first 26 characters of the lower-case base32 of
/// `SHA-256("pylota-identity claim|" ‖ api host ‖ "|" ‖ tenant ‖ "|" ‖ name)`.
pub fn claim_token(api_host: &str, tenant_id: &str, name: &str) -> String {
    let h = pylota_identity_core::ses::sha256_hex(
        format!("pylota-identity claim|{api_host}|{tenant_id}|{name}").as_bytes(),
    );
    encode(&unhex(&h), BASE32)[..26].to_owned()
}

/// The uniform `409 domain_exists` (H14): its only detail is the requester's claim record.
pub fn domain_exists(api_host: &str, tenant_id: &str, name: &str) -> ApiError {
    ApiError::new(
        ErrorCode::DomainExists,
        "This domain is already in use. If you control its DNS, publish the claim record and repeat the request with \"claim\": true.",
    )
    .details(json!({ "claim": {
        "type": "TXT", "name": format!("_pylota-identity.{name}"),
        "value": format!("pm-claim={}", claim_token(api_host, tenant_id, name)),
    } }))
}

/// `403 scope_denied` with `zone_not_allowed`, one body whether or not the zone exists.
pub fn zone_not_allowed() -> ApiError {
    ApiError::new(ErrorCode::ScopeDenied, "This key may not use that zone.")
        .details(json!({ "reason": "zone_not_allowed" }))
}

fn host_only(h: &str) -> &str {
    h.split(':').next().unwrap_or(h)
}

fn under(name: &str, zone: &str) -> bool {
    name.ends_with(&format!(".{zone}"))
}

/// The registrable domains of the platform domain, the API host and the console host.
pub fn deployment_zones<P: Platform>(p: &P, platform_domain: &str) -> Vec<String> {
    let cfg = p.config();
    [
        platform_domain,
        host_only(&cfg.api_host),
        host_only(&cfg.console_host),
    ]
    .iter()
    .map(|h| org_domain(&h.to_ascii_lowercase()).to_owned())
    .filter(|h| h.contains('.'))
    .collect()
}

#[derive(Clone, Debug, Deserialize)]
pub struct Claim {
    pub zone_name: String,
    pub zone_id: Option<String>,
    pub tenant_id: String,
}

pub async fn claims<P: Platform>(p: &P) -> Result<Vec<Claim>, ApiError> {
    Ok(p.db()
        .query(Stmt::new(
            "SELECT zone_name, zone_id, tenant_id FROM zone_claims",
            vec![],
        ))
        .await?)
}

/// What the name check granted a tenant key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grant {
    /// Platform keys skip the check.
    Platform,
    /// Under a zone this tenant claimed: `replace_mx` is allowed.
    Claimed,
    /// Strictly under a listed zone, or an exact-name entry.
    Listed,
}

/// The tenant's `domains.cloudflare_zones`.
pub fn listed_zones(policy: &Value) -> Vec<String> {
    get_path(policy, "domains.cloudflare_zones")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|z| z.as_str().map(str::to_ascii_lowercase))
                .collect()
        })
        .unwrap_or_default()
}

/// Checks `domains.cloudflare_zones` of the policy a write would leave: each entry a DNS name, or `=`
/// and a DNS name (H18).
pub fn validate_policy(policy: &Value) -> Result<(), ApiError> {
    let Some(list) = get_path(policy, "domains.cloudflare_zones").and_then(Value::as_array) else {
        return Ok(());
    };
    for (i, z) in list.iter().enumerate() {
        let ok = z.as_str().is_some_and(|z| {
            let host = z.strip_prefix('=').unwrap_or(z);
            host.contains('.')
                && pylota_identity_core::address::normalise_domain(host).as_deref() == Some(host)
        });
        if !ok {
            return Err(ApiError::invalid(
                &format!("policy.domains.cloudflare_zones[{i}]"),
                "A zone name, or = and an exact host name.",
            ));
        }
    }
    Ok(())
}

/// The zone permission by name (§ Zone permission, check 1), before any Cloudflare call.
pub fn check_name(
    name: &str,
    tenant_id: &str,
    deployment: &[String],
    claims: &[Claim],
    listed: &[String],
    replace_mx: bool,
) -> Result<Grant, ApiError> {
    if deployment.iter().any(|z| name == z || under(name, z)) {
        return Err(zone_not_allowed());
    }
    if claims
        .iter()
        .any(|c| c.tenant_id != tenant_id && (name == c.zone_name || under(name, &c.zone_name)))
    {
        return Err(zone_not_allowed());
    }
    if claims
        .iter()
        .any(|c| c.tenant_id == tenant_id && (name == c.zone_name || under(name, &c.zone_name)))
    {
        return Ok(Grant::Claimed);
    }
    let granted = listed.iter().any(|z| match z.strip_prefix('=') {
        Some(exact) => name == exact,
        None => under(name, z),
    });
    if !granted || replace_mx {
        return Err(zone_not_allowed());
    }
    Ok(Grant::Listed)
}

/// The zone permission on the zone found (check 2).
pub fn check_zone(
    name: &str,
    zone_id: &str,
    zone_name: &str,
    tenant_id: &str,
    claims: &[Claim],
    listed: &[String],
) -> Result<(), ApiError> {
    let claim = claims
        .iter()
        .find(|c| c.zone_id.as_deref() == Some(zone_id) || c.zone_name == zone_name);
    match claim {
        Some(c) if c.tenant_id == tenant_id => Ok(()),
        Some(_) => Err(zone_not_allowed()),
        None => {
            let ok = listed.iter().any(|z| match z.strip_prefix('=') {
                Some(exact) => exact == name && (name == zone_name || under(name, zone_name)),
                None => z == zone_name,
            });
            if ok { Ok(()) } else { Err(zone_not_allowed()) }
        }
    }
}

/// Appends a domain event to the event index (owner `domain`, the payload kept in the index) and
/// queues its fan-out; a lost fan-out is repaired by the every-minute sweep. The ID is derived from
/// `seed`, so a retried transition writes one event.
pub async fn emit<P: Platform>(
    p: &P,
    row: &ddb::Row,
    event_type: &str,
    data: &Value,
    seed: &str,
    now: i64,
) -> Result<(), ApiError> {
    let event_id = pylota_identity_core::ids::derived(
        IdPrefix::Event,
        now,
        &p.secrets().hash_key,
        &format!("{event_type}:{}:{seed}", row.id),
    );
    let owner = if row.monitor_do_id.is_empty() {
        row.id.clone()
    } else {
        row.monitor_do_id.clone()
    };
    let body = envelope(
        &event_id,
        event_type,
        now,
        row.tenant_id.as_deref(),
        None,
        None,
        data,
    );
    p.db()
        .execute(Stmt::new(
            "INSERT OR IGNORE INTO event_index (id, tenant_id, identity_id, type, owner_kind, owner_id, partner_id, payload_json, occurred_at) VALUES (?1, ?2, NULL, ?3, 'domain', ?4, (SELECT partner_id FROM tenants WHERE id = ?2), ?5, ?6)",
            vec![
                event_id.clone().into(),
                row.tenant_id.clone().into(),
                event_type.into(),
                owner.clone().into(),
                body.into(),
                now.into(),
            ],
        ))
        .await?;
    let job = WebhookJob::Fanout {
        v: 1,
        event_id,
        event_type: event_type.to_owned(),
        tenant_id: row.tenant_id.clone(),
        identity_id: None,
        owner_kind: OwnerKind::Domain,
        owner_id: owner,
        occurred_at: now,
    };
    let _ = p.queues().send(QueueName::Webhooks, &job, 0).await;
    Ok(())
}

/// The `domain_remove` job for a domain (Domain removal), queued behind the `JobRunner` (which runs it
/// from M14). `pending_rules` are the `(zone_id, routing_rule_id)` pairs of the pending addresses it
/// deletes.
pub fn remove_job<P: Platform>(
    p: &P,
    domain_id: &str,
    tenant_id: Option<&str>,
    reason: &str,
    pending_rules: &[(String, String)],
    now: i64,
) -> Result<Stmt, ApiError> {
    use pylota_identity_platform::Ids as _;
    use pylota_identity_platform::ObjectClient as _;
    use pylota_identity_platform::durable::DoClass;
    let runner = p
        .objects()
        .new_object_id(DoClass::Job)
        .map_err(|_| ApiError::internal())?;
    Ok(Stmt::new(
        "INSERT INTO jobs (id, tenant_id, kind, status, runner_do_id, params_json, created_by_key_id, created_at, updated_at) VALUES (?1, ?2, 'domain_remove', 'queued', ?3, ?4, NULL, ?5, ?5)",
        vec![
            p.ids().new_id(IdPrefix::Job).into(),
            tenant_id.map(str::to_owned).into(),
            runner.into(),
            json!({ "domain_id": domain_id, "reason": reason, "pending_rules": pending_rules })
                .to_string()
                .into(),
            now.into(),
        ],
    ))
}

/// `{ "domain": … }`, the data of `domain.*` events.
pub fn domain_data(row: &ddb::Row) -> Value {
    json!({ "domain": row.json() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(z: &str, t: &str) -> Claim {
        Claim {
            zone_name: z.into(),
            zone_id: Some(format!("id-{z}")),
            tenant_id: t.into(),
        }
    }

    #[test]
    fn zone_permission() {
        let dep = vec!["agents.example".to_owned()];
        let claims = vec![claim("b.example", "ten_b"), claim("own.example", "ten_a")];
        let listed = vec!["listed.example".to_owned(), "=notify.pylota.io".to_owned()];
        let by_name = |n: &str, mx: bool| check_name(n, "ten_a", &dep, &claims, &listed, mx);
        assert_eq!(
            by_name("mail.listed.example", false).unwrap(),
            Grant::Listed
        );
        assert!(
            by_name("listed.example", false).is_err(),
            "a listed zone's apex is platform-only"
        );
        assert!(
            by_name("mail.listed.example", true).is_err(),
            "replace_mx only on claimed zones"
        );
        assert_eq!(by_name("own.example", true).unwrap(), Grant::Claimed);
        assert_eq!(by_name("notify.pylota.io", false).unwrap(), Grant::Listed);
        for n in [
            "a.notify.pylota.io",
            "pylota.io",
            "accounts.pylota.io",
            "x.agents.example",
            "x.b.example",
            "unlisted.example",
        ] {
            assert!(by_name(n, false).is_err(), "{n}");
        }
        // On the zone found: a listed parent never reaches a zone another tenant claimed.
        assert!(
            check_zone(
                "mail.listed.example",
                "z1",
                "listed.example",
                "ten_a",
                &claims,
                &listed
            )
            .is_ok()
        );
        assert!(
            check_zone(
                "x.b.example",
                "id-b.example",
                "b.example",
                "ten_a",
                &claims,
                &listed
            )
            .is_err()
        );
        assert!(
            check_zone(
                "notify.pylota.io",
                "z9",
                "pylota.io",
                "ten_a",
                &claims,
                &listed
            )
            .is_ok()
        );
        assert!(
            check_zone(
                "other.pylota.io",
                "z9",
                "pylota.io",
                "ten_a",
                &claims,
                &listed
            )
            .is_err()
        );
    }

    #[test]
    fn claim_token_shape() {
        let t = claim_token("api.example", "ten_1", "victim.example");
        assert_eq!(t.len(), 26);
        assert!(
            t.bytes()
                .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
        );
        assert_ne!(t, claim_token("api.example", "ten_2", "victim.example"));
    }
}
