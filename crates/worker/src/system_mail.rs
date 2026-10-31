//! Mail from the system identity ([Identities and domains › The system identity]): alerts, sign-in
//! codes and invitations go through the normal outbound pipeline of the default tenant's system
//! identity, with an `Idempotency-Key` per purpose, so a retried send is sent once.
//!
//! [Identities and domains › The system identity]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use pylota_identity_core::ses::sha256_hex;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::{DoClass, RpcResult};
use pylota_identity_platform::{ControlDb as _, ObjectClient as _, Platform};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::{identities as idb, tenants as tdb};
use crate::mailbox::MailboxRequest;
use crate::mailbox::submit::SubmitInput;

/// Sends one text email to `to`; answers `sent`, or the refusal's error code (`no_system_identity`,
/// `unavailable`, or the submit path's code).
pub async fn send<P: Platform>(
    p: &P,
    to: &str,
    subject: &str,
    text: &str,
    idempotency_key: &str,
    request_id: &str,
) -> String {
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
    let body = json!({ "to": [to], "subject": subject, "text": text });
    let inp = SubmitInput {
        operation: "send".into(),
        target: None,
        key_hash: Some(sha256_hex(idempotency_key.as_bytes())),
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
