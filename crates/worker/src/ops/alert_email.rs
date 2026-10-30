//! Alert email (Observability § 5.5): page alerts, and once a day at 08:00 UTC a summary of the firing
//! ticket alerts, sent to `PM_ALERT_EMAIL` as ordinary sends from the system identity on the default
//! tenant, through the normal outbound pipeline, with `Idempotency-Key: alert-{key}-{fired_at}` (and
//! `-{n}` for a re-notification). At most 20 a UTC hour; above that one email says how many more are
//! firing. Bodies hold alert keys, severities, values (IDs and numbers), times and the runbook URL,
//! never content or addresses. A refused send counts `alert_emails_total{result=refused}` and reports
//! `system_mail_blocked`.

use pylota_identity_core::ses::sha256_hex;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::{DoClass, RpcResult};
use pylota_identity_platform::{ControlDb as _, ObjectClient as _, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::{identities as idb, tenants as tdb};
use crate::mailbox::MailboxRequest;
use crate::mailbox::submit::SubmitInput;

/// Alert emails per UTC hour.
pub const PER_HOUR: u32 = 20;

/// The hour's count, kept in the evaluator's state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// `YYYY-MM-DDTHH`.
    pub hour: String,
    pub sent: u32,
    /// The one "more are firing" email of the hour went out.
    pub capped: bool,
    /// The last UTC day the ticket summary went out.
    #[serde(default)]
    pub summary_day: String,
}

/// One email to send.
pub struct Email {
    pub idempotency_key: String,
    pub subject: String,
    pub text: String,
    pub severity: &'static str,
}

/// The email of one page alert's notification.
pub fn page_email(
    env: &str,
    docs_url: &str,
    key: &str,
    values: &Value,
    fired_at: i64,
    notice: u32,
) -> Email {
    let name = super::alerts::name_of(key);
    Email {
        idempotency_key: pylota_identity_core::slo::alert_email_key(key, fired_at, notice),
        subject: format!("[pylota-identity {env}] page: {key}"),
        text: format!(
            "Alert: {key}\nSeverity: page\nFired: {}\nValues: {values}\nRunbook: {}\nMore: idvy ops status\n",
            pylota_identity_core::days::rfc3339(fired_at),
            super::alerts::runbook_url(docs_url, name),
        ),
        severity: "page",
    }
}

/// The hour's last email once the cap is reached.
pub fn capped_email(env: &str, hour: &str, more: usize) -> Email {
    Email {
        idempotency_key: format!("alert-capped-{hour}"),
        subject: format!("[pylota-identity {env}] page: {more} more alerts"),
        text: format!(
            "{more} more page alerts fired or re-notified this hour ({hour}:00 UTC) after {PER_HOUR} alert emails.\nMore: idvy ops status\n"
        ),
        severity: "page",
    }
}

/// The daily summary of the firing ticket alerts.
pub fn summary_email(env: &str, day: &str, keys: &[(String, i64)]) -> Email {
    let mut text = format!("Ticket alerts firing on {day}:\n");
    for (k, at) in keys {
        text.push_str(&format!(
            "- {k} (since {})\n",
            pylota_identity_core::days::rfc3339(*at)
        ));
    }
    text.push_str("More: idvy ops status\n");
    Email {
        idempotency_key: format!("alert-summary-{day}"),
        subject: format!(
            "[pylota-identity {env}] ticket: {} alerts firing",
            keys.len()
        ),
        text,
        severity: "ticket",
    }
}

/// Sends `e` from the system identity; answers `sent`, `missing` (no address configured) or the
/// refusal's error code.
pub async fn send<P: Platform>(p: &P, e: &Email, request_id: &str) -> String {
    let result = submit(p, e, request_id).await;
    let label = if matches!(result.as_str(), "sent" | "missing") {
        result.as_str()
    } else {
        "refused"
    };
    crate::metrics::count_with(p, "alert_emails_total", None, &[e.severity, label], 1);
    if label == "refused" {
        crate::log::warn("alert_email_refused", &[("code", result.clone().into())]);
        if matches!(
            result.as_str(),
            "daily_cap_reached" | "identity_paused" | "domain_not_ready"
        ) {
            super::alerts::report(
                p,
                "system_mail_blocked",
                None,
                None,
                &json!({ "code": result }),
                request_id,
            )
            .await;
        }
    }
    result
}

async fn submit<P: Platform>(p: &P, e: &Email, request_id: &str) -> String {
    let Some(to) = p.config().alert_email.clone() else {
        return "missing".into();
    };
    #[derive(Deserialize)]
    struct Sys {
        id: String,
    }
    let Ok(Some(sys)) = p
        .db()
        .first::<Sys>(Stmt::new(
            "SELECT id FROM identities WHERE is_system = 1 LIMIT 1",
            vec![],
        ))
        .await
    else {
        return "no_system_identity".into();
    };
    let Ok(Some(idn)) = p.db().first::<idb::Row>(idb::get(&sys.id)).await else {
        return "unavailable".into();
    };
    if idn.mailbox_do_id.is_empty() {
        return "unavailable".into();
    }
    let Ok(Some(t)) = p.db().first::<tdb::Row>(tdb::get(&idn.tenant_id)).await else {
        return "unavailable".into();
    };
    let Ok(ctx) = crate::send::context::load(p, &idn, &t).await else {
        return "unavailable".into();
    };
    let body = json!({ "to": [to], "subject": e.subject, "text": e.text });
    let inp = SubmitInput {
        operation: "send".into(),
        target: None,
        key_hash: Some(sha256_hex(e.idempotency_key.as_bytes())),
        fingerprint: sha256_hex(body.to_string().as_bytes()),
        dry_run: false,
        review: false,
        body,
        ctx,
        request_id: request_id.to_owned(),
        role_relay: false,
    };
    let env = crate::rpc::envelope_for(
        p,
        Some(&t.id),
        Some(&idn.id),
        request_id,
        None,
        MailboxRequest::Submit(Box::new(inp)),
    );
    match p
        .objects()
        .call::<_, Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
        .await
    {
        Ok(RpcResult::Ok(_)) => "sent".into(),
        Ok(RpcResult::Err(e)) => e.error.code.as_str().to_owned(),
        Err(_) => "unavailable".into(),
    }
}
