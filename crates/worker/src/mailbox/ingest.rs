//! `IdentityMailbox.ingest` ([Inbound › IdentityMailbox.ingest]): one transaction stores an inbound
//! message, threads it, decides its quarantine, indexes it and appends its events, so the message is
//! searchable and evented the moment it exists (FR-SRCH-2). It is idempotent on `raw_sha256` (J2, B14).
//!
//! Calendar objects (step 10a), screening (step 11a), receive lists and the service-account gate (rule
//! 4a) arrive with their milestones; their inputs are absent until then.
//!
//! [Inbound › IdentityMailbox.ingest]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::{domain_of, org_domain};
use pylota_identity_core::auth::Verdict;
use pylota_identity_core::mime::Mailbox as CoreMailbox;
use pylota_identity_core::search::fts_doc::{self, MessageForIndex};
use pylota_identity_core::thread::{self, InboundThreadInputs, ThreadDecision};
use pylota_identity_core::thread_token::TokenCheck;
use pylota_identity_core::trust::{self, Contact, TenantDomain, TrustInput};
use pylota_identity_platform::durable::Sql;
use pylota_identity_platform::{Ids as _, PResult, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::messages::{self, AttachmentRow};
use super::outbox::{self, Owner};
use super::threads;
use crate::errors::ApiError;
use crate::webhooks::payloads;

/// The parser version this build stores (`messages.parser_version`).
pub const PARSER_VERSION: u32 = 1;
/// A `wait` registration counts this long after it ends (E5).
pub const WAIT_GRACE_MS: i64 = 30 * 60_000;
/// Brute-force limits of the thread token (Threading § 2.5).
pub const TOKEN_FAILS_PER_SENDER: i64 = 10;
pub const TOKEN_FAILS_TOTAL: i64 = 100;
const HOUR: i64 = 3_600_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Addr {
    pub address: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub part_index: u32,
    pub filename: Option<String>,
    pub content_type: String,
    pub sniffed_type: Option<String>,
    pub size: u64,
    pub sha256: String,
    pub inline: bool,
    pub content_id: Option<String>,
    pub risk: Option<String>,
    /// Whether the risk quarantines the message (every risk but `encrypted_document`).
    pub risk_quarantines: bool,
    /// Eligible for text extraction (Attachment text extraction › Eligibility).
    pub text_eligible: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefRow {
    pub kind: String,
    pub value: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerificationIn {
    pub kind: String,
    pub value: String,
}

/// The policy values ingest reads (effective tenant policy).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InboundPolicy {
    pub on_auth_fail: bool,
    pub unsolicited_otp: bool,
    pub spam_threshold: f64,
    pub per_sender_per_hour: i64,
    pub unauthenticated_per_hour: i64,
    pub triage_enabled: bool,
    pub webhook_text_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IngestInput {
    pub message_id: String,
    pub received_at: i64,
    pub raw_r2_key: String,
    pub raw_size: u64,
    pub raw_sha256: String,
    pub envelope_from: String,
    pub delivered_to: String,
    pub detail: Option<String>,
    pub rfc_message_id: String,
    pub message_id_synthetic: bool,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from: Option<Addr>,
    pub reply_to: Vec<String>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub text: Option<String>,
    pub html_sanitized: Option<String>,
    pub extracted_text: Option<String>,
    pub sent_at: i64,
    pub kind: String,
    pub automated_json: Value,
    pub auth_json: Value,
    pub verdict: String,
    /// `auth_json.dmarc.aligned_by` is `dkim` or `spf`.
    pub aligned: bool,
    /// The content signals' share of the spam score (Trust signals › Spam score).
    pub spam_base: f64,
    pub flags: Vec<String>,
    pub refs: Vec<RefRow>,
    pub attachments: Vec<AttachmentMeta>,
    pub verification: Option<VerificationIn>,
    pub sender_suppressed: bool,
    /// The effective receive decision (`allow`, `block`), from the lists of M9.
    pub receive_decision: Option<String>,
    pub tenant_domains: Vec<(String, bool)>,
    pub platform_domain: String,
    pub tenant_suffix: String,
    pub tenant_name: String,
    pub identity_name: String,
    /// The identity's addresses (any status), never participants.
    pub identity_addresses: Vec<String>,
    pub sender_in_tenant: bool,
    pub sender_is_identity: bool,
    /// `admit` or `throttle` (AdmitInbound, D13).
    pub inbound_admit: String,
    pub is_primary_recipient: bool,
    pub is_bcc: bool,
    /// The envelope recipient is one of the identity's `active` or `retiring` addresses and in To/Cc.
    pub reply_from: Option<String>,
    pub policy: InboundPolicy,
    pub parser_version: u32,
    pub loopback: bool,
    /// `dsn.original_message_id` of a DSN, normalised.
    pub dsn_original_message_id: Option<String>,
    /// The token check made before the transaction against the thread keyring.
    #[serde(skip)]
    pub token: Option<TokenCheckWire>,
}

/// A serialisable [`TokenCheck`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenCheckWire {
    Absent,
    Valid { seq: u64, kid: u8, current: bool },
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttachmentToStore {
    pub attachment_id: String,
    pub part_index: u32,
    pub r2_key: String,
    pub text_pending: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IngestOutcome {
    Stored {
        message_id: String,
        thread_id: String,
        status: String,
        duplicate: bool,
        stored_raw_r2_key: String,
        attachments: Vec<AttachmentToStore>,
        is_system: bool,
        triage: bool,
    },
    Backscatter,
}

fn verdict_of(s: &str) -> Verdict {
    match s {
        "pass" => Verdict::Pass,
        "arc_rescued" => Verdict::ArcRescued,
        "unverified" => Verdict::Unverified,
        "fail" => Verdict::Fail,
        "unaligned" => Verdict::Unaligned,
        "softfail" => Verdict::Softfail,
        _ => Verdict::None,
    }
}

fn verdict_weight(v: Verdict) -> f64 {
    match v {
        Verdict::Pass => 0.0,
        Verdict::ArcRescued => 0.05,
        Verdict::Softfail => 0.20,
        Verdict::None | Verdict::Unaligned => 0.10,
        Verdict::Unverified => 0.20,
        Verdict::Fail => 0.50,
    }
}

#[derive(Deserialize)]
struct Hit {
    rowid: i64,
    id: String,
    thread_id: String,
    status: String,
    raw_r2_key: Option<String>,
}

#[derive(Deserialize)]
struct Count {
    n: i64,
}

#[derive(Deserialize)]
struct ContactRow {
    address: String,
    name: Option<String>,
    inbound_count: i64,
    outbound_count: i64,
}

#[derive(Deserialize)]
struct Same {
    rowid: i64,
    subject: Option<String>,
    text: Option<String>,
    html_sanitized: Option<String>,
}

/// One `rate_windows` upsert; returns the new count.
fn bump<S: Sql>(s: &S, key: &str, window: i64) -> PResult<i64> {
    let c: Option<Count> = s.first(
        "INSERT INTO rate_windows (sender, window_start, count) VALUES (?1, ?2, 1) ON CONFLICT (sender, window_start) DO UPDATE SET count = count + 1 RETURNING count AS n",
        &[key.into(), window.into()],
    )?;
    Ok(c.map_or(1, |c| c.n))
}

fn count_of<S: Sql>(s: &S, key: &str, window: i64) -> PResult<i64> {
    let c: Option<Count> = s.first(
        "SELECT count AS n FROM rate_windows WHERE sender = ?1 AND window_start = ?2",
        &[key.into(), window.into()],
    )?;
    Ok(c.map_or(0, |c| c.n))
}

fn stored_attachments<S: Sql>(s: &S, rowid: i64) -> PResult<Vec<AttachmentRow>> {
    s.query(messages::ATTACHMENTS_OF, &[rowid.into()])
}

/// The duplicate's answer (steps 2 and 3).
fn duplicate<S: Sql>(
    s: &S,
    hit: &Hit,
    inp: &IngestInput,
    is_system: bool,
) -> PResult<IngestOutcome> {
    let atts = stored_attachments(s, hit.rowid)?;
    let attachments = atts
        .iter()
        .enumerate()
        .map(|(i, a)| AttachmentToStore {
            attachment_id: a.id.clone(),
            part_index: inp.attachments.get(i).map_or(i as u32, |m| m.part_index),
            r2_key: a.r2_key.clone(),
            text_pending: a.text_status == "pending",
        })
        .collect();
    Ok(IngestOutcome::Stored {
        message_id: hit.id.clone(),
        thread_id: hit.thread_id.clone(),
        status: hit.status.clone(),
        duplicate: true,
        stored_raw_r2_key: hit.raw_r2_key.clone().unwrap_or_default(),
        attachments,
        is_system,
        triage: false,
    })
}

/// The token check after the brute-force limits: a spent budget answers `Unchecked` without a MAC.
fn token_after_limits<S: Sql>(
    s: &S,
    check: Option<&TokenCheckWire>,
    sender_key: &str,
    window: i64,
) -> PResult<TokenCheck> {
    let Some(check) = check else {
        return Ok(TokenCheck::Absent);
    };
    if *check == TokenCheckWire::Absent {
        return Ok(TokenCheck::Absent);
    }
    let per_sender = format!("tok:{sender_key}");
    if count_of(s, &per_sender, window)? >= TOKEN_FAILS_PER_SENDER
        || count_of(s, "tok:*", window)? >= TOKEN_FAILS_TOTAL
    {
        return Ok(TokenCheck::Unchecked);
    }
    Ok(match check {
        TokenCheckWire::Valid { seq, kid, current } => TokenCheck::Valid {
            seq: *seq,
            kid: *kid,
            current: *current,
        },
        _ => {
            bump(s, &per_sender, window)?;
            bump(s, "tok:*", window)?;
            TokenCheck::Invalid
        }
    })
}

/// Cuts every body so the row stays within the budget (Inbound › Storage caps).
fn fit_bodies(inp: &mut IngestInput, flags: &mut Vec<String>) {
    const ROW_BUDGET: usize = 1_900_000;
    const RESERVE: usize = 16 * 1024;
    let cap = |s: &mut Option<String>, max: usize, flags: &mut Vec<String>| {
        if let Some(t) = s.as_mut()
            && t.len() > max
        {
            let mut end = max;
            while !t.is_char_boundary(end) {
                end -= 1;
            }
            t.truncate(end);
            if !flags.iter().any(|f| f == "body_truncated") {
                flags.push("body_truncated".into());
            }
        }
    };
    cap(&mut inp.extracted_text, 256 * 1024, flags);
    cap(&mut inp.text, 512 * 1024, flags);
    cap(&mut inp.html_sanitized, 1024 * 1024, flags);
    let fixed = inp.subject.len()
        + serde_json::to_string(&inp.to).map_or(0, |s| s.len())
        + serde_json::to_string(&inp.cc).map_or(0, |s| s.len())
        + serde_json::to_string(&inp.references).map_or(0, |s| s.len())
        + inp.automated_json.to_string().len()
        + inp.auth_json.to_string().len()
        + 4096;
    let len = |s: &Option<String>| s.as_ref().map_or(0, String::len);
    let mut room = ROW_BUDGET.saturating_sub(fixed + RESERVE);
    for body in [&mut inp.extracted_text, &mut inp.text] {
        room = room.saturating_sub(len(body));
    }
    if len(&inp.html_sanitized) > room {
        cap(&mut inp.html_sanitized, room, flags);
    }
}

/// Runs the ingest transaction. `is_system` is the mailbox's `meta.is_system`.
pub fn ingest<P: Platform, S: Sql>(
    p: &P,
    s: &S,
    owner: &Owner<'_>,
    mut inp: IngestInput,
    now: i64,
    is_system: bool,
) -> Result<IngestOutcome, ApiError> {
    // 1. Erased check (the mailbox checked meta.erased; an ID an erasure removed is gone for good).
    let erased: Option<Count> = s.first(
        "SELECT 1 AS n FROM erased_ids WHERE message_id = ?1",
        &[inp.message_id.clone().into()],
    )?;
    if erased.is_some() {
        return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
    }
    s.exec(
        "DELETE FROM meta WHERE k = ?1",
        &[format!("parse:{}", inp.message_id).into()],
    )?;

    // 2. Duplicate by hash (B14, J2).
    let by_hash: Option<Hit> = s.first(
        "SELECT m.rowid AS rowid, m.id, t.id AS thread_id, m.status, m.raw_r2_key FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE m.raw_sha256 = ?1 AND m.direction = 'inbound' LIMIT 1",
        &[inp.raw_sha256.clone().into()],
    )?;
    if let Some(hit) = by_hash {
        return Ok(duplicate(s, &hit, &inp, is_system)?);
    }

    // 3. Duplicate by Message-ID (B3): same content is a duplicate; different content is a conflict.
    let mut flags = inp.flags.clone();
    if !inp.message_id_synthetic {
        let same: Vec<Same> = s.query(
            "SELECT rowid, subject, text, html_sanitized FROM messages WHERE rfc_message_id = ?1 AND direction = 'inbound'",
            &[inp.rfc_message_id.clone().into()],
        )?;
        let mut new_hashes: Vec<&str> = inp.attachments.iter().map(|a| a.sha256.as_str()).collect();
        new_hashes.sort_unstable();
        for c in &same {
            let mut old: Vec<String> = stored_attachments(s, c.rowid)?
                .into_iter()
                .map(|a| a.sha256)
                .collect();
            old.sort_unstable();
            let equal = c.subject.as_deref().unwrap_or("") == inp.subject
                && c.text == inp.text
                && c.html_sanitized == inp.html_sanitized
                && old == new_hashes;
            if equal {
                let hit: Option<Hit> = s.first(
                    "SELECT m.rowid AS rowid, m.id, t.id AS thread_id, m.status, m.raw_r2_key FROM messages m JOIN threads t ON t.seq = m.thread_seq WHERE m.rowid = ?1",
                    &[c.rowid.into()],
                )?;
                if let Some(hit) = hit {
                    return Ok(duplicate(s, &hit, &inp, is_system)?);
                }
            }
        }
        if !same.is_empty() {
            flags.push("message_id_conflict".into());
            for c in &same {
                s.exec(
                    "UPDATE messages SET flags_json = json_insert(flags_json, '$[#]', 'message_id_conflict') WHERE rowid = ?1 AND NOT EXISTS (SELECT 1 FROM json_each(flags_json) WHERE value = 'message_id_conflict')",
                    &[c.rowid.into()],
                )?;
            }
        }
    }

    let verdict = verdict_of(&inp.verdict);
    let authenticated = trust::authenticated(verdict, inp.aligned);
    let from_address = inp.from.as_ref().map(|f| f.address.clone());
    let sender_key = match (&from_address, authenticated) {
        (Some(a), true) => a.clone(),
        (Some(a), false) => format!("unauth:{a}"),
        (None, _) => format!("env:{}", inp.envelope_from),
    };
    let window = inp.received_at - inp.received_at.rem_euclid(HOUR);

    // 4. DSN: find the original; none → backscatter, before any write.
    let mut dsn_thread: Option<i64> = None;
    if inp.kind == "dsn" {
        #[derive(Deserialize)]
        struct Orig {
            thread_seq: i64,
        }
        let orig: Option<Orig> = match &inp.dsn_original_message_id {
            Some(id) => s.first(
                "SELECT thread_seq FROM messages WHERE direction = 'outbound' AND (rfc_message_id = ?1 OR provider_message_id = ?1) ORDER BY rowid DESC LIMIT 1",
                &[id.clone().into()],
            )?,
            None => None,
        };
        match orig {
            None => return Ok(IngestOutcome::Backscatter),
            // Delivery state from a DSN is trusted only for an SMTP-relay send (D11, M23): until then
            // every DSN is stored hidden and changes nothing.
            Some(o) => {
                dsn_thread = Some(o.thread_seq);
                flags.push("dsn_untrusted".into());
            }
        }
    }

    // 5–6. Thread token after its limits, then the thread.
    let token = token_after_limits(s, inp.token.as_ref(), &sender_key, window)?;
    let lookup = threads::Lookup(s);
    let resolution = match dsn_thread {
        Some(seq) => thread::ThreadResolution {
            decision: ThreadDecision::Join {
                seq,
                via: thread::JoinVia::InReplyTo,
            },
            join_unverified: false,
        },
        None => thread::resolve_inbound(
            &InboundThreadInputs {
                token,
                in_reply_to: inp.in_reply_to.as_deref(),
                references: &inp.references,
                sender: from_address.as_deref().unwrap_or(""),
                sender_authenticated: authenticated,
                calendar_uid_hash: None,
                calendar_organizer: None,
                calendar_replier: None,
            },
            &lookup,
        ),
    };
    if resolution.join_unverified {
        flags.push("thread_join_unverified".into());
    }

    // 7. Trust and quarantine.
    let contacts: Vec<ContactRow> = s.query(
        "SELECT address, name, inbound_count, outbound_count FROM contacts WHERE outbound_count > 0 OR address = ?1 LIMIT 5000",
        &[from_address.clone().unwrap_or_default().into()],
    )?;
    let core_contacts: Vec<Contact<'_>> = contacts
        .iter()
        .map(|c| Contact {
            address: &c.address,
            name: c.name.as_deref().unwrap_or(""),
            outbound_count: c.outbound_count as u64,
        })
        .collect();
    let domains: Vec<TenantDomain<'_>> = inp
        .tenant_domains
        .iter()
        .map(|(name, platform)| TenantDomain {
            name,
            platform: *platform,
        })
        .collect();
    let receive_allow = inp.receive_decision.as_deref() == Some("allow");
    let signals = inp.from.as_ref().map(|f| {
        let from = CoreMailbox {
            address: f.address.clone(),
            name: f.name.clone(),
        };
        trust::signals(&TrustInput {
            from: &from,
            reply_to: &inp.reply_to,
            verdict,
            aligned: inp.aligned,
            receive_allow,
            platform_domain: &inp.platform_domain,
            sender_in_tenant: inp.sender_in_tenant,
            sender_is_identity: inp.sender_is_identity,
            tenant_domains: &domains,
            tenant_suffix: &inp.tenant_suffix,
            tenant_name: &inp.tenant_name,
            identity_name: &inp.identity_name,
            contacts: &core_contacts,
        })
    });
    let sig = signals.unwrap_or_default();
    for (on, name) in [
        (sig.display_name_spoof, "display_name_spoof"),
        (sig.lookalike_domain, "lookalike_domain"),
        (sig.reply_to_mismatch, "reply_to_mismatch"),
        (sig.shared_domain_sender, "shared_domain_sender"),
    ] {
        if on {
            flags.push(name.into());
        }
    }
    if inp.sender_suppressed {
        flags.push("sender_suppressed".into());
    }
    let known = sig.known_sender;
    let known_correspondent = authenticated
        && contacts
            .iter()
            .any(|c| Some(&c.address) == from_address.as_ref() && c.outbound_count > 0);
    let mut spam = inp.spam_base + verdict_weight(verdict);
    if sig.display_name_spoof {
        spam += 0.30;
    }
    if sig.lookalike_domain {
        spam += 0.30;
    }
    if sig.shared_domain_sender {
        spam += 0.05;
    }
    if sig.reply_to_mismatch {
        spam += 0.10;
    }
    if known {
        spam -= 0.40;
    }
    let previously_quarantined = |addr: &str| -> PResult<bool> {
        let n: Option<Count> = s.first(
            "SELECT COUNT(*) AS n FROM messages WHERE from_address = ?1 AND direction = 'inbound' AND status = 'quarantined'",
            &[addr.into()],
        )?;
        Ok(n.is_some_and(|n| n.n > 0))
    };
    if let Some(c) = contacts
        .iter()
        .find(|c| Some(&c.address) == from_address.as_ref() && c.inbound_count >= 3)
        && !previously_quarantined(&c.address)?
    {
        spam -= 0.10;
    }
    if inp.loopback {
        spam = 0.0;
    }
    let spam = spam.clamp(0.0, 1.0);

    // The inbound volume caps (D5, D13).
    let visible_kind = !matches!(inp.kind.as_str(), "dsn" | "mdn");
    let per_sender = bump(s, &sender_key, window)?;
    let unauth = if authenticated {
        0
    } else {
        bump(s, "unauth:*", window)?
    };
    let exempt = known_correspondent || receive_allow;
    let throttled = per_sender > inp.policy.per_sender_per_hour
        || (!exempt && unauth > inp.policy.unauthenticated_per_hour)
        || (!exempt && inp.inbound_admit == "throttle");

    // E5: a verification match with no `wait` for the sender's domain in the last 30 minutes.
    let sender_domain = from_address
        .as_deref()
        .map(|a| org_domain(domain_of(a)).to_owned())
        .unwrap_or_default();
    let unsolicited = match &inp.verification {
        Some(_) => {
            #[derive(Deserialize)]
            struct W {
                v: String,
            }
            let w: Option<W> = s.first(
                "SELECT v FROM meta WHERE k = ?1",
                &[format!("wait:{sender_domain}").into()],
            )?;
            !w.and_then(|w| w.v.parse::<i64>().ok())
                .is_some_and(|until| until >= inp.received_at - WAIT_GRACE_MS)
        }
        None => false,
    };

    // The quarantine decision, first match.
    let risky = inp.attachments.iter().any(|a| a.risk_quarantines);
    let (status, reason): (&str, Option<&str>) = if inp.kind == "dsn" {
        ("hidden", None)
    } else if inp.receive_decision.as_deref() == Some("block") {
        ("hidden", Some("blocked_sender"))
    } else if throttled {
        ("throttled", None)
    } else if verdict == Verdict::Fail && inp.policy.on_auth_fail {
        ("quarantined", Some("auth_failed"))
    } else if verdict == Verdict::Unverified && inp.policy.on_auth_fail {
        ("quarantined", Some("auth_unverified"))
    } else if risky {
        ("quarantined", Some("risky_attachment"))
    } else if unsolicited && inp.policy.unsolicited_otp {
        ("quarantined", Some("otp_unsolicited"))
    } else if !inp.loopback && spam >= inp.policy.spam_threshold && !receive_allow {
        ("quarantined", Some("spam"))
    } else {
        ("received", None)
    };
    let visible = status == "received";

    // 8. The thread.
    let ids = p.ids();
    let reply_from = if visible && !inp.is_bcc {
        inp.reply_from.clone()
    } else {
        None
    };
    let seq = match resolution.decision {
        ThreadDecision::Join { seq, .. } => seq,
        ThreadDecision::New => {
            let subject = if visible {
                thread::normalise_subject(&inp.subject)
            } else {
                String::new()
            };
            threads::insert(
                s,
                &ids.new_id(IdPrefix::Thread),
                &subject,
                inp.received_at,
                reply_from.as_deref(),
            )?
        }
    };
    #[derive(Deserialize)]
    struct Tid {
        id: String,
    }
    let thread_id = s
        .first::<Tid>("SELECT id FROM threads WHERE seq = ?1", &[seq.into()])?
        .map(|t| t.id)
        .unwrap_or_default();

    // 9. The message.
    if is_system {
        for body in [
            &mut inp.text,
            &mut inp.html_sanitized,
            &mut inp.extracted_text,
        ] {
            if let Some(b) = body.as_mut() {
                *b = pylota_identity_core::text::redact_links_and_codes(b);
            }
        }
        flags.push("body_redacted".into());
    }
    if inp.is_bcc {
        flags.push("bcc".into());
    }
    if inp.loopback {
        flags.push("loopback".into());
    }
    fit_bodies(&mut inp, &mut flags);
    flags.dedup();
    let snippet = pylota_identity_core::text::snippet(inp.extracted_text.as_deref().unwrap_or(""));
    let triage_status = if is_system || !visible_kind || matches!(status, "hidden" | "throttled") {
        "skipped"
    } else if status != "received" {
        ""
    } else if inp.policy.triage_enabled {
        "pending"
    } else {
        "skipped"
    };
    let triage_json = (triage_status == "skipped").then(|| {
        json!({ "status": "skipped", "reason": if is_system || !visible_kind || !visible { "not_eligible" } else { "policy_disabled" }, "risk_flags": [], "model": null, "version": crate::triage::TRIAGE_VERSION, "run": 0 }).to_string()
    });
    let from = inp.from.clone().unwrap_or_default();
    // The verification match is kept with the message, so a release can insert its row (Read path).
    let mut automated = inp.automated_json.clone();
    if let (Some(v), Some(o)) = (&inp.verification, automated.as_object_mut()) {
        o.insert(
            "verification".into(),
            json!({ "kind": v.kind, "value": v.value }),
        );
    }
    #[derive(Deserialize)]
    struct RowId {
        rowid: i64,
    }
    let rowid = s
        .first::<RowId>(
            "INSERT INTO messages (id, thread_seq, direction, status, rfc_message_id, message_id_synthetic, in_reply_to, references_json, raw_sha256, raw_r2_key, raw_size, from_address, from_name, sender_domain, reply_to_json, to_json, cc_json, delivered_to, is_bcc, is_primary_recipient, subject, text, html_sanitized, extracted_text, snippet, sent_at, received_at, kind, automated_json, auth_json, verdict, spam_score, known_sender, quarantine_reason, flags_json, read, triage_status, triage_json, parser_version, metadata_json, received_evented_at) \
             VALUES (?1, ?2, 'inbound', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, 0, ?35, ?36, ?37, '{}', ?38) RETURNING rowid",
            &[
                inp.message_id.clone().into(),
                seq.into(),
                status.into(),
                inp.rfc_message_id.clone().into(),
                i64::from(inp.message_id_synthetic).into(),
                inp.in_reply_to.clone().into(),
                serde_json::to_string(&inp.references).unwrap_or_else(|_| "[]".into()).into(),
                inp.raw_sha256.clone().into(),
                inp.raw_r2_key.clone().into(),
                (inp.raw_size as i64).into(),
                from_address.clone().into(),
                (!from.name.is_empty()).then(|| from.name.clone()).into(),
                (!sender_domain.is_empty()).then(|| sender_domain.clone()).into(),
                serde_json::to_string(&inp.reply_to.iter().map(|a| json!({ "address": a, "name": "" })).collect::<Vec<_>>()).unwrap_or_default().into(),
                serde_json::to_string(&inp.to).unwrap_or_default().into(),
                serde_json::to_string(&inp.cc).unwrap_or_default().into(),
                inp.delivered_to.clone().into(),
                i64::from(inp.is_bcc).into(),
                i64::from(inp.is_primary_recipient).into(),
                inp.subject.clone().into(),
                inp.text.clone().into(),
                inp.html_sanitized.clone().into(),
                inp.extracted_text.clone().into(),
                snippet.clone().into(),
                inp.sent_at.into(),
                inp.received_at.into(),
                inp.kind.clone().into(),
                automated.to_string().into(),
                inp.auth_json.to_string().into(),
                inp.verdict.clone().into(),
                spam.into(),
                i64::from(known).into(),
                reason.map(str::to_owned).into(),
                serde_json::to_string(&flags).unwrap_or_else(|_| "[]".into()).into(),
                (!triage_status.is_empty()).then(|| triage_status.to_owned()).into(),
                triage_json.into(),
                i64::from(inp.parser_version).into(),
                visible.then_some(now).into(),
            ],
        )?
        .map(|r| r.rowid)
        .ok_or_else(ApiError::internal)?;

    // 10. Attachments.
    let mut to_store = Vec::new();
    for a in &inp.attachments {
        let att = ids.new_id(IdPrefix::Attachment);
        let key = format!(
            "t/{}/i/{}/m/{}/a/{att}",
            owner.tenant_id, owner.identity_id, inp.message_id
        );
        let text_pending =
            a.text_eligible && !is_system && !matches!(status, "hidden" | "throttled");
        s.exec(
            "INSERT INTO attachments (id, message_rowid, filename, content_type, sniffed_type, size, sha256, disposition, content_id, r2_key, text_status, risk, scan_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'skipped')",
            &[
                att.clone().into(),
                rowid.into(),
                a.filename.clone().into(),
                a.content_type.clone().into(),
                a.sniffed_type.clone().into(),
                (a.size as i64).into(),
                a.sha256.clone().into(),
                if a.inline { "inline" } else { "attachment" }.into(),
                a.content_id.clone().into(),
                key.clone().into(),
                if text_pending { "pending" } else { "skipped" }.into(),
                a.risk.clone().into(),
            ],
        )?;
        to_store.push(AttachmentToStore {
            attachment_id: att,
            part_index: a.part_index,
            r2_key: key,
            text_pending,
        });
    }

    // 11–12. Keyword index and refs (never in the system mailbox, A15).
    if !is_system {
        let recipients: Vec<(String, String)> = inp
            .to
            .iter()
            .chain(inp.cc.iter())
            .map(|a| (a.name.clone(), a.address.clone()))
            .collect();
        let att_names: Vec<(String, String)> = inp
            .attachments
            .iter()
            .map(|a| (a.filename.clone().unwrap_or_default(), String::new()))
            .collect();
        let ref_values: Vec<String> = inp.refs.iter().map(|r| r.value.clone()).collect();
        let doc = fts_doc::fts_doc(&MessageForIndex {
            subject: &inp.subject,
            from_name: &from.name,
            from_address: &from.address,
            recipients: &recipients,
            delivered_to: &inp.delivered_to,
            extracted_text: inp.extracted_text.as_deref().unwrap_or(""),
            text: inp.text.as_deref().unwrap_or(""),
            attachments: &att_names,
            refs: &ref_values,
        });
        s.exec(
            "INSERT INTO fts (rowid, subject, participants, body_new, body_full, attachments, refs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                rowid.into(),
                doc.subject.clone().into(),
                doc.participants.clone().into(),
                doc.body_new.into(),
                doc.body_full.into(),
                doc.attachments.into(),
                doc.refs.clone().into(),
            ],
        )?;
        s.exec(
            "INSERT INTO fts_tri (rowid, subject, participants, refs) VALUES (?1, ?2, ?3, ?4)",
            &[
                rowid.into(),
                doc.subject.into(),
                doc.participants.into(),
                doc.refs.into(),
            ],
        )?;
        for r in &inp.refs {
            s.exec(
                "INSERT OR IGNORE INTO refs (message_rowid, kind, value, source) VALUES (?1, ?2, ?3, ?4)",
                &[rowid.into(), r.kind.clone().into(), r.value.clone().into(), r.source.clone().into()],
            )?;
        }
    }

    // 13. Contacts (received, quarantined, screening).
    if matches!(status, "received" | "quarantined" | "screening")
        && let Some(addr) = &from_address
    {
        s.exec(
            "INSERT INTO contacts (address, name, domain, first_seen_at, last_seen_at, inbound_count, last_thread_seq) VALUES (?1, ?2, ?3, ?4, ?4, 1, ?5) \
             ON CONFLICT (address) DO UPDATE SET name = COALESCE(NULLIF(excluded.name, ''), contacts.name), last_seen_at = MAX(contacts.last_seen_at, excluded.last_seen_at), inbound_count = contacts.inbound_count + 1, last_thread_seq = excluded.last_thread_seq",
            &[
                addr.clone().into(),
                from.name.clone().into(),
                domain_of(addr).to_owned().into(),
                inp.received_at.into(),
                seq.into(),
            ],
        )?;
    }

    // 14. The verification row: a `pass` verdict and a received message.
    let mut verification_event = None;
    if let Some(v) = &inp.verification
        && verdict == Verdict::Pass
        && visible
    {
        s.exec(
            "INSERT INTO verifications (message_rowid, kind, value, sender_domain, expires_at, consumed_at, account_id) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)",
            &[
                rowid.into(),
                v.kind.clone().into(),
                v.value.clone().into(),
                sender_domain.clone().into(),
                (inp.received_at + 24 * HOUR).into(),
            ],
        )?;
        verification_event = Some(v.kind.clone());
    }

    // 15. The thread's visible columns.
    if visible {
        let mut addrs: Vec<(&str, &str)> = Vec::new();
        if let Some(f) = &inp.from {
            addrs.push((&f.address, &f.name));
        }
        for a in inp.to.iter().chain(inp.cc.iter()) {
            addrs.push((&a.address, &a.name));
        }
        let own: Vec<&str> = inp.identity_addresses.iter().map(String::as_str).collect();
        threads::recompute(s, seq, &addrs, &own, reply_from.as_deref(), true)?;
    }

    // 16. The outbox: message.received or message.quarantined, and verification.received.
    if matches!(status, "received" | "quarantined") {
        let row: Option<messages::Row> = s.first(
            &format!("{} WHERE m.rowid = ?1", messages::SELECT),
            &[rowid.into()],
        )?;
        let row = row.ok_or_else(ApiError::internal)?;
        let atts = stored_attachments(s, rowid)?;
        let (event_type, data) = if visible {
            (
                "message.received",
                payloads::message_received(&row, &atts, &[], inp.policy.webhook_text_bytes, false),
            )
        } else {
            (
                "message.quarantined",
                payloads::message_quarantined(&row, &atts, &[]),
            )
        };
        outbox::append(
            s,
            owner,
            &outbox::new_event_id(p),
            event_type,
            inp.received_at,
            &data,
            now,
        )?;
        if let Some(kind) = verification_event {
            outbox::append(
                s,
                owner,
                &outbox::new_event_id(p),
                "verification.received",
                inp.received_at,
                &payloads::verification_received(&inp.message_id, &sender_domain, &kind),
                now,
            )?;
        }
    }

    Ok(IngestOutcome::Stored {
        message_id: inp.message_id,
        thread_id,
        status: status.to_owned(),
        duplicate: false,
        stored_raw_r2_key: inp.raw_r2_key,
        attachments: to_store,
        is_system,
        triage: triage_status == "pending",
    })
}
