//! Triage ([Triage]): the `pm-index` `Triage` job (§ 1.1 steps 1–10). Rules first, then the model
//! unless the rules decided; invalid model output is `failed`, never guessed (FR-TRI-4). Triage is
//! advisory: besides its own fields it only adds the labels a tenant rule names (FR-TRI-3).
//!
//! [Triage]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/triage.md

pub mod model;
pub mod prompts;
pub mod rules;
pub mod schema;

use std::collections::BTreeSet;

use pylota_identity_api_types::internal::IndexJob;
use pylota_identity_core::triage_rules::{self, Categories, RiskFlag, RuleOutcome, TriageInput};
use pylota_identity_platform::durable::{DoClass, RpcResult};
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{
    BlobStore as _, ControlDb as _, ObjectClient as _, Platform, QueueProducer as _,
    RateLimiter as _, Rng as _,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::consumers::embed::retry_delay;
use crate::consumers::inbound::Done;
use crate::db::{identities as idb, tenants as tdb};
use crate::mailbox::MailboxRequest;
use crate::mailbox::triage::{CommitIn, LoadOut, Loaded, TriageOp};
use crate::quota::{self, CountAnswer, Feature, HoldAnswer, QuotaRequest, UsageMetric};
use crate::rpc::envelope_for;

/// Bumped whenever the system prompt, the output schema, the built-in rules or the merge changes
/// (§ 11).
pub const TRIAGE_VERSION: u32 = 1;
/// The last attempt (the tenth try): a model error there ends `failed`.
pub const LAST_ATTEMPT: u32 = 9;
/// After this many `RL_AI` deferrals the record is `skipped` (`ai_unavailable`).
pub const MAX_DEFERRALS: u32 = 120;
/// Attachment excerpts read per message, and their length.
const EXCERPTS: usize = 3;
const EXCERPT_CHARS: usize = 500;

/// `60 + jitter(0..30)` seconds; one second in the itest build.
fn defer_delay<P: Platform>(p: &P) -> u32 {
    if cfg!(feature = "itest-hooks") {
        return 1;
    }
    60 + (p.rng().u64() % 31) as u32
}

/// The job's fixed facts.
struct Job<'a, P: Platform> {
    p: &'a P,
    job: &'a IndexJob,
    tenant_id: &'a str,
    identity_id: &'a str,
    message_id: &'a str,
    attempt: u32,
    deferrals: u32,
    mailbox: String,
    quota: String,
    timezone: String,
    request_id: String,
}

