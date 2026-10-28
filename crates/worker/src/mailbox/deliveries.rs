//! The outbound state machine inside the mailbox ([Outbound › The outbound consumer], [Applying an event
//! to a message], [Cancel], [Uncertain sends and reconciliation]): `BeginTransport`, `ClaimTransport`,
//! `RecordTransportOutcome`, the provider-suppression sync, back-offs, `ApplyDeliveryEvent` with its
//! roll-up, `Cancel` and `Resolve`.
//!
//! [Outbound › The outbound consumer]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md
//! [Applying an event to a message]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md
//! [Cancel]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md
//! [Uncertain sends and reconciliation]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::{Ids as _, PResult, Platform, Rng as _};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::messages::{self, Row};
use super::{locks, outbox};
use crate::consumers::delivery::{Applied, DeliveryEvent, DeliveryKind};
use crate::errors::ApiError;
use crate::transport::{self, RetryClass, TransportResult};

/// A claim older than this is a transport call whose outcome is unknown.
pub const CLAIM_MS: i64 = 300_000;
/// Every back-off ends 24 hours after submit.
pub const BACKOFF_END_MS: i64 = 86_400_000;
const UNCERTAIN_FIX: &str = "Check the recipient's mailbox or wait for a delivery event; then call resolve with sent or not_sent.";

/// The sending domain as D1 sees it at transport time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainNow {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub transport: String,
    pub sending_paused: bool,
    pub delivery_events_manual: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeginInput {
    pub message_id: String,
    pub domain: Option<DomainNow>,
    pub tenant_paused: bool,
    pub fallback_allowed: bool,
    /// The identity's platform-domain address, for fallback.
    pub platform_address: Option<String>,
    /// Suppressions and send-blocks written since submit: `(address, smtp_response)`.
    pub suppressed: Vec<(String, String)>,
    pub breaker_open: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BeginAnswer {
    NotQueued,
    AlreadyClaimed,
    /// A stale claim was resolved to `uncertain`.
    StaleUncertain,
    /// Nothing is left to send; the hold settles with nothing consumed.
    Ended {
        status: String,
    },
    /// Re-enqueue after `delay_s`; extend the hold to `until`.
    Hold {
        delay_s: u32,
        until: i64,
    },
    Go(Box<GoAhead>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoAhead {
    pub eml_key: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub from: (String, String),
    pub reply_from_platform: bool,
    /// The fallback's `Reply-To` (the platform address with the thread token), set by the mailbox.
    #[serde(default)]
    pub reply_to: Option<String>,
    pub subject: String,
    /// `cloudflare`, `ses`, `smtp` or `test`.
    pub transport: String,
    pub rfc_message_id: String,
}

/// What the consumer settles after `RecordTransportOutcome`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recorded {
    pub stale: bool,
    /// Units of the sends hold to consume (recipients accepted).
    pub consume: u32,
    pub keep: u32,
    /// A back-off: re-enqueue after `delay_s`, hold extended to `until`.
    pub delay_s: Option<u32>,
    pub until: Option<i64>,
    /// The message left `queued` in this record.
    pub settled: bool,
    pub transport_accepted: bool,
}

#[derive(Deserialize)]
struct Kv {
    v: String,
}

#[derive(Clone, Deserialize)]
struct Delivery {
    address: String,
    field: String,
    status: String,
    provider_event_ids_json: String,
    first_reply: i64,
    bounce_type: Option<String>,
}

fn meta<S: Sql>(s: &S, k: &str) -> PResult<Option<String>> {
    Ok(
        s.first::<Kv>("SELECT v FROM meta WHERE k = ?1", &[k.into()])?
            .map(|r| r.v),
    )
}

fn set_meta<S: Sql>(s: &S, k: &str, v: &str) -> PResult<()> {
    s.exec(
        "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
        &[k.into(), v.into()],
    )?;
    Ok(())
}

fn del_meta<S: Sql>(s: &S, msg: &str) -> PResult<()> {
    s.exec(
        "DELETE FROM meta WHERE k IN ('claim:' || ?1, 'backoff:' || ?1, 'dispatch:' || ?1)",
        &[msg.into()],
    )?;
    Ok(())
}

/// The claim of a message: `(token, claimed_at)`.
pub fn claim_of<S: Sql>(s: &S, msg: &str) -> PResult<Option<(String, i64)>> {
    Ok(meta(s, &format!("claim:{msg}"))?.and_then(|v| {
        let (t, at) = v.split_once('|')?;
        Some((t.to_owned(), at.parse().ok()?))
    }))
}

pub fn by_id<S: Sql>(s: &S, msg: &str) -> PResult<Option<Row>> {
    s.first(
        &format!(
            "{} WHERE m.id = ?1 AND m.direction = 'outbound'",
            messages::SELECT
        ),
        &[msg.into()],
    )
}

fn deliveries<S: Sql>(s: &S, rowid: i64) -> PResult<Vec<Delivery>> {
    s.query(
        "SELECT address, field, status, provider_event_ids_json, first_reply, bounce_type FROM deliveries WHERE message_rowid = ?1 ORDER BY rowid",
        &[rowid.into()],
    )
}

fn labels<S: Sql>(s: &S, rowid: i64) -> PResult<Vec<String>> {
    #[derive(Deserialize)]
    struct L {
        label: String,
    }
    Ok(s.query::<L>(
        "SELECT label FROM labels WHERE message_rowid = ?1 ORDER BY label",
        &[rowid.into()],
    )?
    .into_iter()
    .map(|l| l.label)
    .collect())
}

fn set_status<S: Sql>(s: &S, rowid: i64, status: &str) -> PResult<()> {
    s.exec(
        "UPDATE messages SET status = ?2 WHERE rowid = ?1",
        &[rowid.into(), status.into()],
    )?;
    Ok(())
}

fn set_queued_deliveries<S: Sql>(
    s: &S,
    rowid: i64,
    status: &str,
    smtp_response: Option<&str>,
    at: i64,
) -> PResult<u32> {
    let m = s.exec(
        "UPDATE deliveries SET status = ?2, smtp_response = COALESCE(?3, smtp_response), updated_at = ?4 WHERE message_rowid = ?1 AND status = 'queued'",
        &[rowid.into(), status.into(), smtp_response.map(str::to_owned).into(), at.into()],
    )?;
    Ok(m.changes as u32)
}

fn add_flag<S: Sql>(s: &S, rowid: i64, flag: &str) -> PResult<()> {
    s.exec(
        "UPDATE messages SET flags_json = (SELECT json_group_array(value) FROM (SELECT value FROM json_each(flags_json) UNION SELECT ?2)) WHERE rowid = ?1",
        &[rowid.into(), flag.into()],
    )?;
    Ok(())
}

fn event<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    event_type: &str,
    data: &Value,
    now: i64,
) -> PResult<()> {
    outbox::append(
        s,
        owner,
        &outbox::new_event_id(p),
        event_type,
        now,
        data,
        now,
    )?;
    Ok(())
}

/// Leaves `queued` for a final status: deliveries, lock, metadata and the event.
#[allow(clippy::too_many_arguments)]
fn end<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    row: &Row,
    status: &str,
    reason: &str,
    detail: Option<&str>,
    at: i64,
    now: i64,
) -> PResult<()> {
    set_queued_deliveries(s, row.rowid, status, detail, at)?;
    set_status(s, row.rowid, status)?;
    locks::release(s, &row.id)?;
    del_meta(s, &row.id)?;
    let data = match status {
        "rejected" => json!({ "message_id": row.id, "reason": reason, "detail": detail }),
        "uncertain" => json!({ "message_id": row.id, "reason": reason, "fix": UNCERTAIN_FIX }),
        _ => json!({ "message_id": row.id, "reason": reason }),
    };
    event(p, s, owner, &format!("message.{status}"), &data, now)
}

