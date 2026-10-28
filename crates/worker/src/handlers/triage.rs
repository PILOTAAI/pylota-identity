//! `POST /v1/identities/{identity_id}/messages/{message_id}/triage` ([Triage § 10], FR-TRI-5): the
//! mailbox checks eligibility and debounces; a due re-run enqueues `Triage { reason: Rerun }`.
//!
//! [Triage § 10]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/triage.md

use pylota_identity_api_types::internal::{IndexJob, TriageReason};
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::{Platform, QueueProducer as _};

use super::identity_in_scope;
use super::messages::{call_mailbox, review};
use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;
use crate::mailbox::MailboxRequest;
use crate::mailbox::triage::TriageOp;

pub async fn rerun<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let message_id = c.route.param("message_id").to_owned();
    let out = call_mailbox(
        c,
        &idn,
        MailboxRequest::Triage(TriageOp::Rerun {
            message_id: message_id.clone(),
            review: review(c),
        }),
    )
    .await?;
    if out["enqueue"] == true {
        let job = IndexJob::triage(&idn.tenant_id, &idn.id, &message_id, TriageReason::Rerun);
        c.p.queues()
            .send(QueueName::Index, &job, 0)
            .await
            .map_err(|_| ApiError::internal())?;
    }
    Ok(Reply {
        status: 202,
        ..Reply::no_content()
    })
}
