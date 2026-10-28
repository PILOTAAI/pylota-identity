//! `GET /v1/identities/{identity_id}/wait` ([Inbound › The `wait` handler], E4): registers the expected
//! sender domain (E5), polls the mailbox every second until a match or the deadline, and releases a
//! verification code only for authenticated mail from that domain.
//!
//! [Inbound › The `wait` handler]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_core::address::{domain_of, org_domain};
use pylota_identity_platform::{Clock as _, Platform};
use serde_json::json;

use super::identity_in_scope;
use super::messages::{mailbox, review, time};
use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;
use crate::mailbox::read::{ReadRequest, WaitKind};

const POLL_MS: u32 = 1_000;
const REGISTER_EVERY_MS: i64 = 10_000;

fn valid_from(f: &str) -> bool {
    match f.split_once('@') {
        Some(("", d)) => !d.is_empty() && !d.contains(['@', ' ']),
        Some((l, d)) => !l.contains(' ') && !d.is_empty() && !d.contains(['@', ' ']),
        None => false,
    }
}

pub async fn wait<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    // 1. Validate.
    let timeout_s: i64 = match c.query("timeout") {
        None => 30,
        Some(t) => match t.parse::<i64>() {
            Ok(n) if (1..=60).contains(&n) => n,
            _ => return Err(ApiError::invalid("timeout", "1 to 60 seconds.")),
        },
    };
    let from = match c.query("from") {
        None => None,
        Some(f) if valid_from(f) => Some(f.trim().to_ascii_lowercase()),
        Some(_) => return Err(ApiError::invalid("from", "An address or @domain.")),
    };
    let subject_contains = match c.query("subject_contains") {
        Some(s) if s.chars().count() > 200 => {
            return Err(ApiError::invalid(
                "subject_contains",
                "At most 200 characters.",
            ));
        }
        s => s.map(str::to_owned),
    };
    let thread_id = match c.query("thread_id") {
        Some(t) if !t.starts_with("thr_") => {
            return Err(ApiError::invalid("thread_id", "A thr_ ID."));
        }
        t => t.map(str::to_owned),
    };
    let kind = match c.query("kind") {
        None | Some("any") => WaitKind::Any,
        Some("reply") => WaitKind::Reply,
        Some("verification") => WaitKind::Verification,
        Some(_) => return Err(ApiError::invalid("kind", "any, reply or verification.")),
    };
    if kind == WaitKind::Verification && from.is_none() {
        return Err(ApiError::invalid(
            "from",
            "kind=verification needs the expected sender.",
        ));
    }
    let start = c.p.clock().now_ms();
    let since_ms = time(c, "since")?.unwrap_or(start);
    // `@domain` or an address: its domain, then the registrable part.
    let from_domain = from.as_deref().map(|f| org_domain(domain_of(f)).to_owned());
    let deadline = start + timeout_s * 1000;

    // 2. Register, 3. poll (re-registering every 10 s).
    let mut registered_at = None;
    loop {
        let now = c.p.clock().now_ms();
        if let Some(d) = &from_domain
            && registered_at.is_none_or(|t| now - t >= REGISTER_EVERY_MS)
        {
            mailbox(
                c,
                &idn,
                ReadRequest::RegisterWait {
                    from_domain: d.clone(),
                    ttl_ms: (timeout_s + 10) * 1000,
                },
            )
            .await?;
            registered_at = Some(now);
        }
        let out = mailbox(
            c,
            &idn,
            ReadRequest::WaitPoll {
                since_ms,
                from: from.clone(),
                subject_contains: subject_contains.clone(),
                thread_id: thread_id.clone(),
                kind,
                release_domain: from_domain.clone(),
                review: review(c),
            },
        )
        .await?;
        // 5. Answer.
        if !out["message"].is_null() {
            return Ok(Reply::json(
                200,
                json!({ "message": out["message"], "verification": out["verification"], "timed_out": false }),
            ));
        }
        // A last poll at the deadline, so a full `timeout` is always waited.
        let left = deadline - c.p.clock().now_ms();
        if left <= 0 {
            break;
        }
        c.p.clock().sleep(left.min(i64::from(POLL_MS)) as u32).await;
    }
    Ok(Reply::json(
        200,
        json!({ "message": null, "verification": null, "timed_out": true }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_forms() {
        assert!(valid_from("noreply@service.example") && valid_from("@service.example"));
        assert!(!valid_from("service.example") && !valid_from("@") && !valid_from("a@b@c"));
    }
}