/// The back-off of a message still `queued`: the next delay, or `None` once the next retry would fall
/// more than 24 hours after submit (the caller then ends it `failed: quota_exhausted`).
fn backoff<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    row: &Row,
    class: RetryClass,
    now: i64,
) -> PResult<Option<(u32, i64)>> {
    let key = format!("backoff:{}", row.id);
    let n: u32 = meta(s, &key)?.and_then(|v| v.parse().ok()).unwrap_or(0) + 1;
    let base = transport::backoff_s(class, n) * 1000;
    let jitter = ((p.rng().u64() >> 33) as i64 % (base / 5 + 1)) - base / 10;
    let delay_ms = (base + jitter).max(1000);
    if now + delay_ms > row.received_at + BACKOFF_END_MS {
        return Ok(None);
    }
    set_meta(s, &key, &n.to_string())?;
    set_meta(
        s,
        &format!("dispatch:{}", row.id),
        &(now + delay_ms).to_string(),
    )?;
    locks::release(s, &row.id)?;
    super::set_wake(
        s,
        "dispatch",
        now + delay_ms + super::alarms::DISPATCH_AFTER_MS,
    )?;
    // The itest build waits a second whatever the schedule; the clock decides the 24-hour end.
    let queue_s = if cfg!(feature = "itest-hooks") {
        1
    } else {
        (delay_ms / 1000) as u32
    };
    Ok(Some((queue_s, now + delay_ms + 600_000)))
}

/// Holds a message with the `Paused` back-off, or ends it `failed` after 24 hours.
fn hold<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    row: &Row,
    class: RetryClass,
    now: i64,
) -> PResult<BeginAnswer> {
    Ok(match backoff(p, s, row, class, now)? {
        Some((delay_s, until)) => BeginAnswer::Hold { delay_s, until },
        None => {
            end(
                p,
                s,
                owner,
                row,
                "failed",
                "quota_exhausted",
                None,
                now,
                now,
            )?;
            BeginAnswer::Ended {
                status: "failed".into(),
            }
        }
    })
}

