//! The `email()` handler ([Inbound › The email() handler]): one call per envelope recipient. It decides
//! accept, reject or temporary failure, writes the raw message to R2 before acknowledging, and queues a
//! pointer; parsing happens in the `pm-inbound` consumer.
//!
//! Every special branch has its place here, in the order of the design's steps, so later milestones
//! fill a branch in and never reorder the handler (build plan M7): `journal` (M9), `pm-probe` and the
//! forwarding check (M23) are stubs until then; the role-name relay is `inbound::role_mail`.
//!
//! [Inbound › The email() handler]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use std::cell::RefCell;
use std::collections::HashMap;

use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::{self, EnvelopeError};
use pylota_identity_platform::app::{EmailIn, EmailOutcome};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::r2::{BlobBody, BlobMeta};
use pylota_identity_platform::{
    BlobStore as _, Clock as _, ControlDb as _, Ids as _, Platform, QueueProducer as _,
};
use serde::Deserialize;

use crate::consumers::inbound::{InboundJob, InboundPointer, RawSource, StagedPointer};

/// Why the handler raised a temporary failure (logged by variant, never with the address).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempFail {
    Storage,
    Queue,
    TenantSuspended,
    RoleRate,
    Busy,
    Check,
}

impl TempFail {
    fn as_str(self) -> &'static str {
        match self {
            TempFail::Storage => "storage",
            TempFail::Queue => "queue",
            TempFail::TenantSuspended => "tenant_suspended",
            TempFail::RoleRate => "role_rate",
            TempFail::Busy => "busy",
            TempFail::Check => "check",
        }
    }
}

pub const REJECT_UNKNOWN: &str = "550 5.1.1 Recipient address rejected: user unknown";
pub const REJECT_RETIRED: &str =
    "550 5.1.6 Recipient address has moved; contact the sender by other means";
pub const REJECT_DISABLED: &str = "550 5.2.1 Mailbox disabled, not accepting messages";

/// The RFC 2142 operational names relayed to `PM_SECURITY_CONTACT` (Role mail relay).
const OPERATIONAL: &[&str] = &[
    "postmaster",
    "abuse",
    "security",
    "hostmaster",
    "webmaster",
    "noc",
];
/// A suspended tenant's mail is retried by the sender for this long, then refused.
pub const SUSPENDED_GRACE_MS: i64 = 5 * 86_400_000;
/// The directory cache (Inbound › Steps, step 4).
const CACHE_TTL_MS: i64 = 60_000;
const CACHE_MAX: usize = 10_000;
/// Uncached misses per domain per second before the dictionary guard answers `Busy` (A16).
pub const GUARD_PER_SECOND: u32 = 20;
/// How long the deployment domain list is kept (staging, J7).
const DOMAINS_TTL_MS: i64 = 300_000;

/// One directory row (step 4).
#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    pub address_status: String,
    pub identity_id: String,
    pub tenant_id: String,
    pub mailbox_do_id: String,
    pub identity_status: String,
    pub tenant_status: String,
    pub mode: String,
    pub suspended_at: Option<i64>,
}

#[derive(Default)]
struct Isolate {
    /// `base` → (entry or a miss, expiry).
    cache: HashMap<String, (Option<Entry>, i64)>,
    /// domain → (second, uncached misses in it).
    guard: HashMap<String, (i64, u32)>,
    /// The deployment's domains, and when they were read.
    domains: Option<(Vec<String>, i64)>,
}

thread_local! {
    static ISOLATE: RefCell<Isolate> = RefCell::new(Isolate::default());
}

/// The directory query (step 4).
pub fn lookup(base: &str) -> Stmt {
    Stmt::new(
        "SELECT a.status AS address_status, a.identity_id, a.tenant_id, i.mailbox_do_id, i.status AS identity_status, t.status AS tenant_status, t.mode, t.suspended_at \
         FROM addresses a JOIN identities i ON i.id = a.identity_id JOIN tenants t ON t.id = a.tenant_id WHERE a.address = ?1",
        vec![base.into()],
    )
}

fn cached(base: &str, now: i64) -> Option<Option<Entry>> {
    ISOLATE.with(|i| {
        i.borrow()
            .cache
            .get(base)
            .filter(|(_, exp)| *exp > now)
            .map(|(e, _)| e.clone())
    })
}

fn remember(base: &str, entry: Option<Entry>, now: i64) {
    ISOLATE.with(|i| {
        let mut i = i.borrow_mut();
        if i.cache.len() >= CACHE_MAX {
            i.cache.retain(|_, (_, exp)| *exp > now);
            if i.cache.len() >= CACHE_MAX {
                i.cache.clear();
            }
        }
        i.cache.insert(base.to_owned(), (entry, now + CACHE_TTL_MS));
    });
}

