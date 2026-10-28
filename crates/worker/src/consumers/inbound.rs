//! The `pm-inbound` consumer ([Inbound › The pm-inbound consumer]): from a pointer to a stored message.
//! Each message is processed on its own and acked after its post-commit steps; a failure retries it
//! after 30 seconds, and an unexpected error is left to the queue's own retry.
//!
//! The SES source (M23), re-parse (M14's job) and role-mail relay (M13) pointers arrive with their
//! milestones; until then they are acked without work.
//!
//! [Inbound › The pm-inbound consumer]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use std::collections::HashMap;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_api_types::internal::{EmbedReason, IndexJob, TriageReason};
use pylota_identity_core::address;
use pylota_identity_core::attach::Family;
use pylota_identity_core::auth::{DnsAnswers, TxtAnswer};
use pylota_identity_core::mime::{self, Caps};
use pylota_identity_core::refs::{self, Pack};
use pylota_identity_core::{classify, inbound, thread};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::dns::{DnsStatus, RType, Resolver};
use pylota_identity_platform::durable::{DoClass, RpcResult};
use pylota_identity_platform::queues::{Incoming, QueueName};
use pylota_identity_platform::r2::{BlobBody, BlobMeta};
use pylota_identity_platform::{
    BlobStore as _, Clock as _, ControlDb as _, Dns as _, ObjectClient as _, Platform,
    QueueProducer as _,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::{identities as idb, tenants as tdb};
use crate::mailbox::MailboxRequest;
use crate::mailbox::ingest::{
    Addr, AttachmentMeta, InboundPolicy, IngestInput, IngestOutcome, PARSER_VERSION, RefRow,
    VerificationIn,
};
use crate::quota::{AdmitAnswer, QuotaRequest, UsageMetric};
use crate::rpc::envelope_for;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InboundJob {
    Message(InboundPointer),
    Staged(StagedPointer),
    Reparse(Value),
    RoleMail(Value),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InboundPointer {
    pub v: u8,
    pub source: RawSource,
    pub message_id: Option<String>,
    pub tenant_id: Option<String>,
    pub identity_id: Option<String>,
    pub r2_key: Option<String>,
    pub raw_size: u64,
    pub envelope_from: String,
    pub envelope_to: String,
    pub received_at: i64,
    pub request_id: String,
    pub loopback: Option<LoopbackSource>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RawSource {
    Routing,
    /// The SES source (M23).
    Ses(Value),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoopbackSource {
    pub tenant_id: String,
    pub identity_id: String,
    pub message_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StagedPointer {
    pub v: u8,
    pub staging_key: String,
    pub raw_size: u64,
    pub envelope_from: String,
    pub envelope_to: String,
    pub received_at: i64,
    pub request_id: String,
}

/// Retried after this many seconds on a known transient failure (Failure handling); one second in the
/// itest build, so the suite sees retries without waiting.
const RETRY_S: u32 = if cfg!(feature = "itest-hooks") { 1 } else { 30 };
/// From this parse attempt the consumer parses headers only (B2, FR-IN-3).
pub const DEGRADED_FROM_ATTEMPT: i64 = 3;
/// Pointers younger than this have not failed before and go first (Poison messages).
const FRESH_MS: i64 = 120_000;

/// How one message ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    Ack,
    Retry,
}

/// One batch: fresh pointers first, then older ones, oldest last.
pub async fn batch<P: Platform>(p: &P, mut batch: Vec<Incoming<Value>>) {
    let now = p.clock().now_ms();
    let age = |m: &Incoming<Value>| now - m.body["received_at"].as_i64().unwrap_or(now);
    batch.sort_by_key(|m| {
        let a = age(m);
        if a < FRESH_MS { (0, a) } else { (1, -a) }
    });
    for m in batch {
        let job: Result<InboundJob, _> = serde_json::from_value(m.body.clone());
        let done = match job {
            Ok(InboundJob::Message(ptr)) => message(p, ptr).await,
            Ok(InboundJob::Staged(ptr)) => staged(p, ptr).await,
            // Re-parse (M14) and role-mail relay (M13) pointers: nothing to do until then.
            Ok(InboundJob::Reparse(_) | InboundJob::RoleMail(_)) => Done::Ack,
            Err(_) => {
                crate::log::warn("inbound_bad_pointer", &[]);
                Done::Ack
            }
        };
        match done {
            Done::Ack => m.ack(),
            Done::Retry => m.retry(Some(RETRY_S)),
        }
    }
}

/// A staged pointer (J7): routed now, or dropped when the recipient is unroutable.
pub async fn staged<P: Platform>(p: &P, ptr: StagedPointer) -> Done {
    let Ok(env) = address::parse_envelope(&ptr.envelope_to) else {
        let _ = p
            .blobs()
            .delete(std::slice::from_ref(&ptr.staging_key))
            .await;
        return Done::Ack;
    };
    let entry: Option<crate::email::Entry> =
        match p.db().first(crate::email::lookup(&env.base())).await {
            Ok(e) => e,
            Err(_) => return Done::Retry,
        };
    let now = p.clock().now_ms();
    let routable = entry.as_ref().is_some_and(|e| {
        crate::email::decide(Some(e), now).is_ok() || e.tenant_status == "suspended"
    });
    let Some(e) = entry.filter(|_| routable) else {
        let _ = p
            .blobs()
            .delete(std::slice::from_ref(&ptr.staging_key))
            .await;
        crate::metrics::count(p, "inbound_staged_unroutable_total", None, 1);
        return Done::Ack;
    };
    let raw = match p.blobs().get(&ptr.staging_key).await {
        Ok(Some(b)) => b,
        Ok(None) => return Done::Ack,
        Err(_) => return Done::Retry,
    };
    // The message ID is derived from the staging key, so a retry after the copy reuses it.
    let message_id = pylota_identity_core::ids::derived(
        IdPrefix::Message,
        ptr.received_at,
        &p.secrets().hash_key,
        &format!("staged:{}", ptr.staging_key),
    );
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
        ],
    };
    if p.blobs()
        .put(&key, BlobBody::Bytes(raw.body.to_vec()), &meta)
        .await
        .is_err()
    {
        return Done::Retry;
    }
    let done = message(
        p,
        InboundPointer {
            v: 1,
            source: RawSource::Routing,
            message_id: Some(message_id),
            tenant_id: Some(e.tenant_id),
            identity_id: Some(e.identity_id),
            r2_key: Some(key),
            raw_size: raw.size,
            envelope_from: ptr.envelope_from,
            envelope_to: ptr.envelope_to,
            received_at: ptr.received_at,
            request_id: ptr.request_id,
            loopback: None,
        },
    )
    .await;
    if done == Done::Ack {
        let _ = p
            .blobs()
            .delete(std::slice::from_ref(&ptr.staging_key))
            .await;
    }
    done
}

/// The TXT answers of the DNS prefetch (Authentication verdict, step 2): the first resolver, then the
/// second on an error.
async fn prefetch<P: Platform>(p: &P, names: &[String]) -> DnsAnswers {
    let mut out = HashMap::new();
    for name in names {
        let mut answer = TxtAnswer::TempError;
        for r in [Resolver::First, Resolver::Second] {
            match p.dns().query(r, name, RType::Txt).await {
                Ok(a) if a.status == DnsStatus::NxDomain => {
                    answer = TxtAnswer::NotFound;
                    break;
                }
                Ok(a) if a.status == DnsStatus::NoError => {
                    let records: Vec<Vec<u8>> = a
                        .records
                        .iter()
                        .filter(|r| r.rtype == RType::Txt)
                        .map(|r| txt_bytes(&r.data))
                        .collect();
                    answer = if records.is_empty() {
                        TxtAnswer::NotFound
                    } else {
                        TxtAnswer::Records(records)
                    };
                    break;
                }
                _ => {}
            }
        }
        out.insert(name.clone(), answer);
    }
    out
}

/// A DoH TXT `data`: one or more quoted strings, joined.
fn txt_bytes(data: &str) -> Vec<u8> {
    let d = data.trim();
    if !d.starts_with('"') {
        return d.as_bytes().to_vec();
    }
    let mut out = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    for c in d.chars() {
        match (in_str, esc, c) {
            (true, false, '\\') => esc = true,
            (true, false, '"') => in_str = false,
            (false, _, '"') => in_str = true,
            (true, _, c) => {
                esc = false;
                let mut b = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            }
            _ => {}
        }
    }
    out
}

fn addrs(list: &[mime::Mailbox]) -> Vec<Addr> {
    list.iter()
        .take(200)
        .map(|m| Addr {
            address: m.address.clone(),
            name: m.name.clone(),
        })
        .collect()
}

#[derive(Deserialize)]
struct AddrRow {
    address: String,
    identity_id: String,
}

#[derive(Deserialize)]
struct One {
    #[allow(dead_code)]
    n: i64,
}

#[derive(Deserialize)]
struct DomainRow {
    name: String,
    kind: String,
}

/// A message pointer: steps 1–15.
pub async fn message<P: Platform>(p: &P, ptr: InboundPointer) -> Done {
    if !matches!(ptr.source, RawSource::Routing) {
        // The SES source arrives with M23.
        return Done::Ack;
    }
    let (Some(message_id), Some(tenant_id), Some(identity_id), Some(r2_key)) = (
        ptr.message_id.clone(),
        ptr.tenant_id.clone(),
        ptr.identity_id.clone(),
        ptr.r2_key.clone(),
    ) else {
        return Done::Ack;
    };

    // 1. Re-read scope.
    let (idn, t) = match (
        p.db().first::<idb::Row>(idb::get(&identity_id)).await,
        p.db().first::<tdb::Row>(tdb::get(&tenant_id)).await,
    ) {
        (Ok(Some(i)), Ok(Some(t))) if i.tenant_id == t.id => (i, t),
        (Ok(_), Ok(_)) => {
            let _ = p.blobs().delete(std::slice::from_ref(&r2_key)).await;
            crate::metrics::count(p, "inbound_dropped_total", Some(&tenant_id), 1);
            return Done::Ack;
        }
        _ => return Done::Retry,
    };
    if matches!(idn.status.as_str(), "deleting" | "deleted")
        || matches!(t.status.as_str(), "erasing" | "erased")
        || idn.mailbox_do_id.is_empty()
    {
        let _ = p.blobs().delete(std::slice::from_ref(&r2_key)).await;
        crate::metrics::count(p, "inbound_dropped_total", Some(&tenant_id), 1);
        return Done::Ack;
    }
    let mailbox = idn.mailbox_do_id.clone();
    let call = |req: MailboxRequest| {
        let env = envelope_for(
            p,
            Some(&tenant_id),
            Some(&identity_id),
            &ptr.request_id,
            None,
            req,
        );
        let mailbox = mailbox.clone();
        async move {
            p.objects()
                .call::<_, Value>(DoClass::Mailbox, &mailbox, &env)
                .await
        }
    };

    // 2. Fetch raw.
    let raw = match p.blobs().get(&r2_key).await {
        Ok(Some(b)) => b.body.to_vec(),
        Ok(None) => {
            let stored = call(MailboxRequest::ConfirmStored {
                message_id: message_id.clone(),
            })
            .await;
            if !matches!(&stored, Ok(RpcResult::Ok(v)) if v["stored"] == true) {
                crate::metrics::count(p, "inbound_raw_missing_total", Some(&tenant_id), 1);
            }
            return Done::Ack;
        }
        Err(_) => return Done::Retry,
    };

    // 3. Hash.
    let raw_sha256 = pylota_identity_core::ses::sha256_hex(&raw);

    // 4. Parse, degraded from the third attempt.
    let attempts = match call(MailboxRequest::NoteParseAttempt {
        message_id: message_id.clone(),
    })
    .await
    {
        Ok(RpcResult::Ok(v)) => v["attempts"].as_i64().unwrap_or(1),
        _ => return Done::Retry,
    };
    let degraded = attempts >= DEGRADED_FROM_ATTEMPT;
    #[cfg(all(feature = "itest-hooks", target_arch = "wasm32"))]
    if !degraded
        && pylota_identity_platform::cf::itest::take_fault("parse.panic", "")
            .await
            .is_some()
    {
        panic!("the parse.panic fault hook");
    }
    let mut parsed = if degraded {
        mime::parse_headers_only(&raw)
    } else {
        mime::parse(&raw, &Caps::default())
    };
    if degraded {
        parsed.flags.insert(mime::Flag::ParseDegraded);
    }

    // 5. Effective policy.
    let policy = crate::policy::effective(p, &t);
    let pol = |path: &str| {
        pylota_identity_core::policy::get_path(&policy, path)
            .cloned()
            .unwrap_or(Value::Null)
    };

    // 6. Classify.
    let classification = classify::classify(&parsed, &ptr.envelope_from);

    // 7. Authenticate (loopback: L3; only a pointer of this tenant passes).
    let loopback = ptr
        .loopback
        .as_ref()
        .is_some_and(|l| l.tenant_id == tenant_id);
    let from_domain = inbound::from_domain(&parsed);
    let dns = if loopback || degraded {
        DnsAnswers::new()
    } else {
        let names = pylota_identity_core::auth::prefetch_names(
            &raw,
            (!from_domain.is_empty()).then_some(from_domain.as_str()),
        );
        prefetch(p, &names).await
    };
    let auth = inbound::authenticate(
        &raw,
        &parsed,
        &ptr.envelope_from,
        &dns,
        p.config().trusted_authserv_id.as_deref().unwrap_or(""),
        None,
        loopback,
    );

    // 8. Content.
    let content = if degraded {
        inbound::Content::default()
    } else {
        inbound::content(&parsed)
    };

    // 9. References.
    let packs: Vec<Pack> = pol("search.refs_packs")
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().and_then(Pack::from_name))
                .collect()
        })
        .unwrap_or_else(|| vec![Pack::Core]);
    let subject = mime::clean_display(parsed.subject.as_deref().unwrap_or(""), 998);
    let refs: Vec<RefRow> = if degraded || idn.is_system == 1 {
        Vec::new()
    } else {
        refs::extract(&subject, &content.extracted_text, &packs, &[], &t.timezone)
            .into_iter()
            .map(|r| RefRow {
                kind: r.kind.as_str().into_owned(),
                value: r.value,
                source: r.source.to_string(),
            })
            .collect()
    };

    // 10. Attachments: sniff and classify.
    let extract_kinds: Vec<String> = pol("inbound.extract_attachment_text")
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let image_text = pol("inbound.extract_image_text") == Value::Bool(true);
    let attachments: Vec<AttachmentMeta> = parsed
        .attachments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let assessment = pylota_identity_core::attach::assess(
                &a.data,
                a.filename.as_deref(),
                &a.content_type,
            );
            let family = match assessment.family {
                Family::Pdf => Some("pdf"),
                Family::Document => Some("office"),
                Family::Text => Some("text"),
                Family::Html => Some("html"),
                Family::Image => Some("image"),
                _ => None,
            };
            let eligible = assessment.risk.is_none()
                && a.size <= 20 * 1024 * 1024
                && family.is_some_and(|f| {
                    (f == "image" && image_text && !(a.inline && a.size < 10 * 1024))
                        || extract_kinds.iter().any(|k| k == f)
                })
                || a.content_type.eq_ignore_ascii_case("message/rfc822");
            AttachmentMeta {
                part_index: i as u32,
                filename: a.filename.as_deref().map(mime::sanitise_filename),
                content_type: a.content_type.clone(),
                sniffed_type: Some(assessment.sniffed_type.to_owned()),
                size: a.size as u64,
                sha256: pylota_identity_core::ses::sha256_hex(&a.data),
                inline: a.inline,
                content_id: a.content_id.clone(),
                risk: assessment
                    .risk
                    .and_then(|r| serde_json::to_value(r).ok())
                    .and_then(|v| v.as_str().map(str::to_owned)),
                risk_quarantines: assessment.risk.is_some_and(|r| r.quarantines()),
                text_eligible: eligible,
            }
        })
        .collect();

    // 11. Verification codes and links.
    let links = content
        .html_sanitized
        .as_deref()
        .map(inbound::links)
        .unwrap_or_default();
    let verification =
        classify::find_verification(&subject, &content.extracted_text, &links).map(|v| {
            VerificationIn {
                kind: v.kind.to_owned(),
                value: v.value,
            }
        });

    // 12. D1 facts: suppression, tenant domains, the identity's and the tenant's addresses.
    let from_address = parsed.from.as_ref().map(|f| f.address.clone());
    let hash = from_address
        .as_deref()
        .map(|a| address::address_hash(&p.secrets().hash_key, a))
        .unwrap_or_default();
    let header_recipients: Vec<String> = parsed
        .to
        .iter()
        .chain(parsed.cc.iter())
        .take(50)
        .map(|m| m.address.clone())
        .collect();
    let suppressed: Result<Option<One>, _> = p
        .db()
        .first(Stmt::new(
            "SELECT 1 AS n FROM suppressions WHERE tenant_id = ?1 AND address_hash = ?2 AND (expires_at IS NULL OR expires_at > ?3)",
            vec![tenant_id.clone().into(), hash.into(), ptr.received_at.into()],
        ))
        .await;
    let domains: Result<Vec<DomainRow>, _> = p
        .db()
        .query(Stmt::new(
            "SELECT name, kind FROM domains WHERE (tenant_id = ?1 OR kind = 'platform') AND state <> 'removed'",
            vec![tenant_id.clone().into()],
        ))
        .await;
    let mut lookup_addrs = header_recipients.clone();
    if let Some(f) = &from_address {
        lookup_addrs.push(f.clone());
    }
    let tenant_addrs: Result<Vec<AddrRow>, _> = p
        .db()
        .query(Stmt::new(
            "SELECT address, identity_id FROM addresses WHERE tenant_id = ?1 AND status IN ('active','retiring') AND address IN (SELECT value FROM json_each(?2))",
            vec![tenant_id.clone().into(), serde_json::to_string(&lookup_addrs).unwrap_or_default().into()],
        ))
        .await;
    #[derive(Deserialize)]
    struct Own {
        address: String,
        status: String,
    }
    let own: Result<Vec<Own>, _> = p
        .db()
        .query(Stmt::new(
            "SELECT address, status FROM addresses WHERE identity_id = ?1",
            vec![identity_id.clone().into()],
        ))
        .await;
    let (Ok(suppressed), Ok(domains), Ok(tenant_addrs), Ok(own)) =
        (suppressed, domains, tenant_addrs, own)
    else {
        return Done::Retry;
    };

    // A9: the primary recipient, A10: the BCC flag, Threading § 5: reply_from_address.
    let delivered_to = address::parse_envelope(&ptr.envelope_to)
        .map(|e| e.base())
        .unwrap_or_else(|_| ptr.envelope_to.clone());
    let primary = header_recipients
        .iter()
        .find_map(|r| tenant_addrs.iter().find(|a| a.address == *r));
    let is_primary_recipient = primary.is_none_or(|a| a.identity_id == identity_id);
    let is_bcc = !header_recipients.contains(&delivered_to);
    let identity_live: Vec<&str> = own
        .iter()
        .filter(|a| matches!(a.status.as_str(), "active" | "retiring"))
        .map(|a| a.address.as_str())
        .collect();
    let reply_from =
        (!is_bcc && identity_live.contains(&delivered_to.as_str())).then(|| delivered_to.clone());
    let sender_in_tenant = from_address
        .as_ref()
        .is_some_and(|f| tenant_addrs.iter().any(|a| a.address == *f));
    let sender_is_identity = from_address
        .as_ref()
        .is_some_and(|f| own.iter().any(|a| a.address == *f));

    // 13. Admit against the per-tenant cap (TenantQuota unavailable → admit).
    let authenticated =
        pylota_identity_core::trust::authenticated(auth.verdict, auth.dmarc.aligned_by.is_some());
    let bucket = if authenticated {
        from_address.clone().unwrap_or_default()
    } else {
        "unauth:*".to_owned()
    };
    let cap = pol("inbound.per_tenant_per_hour").as_u64().unwrap_or(2000) as u32;
    let admit = if loopback {
        "admit"
    } else {
        let env = envelope_for(
            p,
            Some(&tenant_id),
            None,
            &ptr.request_id,
            None,
            QuotaRequest::AdmitInbound {
                identity_id: identity_id.clone(),
                bucket,
                at: ptr.received_at,
                cap,
            },
        );
        match p
            .objects()
            .call::<_, Value>(DoClass::Quota, &t.quota_do_id, &env)
            .await
        {
            Ok(RpcResult::Ok(v))
                if serde_json::from_value::<AdmitAnswer>(v.clone()).ok()
                    == Some(AdmitAnswer::Throttle) =>
            {
                "throttle"
            }
            _ => "admit",
        }
    };

    // 14. Ingest.
    let mut flags: Vec<String> = content.flags.iter().cloned().collect();
    if degraded && !flags.iter().any(|f| f == "parse_degraded") {
        flags.push("parse_degraded".into());
    }
    let synthetic = parsed.has(mime::Flag::MessageIdSynthetic);
    let auth_json = json!({
        "authserv": auth.trusted.as_ref().map(|_| json!({ "id": p.config().trusted_authserv_id, "trusted": true })),
        "spf": { "result": auth.spf.result, "domain": auth.spf.domain, "source": if loopback { Value::from("loopback") } else if auth.spf.checked { Value::from("authserv") } else { Value::Null }, "aligned": auth.dmarc.spf_aligned },
        "dkim": auth.dkim,
        "dmarc": auth.dmarc,
        "arc": { "result": "none", "instances": 0 },
        "multiple_from": auth.multiple_from,
        "signature": parsed.signature,
    });
    let spam_base = inbound::spam_base(
        &subject,
        parsed.text.is_some(),
        content.text.as_deref().unwrap_or(""),
        &links,
        flags.iter().any(|f| f == "hidden_text"),
    );
    let input = IngestInput {
        message_id: message_id.clone(),
        received_at: ptr.received_at,
        raw_r2_key: r2_key.clone(),
        raw_size: raw.len() as u64,
        raw_sha256,
        envelope_from: ptr.envelope_from.clone(),
        delivered_to: delivered_to.clone(),
        detail: address::parse_envelope(&ptr.envelope_to)
            .ok()
            .and_then(|e| e.detail),
        rfc_message_id: parsed.message_id.clone(),
        message_id_synthetic: synthetic,
        in_reply_to: parsed.in_reply_to.clone(),
        references: parsed
            .references
            .iter()
            .rev()
            .take(200)
            .rev()
            .cloned()
            .collect(),
        from: parsed.from.as_ref().map(|f| Addr {
            address: f.address.clone(),
            name: f.name.clone(),
        }),
        reply_to: parsed
            .reply_to
            .iter()
            .take(10)
            .map(|m| m.address.clone())
            .collect(),
        to: addrs(&parsed.to),
        cc: addrs(&parsed.cc),
        subject,
        text: content.text,
        html_sanitized: content.html_sanitized,
        extracted_text: (!degraded).then_some(content.extracted_text),
        sent_at: mime::sent_at(parsed.date, ptr.received_at),
        kind: serde_json::to_value(classification.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "normal".into()),
        automated_json: serde_json::to_value(&classification).unwrap_or(Value::Null),
        auth_json,
        verdict: serde_json::to_value(auth.verdict)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "none".into()),
        aligned: auth.dmarc.aligned_by.is_some(),
        spam_base,
        flags,
        refs,
        attachments,
        verification,
        sender_suppressed: suppressed.is_some(),
        receive_decision: None,
        tenant_domains: domains
            .into_iter()
            .map(|d| (d.name, d.kind == "platform"))
            .collect(),
        platform_domain: p.config().platform_domain.clone(),
        tenant_suffix: t.address_suffix.clone(),
        tenant_name: t.name.clone(),
        identity_name: idn.display_name.clone(),
        identity_addresses: own.iter().map(|a| a.address.clone()).collect(),
        sender_in_tenant,
        sender_is_identity,
        inbound_admit: admit.to_owned(),
        is_primary_recipient,
        is_bcc,
        reply_from,
        policy: InboundPolicy {
            on_auth_fail: pol("quarantine.on_auth_fail") != Value::Bool(false),
            unsolicited_otp: pol("quarantine.unsolicited_otp") != Value::Bool(false),
            spam_threshold: pol("quarantine.spam_threshold").as_f64().unwrap_or(0.8),
            per_sender_per_hour: pol("inbound.per_sender_per_hour").as_i64().unwrap_or(60),
            unauthenticated_per_hour: pol("inbound.unauthenticated_per_hour")
                .as_i64()
                .unwrap_or(120),
            triage_enabled: pol("triage.enabled") != Value::Bool(false),
            webhook_text_bytes: pol("webhook_text_bytes").as_u64().unwrap_or(16_384) as usize,
        },
        parser_version: PARSER_VERSION,
        loopback,
        dsn_original_message_id: classification
            .dsn
            .as_ref()
            .and_then(|d| d.original_message_id.as_deref())
            .and_then(thread::normalise_msg_id),
        token: None,
    };
    let outcome = match call(MailboxRequest::Ingest(Box::new(input))).await {
        Ok(RpcResult::Ok(v)) => match serde_json::from_value::<IngestOutcome>(v) {
            Ok(o) => o,
            Err(_) => return Done::Retry,
        },
        Ok(RpcResult::Err(e)) if e.error.code == ErrorCode::IdentityNotFound => {
            let _ = p.blobs().delete(std::slice::from_ref(&r2_key)).await;
            return Done::Ack;
        }
        _ => return Done::Retry,
    };

    // J2's fault hook: the consumer dies after the ingest committed, before its post-commit steps.
    #[cfg(all(feature = "itest-hooks", target_arch = "wasm32"))]
    if pylota_identity_platform::cf::itest::take_fault("inbound.after_ingest", "")
        .await
        .is_some()
    {
        panic!("the inbound.after_ingest fault hook");
    }

    // 15. Post-commit.
    let IngestOutcome::Stored {
        message_id: stored_id,
        status,
        duplicate,
        stored_raw_r2_key,
        attachments: to_store,
        is_system,
        triage,
        ..
    } = outcome
    else {
        // Backscatter (D4): nothing was stored.
        let _ = p.blobs().delete(std::slice::from_ref(&r2_key)).await;
        crate::metrics::count(p, "backscatter_total", Some(&tenant_id), 1);
        return Done::Ack;
    };
    if duplicate && stored_raw_r2_key != r2_key {
        let _ = p.blobs().delete(std::slice::from_ref(&r2_key)).await;
    }
    for a in &to_store {
        let Some(part) = parsed.attachments.get(a.part_index as usize) else {
            continue;
        };
        if duplicate && matches!(p.blobs().head(&a.r2_key).await, Ok(Some(_))) {
            continue;
        }
        let meta = BlobMeta {
            content_type: Some(part.content_type.clone()),
            custom: vec![
                ("tenant".into(), tenant_id.clone()),
                ("identity".into(), identity_id.clone()),
                ("message".into(), stored_id.clone()),
                (
                    "sha256".into(),
                    pylota_identity_core::ses::sha256_hex(&part.data),
                ),
            ],
        };
        if p.blobs()
            .put(&a.r2_key, BlobBody::Bytes(part.data.clone()), &meta)
            .await
            .is_err()
        {
            return Done::Retry;
        }
    }
    if !to_store.is_empty()
        && !matches!(
            call(MailboxRequest::ConfirmStored { message_id: stored_id.clone() }).await,
            Ok(RpcResult::Ok(ref v)) if v["stored"] == true
        )
    {
        let keys: Vec<String> = to_store.iter().map(|a| a.r2_key.clone()).collect();
        let _ = p.blobs().delete(&keys).await;
    }
    // Index jobs: none for the system mailbox (A15), nor for hidden or throttled mail.
    if !is_system && !matches!(status.as_str(), "hidden" | "throttled") {
        let mut jobs = Vec::new();
        if to_store.iter().any(|a| a.text_pending) {
            jobs.push(IndexJob::attachment_text(
                &tenant_id,
                &identity_id,
                &stored_id,
            ));
        }
        jobs.push(IndexJob::embed(
            &tenant_id,
            &identity_id,
            &stored_id,
            EmbedReason::New,
        ));
        if triage {
            jobs.push(IndexJob::triage(
                &tenant_id,
                &identity_id,
                &stored_id,
                TriageReason::Ingest,
            ));
        }
        if p.queues()
            .send_batch(QueueName::Index, &jobs, 0)
            .await
            .is_err()
        {
            return Done::Retry;
        }
    }
    if !duplicate && !matches!(status.as_str(), "hidden" | "throttled") {
        let env = envelope_for(
            p,
            Some(&tenant_id),
            None,
            &ptr.request_id,
            None,
            QuotaRequest::RecordUsage {
                metric: UsageMetric::Inbound,
                n: 1,
            },
        );
        let _ = p
            .objects()
            .call::<_, Value>(DoClass::Quota, &t.quota_do_id, &env)
            .await;
    }
    crate::metrics::count(p, "inbound_stored_total", Some(&tenant_id), 1);
    Done::Ack
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_data_is_unquoted_and_joined() {
        assert_eq!(txt_bytes(r#""v=DMARC1; p=reject""#), b"v=DMARC1; p=reject");
        assert_eq!(
            txt_bytes(r#""v=DKIM1; k=rsa; " "p=AB\"C""#),
            b"v=DKIM1; k=rsa; p=AB\"C"
        );
        assert_eq!(txt_bytes("plain"), b"plain");
    }
}