/// The stale-claim rule: the claimed call's outcome is unknown, so the message becomes `uncertain`
/// with the deliveries' `updated_at` at `claimed_at`.
pub fn stale_claim<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    row: &Row,
    claimed_at: i64,
    now: i64,
) -> PResult<()> {
    end(
        p,
        s,
        owner,
        row,
        "uncertain",
        "transport_connection_lost",
        None,
        claimed_at,
        now,
    )
}

/// `BeginTransport` (consumer step 2), in one transaction.
pub fn begin<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    inp: &BeginInput,
    test: bool,
    now: i64,
) -> PResult<BeginAnswer> {
    let Some(row) = by_id(s, &inp.message_id)? else {
        return Ok(BeginAnswer::NotQueued);
    };
    if row.status != "queued" {
        return Ok(BeginAnswer::NotQueued);
    }
    if let Some((_, at)) = claim_of(s, &row.id)? {
        if now - at < CLAIM_MS {
            return Ok(BeginAnswer::AlreadyClaimed);
        }
        stale_claim(p, s, owner, &row, at, now)?;
        return Ok(BeginAnswer::StaleUncertain);
    }
    // Suppressions after submit (G4).
    for (a, smtp) in &inp.suppressed {
        s.exec(
            "UPDATE deliveries SET status = 'suppressed', smtp_response = ?3, updated_at = ?4 WHERE message_rowid = ?1 AND address = ?2 AND status = 'queued'",
            &[row.rowid.into(), a.clone().into(), smtp.clone().into(), now.into()],
        )?;
    }
    let all = deliveries(s, row.rowid)?;
    let queued: Vec<&Delivery> = all.iter().filter(|d| d.status == "queued").collect();
    if queued.is_empty() {
        if all.iter().all(|d| d.status == "suppressed") {
            set_status(s, row.rowid, "suppressed")?;
            locks::release(s, &row.id)?;
            del_meta(s, &row.id)?;
            let addrs: Vec<&str> = all.iter().map(|d| d.address.as_str()).collect();
            event(
                p,
                s,
                owner,
                "message.suppressed",
                &json!({ "message_id": row.id, "recipients": addrs }),
                now,
            )?;
            return Ok(BeginAnswer::Ended {
                status: "suppressed".into(),
            });
        }
        rollup_and_event(p, s, owner, &row, now)?;
        return Ok(BeginAnswer::Ended {
            status: "settled".into(),
        });
    }
    let mut from = (
        row.from_address.clone().unwrap_or_default(),
        row.from_name.clone().unwrap_or_default(),
    );
    let mut transport = if test {
        "test".to_owned()
    } else {
        "cloudflare".to_owned()
    };
    let mut fallback = false;
    if !test {
        let Some(d) = &inp.domain else {
            end(
                p,
                s,
                owner,
                &row,
                "rejected",
                "sender_domain_unavailable",
                None,
                now,
                now,
            )?;
            return Ok(BeginAnswer::Ended {
                status: "rejected".into(),
            });
        };
        if matches!(d.state.as_str(), "pending" | "verifying") || inp.breaker_open {
            return hold(p, s, owner, &row, RetryClass::Paused, now);
        }
        if d.sending_paused || inp.tenant_paused {
            return hold(p, s, owner, &row, RetryClass::Paused, now);
        }
        match d.state.as_str() {
            "removing" | "removed" => {
                end(
                    p,
                    s,
                    owner,
                    &row,
                    "rejected",
                    "sender_domain_unavailable",
                    None,
                    now,
                    now,
                )?;
                return Ok(BeginAnswer::Ended {
                    status: "rejected".into(),
                });
            }
            "failing" | "suspended" => fallback = true,
            _ if d.delivery_events_manual && d.transport == "cloudflare" => fallback = true,
            _ => {}
        }
        transport = d.transport.clone();
        if fallback {
            let platform = inp
                .platform_address
                .clone()
                .filter(|_| d.kind != "platform");
            match platform {
                Some(pa) if inp.fallback_allowed => {
                    from.0 = pa;
                    transport = "cloudflare".into();
                    s.exec(
                        "UPDATE messages SET from_address = ?2 WHERE rowid = ?1",
                        &[row.rowid.into(), from.0.clone().into()],
                    )?;
                    add_flag(s, row.rowid, "sent_via_fallback")?;
                    s.exec(
                        "UPDATE threads SET fallback_pinned = 1 WHERE seq = ?1",
                        &[row.thread_seq.into()],
                    )?;
                }
                _ => {
                    end(
                        p,
                        s,
                        owner,
                        &row,
                        "failed",
                        "domain_failing_no_fallback",
                        None,
                        now,
                        now,
                    )?;
                    return Ok(BeginAnswer::Ended {
                        status: "failed".into(),
                    });
                }
            }
        }
        if row.kind == "marketing" && transport == "cloudflare" {
            end(
                p,
                s,
                owner,
                &row,
                "rejected",
                "marketing_needs_ses",
                Some("marketing_needs_ses"),
                now,
                now,
            )?;
            return Ok(BeginAnswer::Ended {
                status: "rejected".into(),
            });
        }
    }
    s.exec(
        "UPDATE threads SET lock_until = ?2 WHERE lock_owner = ?1",
        &[
            row.id.clone().into(),
            (now + locks::TRANSPORT_LEASE_MS).into(),
        ],
    )?;
    let by = |f: &str| -> Vec<String> {
        queued
            .iter()
            .filter(|d| d.field == f)
            .map(|d| d.address.clone())
            .collect()
    };
    Ok(BeginAnswer::Go(Box::new(GoAhead {
        eml_key: row.raw_r2_key.clone().unwrap_or_default(),
        to: by("to"),
        cc: by("cc"),
        bcc: by("bcc"),
        reply_from_platform: fallback,
        reply_to: None,
        from,
        subject: row.subject.clone().unwrap_or_default(),
        transport,
        rfc_message_id: format!(
            "{}@{}",
            row.id[4..].to_ascii_lowercase(),
            pylota_identity_core::address::domain_of(row.from_address.as_deref().unwrap_or(""))
        ),
    })))
}