/// Whether the dictionary guard is closed for `domain` in this second (A16).
fn guard_closed(domain: &str, now: i64) -> bool {
    let second = now / 1000;
    ISOLATE.with(|i| {
        i.borrow()
            .guard
            .get(domain)
            .is_some_and(|(s, n)| *s == second && *n >= GUARD_PER_SECOND)
    })
}

fn count_miss(domain: &str, now: i64) {
    let second = now / 1000;
    ISOLATE.with(|i| {
        let mut i = i.borrow_mut();
        let e = i.guard.entry(domain.to_owned()).or_insert((second, 0));
        if e.0 != second {
            *e = (second, 0);
        }
        e.1 += 1;
    });
}

/// The deployment's domains for staging: the platform domain and every live `domains` name. `None`
/// when they could never be read (then any domain may be staged).
pub(crate) async fn deployment_domains<P: Platform>(p: &P, now: i64) -> Option<Vec<String>> {
    let fresh = ISOLATE.with(|i| {
        i.borrow()
            .domains
            .as_ref()
            .filter(|(_, at)| now - at < DOMAINS_TTL_MS)
            .map(|(d, _)| d.clone())
    });
    if fresh.is_some() {
        return fresh;
    }
    #[derive(Deserialize)]
    struct N {
        name: String,
    }
    match p
        .db()
        .query::<N>(Stmt::new(
            "SELECT name FROM domains WHERE state <> 'removed'",
            vec![],
        ))
        .await
    {
        Ok(rows) => {
            let mut names: Vec<String> = rows.into_iter().map(|r| r.name).collect();
            names.push(p.config().platform_domain.clone());
            ISOLATE.with(|i| i.borrow_mut().domains = Some((names.clone(), now)));
            Some(names)
        }
        Err(_) => ISOLATE.with(|i| i.borrow().domains.as_ref().map(|(d, _)| d.clone())),
    }
}

/// The local-part form staging accepts during a directory outage.
fn username_form(local: &str) -> bool {
    let b = local.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
        })
}

/// The step-5 decision for a directory row.
pub fn decide(e: Option<&Entry>, now: i64) -> Result<(), Decision> {
    let Some(e) = e else {
        return Err(Decision::Reject(REJECT_UNKNOWN));
    };
    match e.address_status.as_str() {
        "pending" => return Err(Decision::Reject(REJECT_UNKNOWN)),
        "retired" => return Err(Decision::Reject(REJECT_RETIRED)),
        _ => {}
    }
    if matches!(e.identity_status.as_str(), "deleting" | "deleted")
        || matches!(e.tenant_status.as_str(), "erasing" | "erased")
    {
        return Err(Decision::Reject(REJECT_UNKNOWN));
    }
    if e.tenant_status == "suspended" {
        return Err(
            if now - e.suspended_at.unwrap_or(now) < SUSPENDED_GRACE_MS {
                Decision::Temp(TempFail::TenantSuspended)
            } else {
                Decision::Reject(REJECT_DISABLED)
            },
        );
    }
    Ok(())
}

/// What step 5 decided against accepting.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Reject(&'static str),
    Temp(TempFail),
}

/// The handler: `Accept`, `Reject` (a permanent `5xx`) or `TempFail`.
pub async fn handle<P: Platform>(p: &P, msg: &EmailIn) -> EmailOutcome {
    // A restore freeze (Observability § 9): a temporary failure before anything is read, so nothing
    // is accepted that the restore could lose; senders retry.
    if p.config().frozen {
        crate::metrics::count_with(p, "inbound_received_total", None, &["tempfail_frozen"], 1);
        return EmailOutcome::TempFail;
    }
    match handle_email(p, msg).await {
        Ok(()) => EmailOutcome::Accept,
        Err(Decision::Reject(smtp)) => EmailOutcome::Reject {
            smtp: smtp.to_owned(),
        },
        Err(Decision::Temp(t)) => {
            crate::log::warn("inbound_temp_fail", &[("reason", t.as_str().into())]);
            crate::metrics::count_with(
                p,
                "inbound_received_total",
                None,
                &[match t {
                    TempFail::TenantSuspended => "tempfail_suspended",
                    _ => "tempfail_storage",
                }],
                1,
            );
            EmailOutcome::TempFail
        }
    }
}

