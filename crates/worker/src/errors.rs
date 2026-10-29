//! The error envelope ([Errors], Design conventions § 2): every failure that reaches a client is an
//! [`ApiError`], rendered with `retryable` from the catalogue, the `docs` link built from `PM_DOCS_URL`
//! here and never at a call site, and the request ID.
//!
//! [Errors]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/errors.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::{PlatformError, PlatformErrorKind};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub code: ErrorCode,
    /// Human text; never message content or addresses.
    pub message: String,
    /// Overrides the code's default fix when more specific.
    pub fix: Option<String>,
    pub details: Option<Value>,
    /// Extra response headers (`Allow`, `Retry-After`, …).
    pub headers: Vec<(String, String)>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            fix: None,
            details: None,
            headers: Vec::new(),
        }
    }

    pub fn details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    /// `400 invalid_request` naming one field.
    pub fn invalid(path: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self::new(ErrorCode::InvalidRequest, "The request is not valid.")
            .details(json!({ "errors": [ { "path": path, "message": message } ] }))
    }

    /// `400 invalid_request` with a `details.reason`.
    pub fn invalid_reason(reason: &str, message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message).details(json!({ "reason": reason }))
    }

    /// A resource's own `*_not_found`, identical for a missing ID and one out of scope (NFR-SEC-1).
    pub fn not_found(code: ErrorCode) -> Self {
        Self::new(code, "Not found.")
    }

    pub fn permission_denied(required: &[&str]) -> Self {
        Self::new(
            ErrorCode::PermissionDenied,
            "The key lacks a permission this route needs.",
        )
        .details(json!({ "required": required }))
    }

    pub fn scope_denied(field: Option<&str>) -> Self {
        let e = Self::new(ErrorCode::ScopeDenied, "This needs a higher key level.");
        match field {
            Some(f) => e.details(json!({ "field": f })),
            None => e,
        }
    }

    pub fn internal() -> Self {
        Self::new(
            ErrorCode::InternalError,
            "Something went wrong on our side.",
        )
    }
}

/// Platform failures (Design conventions § 2). `NotFound` here is an internal object; a resource the
/// caller named is mapped by its handler.
impl From<PlatformError> for ApiError {
    fn from(e: PlatformError) -> Self {
        let code = match e.kind {
            PlatformErrorKind::Unavailable | PlatformErrorKind::Conflict => ErrorCode::Unavailable,
            PlatformErrorKind::Timeout => ErrorCode::Timeout,
            PlatformErrorKind::Upstream => ErrorCode::UpstreamError,
            PlatformErrorKind::Invalid
            | PlatformErrorKind::NotFound
            | PlatformErrorKind::Corrupt
            | PlatformErrorKind::Internal => ErrorCode::InternalError,
        };
        crate::log::error(
            "platform_error",
            &[
                ("binding", e.binding.into()),
                ("kind", format!("{:?}", e.kind).into()),
                // Machine text from the platform (an SQLite message, a status); never content.
                (
                    "detail",
                    e.detail.chars().take(200).collect::<String>().into(),
                ),
            ],
        );
        ApiError::new(
            code,
            match code {
                ErrorCode::Unavailable => "A service this request needs is unavailable.",
                ErrorCode::Timeout => "An internal deadline passed.",
                ErrorCode::UpstreamError => "A provider answered unexpectedly.",
                _ => "Something went wrong on our side.",
            },
        )
    }
}

/// The code's default fix, by kind of failure.
fn default_fix(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthenticated => "Send a valid key as `Authorization: Bearer pmk_…`.",
        ErrorCode::KeyExpired | ErrorCode::KeyRevoked => "Use another key, or mint a new one.",
        ErrorCode::PermissionDenied => "Use a key that holds the permissions in details.required.",
        ErrorCode::ScopeDenied => "Use a key of a higher level for this route or field.",
        ErrorCode::KeyScopeExceeded => "A key can only create or manage keys no wider than itself.",
        ErrorCode::PartnerSuspended => "Ask the platform operator to reactivate the partner.",
        ErrorCode::RouteNotFound => "Check the method and path against the API reference.",
        ErrorCode::MethodNotAllowed => "Use one of the methods in the Allow header.",
        ErrorCode::IdempotencyConflict => {
            "Use a new Idempotency-Key for a different request, or resend the original body."
        }
        ErrorCode::RequestInProgress => "Retry the same request once the first one has finished.",
        ErrorCode::RateLimited => "Wait for Retry-After seconds, then retry.",
        ErrorCode::InvalidRequest => "Correct the fields in details and resend.",
        _ if code.retryable() => "Retry with backoff.",
        _ => "Change the request; see the code's entry on the errors page.",
    }
}

/// The envelope's JSON.
pub fn envelope(e: &ApiError, request_id: &str, docs_url: &str) -> Value {
    let mut error = json!({
        "code": e.code.as_str(),
        "message": e.message,
        "retryable": e.code.retryable(),
        "fix": e.fix.as_deref().unwrap_or(default_fix(e.code)),
        "docs": format!("{docs_url}/reference/errors#code-{}", e.code.as_str()),
        "request_id": request_id,
    });
    if let Some(d) = &e.details {
        error["details"] = d.clone();
    }
    json!({ "error": error })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_shape_matches_the_contract() {
        let e = ApiError::invalid("slug", "too short");
        let v = envelope(
            &e,
            "req_01ARYZ6S410000000000000000",
            "https://pylotamail.com/docs",
        );
        let parsed: pylota_identity_api_types::errors::Error =
            serde_json::from_value(v.clone()).unwrap();
        assert_eq!(parsed.error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            v["error"]["docs"],
            "https://pylotamail.com/docs/reference/errors#code-invalid_request"
        );
        assert_eq!(v["error"]["details"]["errors"][0]["path"], "slug");
        let p: ApiError = PlatformError::unavailable("DB", "x").into();
        assert_eq!(p.code, ErrorCode::Unavailable);
    }
}