/// `ClaimTransport` (consumer step 4): the claim token, or `None` (ack: not queued, or claimed).
pub fn claim<P: Platform, S: Sql>(p: &P, s: &S, msg: &str, now: i64) -> PResult<Option<String>> {
    let Some(row) = by_id(s, msg)? else {
        return Ok(None);
    };
    if row.status != "queued" || claim_of(s, msg)?.is_some_and(|(_, at)| now - at < CLAIM_MS) {
        return Ok(None);
    }
    let token = p.ids().new_id(IdPrefix::Request);
    set_meta(s, &format!("claim:{msg}"), &format!("{token}|{now}"))?;
    super::set_wake(s, "claim", now + CLAIM_MS)?;
    Ok(Some(token))
}

/// G4 step 2: deliveries the provider suppressed become `suppressed`; answers the recipients left.
pub fn mark_suppressed<S: Sql>(
    s: &S,
    msg: &str,
    items: &[(String, String)],
    now: i64,
) -> PResult<Vec<String>> {
    let Some(row) = by_id(s, msg)? else {
        return Ok(Vec::new());
    };
    for (a, smtp) in items {
        s.exec(
            "UPDATE deliveries SET status = 'suppressed', smtp_response = ?3, updated_at = ?4 WHERE message_rowid = ?1 AND address = ?2 AND status = 'queued'",
            &[row.rowid.into(), a.clone().into(), smtp.clone().into(), now.into()],
        )?;
    }
    Ok(deliveries(s, row.rowid)?
        .into_iter()
        .filter(|d| d.status == "queued")
        .map(|d| d.address)
        .collect())
}

