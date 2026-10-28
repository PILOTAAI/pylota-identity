//! The triage job natively ([Triage]): rules, the model, the record and the roll-up on the fakes.
//!
//! [Triage]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/triage.md

use pylota_identity_platform::queues::QueueName;
use serde_json::{Value, json};

use super::Harness;
use super::inbound::{drain, email, plain, setup};

const MODEL: &str = "@cf/openai/gpt-oss-20b";

#[test]
fn triage_job_stores_a_merged_record() {
    let h = Harness::new();
    let (_, _, address, mb) = setup(&h);
    email(
        &h,
        "ops@vendor.example",
        &address,
        &plain(
            &address,
            "t1@vendor.example",
            "Quote please",
            "Could you send a quote for brake pads?",
        ),
    );
    drain(&h, QueueName::Inbound);
    h.mailbox_sql(&mb, "UPDATE messages SET status = 'received'");
    h.app.p.ai.answer(
        MODEL,
        Ok(
            json!({ "response": { "category": "customer_request", "needs_reply": 0.9, "urgency": 1,
            "summary": "A vendor asks for a quote.", "language": "en", "risk_flags": [] } }),
        ),
    );
    drain(&h, QueueName::Index);
    let rows = h.mailbox_sql(&mb, "SELECT triage_status, triage_json FROM messages");
    assert_eq!(rows[0]["triage_status"], "done", "{rows:?}");
    let t: Value = serde_json::from_str(rows[0]["triage_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        (t["category"].as_str(), t["run"].as_u64()),
        (Some("customer_request"), Some(1))
    );
    let th = h.mailbox_sql(&mb, "SELECT category, needs_reply FROM threads");
    assert_eq!(th[0]["category"], "customer_request");
    let ev = h.mailbox_sql(
        &mb,
        "SELECT type FROM outbox WHERE type = 'message.triaged'",
    );
    assert_eq!(ev.len(), 1);
}