impl<P: Platform> Job<'_, P> {
    async fn mailbox<T: DeserializeOwned>(&self, op: TriageOp) -> Result<T, Done> {
        let env = envelope_for(
            self.p,
            Some(self.tenant_id),
            Some(self.identity_id),
            &self.request_id,
            None,
            MailboxRequest::Triage(op),
        );
        match self
            .p
            .objects()
            .call::<_, Value>(DoClass::Mailbox, &self.mailbox, &env)
            .await
        {
            Ok(RpcResult::Ok(v)) => serde_json::from_value(v).map_err(|_| Done::Ack),
            // The identity was erased meanwhile.
            Ok(RpcResult::Err(_)) => Err(Done::Ack),
            Err(_) => Err(Done::Retry),
        }
    }

    async fn quota(&self, req: QuotaRequest) -> Result<Value, Done> {
        quota::call(self.p, &self.quota, self.tenant_id, &self.request_id, req)
            .await
            .map_err(|_| Done::Retry)
    }

    async fn settle(&self, consume: u32) {
        let _ = self
            .quota(QuotaRequest::Settle {
                feature: Feature::Triage,
                r#ref: self.message_id.to_owned(),
                consume,
                keep: 0,
            })
            .await;
    }

    async fn requeue(&self, next: IndexJob, delay: u32) -> Done {
        match self.p.queues().send(QueueName::Index, &next, delay).await {
            Ok(()) => Done::Ack,
            Err(_) => Done::Retry,
        }
    }

    /// Stores `record`; the hold is consumed only for a stored `done` record (§ 1.2).
    async fn commit(&self, record: Value, labels_add: Vec<String>, held: bool) -> Done {
        let done = record["status"] == "done";
        let out: Result<Value, Done> = self
            .mailbox(TriageOp::Commit(Box::new(CommitIn {
                message_id: self.message_id.to_owned(),
                record,
                labels_add,
            })))
            .await;
        let committed = match out {
            Ok(v) => v["committed"] == true,
            Err(d) => {
                if held {
                    self.settle(0).await;
                }
                return d;
            }
        };
        if held {
            self.settle(u32::from(committed && done)).await;
        }
        Done::Ack
    }

    /// `RL_AI` refused, or Workers AI is rate limited: the job again later, or `ai_unavailable`.
    async fn defer(&self, flags: &BTreeSet<RiskFlag>, matched: &[String]) -> Done {
        crate::metrics::count(self.p, "ai_deferred_total", Some(self.tenant_id), 1);
        self.settle(0).await;
        if self.deferrals >= MAX_DEFERRALS {
            return self
                .commit(
                    unfinished("skipped", "ai_unavailable", flags, matched),
                    Vec::new(),
                    false,
                )
                .await;
        }
        let mut next = self.job.clone();
        if let IndexJob::Triage { deferrals, .. } = &mut next {
            *deferrals += 1;
        }
        self.requeue(next, defer_delay(self.p)).await
    }

    async fn record_usage(&self, model: &str, out: &Value) {
        let (input, output) = schema::usage(out);
        match model::neurons(model, input, output) {
            Some(n) if n > 0 => {
                let _ = self
                    .quota(QuotaRequest::RecordUsage {
                        metric: UsageMetric::AiNeurons,
                        n,
                    })
                    .await;
            }
            Some(_) => {}
            None => crate::log::info(
                "ai_usage_unmetered",
                &[
                    ("model", model.into()),
                    ("input_tokens", (input as i64).into()),
                    ("output_tokens", (output as i64).into()),
                ],
            ),
        }
    }
}

fn flag_names(flags: &BTreeSet<RiskFlag>) -> Value {
    serde_json::to_value(flags).unwrap_or_else(|_| json!([]))
}

/// A `skipped` or `failed` record: model fields `null`, the rule flags kept (§ 7).
fn unfinished(status: &str, reason: &str, flags: &BTreeSet<RiskFlag>, matched: &[String]) -> Value {
    let mut r = json!({
        "status": status, "category": null, "needs_reply": null, "urgency": null, "summary": null,
        "language": null, "risk_flags": flag_names(flags), "model": null, "version": TRIAGE_VERSION,
        "reason": reason,
    });
    if !matched.is_empty() {
        r["rules"] = json!(matched);
    }
    r
}

/// § 5.1, or `None` when no rule set a category and a custom list has no `other`.
fn rules_only(i: &TriageInput, o: &RuleOutcome, c: &Categories) -> Option<Value> {
    let r = triage_rules::rules_only(i, o, c)?;
    let mut v = json!({
        "status": "done", "category": r.category, "needs_reply": r.needs_reply, "urgency": r.urgency,
        "summary": r.summary, "language": triage_rules::language(&i.extracted_text),
        "risk_flags": r.risk_flags, "model": "rules", "version": TRIAGE_VERSION,
    });
    if !o.matched.is_empty() {
        v["rules"] = json!(o.matched);
    }
    Some(v)
}

/// § 5.2.
fn merge(o: &RuleOutcome, m: &schema::ModelOut, model: &str) -> Value {
    let mut flags = o.risk_flags.clone();
    flags.extend(m.risk_flags.iter().copied());
    let mut v = json!({
        "status": "done",
        "category": o.category.as_ref().map_or(m.category.clone(), |(c, _)| c.clone()),
        "needs_reply": o.needs_reply.as_ref().map_or(m.needs_reply, |(n, _)| f64::from(*n)),
        "urgency": m.urgency.max(o.urgency_min),
        "summary": m.summary, "language": m.language,
        "risk_flags": flag_names(&flags), "model": model, "version": TRIAGE_VERSION,
    });
    if !o.matched.is_empty() {
        v["rules"] = json!(o.matched);
    }
    v
}