/// `RecordTransportOutcome` (consumer step 6), in one transaction.
#[allow(clippy::too_many_arguments)]
pub fn record<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    msg: &str,
    token: &str,
    provider: &str,
    result: &TransportResult,
    now: i64,
) -> PResult<Recorded> {
    let Some(row) = by_id(s, msg)? else {
        return Ok(Recorded {
            stale: true,
            ..Recorded::default()
        });
    };
    let claim = claim_of(s, msg)?;
    let Some((_, claimed_at)) = claim.filter(|(t, _)| t == token) else {
        // A stale token: only an acceptance is still evidence.
        if let TransportResult::Accepted {
            provider_message_id,
            rfc_message_id,
            ..
        } = result
            && row.status == "uncertain"
            && row.provider_message_id.is_none()
        {
            s.exec(
                "UPDATE messages SET provider = ?2, provider_message_id = ?3, rfc_message_id = COALESCE(?4, rfc_message_id) WHERE rowid = ?1",
                &[row.rowid.into(), provider.into(), provider_message_id.clone().into(), rfc_message_id.clone().into()],
            )?;
            let n = s.exec(
                "UPDATE deliveries SET status = 'submitted', updated_at = ?2 WHERE message_rowid = ?1 AND status = 'uncertain'",
                &[row.rowid.into(), now.into()],
            )?;
            add_flag(s, row.rowid, "reconciled")?;
            set_status(s, row.rowid, "submitted")?;
            event(
                p,
                s,
                owner,
                "message.reconciled",
                &json!({ "message_id": row.id, "status": "submitted" }),
                now,
            )?;
            return Ok(Recorded {
                stale: true,
                consume: n.changes as u32,
                settled: true,
                ..Recorded::default()
            });
        }
        return Ok(Recorded {
            stale: true,
            ..Recorded::default()
        });
    };
    s.exec(
        "DELETE FROM meta WHERE k = ?1",
        &[format!("claim:{msg}").into()],
    )?;
    if row.status != "queued" {
        return Ok(Recorded::default());
    }
    match result {
        TransportResult::Accepted {
            provider_message_id,
            rfc_message_id,
            refused,
            deferred,
        } => {
            let rfc = rfc_message_id.clone().unwrap_or_else(|| {
                format!(
                    "{}@{}",
                    row.id[4..].to_ascii_lowercase(),
                    pylota_identity_core::address::domain_of(
                        row.from_address.as_deref().unwrap_or("")
                    )
                )
            });
            s.exec(
                "UPDATE messages SET provider = ?2, provider_message_id = ?3, rfc_message_id = ?4 WHERE rowid = ?1",
                &[row.rowid.into(), provider.into(), provider_message_id.clone().into(), rfc.into()],
            )?;
            for r in refused {
                s.exec(
                    "UPDATE deliveries SET status = 'rejected', smtp_response = ?3, updated_at = ?4 WHERE message_rowid = ?1 AND address = ?2 AND status = 'queued'",
                    &[row.rowid.into(), r.address.clone().into(), r.detail.clone().into(), now.into()],
                )?;
            }
            let mut submitted = 0u32;
            for d in deliveries(s, row.rowid)?
                .iter()
                .filter(|d| d.status == "queued" && !deferred.contains(&d.address))
            {
                s.exec(
                    "UPDATE deliveries SET status = 'submitted', updated_at = ?3 WHERE message_rowid = ?1 AND address = ?2",
                    &[row.rowid.into(), d.address.clone().into(), now.into()],
                )?;
                submitted += 1;
            }
            if !deferred.is_empty() {
                let b = backoff(p, s, &row, RetryClass::Relay, now)?;
                return Ok(Recorded {
                    consume: submitted,
                    keep: deferred.len() as u32,
                    delay_s: b.map(|b| b.0),
                    until: b.map(|b| b.1),
                    transport_accepted: true,
                    ..Recorded::default()
                });
            }
            set_status(s, row.rowid, "submitted")?;
            locks::release(s, &row.id)?;
            del_meta(s, &row.id)?;
            let row = by_id(s, msg)?.unwrap_or(row);
            let flags = row.flags();
            event(
                p,
                s,
                owner,
                "message.sent",
                &json!({
                    "message": row.summary(&labels(s, row.rowid)?), "provider": provider,
                    "provider_message_id": provider_message_id,
                    "sent_via_fallback": flags.iter().any(|f| f == "sent_via_fallback"),
                }),
                now,
            )?;
            Ok(Recorded {
                consume: submitted,
                settled: true,
                transport_accepted: true,
                ..Recorded::default()
            })
        }
        TransportResult::Rejected { reason, detail, .. } => {
            end(
                p,
                s,
                owner,
                &row,
                "rejected",
                reason,
                Some(detail),
                now,
                now,
            )?;
            Ok(Recorded {
                settled: true,
                ..Recorded::default()
            })
        }
        TransportResult::RecipientSuppressed { code } => {
            end(
                p,
                s,
                owner,
                &row,
                "rejected",
                "recipient_suppressed_by_provider",
                Some(code),
                now,
                now,
            )?;
            Ok(Recorded {
                settled: true,
                ..Recorded::default()
            })
        }
        TransportResult::Unknown { reason, .. } => {
            end(
                p,
                s,
                owner,
                &row,
                "uncertain",
                reason,
                None,
                claimed_at,
                now,
            )?;
            Ok(Recorded {
                settled: true,
                ..Recorded::default()
            })
        }
        TransportResult::RetryLater { class, .. } => match backoff(p, s, &row, *class, now)? {
            Some((d, until)) => Ok(Recorded {
                delay_s: Some(d),
                until: Some(until),
                ..Recorded::default()
            }),
            None => {
                end(
                    p,
                    s,
                    owner,
                    &row,
                    "failed",
                    "quota_exhausted",
                    None,
                    now,
                    now,
                )?;
                Ok(Recorded {
                    settled: true,
                    ..Recorded::default()
                })
            }
        },
    }
}

/// Ends a message whose recipients the provider suppressed entirely (G4 step 4).
pub fn all_suppressed<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    msg: &str,
    now: i64,
) -> PResult<()> {
    let Some(row) = by_id(s, msg)? else {
        return Ok(());
    };
    s.exec(
        "DELETE FROM meta WHERE k = ?1",
        &[format!("claim:{msg}").into()],
    )?;
    set_status(s, row.rowid, "suppressed")?;
    locks::release(s, &row.id)?;
    del_meta(s, &row.id)?;
    let addrs: Vec<String> = deliveries(s, row.rowid)?
        .into_iter()
        .map(|d| d.address)
        .collect();
    event(
        p,
        s,
        owner,
        "message.suppressed",
        &json!({ "message_id": row.id, "recipients": addrs }),
        now,
    )
}

