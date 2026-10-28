//! Role mail relay ([Inbound › Role mail relay]): mail to an operational name of the platform domain,
//! or to `postmaster@` / `abuse@` of a tenant domain, becomes a new message of the system identity to
//! `PM_SECURITY_CONTACT` or the tenant's owner, with every control a send has (A4, D14). `accept` is
//! step 3 of the `email()` handler; `relay` is the `pm-inbound` consumer's part.
//!
//! [Inbound › Role mail relay]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::{self, Envelope};
use pylota_identity_core::auth::{DnsAnswers, Verdict};
use pylota_identity_core::mime::{self, Caps};
use pylota_identity_core::ses::sha256_hex;
use pylota_identity_platform::app::EmailIn;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::{DoClass, RpcResult};
use pylota_identity_platform::r2::BlobMeta;
use pylota_identity_platform::{
    BlobStore as _, Clock as _, ControlDb as _, Ids as _, ObjectClient as _, Platform,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::consumers::inbound::{Done, InboundJob};
use crate::db::{identities as idb, tenants as tdb};
use crate::domains::monitor::{self, DomainRequest};
use crate::email::{Decision, REJECT_UNKNOWN, TempFail};
use crate::mailbox::MailboxRequest;
use crate::mailbox::submit::SubmitInput;
use crate::quota::{QuotaRequest, SystemMailAnswer, SystemMailClass};

pub const REJECT_LOOP: &str = "550 5.4.6 Routing loop detected";
/// The header the relay adds, and the loop guard refuses.
pub const HOP_HEADER: &str = "X-Pylota-Identity-Role-Hop";
/// The original is attached whole up to this size (security contact, clean messages only).
const ORIGINAL_MAX: usize = 4 * 1024 * 1024;
const HEADERS_MAX: usize = 64 * 1024;
const OWNER_TEXT_CHARS: usize = 2_000;
/// A pointer the system-mail budget still refuses this long after it was received is dropped.
const BUDGET_GIVE_UP_MS: i64 = 86_400_000;
const BUDGET_RETRY_S: u32 = 3_600;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoleMailPointer {
    pub v: u8,
    pub staging_key: String,
    pub domain: String,
    pub base_local: String,
    /// `security_contact` or `tenant_owner`.
    pub destination_kind: String,
    pub tenant_id: Option<String>,
    pub raw_size: u64,
    pub envelope_from: String,
    pub received_at: i64,
    pub request_id: String,
}

/// Where a role message goes.
struct Destination {
    address: String,
    kind: &'static str,
    tenant_id: Option<String>,
    monitor_do_id: String,
    domain_id: String,
}

#[derive(Deserialize)]
struct OwnerRow {
    id: String,
    tenant_id: Option<String>,
    monitor_do_id: String,
    email: Option<String>,
}

#[derive(Deserialize)]
struct PlatformRow {
    id: String,
    monitor_do_id: String,
}

#[cfg(feature = "itest-hooks")]
thread_local! {
    /// `/__test/security-contact`: `Some(None)` reads as unset.
    static CONTACT: std::cell::RefCell<Option<Option<String>>> = const { std::cell::RefCell::new(None) };
}

/// The itest override of `PM_SECURITY_CONTACT`; `None` restores the configured value.
#[cfg(feature = "itest-hooks")]
pub fn override_contact(v: Option<Option<String>>) {
    CONTACT.with(|c| *c.borrow_mut() = v);
}

fn configured_contact<P: Platform>(p: &P) -> Option<String> {
    #[cfg(feature = "itest-hooks")]
    if let Some(v) = CONTACT.with(|c| c.borrow().clone()) {
        return v;
    }
    p.config().security_contact.clone()
}

/// `PM_SECURITY_CONTACT` as a bare address, unless it is not one or routes back into the deployment.
async fn security_contact<P: Platform>(p: &P, now: i64) -> Option<String> {
    let raw = configured_contact(p)?;
    let bare = raw.trim().strip_prefix("mailto:").unwrap_or(raw.trim());
    let env = address::parse_envelope(bare).ok()?;
    let a = env.base();
    (!routes_back(p, &a, now).await).then_some(a)
}

/// Whether mail to `address` would arrive back at this deployment: its domain is the platform domain
/// or a live `domains` row.
async fn routes_back<P: Platform>(p: &P, address: &str, now: i64) -> bool {
    let domain = address
        .rsplit_once('@')
        .map(|(_, d)| d.to_ascii_lowercase())
        .unwrap_or_default();
    if domain == p.config().platform_domain {
        return true;
    }
    crate::email::deployment_domains(p, now)
        .await
        .is_some_and(|d| d.contains(&domain))
}

/// Step 1: the destination of a role message, `Ok(None)` when there is none (`550 5.1.1`).
async fn destination<P: Platform>(
    p: &P,
    domain: &str,
    platform: bool,
    now: i64,
) -> Result<Option<Destination>, ()> {
    if platform {
        let row: Option<PlatformRow> = p
            .db()
            .first(Stmt::new(
                "SELECT id, monitor_do_id FROM domains WHERE kind = 'platform' AND name = ?1",
                vec![domain.into()],
            ))
            .await
            .map_err(|_| ())?;
        let Some(row) = row else { return Ok(None) };
        return Ok(security_contact(p, now).await.map(|a| Destination {
            address: a,
            kind: "security_contact",
            tenant_id: None,
            monitor_do_id: row.monitor_do_id,
            domain_id: row.id,
        }));
    }
    let row: Option<OwnerRow> = p
        .db()
        .first(Stmt::new(
            "SELECT d.id, d.tenant_id, d.monitor_do_id, u.email FROM domains d LEFT JOIN members m ON m.tenant_id = d.tenant_id AND m.role = 'owner' LEFT JOIN users u ON u.id = m.user_id WHERE d.name = ?1 AND d.kind <> 'platform' AND d.state NOT IN ('removing', 'removed')",
            vec![domain.into()],
        ))
        .await
        .map_err(|_| ())?;
    let Some(row) = row else { return Ok(None) };
    if let Some(owner) = row.email.as_deref().map(str::to_ascii_lowercase)
        && owner.contains('@')
        && !routes_back(p, &owner, now).await
    {
        return Ok(Some(Destination {
            address: owner,
            kind: "tenant_owner",
            tenant_id: row.tenant_id,
            monitor_do_id: row.monitor_do_id,
            domain_id: row.id,
        }));
    }
    Ok(security_contact(p, now).await.map(|a| Destination {
        address: a,
        kind: "security_contact",
        tenant_id: row.tenant_id,
        monitor_do_id: row.monitor_do_id,
        domain_id: row.id,
    }))
}

/// Whether the envelope sender is an address of the system identity.
async fn from_system<P: Platform>(p: &P, sender: &str) -> Result<bool, ()> {
    if sender.is_empty() {
        return Ok(false);
    }
    #[derive(Deserialize)]
    struct N {
        #[allow(dead_code)]
        n: i64,
    }
    let base = address::parse_envelope(sender)
        .map(|e| e.base())
        .unwrap_or_else(|_| sender.to_ascii_lowercase());
    p.db()
        .first::<N>(Stmt::new(
            "SELECT 1 AS n FROM addresses a JOIN identities i ON i.id = a.identity_id WHERE i.is_system = 1 AND a.address = ?1",
            vec![base.into()],
        ))
        .await
        .map(|r| r.is_some())
        .map_err(|_| ())
}

/// Step 3 of the `email()` handler for a role address: destination, loop guard, rate limits, staging.
pub async fn accept<P: Platform>(
    p: &P,
    msg: &EmailIn,
    env: &Envelope,
    platform: bool,
    received_at: i64,
    request_id: &str,
) -> Result<(), Decision> {
    // 1. Destination.
    let Some(dest) = destination(p, &env.domain, platform, received_at)
        .await
        .map_err(|()| Decision::Temp(TempFail::Check))?
    else {
        return Err(Decision::Reject(REJECT_UNKNOWN));
    };
    // 2. Loop guard.
    if msg
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case(HOP_HEADER))
        || from_system(p, &msg.from)
            .await
            .map_err(|()| Decision::Temp(TempFail::Check))?
    {
        return Err(Decision::Reject(REJECT_LOOP));
    }
    // 3. Rate limits on the domain's monitor.
    let sender = if msg.from.is_empty() {
        "<>".to_owned()
    } else {
        msg.from.to_ascii_lowercase()
    };
    let sender_hash = address::address_hash(&p.secrets().hash_key, &sender);
    let admitted = match monitor::call(
        p,
        &dest.monitor_do_id,
        DomainRequest::AdmitRoleMail {
            domain_id: dest.domain_id.clone(),
            sender_hash,
            now: received_at,
        },
    )
    .await
    {
        Ok(v) => v["admitted"] == true,
        Err(_) => return Err(Decision::Temp(TempFail::Check)),
    };
    if !admitted {
        return Err(Decision::Temp(TempFail::RoleRate));
    }
    // 4. Accept: stage and queue.
    let date = pylota_identity_core::days::utc_day(received_at).replace('-', "/");
    let ulid = p.ids().new_id(IdPrefix::Message);
    let key = format!(
        "inbound-staging/role/{date}/{}.eml",
        ulid.trim_start_matches("msg_")
    );
    let meta = BlobMeta {
        content_type: Some("message/rfc822".into()),
        custom: vec![("received_at".into(), received_at.to_string())],
    };
    crate::email::put_with_retries(p, &key, msg, &meta).await?;
    let job = InboundJob::RoleMail(RoleMailPointer {
        v: 1,
        staging_key: key.clone(),
        domain: env.domain.clone(),
        base_local: env.base_local.clone(),
        destination_kind: dest.kind.to_owned(),
        tenant_id: dest.tenant_id,
        raw_size: msg.raw_size,
        envelope_from: msg.from.clone(),
        received_at,
        request_id: request_id.to_owned(),
    });
    if !crate::email::send_with_retries(p, &job).await {
        let _ = p.blobs().delete(std::slice::from_ref(&key)).await;
        return Err(Decision::Temp(TempFail::Queue));
    }
    crate::metrics::count(p, "role_mail_accepted_total", None, 1);
    Ok(())
}

