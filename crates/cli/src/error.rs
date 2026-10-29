//! Why a command failed, its exit code ([CLI and setup §3.4]) and its error document ([§3.3]).
//!
//! [CLI and setup §3.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#34-exit-codes
//! [§3.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#33-errors

use pylota_identity::ApiError;
use serde_json::{Value, json};

/// A failure raised by the CLI itself: a message, an optional fix and details.
#[derive(Clone, Debug, PartialEq)]
pub struct Local {
    pub message: String,
    pub fix: Option<String>,
    pub details: Option<Value>,
}

/// One entry of a Cloudflare envelope's `errors`.
#[derive(Clone, Debug, PartialEq)]
pub struct CfError {
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CliError {
    /// Exit 2.
    Usage(Local),
    /// Exit 3.
    Config(Local),
    /// The service's envelope; exit by status.
    Api(Box<ApiError>),
    /// A non-success status without the service's envelope: a proxy or another host.
    Upstream { status: u16, body: String },
    /// Connect, TLS or timeout; exit 9. `idempotency_key` is the key to retry with.
    Network {
        message: String,
        idempotency_key: Option<String>,
    },
    /// A Cloudflare API call or Wrangler run failed; exit 10.
    Cloudflare {
        step: String,
        status: Option<u16>,
        errors: Vec<CfError>,
        fix: Option<String>,
    },
    /// A missing or unmet prerequisite; exit 10.
    Prerequisite(Local),
    /// Exit 11.
    Verification(Local),
    /// `doctor` reported this many failures; exit 12.
    DoctorFailed(u32),
    /// Exit 13.
    Timeout(Local),
    /// Exit 15.
    Precondition(Local),
    /// Exit 130.
    Interrupted,
    /// Exit 1.
    Internal(Local),
}

fn local(m: impl Into<String>) -> Local {
    Local {
        message: m.into(),
        fix: None,
        details: None,
    }
}

impl CliError {
    pub fn usage(m: impl Into<String>) -> Self {
        CliError::Usage(local(m))
    }

    pub fn config(m: impl Into<String>) -> Self {
        CliError::Config(local(m))
    }

    pub fn internal(m: impl Into<String>) -> Self {
        CliError::Internal(local(m))
    }

    pub fn verification(m: impl Into<String>) -> Self {
        CliError::Verification(local(m))
    }

    pub fn timeout(m: impl Into<String>) -> Self {
        CliError::Timeout(local(m))
    }

    pub fn prerequisite(m: impl Into<String>) -> Self {
        CliError::Prerequisite(local(m))
    }

    /// The same error with a fix.
    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        let fix = Some(fix.into());
        match &mut self {
            CliError::Usage(l)
            | CliError::Config(l)
            | CliError::Prerequisite(l)
            | CliError::Verification(l)
            | CliError::Timeout(l)
            | CliError::Precondition(l)
            | CliError::Internal(l) => l.fix = fix,
            CliError::Cloudflare { fix: f, .. } => *f = fix,
            _ => {}
        }
        self
    }

    /// The same error with details.
    pub fn with_details(mut self, details: Value) -> Self {
        match &mut self {
            CliError::Usage(l)
            | CliError::Config(l)
            | CliError::Prerequisite(l)
            | CliError::Verification(l)
            | CliError::Timeout(l)
            | CliError::Precondition(l)
            | CliError::Internal(l) => l.details = Some(details),
            _ => {}
        }
        self
    }

    /// The API error envelope, when the service sent one.
    pub fn api(&self) -> Option<&ApiError> {
        match self {
            CliError::Api(e) => Some(e),
            _ => None,
        }
    }