/// The roll-up of a message's status over its deliveries that are not `suppressed`.
fn rollup(current: &str, ds: &[Delivery]) -> String {
    let live: Vec<&str> = ds
        .iter()
        .filter(|d| d.status != "suppressed")
        .map(|d| d.status.as_str())
        .collect();
    let any = |s: &str| live.contains(&s);
    let next = if live.is_empty() {
        "suppressed"
    } else if any("complained") {
        "complained"
    } else if any("queued") {
        "queued"
    } else if any("submitted") || any("deferred") || any("uncertain") {
        if current == "uncertain"
            && live
                .iter()
                .filter(|s| matches!(**s, "submitted" | "deferred" | "uncertain"))
                .all(|s| *s == "uncertain")
        {
            "uncertain"
        } else if any("deferred") {
            "deferred"
        } else {
            "submitted"
        }
    } else if any("bounced") {
        "bounced"
    } else if any("failed") {
        "failed"
    } else if any("rejected") {
        "rejected"
    } else {
        "delivered"
    };
    let terminal = |s: &str| {
        matches!(
            s,
            "delivered"
                | "bounced"
                | "complained"
                | "failed"
                | "rejected"
                | "suppressed"
                | "canceled"
        )
    };
    if terminal(current) {
        let allowed = matches!(
            (current, next),
            ("delivered", "bounced") | ("delivered", "complained") | ("bounced", "complained")
        );
        return if allowed {
            next.to_owned()
        } else {
            current.to_owned()
        };
    }
    next.to_owned()
}

fn rollup_and_event<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    row: &Row,
    now: i64,
) -> PResult<String> {
    let ds = deliveries(s, row.rowid)?;
    let next = rollup(&row.status, &ds);
    if next != row.status {
        set_status(s, row.rowid, &next)?;
        if next != "queued" {
            locks::release(s, &row.id)?;
        }
        match next.as_str() {
            "rejected" => event(
                p,
                s,
                owner,
                "message.rejected",
                &json!({ "message_id": row.id, "reason": "provider_validation", "detail": null }),
                now,
            )?,
            "failed" => event(
                p,
                s,
                owner,
                "message.failed",
                &json!({ "message_id": row.id, "reason": "provider_failed" }),
                now,
            )?,
            _ => {}
        }
    }
    Ok(next)
}

/// The next delivery status for an event, or `None` (ignored or stale).
fn transition(current: &str, kind: DeliveryKind) -> Option<&'static str> {
    use DeliveryKind::*;
    let to = match kind {
        Delivered => "delivered",
        Deferred => "deferred",
        Bounced { .. } => "bounced",
        Failed => "failed",
        Rejected => "rejected",
        Complained => "complained",
    };
    match (current, kind) {
        ("queued" | "submitted" | "uncertain" | "deferred", _) => Some(to),
        ("delivered", Bounced { .. } | Complained) => Some(to),
        ("bounced", Bounced { .. } | Complained) => Some(to),
        _ => None,
    }
}

fn outcome_of(status: &str, bounce_type: Option<&str>) -> (Option<String>, Option<String>) {
    match (status, bounce_type) {
        ("delivered", _) => (Some("delivered".into()), None),
        ("bounced", Some("hard")) => (Some("bounced".into()), Some("hard_bounce".into())),
        ("complained", _) => (Some("complained".into()), Some("complaint".into())),
        ("bounced" | "failed" | "rejected", _) => (Some("other".into()), None),
        _ => (None, None),
    }
}

