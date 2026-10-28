//! `SesControl` ([Domains on any DNS host § 4.8]): one object per deployment holding the token bucket
//! that every SES control-plane call takes from first, 1 token per second with a burst of 1, so the
//! account's one-request-per-second limit is never exceeded by this deployment's own calls.
//!
//! [Domains on any DNS host § 4.8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/domain-connections.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::{DoClass, RpcEnvelope, RpcResult, Sql};
use pylota_identity_platform::{Clock as _, ControlDb as _, ObjectClient as _, PResult, Platform};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::errors::ApiError;
use crate::rpc::{envelope_for, error_body};

/// The interval between two granted calls.
pub const TOKEN_MS: i64 = 1_000;
/// How long a request-path caller and a background caller wait at most.
pub const REQUEST_WAIT_MS: i64 = 5_000;
pub const BACKGROUND_WAIT_MS: i64 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SesCaller {
    Request,
    DomainCheck,
    PlatformCheck,
    RuleSync,
    Removal,
}

impl SesCaller {
    pub fn wait_ms(self) -> i64 {
        match self {
            SesCaller::Request => REQUEST_WAIT_MS,
            _ => BACKGROUND_WAIT_MS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SesControlRequest {
    Init,
    Acquire { caller: SesCaller, deadline_ms: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AcquireAnswer {
    Granted { at_ms: i64 },
    Busy { retry_after_ms: i64 },
}

#[derive(Deserialize)]
struct V {
    v: String,
}

/// The object's logic, over its SQL storage.
pub struct SesControl<'a, P: Platform, S: Sql> {
    pub p: &'a P,
    pub sql: &'a S,
}

impl<P: Platform, S: Sql> SesControl<'_, P, S> {
    pub fn handle(&self, envelope: &Value) -> Value {
        let request_id = envelope["request_id"].as_str().unwrap_or("").to_owned();
        let docs = self.p.config().docs_url.clone();
        let env: RpcEnvelope<SesControlRequest> = match serde_json::from_value(envelope.clone()) {
            Ok(e) => e,
            Err(_) => {
                let e = ApiError::new(ErrorCode::InvalidRequest, "Unknown SesControl request.");
                return serde_json::to_value(error_body::<()>(&e, &request_id, &docs))
                    .unwrap_or(Value::Null);
            }
        };
        match self.run(env.req) {
            Ok(v) => json!({ "ok": v }),
            Err(_) => {
                let e = ApiError::internal();
                serde_json::to_value(error_body::<()>(&e, &request_id, &docs))
                    .unwrap_or(Value::Null)
            }
        }
    }

    fn run(&self, req: SesControlRequest) -> PResult<Value> {
        self.sql.exec(
            "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
            &[],
        )?;
        let now = self.p.clock().now_ms();
        match req {
            SesControlRequest::Init => Ok(Value::Null),
            SesControlRequest::Acquire { deadline_ms, .. } => {
                let answer = self.sql.transaction_sync(|s| -> PResult<AcquireAnswer> {
                    let next = s
                        .first::<V>("SELECT v FROM meta WHERE k = 'next_free_ms'", &[])?
                        .and_then(|r| r.v.parse::<i64>().ok())
                        .unwrap_or(0);
                    let at = now.max(next);
                    if at > deadline_ms {
                        return Ok(AcquireAnswer::Busy {
                            retry_after_ms: at - now,
                        });
                    }
                    s.exec(
                        "INSERT INTO meta (k, v) VALUES ('next_free_ms', ?1) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
                        &[(at + TOKEN_MS).to_string().into()],
                    )?;
                    Ok(AcquireAnswer::Granted { at_ms: at })
                })?;
                Ok(serde_json::to_value(answer).unwrap_or(Value::Null))
            }
        }
    }
}

#[derive(Deserialize)]
struct ObjRow {
    do_id: String,
}

/// D1 or the object could not be reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreachable;

/// The deployment's `SesControl` ID: read from `platform_objects`, minted when missing (the insert is
/// idempotent, so a race keeps the first ID).
pub async fn object_id<P: Platform>(p: &P) -> Result<String, Unreachable> {
    let read = || {
        Stmt::new(
            "SELECT do_id FROM platform_objects WHERE name = 'ses_control'",
            vec![],
        )
    };
    if let Ok(Some(r)) = p.db().first::<ObjRow>(read()).await {
        return Ok(r.do_id);
    }
    let id = p
        .objects()
        .new_object_id(DoClass::SesControl)
        .map_err(|_| Unreachable)?;
    p.db()
        .execute(Stmt::new(
            "INSERT INTO platform_objects (name, do_id, created_at) VALUES ('ses_control', ?1, ?2) ON CONFLICT (name) DO NOTHING",
            vec![id.into(), p.clock().now_ms().into()],
        ))
        .await
        .map_err(|_| Unreachable)?;
    p.db()
        .first::<ObjRow>(read())
        .await
        .ok()
        .flatten()
        .map(|r| r.do_id)
        .ok_or(Unreachable)
}

/// Takes a token for `caller`, then waits until its second. `Err(retry_after_ms)` when the bucket is
/// busy past the caller's deadline (or does not answer: then a minute).
pub async fn acquire<P: Platform>(p: &P, caller: SesCaller) -> Result<(), i64> {
    let id = object_id(p).await.map_err(|_| 60_000)?;
    let now = p.clock().now_ms();
    let env = envelope_for(
        p,
        None,
        None,
        &crate::request_id::new(p),
        None,
        SesControlRequest::Acquire {
            caller,
            deadline_ms: now + caller.wait_ms(),
        },
    );
    let answer = match p
        .objects()
        .call::<_, AcquireAnswer>(DoClass::SesControl, &id, &env)
        .await
    {
        Ok(RpcResult::Ok(a)) => a,
        _ => return Err(60_000),
    };
    match answer {
        AcquireAnswer::Granted { at_ms } => {
            let wait = at_ms - p.clock().now_ms();
            if wait > 0 {
                p.clock().sleep(wait as u32).await;
            }
            Ok(())
        }
        AcquireAnswer::Busy { retry_after_ms } => Err(retry_after_ms),
    }
}

#[cfg(test)]
mod tests {
    use pylota_identity_platform::fakes::{FakePlatform, FakeSql};

    use super::*;

    fn env(req: Value) -> Value {
        json!({ "v": 1, "tenant_id": null, "identity_id": null, "request_id": "req_1", "actor_key_id": null,
                "deadline_ms": i64::MAX, "req": req })
    }

    /// Calls are granted one second apart; a caller whose deadline falls before its second is `Busy`
    /// and takes nothing.
    #[test]
    fn grants_one_per_second() {
        let p = FakePlatform::new();
        let sql = FakeSql::new();
        let c = SesControl { p: &p, sql: &sql };
        let now = p.clock().now_ms();
        let at: Vec<i64> = (0..20)
            .map(|_| {
                let v = c.handle(&env(
                    json!({ "op": "acquire", "caller": "domain_check", "deadline_ms": now + 60_000 }),
                ));
                v["ok"]["at_ms"].as_i64().unwrap()
            })
            .collect();
        for w in at.windows(2) {
            assert_eq!(w[1] - w[0], TOKEN_MS);
        }
        let v = c.handle(&env(
            json!({ "op": "acquire", "caller": "request", "deadline_ms": now + 5_000 }),
        ));
        assert_eq!(v["ok"]["kind"], "busy");
        assert_eq!(v["ok"]["retry_after_ms"], 20_000);
    }
}