async fn handle_email<P: Platform>(p: &P, msg: &EmailIn) -> Result<(), Decision> {
    // 1. Start.
    let request_id = crate::request_id::new(p);
    let received_at = p.clock().now_ms();
    let cfg = p.config();

    // 2. Normalise the recipient.
    let env = match address::parse_envelope(&msg.to) {
        Ok(env) => env,
        Err(EnvelopeError::NonAsciiLocal | EnvelopeError::Malformed) => {
            return Err(Decision::Reject(REJECT_UNKNOWN));
        }
    };
    let base = env.base();

    // 3. Platform-domain special addresses, then tenant-domain role addresses.
    if env.domain == cfg.platform_domain {
        // The forwarding check (M23 creates the tokens): every token is unknown until then, and the
        // message continues as ordinary mail.
        match env.base_local.as_str() {
            // Message-ID learning, strategy B (M9): nothing is stored.
            "journal" if env.detail.is_some() => return Ok(()),
            // The alignment probe of an smtp_relay domain (M23): no effect for an unknown token.
            "pm-probe" if env.detail.is_some() => return Ok(()),
            // Relayed to `PM_SECURITY_CONTACT` through the system identity.
            l if OPERATIONAL.contains(&l) => {
                return crate::inbound::role_mail::accept(
                    p,
                    msg,
                    &env,
                    true,
                    received_at,
                    &request_id,
                )
                .await;
            }
            _ => {}
        }
    } else if matches!(env.base_local.as_str(), "postmaster" | "abuse") {
        // Relayed to the tenant's owner, else to `PM_SECURITY_CONTACT`.
        return crate::inbound::role_mail::accept(p, msg, &env, false, received_at, &request_id)
            .await;
    }

    // 4. Directory lookup: the cache, the dictionary guard, then D1.
    let entry = match cached(&base, received_at) {
        Some(e) => e,
        None => {
            if guard_closed(&env.domain, received_at) {
                crate::metrics::count(p, "inbound_dictionary_guard_total", None, 1);
                return Err(Decision::Temp(TempFail::Busy));
            }
            match p.db().first::<Entry>(lookup(&base)).await {
                Ok(e) => {
                    if e.is_none() {
                        count_miss(&env.domain, received_at);
                    }
                    remember(&base, e.clone(), received_at);
                    e
                }
                Err(_) => {
                    return stage(p, msg, &env, &base, received_at, &request_id).await;
                }
            }
        }
    };

    // 5. Decide.
    decide(entry.as_ref(), received_at)?;
    let e = entry.expect("decide accepts only a row");

    // 6. The raw message to R2 before acknowledging (FR-IN-1, J1).
    let message_id = p.ids().new_id(IdPrefix::Message);
    let key = format!(
        "t/{}/i/{}/m/{message_id}/raw.eml",
        e.tenant_id, e.identity_id
    );
    let meta = BlobMeta {
        content_type: Some("message/rfc822".into()),
        custom: vec![
            ("tenant".into(), e.tenant_id.clone()),
            ("identity".into(), e.identity_id.clone()),
            ("message".into(), message_id.clone()),
            // What a restore's reconcile job re-queues the pointer with (Restore from PITR, step 5).
            ("envelope_from".into(), msg.from.clone()),
            ("envelope_to".into(), env_to(&env)),
        ],
    };
    put_with_retries(p, &key, msg, &meta).await?;

    // 7. Queue the pointer.
    let job = InboundJob::Message(InboundPointer {
        v: 1,
        source: RawSource::Routing,
        message_id: Some(message_id.clone()),
        tenant_id: Some(e.tenant_id.clone()),
        identity_id: Some(e.identity_id.clone()),
        r2_key: Some(key.clone()),
        raw_size: msg.raw_size,
        envelope_from: msg.from.clone(),
        envelope_to: env_to(&env),
        received_at,
        request_id: request_id.clone(),
        loopback: None,
    });
    if !send_with_retries(p, &job).await {
        if p.blobs().delete(std::slice::from_ref(&key)).await.is_err() {
            crate::metrics::count(p, "inbound_orphan_raw_total", None, 1);
        }
        return Err(Decision::Temp(TempFail::Queue));
    }

    // 8. Accepted.
    crate::log::info(
        "inbound_accepted",
        &[
            ("tenant", e.tenant_id.clone().into()),
            ("identity", e.identity_id.clone().into()),
            ("message", message_id.into()),
            ("size", (msg.raw_size as i64).into()),
            (
                "recipient",
                address::address_hash(&p.secrets().hash_key, &base)[..16]
                    .to_owned()
                    .into(),
            ),
        ],
    );
    crate::metrics::count_with(
        p,
        "inbound_received_total",
        Some(&e.tenant_id),
        &["accepted"],
        1,
    );
    Ok(())
}

/// The normalised envelope recipient with its detail.
fn env_to(env: &address::Envelope) -> String {
    match &env.detail {
        Some(d) => format!("{}+{d}@{}", env.base_local, env.domain),
        None => env.base(),
    }
}