/// `ApplyDeliveryEvent`, in one transaction.
pub fn apply<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    e: &DeliveryEvent,
    now: i64,
) -> PResult<Applied> {
    let recipient = e.recipient.to_ascii_lowercase();
    // 1. The message, or one uncertain candidate (FR-DLV-4).
    let mut reconciled = false;
    let mut row: Option<Row> = match &e.provider_message_id {
        Some(pid) => s.first(
            &format!(
                "{} WHERE m.direction = 'outbound' AND m.provider_message_id = ?1",
                messages::SELECT
            ),
            &[pid.clone().into()],
        )?,
        None => None,
    };
    if row.is_none()
        && let Some(subject) = &e.subject
    {
        #[derive(Deserialize)]
        struct C {
            id: String,
        }
        let c: Vec<C> = s.query(
            "SELECT m.id FROM messages m JOIN deliveries d ON d.message_rowid = m.rowid WHERE m.direction = 'outbound' AND m.status = 'uncertain' AND m.provider_message_id IS NULL AND m.from_address = ?1 AND d.address = ?2 AND d.status = 'uncertain' AND m.subject = ?3 AND d.updated_at BETWEEN ?4 AND ?5",
            &[e.sender.clone().into(), recipient.clone().into(), subject.clone().into(), (e.occurred_at - 1_800_000).into(), (e.occurred_at + 300_000).into()],
        )?;
        if c.len() == 1 {
            row = by_id(s, &c[0].id)?;
            if let (Some(r), Some(pid)) = (&row, &e.provider_message_id) {
                s.exec(
                    "UPDATE messages SET provider_message_id = ?2 WHERE rowid = ?1",
                    &[r.rowid.into(), pid.clone().into()],
                )?;
                add_flag(s, r.rowid, "reconciled")?;
                reconciled = true;
            }
        } else if c.len() > 1 {
            crate::log::warn("reconcile_ambiguous", &[]);
        }
    }
    let Some(row) = row else {
        return Ok(Applied {
            kind: "not_found".into(),
            ..Applied::default()
        });
    };
    // 2–3.
    let ds = deliveries(s, row.rowid)?;
    let Some(d) = ds.iter().find(|d| d.address == recipient).cloned() else {
        return Ok(Applied {
            kind: "duplicate".into(),
            message_id: Some(row.id),
            ..Applied::default()
        });
    };
    let mut ids: Vec<String> = serde_json::from_str(&d.provider_event_ids_json).unwrap_or_default();
    if ids.contains(&e.event_id) {
        let (outcome, suppress) = outcome_of(&d.status, d.bounce_type.as_deref());
        return Ok(Applied {
            kind: "duplicate".into(),
            message_id: Some(row.id),
            outcome,
            suppress,
            first_reply: d.first_reply == 1,
        });
    }
    ids.push(e.event_id.clone());
    if ids.len() > 20 {
        ids.drain(..ids.len() - 20);
    }
    s.exec(
        "UPDATE deliveries SET provider_event_ids_json = ?3 WHERE message_rowid = ?1 AND address = ?2",
        &[row.rowid.into(), recipient.clone().into(), serde_json::to_string(&ids).unwrap_or_default().into()],
    )?;
    // 4. The transition (G6).
    let next = transition(&d.status, e.kind);
    let bounce_type = match e.kind {
        DeliveryKind::Bounced { hard } => Some(if hard { "hard" } else { "soft" }),
        _ => None,
    };
    if let Some(next) = next {
        s.exec(
            "UPDATE deliveries SET status = ?3, smtp_code = ?4, enhanced_code = ?5, smtp_response = ?6, bounce_type = COALESCE(?7, bounce_type), updated_at = ?8 WHERE message_rowid = ?1 AND address = ?2",
            &[row.rowid.into(), recipient.clone().into(), next.into(), e.smtp_code.clone().into(), e.enhanced_code.clone().into(),
              e.smtp_response.clone().into(), bounce_type.map(str::to_owned).into(), now.into()],
        )?;
    }
    // 5. The roll-up.
    let status = rollup_and_event(p, s, owner, &row, now)?;
    if reconciled {
        event(
            p,
            s,
            owner,
            "message.reconciled",
            &json!({ "message_id": row.id, "status": status }),
            now,
        )?;
    }
    // 6. Per-recipient events.
    let hard = bounce_type == Some("hard");
    if let Some(next) = next {
        let data = match next {
            "delivered" => Some(
                json!({ "message_id": row.id, "recipient": recipient, "smtp_code": e.smtp_code }),
            ),
            "deferred" => Some(
                json!({ "message_id": row.id, "recipient": recipient, "smtp_code": e.smtp_code, "smtp_response": e.smtp_response }),
            ),
            "bounced" => Some(
                json!({ "message_id": row.id, "recipient": recipient, "bounce_type": bounce_type.unwrap_or("hard"),
                "smtp_code": e.smtp_code, "smtp_response": e.smtp_response, "suppressed": hard }),
            ),
            "complained" => {
                Some(json!({ "message_id": row.id, "recipient": recipient, "suppressed": true }))
            }
            _ => None,
        };
        if let Some(data) = data {
            event(p, s, owner, &format!("message.{next}"), &data, now)?;
        }
    }
    // 7.
    let (outcome, suppress) = match next {
        Some(n) => outcome_of(n, bounce_type),
        None => (None, None),
    };
    Ok(Applied {
        kind: if reconciled { "reconciled" } else { "applied" }.into(),
        message_id: Some(row.id),
        outcome,
        suppress,
        first_reply: d.first_reply == 1,
    })
}

