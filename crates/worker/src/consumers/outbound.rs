//! `pm-outbound` ([Outbound › The outbound consumer]): `Send` re-reads the scope, asks the mailbox to
//! begin, builds the message from the sent copy, claims, calls the transport and records the outcome;
//! `TransportEvent` carries the simulator's and loopback's delivery events to the delivery consumer.
//!
//! [Outbound › The outbound consumer]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::mime::{Caps, parse};
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::email::OutAttachment;
use pylota_identity_platform::queues::{Incoming, QueueName};
use pylota_identity_platform::{
    BlobStore as _, Clock as _, ControlDb as _, Ids as _, ObjectClient as _, Platform,
    QueueProducer as _,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::delivery::{self, DeliveryEvent, DeliveryKind, EventSource, Handled};
use crate::db::identities as idb;
use crate::db::tenants as tdb;
use crate::mailbox::MailboxRequest;
use crate::mailbox::deliveries::{BeginAnswer, BeginInput, DomainNow, GoAhead, Recorded};
use crate::quota::{self, Feature, QuotaRequest, UsageMetric};
use crate::rpc::{RpcResult, envelope_for};
use crate::transport::{self, OutgoingMessage, Provider, TransportResult, breaker, simulator};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutboundJob {
    Send {
        v: u8,
        tenant_id: String,
        identity_id: String,
        message_id: String,
        request_id: String,
    },
    TransportEvent {
        v: u8,
        tenant_id: String,
        identity_id: String,
        event: DeliveryEvent,
    },
}

const RETRY_S: u32 = if cfg!(feature = "itest-hooks") { 1 } else { 30 };
/// Rounds of the provider-suppression resend (G4).
const SUPPRESSION_ROUNDS: u32 = 3;

/// Queues `OutboundJob::Send`; `false` when the queue refused it (the dispatch alarm re-sends it).
pub async fn enqueue<P: Platform>(
    p: &P,
    tenant_id: &str,
    identity_id: &str,
    message_id: &str,
    request_id: &str,
    delay_s: u32,
) -> bool {
    let job = OutboundJob::Send {
        v: 1,
        tenant_id: tenant_id.to_owned(),
        identity_id: identity_id.to_owned(),
        message_id: message_id.to_owned(),
        request_id: request_id.to_owned(),
    };
    p.queues()
        .send(QueueName::Outbound, &job, delay_s)
        .await
        .is_ok()
}

pub async fn batch<P: Platform>(p: &P, batch: Vec<Incoming<Value>>) {
    #[cfg(feature = "itest-hooks")]
    crate::hooks::tap_queue(p, "pm-outbound", &batch).await;
    for m in batch {
        let handled = match serde_json::from_value::<OutboundJob>(m.body.clone()) {
            Ok(OutboundJob::Send {
                tenant_id,
                identity_id,
                message_id,
                request_id,
                ..
            }) => send(p, &tenant_id, &identity_id, &message_id, &request_id).await,
            Ok(OutboundJob::TransportEvent { event, .. }) => delivery::handle(p, &event).await,
            Err(_) => Handled::Ack,
        };
        match handled {
            Handled::Ack => m.ack(),
            Handled::Retry(d) => m.retry(Some(d)),
        }
    }
}

/// One mailbox call; `None` when the object did not answer (the message is retried).
async fn mailbox<P: Platform, A: serde::de::DeserializeOwned>(
    p: &P,
    idn: &idb::Row,
    request_id: &str,
    req: MailboxRequest,
) -> Option<Result<A, Value>> {
    let env = envelope_for(
        p,
        Some(&idn.tenant_id),
        Some(&idn.id),
        request_id,
        None,
        req,
    );
    match p
        .objects()
        .call::<_, Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
        .await
    {
        Ok(RpcResult::Ok(v)) => Some(serde_json::from_value(v).map_err(|_| Value::Null)),
        Ok(RpcResult::Err(e)) => Some(Err(serde_json::to_value(e).unwrap_or(Value::Null))),
        Err(_) => None,
    }
}

async fn settle<P: Platform>(
    p: &P,
    t: &tdb::Row,
    request_id: &str,
    msg: &str,
    consume: u32,
    keep: u32,
) {
    let _ = quota::call(
        p,
        &t.quota_do_id,
        &t.id,
        request_id,
        QuotaRequest::Settle {
            feature: Feature::Sends,
            r#ref: msg.to_owned(),
            consume,
            keep,
        },
    )
    .await;
}

async fn extend<P: Platform>(p: &P, t: &tdb::Row, request_id: &str, msg: &str, until: i64) {
    let _ = quota::call(
        p,
        &t.quota_do_id,
        &t.id,
        request_id,
        QuotaRequest::Extend {
            feature: Feature::Sends,
            r#ref: msg.to_owned(),
            until,
        },
    )
    .await;
}

#[derive(Deserialize)]
struct Facts {
    status: Option<String>,
    from_address: Option<String>,
    queued: Vec<String>,
}

#[derive(Deserialize)]
struct Dom {
    id: String,
    kind: String,
    state: String,
    transport: String,
    sending_paused_at: Option<i64>,
    event_subscription_id: Option<String>,
}

#[derive(Deserialize)]
struct Extra {
    sending_pause_reason: Option<String>,
    partner_status: Option<String>,
}

/// Consumer steps 1–7 for one message.
async fn send<P: Platform>(
    p: &P,
    tenant_id: &str,
    identity_id: &str,
    msg: &str,
    request_id: &str,
) -> Handled {
    let db = p.db();
    let now = p.clock().now_ms();
    let (Ok(Some(idn)), Ok(Some(t))) = (
        db.first::<idb::Row>(idb::get(identity_id)).await,
        db.first::<tdb::Row>(tdb::get(tenant_id)).await,
    ) else {
        return Handled::Retry(RETRY_S);
    };
    let Some(Ok(facts)) = mailbox::<_, Facts>(
        p,
        &idn,
        request_id,
        MailboxRequest::OutboundFacts {
            message_id: msg.to_owned(),
        },
    )
    .await
    else {
        return Handled::Retry(RETRY_S);
    };
    if facts.status.as_deref() != Some("queued") {
        return Handled::Ack;
    }
    let extra: Option<Extra> = db
        .first(Stmt::new(
            "SELECT t.sending_pause_reason, p.status AS partner_status FROM tenants t LEFT JOIN partners p ON p.id = t.partner_id WHERE t.id = ?1",
            vec![tenant_id.into()],
        ))
        .await
        .ok()
        .flatten();
    let partner_suspended =
        extra.as_ref().and_then(|e| e.partner_status.as_deref()) == Some("suspended");
    let _ = extra.as_ref().map(|e| &e.sending_pause_reason);
    // Step 1: a paused or deleted identity, a suspended tenant or partner (J13) cancels the message.
    if matches!(idn.status.as_str(), "paused" | "deleting" | "deleted")
        || t.status == "suspended"
        || partner_suspended
    {
        return cancel_by_system(p, &idn, &t, msg, request_id, now).await;
    }
    let from = facts.from_address.unwrap_or_default();
    let dom: Option<Dom> = match db
        .first(Stmt::new(
            "SELECT d.id, d.kind, d.state, d.transport, d.sending_paused_at, d.event_subscription_id FROM addresses a JOIN domains d ON d.id = a.domain_id WHERE a.address = ?1",
            vec![from.clone().into()],
        ))
        .await
    {
        Ok(d) => d,
        Err(_) => return Handled::Retry(RETRY_S),
    };
    let test = t.mode == "test";
    let Ok(rf) =
        crate::send::context::recipient_facts(p, &t.id, &idn.id, &facts.queued, false, now).await
    else {
        return Handled::Retry(RETRY_S);
    };
    let mut suppressed = Vec::new();
    for a in &facts.queued {
        if let Some(reason) = rf.suppression(a) {
            suppressed.push((a.clone(), format!("policy: suppression ({reason})")));
        } else if let Some(d) = rf.decision(a).filter(|d| d.block) {
            suppressed.push((
                a.clone(),
                if d.identity {
                    "policy: send_block (identity)"
                } else {
                    "policy: send_block"
                }
                .to_owned(),
            ));
        }
    }
    let scope = "cloudflare";
    let live_cf = !test
        && dom.as_ref().is_some_and(|d| {
            d.transport == "cloudflare" || d.state == "failing" || d.state == "suspended"
        });
    let gate = if live_cf {
        breaker::gate(p, scope, msg, now)
            .await
            .unwrap_or(breaker::Gate::Closed)
    } else {
        breaker::Gate::Closed
    };
    let platform_address: Option<String> = db
        .first::<Addr>(Stmt::new(
            "SELECT a.address FROM addresses a JOIN domains d ON d.id = a.domain_id WHERE a.identity_id = ?1 AND d.kind = 'platform' AND a.status <> 'retired' LIMIT 1",
            vec![idn.id.clone().into()],
        ))
        .await
        .ok()
        .flatten()
        .map(|a| a.address);
    let policy = crate::policy::effective(p, &t);
    let begin = BeginInput {
        message_id: msg.to_owned(),
        domain: dom.as_ref().map(|d| DomainNow {
            id: d.id.clone(),
            kind: d.kind.clone(),
            state: d.state.clone(),
            transport: d.transport.clone(),
            sending_paused: d.sending_paused_at.is_some(),
            delivery_events_manual: d.kind != "platform"
                && d.transport == "cloudflare"
                && d.event_subscription_id.is_none(),
        }),
        tenant_paused: t.sending_paused_at.is_some() && idn.is_system == 0,
        fallback_allowed: policy["domain_fallback"].as_bool().unwrap_or(true),
        platform_address,
        suppressed,
        breaker_open: gate == breaker::Gate::Open,
    };
    let Some(Ok(answer)) = mailbox::<_, BeginAnswer>(
        p,
        &idn,
        request_id,
        MailboxRequest::BeginTransport(Box::new(begin)),
    )
    .await
    else {
        return Handled::Retry(RETRY_S);
    };
    let go = match answer {
        BeginAnswer::NotQueued | BeginAnswer::AlreadyClaimed => return Handled::Ack,
        BeginAnswer::StaleUncertain | BeginAnswer::Ended { .. } => {
            settle(p, &t, request_id, msg, 0, 0).await;
            return Handled::Ack;
        }
        BeginAnswer::Hold { delay_s, until } => {
            extend(p, &t, request_id, msg, until).await;
            enqueue(p, &t.id, &idn.id, msg, request_id, delay_s).await;
            return Handled::Ack;
        }
        BeginAnswer::Go(g) => g,
    };
    // Step 3: build from the sent copy.
    let Ok(Some(blob)) = p.blobs().get(&go.eml_key).await else {
        return Handled::Retry(RETRY_S);
    };
    let mime = blob.body.to_vec();
    let mut out = build(&go, &mime, msg);
    // Step 4: the claim.
    let Some(Ok(token)) = mailbox::<_, Option<String>>(
        p,
        &idn,
        request_id,
        MailboxRequest::ClaimTransport {
            message_id: msg.to_owned(),
        },
    )
    .await
    else {
        return Handled::Retry(RETRY_S);
    };
    let Some(token) = token else {
        return Handled::Ack;
    };
    // Step 5: the transport.
    let (provider, result, after) = if go.transport == "test" {
        test_send(p, &t, &idn.id, &out, now, request_id).await
    } else if go.transport == "cloudflare" {
        let mut result = transport::cloudflare::send(p, &out).await;
        let mut rounds = 0;
        while let TransportResult::RecipientSuppressed { .. } = &result {
            rounds += 1;
            let sending_domain = pylota_identity_core::address::domain_of(&out.from.0).to_owned();
            let platform = p.config().platform_domain.clone();
            let mut items = Vec::new();
            let recipients: Vec<String> = out.recipients().cloned().collect();
            for a in &recipients {
                match crate::domains::cloudflare_api::suppressions(p, a, &sending_domain).await {
                    Some(found) if !found.is_empty() => {
                        let shared = found.iter().all(|f| f.scope == platform);
                        if shared {
                            items.push((
                                a.clone(),
                                "provider: suppressed on the shared domain".to_owned(),
                            ));
                        } else {
                            let reason = found[0].reason.clone();
                            let _ = delivery::suppress(
                                p,
                                &t.id,
                                &idn.id,
                                &idn.mailbox_do_id,
                                a,
                                "provider",
                                msg,
                                None,
                            )
                            .await;
                            items.push((a.clone(), format!("provider: suppressed ({reason})")));
                        }
                    }
                    Some(_) => {}
                    None if recipients.len() == 1 => {
                        items.push((a.clone(), "provider: suppressed (unknown)".to_owned()));
                    }
                    None => {}
                }
            }
            if items.is_empty() {
                result = TransportResult::Rejected {
                    reason: "recipient_suppressed_by_provider".into(),
                    code: "E_RECIPIENT_SUPPRESSED".into(),
                    detail: "E_RECIPIENT_SUPPRESSED".into(),
                };
                break;
            }
            let Some(Ok(remaining)) = mailbox::<_, Vec<String>>(
                p,
                &idn,
                request_id,
                MailboxRequest::MarkSuppressed {
                    message_id: msg.to_owned(),
                    items,
                },
            )
            .await
            else {
                return Handled::Retry(RETRY_S);
            };
            if remaining.is_empty() {
                let _ = mailbox::<_, Value>(
                    p,
                    &idn,
                    request_id,
                    MailboxRequest::AllSuppressed {
                        message_id: msg.to_owned(),
                    },
                )
                .await;
                settle(p, &t, request_id, msg, 0, 0).await;
                return Handled::Ack;
            }
            out.to.retain(|a| remaining.contains(a));
            out.cc.retain(|a| remaining.contains(a));
            out.bcc.retain(|a| remaining.contains(a));
            if rounds >= SUPPRESSION_ROUNDS {
                break;
            }
            result = transport::cloudflare::send(p, &out).await;
        }
        if live_cf
            && let Ok(true) =
                breaker::record(p, scope, result.definitive(), p.clock().now_ms()).await
        {
            crate::log::error("transport_breaker_open", &[("scope", scope.into())]);
            crate::metrics::count(p, "transport_breaker_open_total", None, 1);
        }
        (Provider::Cloudflare, result, Vec::new())
    } else {
        // SES and SMTP relays arrive with M13: definitely not sent, so the message waits.
        (
            Provider::Cloudflare,
            TransportResult::RetryLater {
                class: transport::RetryClass::Paused,
                code: "transport_not_built".into(),
                refused: Vec::new(),
            },
            Vec::new(),
        )
    };
    if let TransportResult::RetryLater { code, .. } = &result
        && code == "E_DAILY_LIMIT_EXCEEDED"
    {
        crate::log::error("provider_quota", &[]);
    }
    // Step 6.
    let record = MailboxRequest::RecordTransportOutcome {
        message_id: msg.to_owned(),
        claim_token: token,
        provider: provider.as_str().to_owned(),
        result: result.clone(),
    };
    let Some(Ok(rec)) = mailbox::<_, Recorded>(p, &idn, request_id, record).await else {
        return Handled::Retry(RETRY_S);
    };
    if rec.stale && rec.consume == 0 {
        crate::metrics::count(p, "stale_claim_outcome_total", Some(&t.id), 1);
    }
    if rec.settled || rec.consume > 0 || rec.keep > 0 {
        settle(p, &t, request_id, msg, rec.consume, rec.keep).await;
    }
    if rec.transport_accepted {
        let _ = quota::call(
            p,
            &t.quota_do_id,
            &t.id,
            request_id,
            QuotaRequest::RecordUsage {
                metric: UsageMetric::Outbound,
                n: 1,
            },
        )
        .await;
        if provider == Provider::Cloudflare {
            let _ = quota::call(
                p,
                &t.quota_do_id,
                &t.id,
                request_id,
                QuotaRequest::RecordUsage {
                    metric: UsageMetric::CfRecipients,
                    n: u64::from(rec.consume),
                },
            )
            .await;
        }
    }
    for (delay, ev) in after {
        let job = OutboundJob::TransportEvent {
            v: 1,
            tenant_id: t.id.clone(),
            identity_id: idn.id.clone(),
            event: ev,
        };
        let _ = p.queues().send(QueueName::Outbound, &job, delay).await;
    }
    // Step 7.
    if let (Some(delay), Some(until)) = (rec.delay_s, rec.until) {
        extend(p, &t, request_id, msg, until).await;
        enqueue(p, &t.id, &idn.id, msg, request_id, delay).await;
    }
    Handled::Ack
}

#[derive(Deserialize)]
struct Addr {
    address: String,
}

/// The `OutgoingMessage` of a go-ahead: the structured fields read back from the sent copy.
fn build(go: &GoAhead, mime: &[u8], msg: &str) -> OutgoingMessage {
    let parsed = parse(mime, &Caps::default());
    let reply_to = parsed.reply_to.first().map(|r| r.address.clone());
    OutgoingMessage {
        message_id: msg.to_owned(),
        rfc_message_id: format!("<{}>", go.rfc_message_id),
        from: go.from.clone(),
        reply_to: if go.reply_from_platform {
            go.reply_to.clone()
        } else {
            reply_to
        },
        to: go.to.clone(),
        cc: go.cc.clone(),
        bcc: go.bcc.clone(),
        subject: go.subject.clone(),
        text: parsed.text.clone(),
        html: parsed.html.clone(),
        headers: pylota_identity_core::compose::transport_headers(mime),
        attachments: parsed
            .attachments
            .iter()
            .map(|a| OutAttachment {
                filename: a.filename.clone().unwrap_or_else(|| "attachment".into()),
                content_type: a.content_type.clone(),
                bytes: a.data.clone(),
                inline_content_id: a.content_id.clone().filter(|_| a.inline),
            })
            .collect(),
        mime: mime.to_vec(),
    }
}

/// A test tenant's send: simulator recipients by script, the others by loopback (L2, L3).
async fn test_send<P: Platform>(
    p: &P,
    t: &tdb::Row,
    identity_id: &str,
    out: &OutgoingMessage,
    now: i64,
    request_id: &str,
) -> (Provider, TransportResult, Vec<(u32, DeliveryEvent)>) {
    let all: Vec<&String> = out.recipients().collect();
    let sim: Vec<&str> = all
        .iter()
        .filter(|a| a.ends_with(&format!("@{}", simulator::DOMAIN)))
        .map(|a| a.as_str())
        .collect();
    let others: Vec<String> = all
        .iter()
        .filter(|a| !sim.contains(&a.as_str()))
        .map(|a| (*a).clone())
        .collect();
    let provider = if sim.is_empty() {
        Provider::Loopback
    } else {
        Provider::Simulator
    };
    let result = if sim.is_empty() {
        TransportResult::Accepted {
            provider_message_id: simulator::provider_id(&out.message_id),
            rfc_message_id: None,
            refused: Vec::new(),
            deferred: Vec::new(),
        }
    } else {
        simulator::result(&out.message_id, &sim)
    };
    let mut events = Vec::new();
    if !matches!(result, TransportResult::Accepted { .. }) {
        return (provider, result, events);
    }
    for a in &sim {
        events.extend(simulator::events(
            &out.message_id,
            &out.from.0,
            a,
            &out.subject,
            now,
        ));
    }
    if !others.is_empty() {
        #[derive(Deserialize)]
        struct Dir {
            address: String,
            tenant_id: String,
            identity_id: String,
        }
        let dir: Vec<Dir> = p
            .db()
            .query(Stmt::new(
                "SELECT address, tenant_id, identity_id FROM addresses WHERE address IN (SELECT value FROM json_each(?1)) AND status IN ('active','retiring')",
                vec![serde_json::to_string(&others).unwrap_or_default().into()],
            ))
            .await
            .unwrap_or_default();
        let targets: Vec<transport::loopback::Target> = dir
            .into_iter()
            .map(|d| transport::loopback::Target {
                address: d.address,
                tenant_id: d.tenant_id,
                identity_id: d.identity_id,
            })
            .collect();
        let ok = transport::loopback::inject(
            p,
            (&t.id, identity_id, &out.message_id),
            &out.from.0,
            &targets,
            &out.mime,
            now,
            request_id,
        )
        .await;
        if !ok {
            return (
                provider,
                TransportResult::Unknown {
                    reason: "transport_connection_lost".into(),
                    detail: "loopback injection failed".into(),
                },
                Vec::new(),
            );
        }
        for tg in &targets {
            events.push((
                1,
                DeliveryEvent {
                    source: EventSource::Loopback,
                    event_id: format!("loop:{}:{}", out.message_id, tg.address),
                    provider_message_id: Some(simulator::provider_id(&out.message_id)),
                    sender: out.from.0.clone(),
                    recipient: tg.address.clone(),
                    subject: Some(out.subject.clone()),
                    kind: DeliveryKind::Delivered,
                    smtp_code: Some("250".into()),
                    enhanced_code: None,
                    smtp_response: None,
                    occurred_at: now,
                },
            ));
        }
    }
    (provider, result, events)
}

/// Step 1: `Cancel` with actor `system`, its audit row and the released reservations.
async fn cancel_by_system<P: Platform>(
    p: &P,
    idn: &idb::Row,
    t: &tdb::Row,
    msg: &str,
    request_id: &str,
    now: i64,
) -> Handled {
    #[derive(Deserialize)]
    struct C {
        day: Option<String>,
        tenant_counted: bool,
    }
    match mailbox::<_, C>(
        p,
        idn,
        request_id,
        MailboxRequest::Cancel {
            message_id: msg.to_owned(),
        },
    )
    .await
    {
        None => Handled::Retry(RETRY_S),
        Some(Ok(c)) => {
            if let Some(day) = c.day {
                let _ = quota::call(
                    p,
                    &t.quota_do_id,
                    &t.id,
                    request_id,
                    QuotaRequest::Release {
                        identity_id: idn.id.clone(),
                        day,
                        tenant_counted: c.tenant_counted,
                    },
                )
                .await;
            }
            settle(p, t, request_id, msg, 0, 0).await;
            let _ = p
                .db()
                .execute(crate::db::audit::insert(
                    &p.ids().new_id(IdPrefix::Audit),
                    Some(&t.id),
                    None,
                    "message.cancel",
                    "message",
                    msg,
                    &json!({ "actor": "system", "identity_id": idn.id }),
                    request_id,
                    now,
                ))
                .await;
            Handled::Ack
        }
        Some(Err(_)) => Handled::Ack,
    }
}
