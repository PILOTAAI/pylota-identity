//! `LoopbackTransport` ([Outbound › Loopback], L3, G11): a test tenant's mail to an identity of its own
//! tenant, or of a test tenant with the same partner, is injected as inbound mail, and a `delivered`
//! event follows on `pm-outbound`.
//!
//! [Outbound › Loopback]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_platform::queues::QueueName;
use pylota_identity_platform::r2::{BlobBody, BlobMeta};
use pylota_identity_platform::{BlobStore as _, Ids as _, Platform, QueueProducer as _};

use crate::consumers::inbound::{InboundJob, InboundPointer, LoopbackSource, RawSource};

/// One loopback recipient: `(tenant, identity)` of its address.
pub struct Target {
    pub address: String,
    pub tenant_id: String,
    pub identity_id: String,
}

/// Injects `mime` for each target; `false` when a write or a queue send failed (nothing is retried
/// here: the transport result is then unknown).
pub async fn inject<P: Platform>(
    p: &P,
    sender: (&str, &str, &str),
    from_address: &str,
    targets: &[Target],
    mime: &[u8],
    now: i64,
    request_id: &str,
) -> bool {
    let (tenant_id, identity_id, message_id) = sender;
    for t in targets {
        let msg = p.ids().new_id(IdPrefix::Message);
        let key = format!("t/{}/i/{}/m/{msg}/raw.eml", t.tenant_id, t.identity_id);
        if p.blobs()
            .put(&key, BlobBody::Bytes(mime.to_vec()), &BlobMeta::default())
            .await
            .is_err()
        {
            return false;
        }
        let ptr = InboundJob::Message(InboundPointer {
            v: 1,
            source: RawSource::Routing,
            message_id: Some(msg),
            tenant_id: Some(t.tenant_id.clone()),
            identity_id: Some(t.identity_id.clone()),
            r2_key: Some(key),
            raw_size: mime.len() as u64,
            envelope_from: from_address.to_owned(),
            envelope_to: t.address.clone(),
            received_at: now,
            request_id: request_id.to_owned(),
            loopback: Some(LoopbackSource {
                tenant_id: tenant_id.to_owned(),
                identity_id: identity_id.to_owned(),
                message_id: message_id.to_owned(),
            }),
        });
        if p.queues().send(QueueName::Inbound, &ptr, 0).await.is_err() {
            return false;
        }
    }
    true
}