/// `Cancel` (FR-OUT-11): only while `queued`, unclaimed and with nothing submitted. Answers the day of
/// the reservation to release.
pub fn cancel<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    msg: &str,
    now: i64,
) -> Result<Value, ApiError> {
    let row = by_id(s, msg)?.ok_or_else(|| ApiError::not_found(ErrorCode::MessageNotFound))?;
    let not = || {
        ApiError::new(
            ErrorCode::NotCancelable,
            "Only a queued message that is not being sent can be canceled.",
        )
        .details(json!({ "status": row.status }))
    };
    if row.status != "queued" || claim_of(s, msg)?.is_some_and(|(_, at)| now - at < CLAIM_MS) {
        return Err(not());
    }
    if deliveries(s, row.rowid)?
        .iter()
        .any(|d| d.status == "submitted")
    {
        return Err(not());
    }
    set_status(s, row.rowid, "canceled")?;
    locks::release(s, msg)?;
    del_meta(s, msg)?;
    let reserve = meta(s, &format!("reserve:{msg}"))?;
    s.exec(
        "DELETE FROM meta WHERE k = ?1",
        &[format!("reserve:{msg}").into()],
    )?;
    event(
        p,
        s,
        owner,
        "message.canceled",
        &json!({ "message_id": msg }),
        now,
    )?;
    let (day, counted) = reserve
        .as_deref()
        .and_then(|r| r.split_once('|'))
        .map_or((None, true), |(d, c)| (Some(d.to_owned()), c == "1"));
    Ok(json!({ "day": day, "tenant_counted": counted }))
}

/// `Resolve` (FR-DLV-7).
pub fn resolve<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &outbox::Owner<'_>,
    msg: &str,
    outcome: &str,
    now: i64,
) -> Result<Value, ApiError> {
    let row = by_id(s, msg)?.ok_or_else(|| ApiError::not_found(ErrorCode::MessageNotFound))?;
    if row.status != "uncertain" {
        return Err(ApiError::new(
            ErrorCode::NotUncertain,
            "Only an uncertain message can be resolved.",
        )
        .details(json!({ "status": row.status })));
    }
    let (status, n) = if outcome == "sent" {
        let n = s.exec(
            "UPDATE deliveries SET status = 'submitted', updated_at = ?2 WHERE message_rowid = ?1 AND status = 'uncertain'",
            &[row.rowid.into(), now.into()],
        )?;
        set_status(s, row.rowid, "submitted")?;
        let row = by_id(s, msg)?.unwrap_or(row.clone());
        event(
            p,
            s,
            owner,
            "message.sent",
            &json!({
                "message": row.summary(&labels(s, row.rowid)?), "provider": row_provider(&row),
                "provider_message_id": null, "sent_via_fallback": row.flags().iter().any(|f| f == "sent_via_fallback"),
            }),
            now,
        )?;
        ("submitted", n.changes)
    } else {
        s.exec(
            "UPDATE deliveries SET status = 'failed', updated_at = ?2 WHERE message_rowid = ?1 AND status = 'uncertain'",
            &[row.rowid.into(), now.into()],
        )?;
        set_status(s, row.rowid, "failed")?;
        event(
            p,
            s,
            owner,
            "message.failed",
            &json!({ "message_id": msg, "reason": "resolved_not_sent" }),
            now,
        )?;
        ("failed", 0)
    };
    Ok(json!({ "status": status, "consume": n }))
}

fn row_provider(row: &Row) -> Option<String> {
    row.provider.clone()
}

/// The outbound facts the consumer reads before D1: `(status, from, queued recipients, kind)`.
pub fn facts<S: Sql>(s: &S, msg: &str) -> PResult<Value> {
    let Some(row) = by_id(s, msg)? else {
        return Ok(json!({ "status": null }));
    };
    let queued: Vec<String> = deliveries(s, row.rowid)?
        .into_iter()
        .filter(|d| d.status == "queued")
        .map(|d| d.address)
        .collect();
    Ok(
        json!({ "status": row.status, "from_address": row.from_address, "queued": queued, "kind": row.kind,
               "received_at": row.received_at }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(status: &str) -> Delivery {
        Delivery {
            address: "a@x".into(),
            field: "to".into(),
            status: status.into(),
            provider_event_ids_json: "[]".into(),
            first_reply: 0,
            bounce_type: None,
        }
    }

    #[test]
    fn g6_transitions_and_rollup() {
        use DeliveryKind::*;
        assert_eq!(transition("submitted", Delivered), Some("delivered"));
        assert_eq!(transition("delivered", Deferred), None);
        assert_eq!(
            transition("delivered", Bounced { hard: true }),
            Some("bounced")
        );
        assert_eq!(transition("bounced", Delivered), None);
        assert_eq!(transition("complained", Bounced { hard: true }), None);
        assert_eq!(
            rollup("submitted", &[d("delivered"), d("bounced")]),
            "bounced"
        );
        assert_eq!(
            rollup("submitted", &[d("delivered"), d("deferred")]),
            "deferred"
        );
        assert_eq!(
            rollup("delivered", &[d("delivered"), d("complained")]),
            "complained"
        );
        assert_eq!(
            rollup("bounced", &[d("delivered")]),
            "bounced",
            "a terminal status only worsens"
        );
        assert_eq!(
            rollup("uncertain", &[d("uncertain"), d("delivered")]),
            "uncertain"
        );
        assert_eq!(rollup("submitted", &[d("suppressed")]), "suppressed");
    }
}