    /// The exit code ([CLI and setup §3.4]).
    ///
    /// [CLI and setup §3.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#34-exit-codes
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Usage(_) => 2,
            CliError::Config(_) => 3,
            CliError::Api(e) => status_exit(e.status),
            CliError::Upstream { status: 404, .. } => 9,
            CliError::Upstream { status, .. } => status_exit(*status),
            CliError::Network { .. } => 9,
            CliError::Cloudflare { .. } | CliError::Prerequisite(_) => 10,
            CliError::Verification(_) => 11,
            CliError::DoctorFailed(_) => 12,
            CliError::Timeout(_) => 13,
            CliError::Precondition(_) => 15,
            CliError::Interrupted => 130,
            CliError::Internal(_) => 1,
        }
    }

    /// The error document: the service's envelope as sent, or one of the same shape with a CLI code.
    pub fn envelope(&self) -> Value {
        let cli = |code: &str, l: &Local, retryable: bool| {
            json!({ "error": {
                "code": code, "message": l.message, "fix": l.fix, "docs": Value::Null,
                "details": l.details, "retryable": retryable, "request_id": Value::Null,
            }})
        };
        match self {
            CliError::Api(e) => json!({ "error": {
                "code": e.code, "message": e.message, "fix": e.fix, "docs": e.docs,
                "details": e.details, "retryable": e.retryable, "request_id": e.request_id,
            }}),
            CliError::Upstream { status, body } => {
                let code = if *status == 404 {
                    "upstream_not_found"
                } else {
                    "upstream_error"
                };
                json!({ "error": {
                    "code": code,
                    "message": format!("HTTP {status} without the service's error envelope: a proxy or another host answered."),
                    "fix": "Check the API URL.",
                    "docs": Value::Null,
                    "details": { "status": status, "body": body },
                    "retryable": *status >= 500,
                    "request_id": Value::Null,
                }})
            }
            CliError::Network {
                message,
                idempotency_key,
            } => json!({ "error": {
                "code": "unavailable",
                "message": format!("The request did not complete: {message}"),
                "fix": idempotency_key.as_ref().map_or_else(
                    || "Retry the command.".to_owned(),
                    |k| format!("Retry with --idempotency-key {k}; a replay returns the original result."),
                ),
                "docs": Value::Null,
                "details": { "idempotency_key": idempotency_key },
                "retryable": true,
                "request_id": Value::Null,
            }}),
            CliError::Cloudflare {
                step,
                status,
                errors,
                fix,
            } => json!({ "error": {
                "code": "cli_cloudflare",
                "message": format!("Cloudflare: {step} failed{}.", status.map(|s| format!(" (HTTP {s})")).unwrap_or_default()),
                "fix": fix,
                "docs": Value::Null,
                "details": {
                    "step": step,
                    "status": status,
                    "errors": errors.iter().map(|e| json!({ "code": e.code, "message": e.message })).collect::<Vec<_>>(),
                },
                "retryable": false,
                "request_id": Value::Null,
            }}),
            CliError::Usage(l) => cli("cli_usage", l, false),
            CliError::Config(l) => cli("cli_config", l, false),
            CliError::Prerequisite(l) => cli("cli_prerequisite", l, false),
            CliError::Verification(l) => cli("cli_verification", l, false),
            CliError::Timeout(l) => cli("cli_timeout", l, true),
            CliError::Precondition(l) => cli("cli_precondition", l, false),
            CliError::Internal(l) => cli("cli_internal", l, false),
            CliError::DoctorFailed(n) => cli(
                "cli_doctor_failed",
                &local(format!("{n} check(s) failed.")),
                false,
            ),
            CliError::Interrupted => cli("cli_interrupted", &local("Interrupted."), false),
        }
    }
}

/// The exit code of an API status.
pub fn status_exit(status: u16) -> i32 {
    match status {
        401 | 403 => 4,
        404 => 5,
        409 | 410 | 423 => 6,
        400 | 413 | 422 => 7,
        402 | 429 => 8,
        500..=599 => 9,
        _ => 1,
    }
}

impl From<pylota_identity::Error> for CliError {
    fn from(e: pylota_identity::Error) -> Self {
        use pylota_identity::Error as E;
        match e {
            E::Api(a) => CliError::Api(a),
            E::Status { status, body } => CliError::Upstream { status, body },
            E::Transport {
                message,
                idempotency_key,
            } => CliError::Network {
                message,
                idempotency_key,
            },
            E::Decode(m) => CliError::internal(format!("unexpected response: {m}")),
            E::Config(m) => CliError::config(m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cli::output::exit_codes`: each HTTP status and CLI error maps to the code of §3.4; a `404`
    /// without the envelope is exit 9.
    #[test]
    fn exit_codes() {
        let api = |status: u16| {
            CliError::from(pylota_identity::Error::from_response(
                status,
                br#"{"error":{"code":"x","message":"y"}}"#,
            ))
        };
        for (status, code) in [
            (400, 7),
            (401, 4),
            (402, 8),
            (403, 4),
            (404, 5),
            (409, 6),
            (410, 6),
            (413, 7),
            (422, 7),
            (423, 6),
            (429, 8),
            (500, 9),
            (503, 9),
        ] {
            assert_eq!(api(status).exit_code(), code, "{status}");
        }
        let bare = CliError::from(pylota_identity::Error::from_response(404, b"<html>"));
        assert_eq!(bare.exit_code(), 9);
        assert_eq!(bare.envelope()["error"]["code"], "upstream_not_found");
        for (e, code) in [
            (CliError::usage("x"), 2),
            (CliError::config("x"), 3),
            (
                CliError::Network {
                    message: "reset".into(),
                    idempotency_key: None,
                },
                9,
            ),
            (CliError::prerequisite("x"), 10),
            (CliError::verification("x"), 11),
            (CliError::DoctorFailed(1), 12),
            (CliError::timeout("x"), 13),
            (CliError::Precondition(local("x")), 15),
            (CliError::Interrupted, 130),
            (CliError::internal("x"), 1),
        ] {
            assert_eq!(e.exit_code(), code, "{e:?}");
            assert!(e.envelope()["error"]["code"].is_string());
            assert_eq!(e.envelope()["error"]["request_id"], Value::Null);
        }
    }
}