/// The first 500 characters of up to three ready attachment texts without a risk (§ 1.1 step 3).
async fn excerpts<P: Platform>(p: &P, l: &Loaded) -> Vec<Option<String>> {
    let mut out = vec![None; l.attachments.len()];
    let mut read = 0;
    for (i, a) in l.attachments.iter().enumerate() {
        if read == EXCERPTS {
            break;
        }
        let Some(key) = a.text_r2_key.as_deref().filter(|_| a.risk.is_none()) else {
            continue;
        };
        read += 1;
        if let Ok(Some(bytes)) = p
            .blobs()
            .get_range(key, 0, (EXCERPT_CHARS * 4) as u64)
            .await
        {
            let text = String::from_utf8_lossy(&bytes);
            out[i] = Some(
                text.trim_end_matches('\u{FFFD}')
                    .chars()
                    .take(EXCERPT_CHARS)
                    .collect(),
            );
        }
    }
    out
}

/// One `Triage` job.
pub async fn run<P: Platform>(p: &P, job: IndexJob) -> Done {
    let IndexJob::Triage {
        tenant_id,
        identity_id,
        message_id,
        reason,
        attempt,
        deferrals,
    } = &job
    else {
        return Done::Ack;
    };
    let (i, t) = match (
        p.db().first::<idb::Row>(idb::get(identity_id)).await,
        p.db().first::<tdb::Row>(tdb::get(tenant_id)).await,
    ) {
        (Ok(Some(i)), Ok(Some(t)))
            if i.tenant_id == t.id && !i.mailbox_do_id.is_empty() && i.is_system == 0 =>
        {
            (i, t)
        }
        (Ok(_), Ok(_)) => return Done::Ack,
        _ => return Done::Retry,
    };
    let j = Job {
        p,
        job: &job,
        tenant_id,
        identity_id,
        message_id,
        attempt: *attempt,
        deferrals: *deferrals,
        mailbox: i.mailbox_do_id.clone(),
        quota: t.quota_do_id.clone(),
        timezone: t.timezone.clone(),
        request_id: crate::request_id::new(p),
    };
    // 1. Load.
    let loaded = match j
        .mailbox::<LoadOut>(TriageOp::Load {
            message_id: message_id.clone(),
            reason: *reason,
            version: TRIAGE_VERSION,
        })
        .await
    {
        Ok(LoadOut::Triage(l)) => l,
        Ok(LoadOut::Skip) => return Done::Ack,
        Err(d) => return d,
    };
    // 4–5. Policy and rules (read before the hold: a denied hold keeps the built-in rule flags).
    let settings = rules::settings(&crate::policy::effective(p, &t));
    let ex = excerpts(p, &loaded).await;
    let input = rules::input(&loaded, identity_id, &i.username, &ex);
    // 2. Hold one unit.
    let hold = match j
        .quota(QuotaRequest::Hold {
            feature: Feature::Triage,
            units: 1,
            r#ref: message_id.clone(),
            gates: Vec::new(),
        })
        .await
        .map(serde_json::from_value::<HoldAnswer>)
    {
        Ok(Ok(h)) => h,
        Ok(Err(_)) | Err(_) => return Done::Retry,
    };
    if let HoldAnswer::Denied(_) = hold {
        let builtin = triage_rules::evaluate(&input, &[], &settings.categories);
        return j
            .commit(
                unfinished("skipped", "allowance", &builtin.risk_flags, &[]),
                Vec::new(),
                false,
            )
            .await;
    }
    let o = triage_rules::evaluate(&input, &settings.rules, &settings.categories);
    let labels = o.labels_add.clone();
    // 6a. Rules alone.
    if o.skip_model
        && let Some(r) = rules_only(&input, &o, &settings.categories)
    {
        return j.commit(r, labels, true).await;
    }
    // 6b. The model: the daily cap, then RL_AI.
    let cap = j
        .quota(QuotaRequest::CountTriageModel {
            tz: j.timezone.clone(),
            cap: settings.daily_model_cap,
            r#ref: message_id.clone(),
        })
        .await
        .map(serde_json::from_value::<CountAnswer>);
    match cap {
        Ok(Ok(CountAnswer::Ok)) => {}
        Ok(Ok(CountAnswer::CapReached)) => {
            crate::metrics::count(p, "triage_model_cap_total", Some(tenant_id), 1);
            let r = rules_only(&input, &o, &settings.categories).unwrap_or_else(|| {
                unfinished("failed", "input_unavailable", &o.risk_flags, &o.matched)
            });
            return j.commit(r, labels, true).await;
        }
        _ => {
            j.settle(0).await;
            return Done::Retry;
        }
    }
    let model_id = p.config().triage_model.clone();
    let nonce = crate::search::agentic::nonce(p);
    let mut messages = vec![
        json!({ "role": "system", "content": prompts::system(nonce.as_str(), &settings.categories) }),
        json!({ "role": "user", "content": model::user_message(&nonce, &loaded, &o, &ex, i.purpose.as_deref()) }),
    ];
    let out_schema = schema::schema(&settings.categories);
    // 7. Two tries at most: the second carries the validator's reason.
    for retry in [false, true] {
        if !p
            .limits()
            .allow(RlBucket::Ai, tenant_id)
            .await
            .unwrap_or(false)
        {
            return j.defer(&o.risk_flags, &o.matched).await;
        }
        let out = match model::call(p, &model_id, &model::request(&messages, &out_schema)).await {
            Ok(out) => out,
            Err(model::CallError::RateLimited) => {
                crate::metrics::count(p, "ai_rate_limited_total", Some(tenant_id), 1);
                return j.defer(&o.risk_flags, &o.matched).await;
            }
            Err(model::CallError::Failed) => {
                j.settle(0).await;
                if j.attempt >= LAST_ATTEMPT {
                    return j
                        .commit(
                            unfinished("failed", "model_unavailable", &o.risk_flags, &o.matched),
                            Vec::new(),
                            false,
                        )
                        .await;
                }
                let mut next = job.clone();
                if let IndexJob::Triage { attempt, .. } = &mut next {
                    *attempt += 1;
                }
                return j.requeue(next, retry_delay(j.attempt)).await;
            }
        };
        j.record_usage(&model_id, &out).await;
        let checked = schema::extract(&out)
            .ok_or_else(|| "the output is not one JSON object".to_owned())
            .and_then(|text| schema::validate(&text, &settings.categories, nonce.as_str()));
        match checked {
            Ok(m) => return j.commit(merge(&o, &m, &model_id), labels, true).await,
            Err(why) if !retry => {
                crate::log::info(
                    "triage_output_invalid",
                    &[
                        ("tenant_id", tenant_id.as_str().into()),
                        ("reason", why.clone().into()),
                    ],
                );
                let prior = schema::extract(&out).unwrap_or_default();
                messages.push(json!({ "role": "assistant", "content": prior }));
                messages.push(json!({ "role": "user", "content": prompts::invalid(&why) }));
            }
            Err(_) => break,
        }
    }
    crate::metrics::count(p, "triage_invalid_output_total", Some(tenant_id), 1);
    j.commit(
        unfinished("failed", "invalid_output", &o.risk_flags, &o.matched),
        Vec::new(),
        true,
    )
    .await
}