pub(crate) async fn put_with_retries<P: Platform>(
    p: &P,
    key: &str,
    msg: &EmailIn,
    meta: &BlobMeta,
) -> Result<(), Decision> {
    for (n, wait) in [0u32, 100, 300].into_iter().enumerate() {
        if n > 0 {
            p.clock().sleep(wait).await;
        }
        if p.blobs()
            .put(key, BlobBody::Js(msg.raw.clone()), meta)
            .await
            .is_ok()
        {
            return Ok(());
        }
    }
    Err(Decision::Temp(TempFail::Storage))
}

pub(crate) async fn send_with_retries<P: Platform>(p: &P, job: &InboundJob) -> bool {
    for (n, wait) in [0u32, 100, 300].into_iter().enumerate() {
        if n > 0 {
            p.clock().sleep(wait).await;
        }
        if p.queues().send(QueueName::Inbound, job, 0).await.is_ok() {
            return true;
        }
    }
    false
}

/// Staging when the directory is unavailable (J7): only mail that could be routable, under the same
/// dictionary guard; anything else is `550 5.1.1`.
async fn stage<P: Platform>(
    p: &P,
    msg: &EmailIn,
    env: &address::Envelope,
    base: &str,
    received_at: i64,
    request_id: &str,
) -> Result<(), Decision> {
    let routable = deployment_domains(p, received_at)
        .await
        .is_none_or(|d| d.contains(&env.domain));
    if !routable || !username_form(&env.base_local) {
        return Err(Decision::Reject(REJECT_UNKNOWN));
    }
    count_miss(&env.domain, received_at);
    let date = pylota_identity_core::days::utc_day(received_at).replace('-', "/");
    let ulid = p.ids().new_id(IdPrefix::Message);
    let key = format!(
        "inbound-staging/{date}/{}.eml",
        ulid.trim_start_matches("msg_")
    );
    let envelope_to = env_to(env);
    let meta = BlobMeta {
        content_type: Some("message/rfc822".into()),
        custom: vec![
            (
                "envelope_to_hash".into(),
                address::address_hash(&p.secrets().hash_key, base),
            ),
            ("envelope_from".into(), msg.from.clone()),
            ("envelope_to".into(), envelope_to.clone()),
            ("received_at".into(), received_at.to_string()),
        ],
    };
    put_with_retries(p, &key, msg, &meta).await?;
    let job = InboundJob::Staged(StagedPointer {
        v: 1,
        staging_key: key.clone(),
        raw_size: msg.raw_size,
        envelope_from: msg.from.clone(),
        envelope_to,
        received_at,
        request_id: request_id.to_owned(),
    });
    if !send_with_retries(p, &job).await {
        let _ = p.blobs().delete(std::slice::from_ref(&key)).await;
        return Err(Decision::Temp(TempFail::Queue));
    }
    crate::metrics::count_with(p, "inbound_received_total", None, &["staged"], 1);
    Ok(())
}

#[cfg(test)]
pub fn reset_isolate() {
    ISOLATE.with(|i| *i.borrow_mut() = Isolate::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(address: &str, identity: &str, tenant: &str, at: Option<i64>) -> Entry {
        Entry {
            address_status: address.into(),
            identity_id: "idn".into(),
            tenant_id: "ten".into(),
            mailbox_do_id: "m".into(),
            identity_status: identity.into(),
            tenant_status: tenant.into(),
            mode: "live".into(),
            suspended_at: at,
        }
    }

    /// A6: the step-5 table, first match.
    #[test]
    fn decide_table() {
        let now = 10 * 86_400_000;
        assert_eq!(decide(None, now), Err(Decision::Reject(REJECT_UNKNOWN)));
        assert_eq!(
            decide(Some(&entry("pending", "active", "active", None)), now),
            Err(Decision::Reject(REJECT_UNKNOWN))
        );
        assert_eq!(
            decide(Some(&entry("retired", "active", "active", None)), now),
            Err(Decision::Reject(REJECT_RETIRED))
        );
        assert_eq!(
            decide(Some(&entry("active", "deleting", "active", None)), now),
            Err(Decision::Reject(REJECT_UNKNOWN))
        );
        assert_eq!(
            decide(
                Some(&entry("active", "active", "suspended", Some(now - 1))),
                now
            ),
            Err(Decision::Temp(TempFail::TenantSuspended))
        );
        assert_eq!(
            decide(
                Some(&entry(
                    "active",
                    "active",
                    "suspended",
                    Some(now - SUSPENDED_GRACE_MS)
                )),
                now
            ),
            Err(Decision::Reject(REJECT_DISABLED))
        );
        assert_eq!(
            decide(Some(&entry("retiring", "paused", "active", None)), now),
            Ok(())
        );
    }

    #[test]
    fn staging_form() {
        assert!(username_form("bookings.acme"));
        assert!(!username_form("-x") && !username_form("") && !username_form("a+b"));
    }
}
