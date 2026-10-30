//! Durable Object RPC (Design conventions § 5): the envelope and result types live in
//! `platform::durable`, so the platform's client and the objects share them; this module adds the
//! caller's defaults and the owner check every object runs.

use pylota_identity_api_types::ErrorCode;
pub use pylota_identity_platform::durable::{ApiErrorBody, RpcEnvelope, RpcResult};
use pylota_identity_platform::{Clock as _, Platform};
use serde_json::json;

use crate::errors::{ApiError, envelope};

/// The caller's default deadline (10 seconds).
pub const DEFAULT_DEADLINE_MS: i64 = 10_000;

/// An envelope for `req` from the resolved scope, never from the request.
pub fn envelope_for<P: Platform, T>(
    p: &P,
    tenant_id: Option<&str>,
    identity_id: Option<&str>,
    request_id: &str,
    actor_key_id: Option<&str>,
    req: T,
) -> RpcEnvelope<T> {
    RpcEnvelope {
        v: 1,
        tenant_id: tenant_id.map(str::to_owned),
        identity_id: identity_id.map(str::to_owned),
        request_id: request_id.to_owned(),
        actor_key_id: actor_key_id.map(str::to_owned),
        deadline_ms: p.clock().now_ms() + DEFAULT_DEADLINE_MS,
        req,
    }
}

/// An object's answer to a request it refuses, as the API error body.
pub fn error_body<T>(e: &ApiError, request_id: &str, docs_url: &str) -> RpcResult<T> {
    let v = envelope(e, request_id, docs_url);
    match serde_json::from_value(v) {
        Ok(body) => RpcResult::Err(Box::new(body)),
        Err(_) => RpcResult::Err(Box::new(
            serde_json::from_value(
                json!({ "error": { "code": "internal_error", "message": "", "retryable": true,
                "fix": "", "docs": docs_url, "request_id": request_id } }),
            )
            .expect("a fixed body"),
        )),
    }
}

/// The owner check of every object (Security § 5.2 step 6): a mismatch is `internal_error`, logged as
/// the security event `rpc_owner_mismatch`, counted in `rpc_owner_mismatch_total`, and contained: the
/// invocation turns the deployment read-only as it ends (`ops::containment`).
pub fn check_owner<P: Platform>(
    p: &P,
    stored: Option<&str>,
    sent: Option<&str>,
    class: &'static str,
) -> Result<(), ApiError> {
    if stored == sent {
        return Ok(());
    }
    crate::log::security(
        "rpc_owner_mismatch",
        &[
            ("class", class.into()),
            ("stored", stored.unwrap_or("").into()),
            ("sent", sent.unwrap_or("").into()),
        ],
    );
    crate::metrics::count(p, "rpc_owner_mismatch_total", None, 1);
    crate::ops::containment::note_owner_mismatch();
    Err(ApiError::new(
        ErrorCode::InternalError,
        "Something went wrong on our side.",
    ))
}