#[cfg(test)]
mod tests {
    use pylota_identity_core::triage_rules::RuleSource;

    use super::*;

    #[test]
    fn merge_keeps_rule_fields() {
        let mut o = RuleOutcome {
            category: Some(("legal_compliance".into(), RuleSource::Tenant("pcn".into()))),
            urgency_min: 2,
            ..RuleOutcome::default()
        };
        o.risk_flags.insert(RiskFlag::UnknownSender);
        let m = schema::ModelOut {
            category: "billing".into(),
            needs_reply: 0.4,
            urgency: 1,
            summary: "A council asks for payment.".into(),
            language: "en".into(),
            risk_flags: vec![RiskFlag::UrgentPressure],
        };
        let v = merge(&o, &m, "@cf/openai/gpt-oss-20b");
        assert_eq!(v["category"], "legal_compliance");
        assert_eq!(v["urgency"], 2);
        assert_eq!(v["needs_reply"], 0.4);
        assert_eq!(
            v["risk_flags"],
            json!(["urgent_pressure", "unknown_sender"])
        );
        let f = unfinished("failed", "invalid_output", &o.risk_flags, &[]);
        assert_eq!(
            (f["category"].clone(), f["risk_flags"].clone()),
            (Value::Null, json!(["unknown_sender"]))
        );
    }
}