fn failed<P: Platform>(p: &P, reason: &'static str) {
    crate::log::warn("role_mail_failed", &[("reason", reason.into())]);
    crate::metrics::count(p, "role_mail_failed_total", None, 1);
}

/// The header block of `raw`, at most [`HEADERS_MAX`] bytes.
fn header_block(raw: &[u8]) -> &[u8] {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 2)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| i + 1))
        .unwrap_or(raw.len());
    &raw[..end.min(HEADERS_MAX)]
}

/// Every `http://` and `https://` made unclickable (`hxxp://`, `hxxps://`), in any case.
pub fn defang(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &lower[i..];
        if rest.starts_with("http://") || rest.starts_with("https://") {
            out.push_str("hxxp");
            i += 4;
            continue;
        }
        let c = text[i..].chars().next().expect("inside the text");
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn verdict_str(v: Verdict) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The `pm-inbound` consumer's relay of one staged role message (steps 5 and 6).
pub async fn relay<P: Platform>(p: &P, ptr: RoleMailPointer) -> Done {
    let now = p.clock().now_ms();
    let drop_staged = || async {
        let _ = p
            .blobs()
            .delete(std::slice::from_ref(&ptr.staging_key))
            .await;
    };
    // The destination again: it is never in the queue message.
    let platform = ptr.domain == p.config().platform_domain;
    let dest = match destination(p, &ptr.domain, platform, now).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            failed(p, "no_destination");
            drop_staged().await;
            return Done::Ack;
        }
        Err(()) => return Done::Retry,
    };
    let raw = match p.blobs().get(&ptr.staging_key).await {
        Ok(Some(b)) => b.body.to_vec(),
        Ok(None) => return Done::Ack,
        Err(_) => return Done::Retry,
    };

    // The system identity and its tenant.
    #[derive(Deserialize)]
    struct I {
        id: String,
    }
    let sys: Option<I> = match p
        .db()
        .first(Stmt::new(
            "SELECT id FROM identities WHERE is_system = 1 LIMIT 1",
            vec![],
        ))
        .await
    {
        Ok(r) => r,
        Err(_) => return Done::Retry,
    };
    let Some(sys) = sys else {
        failed(p, "no_system_identity");
        drop_staged().await;
        return Done::Ack;
    };
    let idn = match p.db().first::<idb::Row>(idb::get(&sys.id)).await {
        Ok(Some(i)) => i,
        _ => return Done::Retry,
    };
    let t = match p.db().first::<tdb::Row>(tdb::get(&idn.tenant_id)).await {
        Ok(Some(t)) => t,
        _ => return Done::Retry,
    };

    // The system-mail budget: role relays share the notification class of the system identity.
    let dest_hash = address::address_hash(&p.secrets().hash_key, &dest.address);
    let budget = crate::quota::call(
        p,
        &t.quota_do_id,
        &t.id,
        &ptr.request_id,
        QuotaRequest::SystemMail {
            class: SystemMailClass::Notification,
            keys: vec![dest_hash.clone(), format!("role:{}", ptr.domain)],
            day: pylota_identity_core::days::utc_day(now),
        },
    )
    .await;
    match budget.map(|v| serde_json::from_value::<SystemMailAnswer>(v).ok()) {
        Ok(Some(SystemMailAnswer::Ok)) => {}
        Ok(_) if now - ptr.received_at >= BUDGET_GIVE_UP_MS => {
            failed(p, "budget");
            drop_staged().await;
            return Done::Ack;
        }
        Ok(_) => return Done::Later(BUDGET_RETRY_S),
        Err(_) => return Done::Retry,
    }

    // Authenticate and classify the attachments.
    let parsed = mime::parse(&raw, &Caps::default());
    let from_domain = pylota_identity_core::inbound::from_domain(&parsed);
    let names = pylota_identity_core::auth::prefetch_names(
        &raw,
        (!from_domain.is_empty()).then_some(from_domain.as_str()),
    );
    let dns: DnsAnswers = crate::consumers::inbound::prefetch(p, &names).await;
    let auth = pylota_identity_core::inbound::authenticate(
        &raw,
        &parsed,
        &ptr.envelope_from,
        &dns,
        p.config().trusted_authserv_id.as_deref().unwrap_or(""),
        None,
        false,
    );
    let risky = parsed.attachments.iter().any(|a| {
        pylota_identity_core::attach::assess(&a.data, a.filename.as_deref(), &a.content_type)
            .risk
            .is_some()
    });
    let verdict = verdict_str(auth.verdict);
    let whole = dest.kind == "security_contact"
        && auth.verdict == Verdict::Pass
        && !risky
        && raw.len() <= ORIGINAL_MAX;

    // The new message.
    let subject: String = format!(
        "[{}] {} mail: {}",
        ptr.domain,
        ptr.base_local,
        mime::clean_display(parsed.subject.as_deref().unwrap_or(""), 998)
    )
    .chars()
    .filter(|c| !c.is_control())
    .take(998)
    .collect();
    let from = parsed
        .from
        .as_ref()
        .map(|f| f.address.clone())
        .unwrap_or_else(|| ptr.envelope_from.clone());
    let date = pylota_identity_core::days::rfc3339(
        parsed
            .date
            .map_or(ptr.received_at, |s| s.saturating_mul(1000)),
    );
    let mut text = format!(
        "Mail to {}@{} from {from} (envelope sender {}), dated {date}. Authentication verdict: {verdict}.\n",
        ptr.base_local,
        ptr.domain,
        if ptr.envelope_from.is_empty() {
            "<>"
        } else {
            &ptr.envelope_from
        },
    );
    if dest.kind == "tenant_owner" {
        let body: String = pylota_identity_core::inbound::content(&parsed)
            .extracted_text
            .chars()
            .take(OWNER_TEXT_CHARS)
            .collect();
        text.push_str("\n--- The first part of its text ---\n");
        text.push_str(&defang(&body));
    }
    let attachment = if whole {
        json!({ "filename": "original.eml", "content_type": "message/rfc822",
            "content_base64": pylota_identity_core::links::encode_base64(&raw) })
    } else {
        json!({ "filename": "headers.txt", "content_type": "text/rfc822-headers",
            "content_base64": pylota_identity_core::links::encode_base64(header_block(&raw)) })
    };
    let body = json!({
        "to": [dest.address.clone()],
        "subject": subject,
        "text": text,
        "attachments": [attachment],
    });
    let key = format!(
        "role:{}:{}",
        sha256_hex(&raw),
        &address::address_hash(&p.secrets().hash_key, &dest.address)[..16]
    );
    let ctx = match crate::send::context::load(p, &idn, &t).await {
        Ok(c) => c,
        Err(_) => return Done::Retry,
    };
    let inp = SubmitInput {
        operation: "send".into(),
        target: None,
        key_hash: Some(sha256_hex(key.as_bytes())),
        fingerprint: sha256_hex(body.to_string().as_bytes()),
        dry_run: false,
        review: false,
        body,
        ctx,
        request_id: ptr.request_id.clone(),
        role_relay: true,
    };
    let mut env = crate::rpc::envelope_for(
        p,
        Some(&t.id),
        Some(&idn.id),
        &ptr.request_id,
        None,
        MailboxRequest::Submit(Box::new(inp)),
    );
    env.deadline_ms = now + 30_000;
    match p
        .objects()
        .call::<_, Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
        .await
    {
        Ok(RpcResult::Ok(_)) => {
            crate::metrics::count(p, "role_mail_relayed_total", None, 1);
        }
        Ok(RpcResult::Err(e)) => {
            crate::log::warn(
                "role_mail_failed",
                &[("reason", e.error.code.as_str().into())],
            );
            crate::metrics::count(p, "role_mail_failed_total", None, 1);
        }
        Err(_) => return Done::Retry,
    }
    drop_staged().await;
    Done::Ack
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defang_links() {
        assert_eq!(
            defang("see HTTPS://x.example/a and http://y"),
            "see hxxpS://x.example/a and hxxp://y"
        );
        assert_eq!(defang("naïve https://é"), "naïve hxxps://é");
    }

    #[test]
    fn header_block_ends_at_blank_line() {
        assert_eq!(
            header_block(b"A: 1\r\nB: 2\r\n\r\nbody"),
            b"A: 1\r\nB: 2\r\n"
        );
        assert_eq!(header_block(b"A: 1\n\nbody"), b"A: 1\n");
    }
}
