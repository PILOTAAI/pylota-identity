//! The transport circuit breaker ([Outbound › Transport circuit breaker], J28): one D1 row per scope.
//! Three `Unknown` outcomes in a row open it for `5 min × 2^open_count` (at most an hour); after that
//! one message probes it half-open; a definitive outcome closes it.
//!
//! [Outbound › Transport circuit breaker]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, PResult, Platform};
use serde::Deserialize;

const OPEN_BASE_MS: i64 = 300_000;
const OPEN_MAX_MS: i64 = 3_600_000;
const PROBE_RETRY_MS: i64 = 600_000;
const OPEN_AFTER: i64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Closed,
    /// Hold: no claim.
    Open,
    /// This message is the half-open probe.
    Probe,
}

#[derive(Deserialize)]
struct Row {
    consecutive_unknown: i64,
    state: String,
    open_until: Option<i64>,
    open_count: i64,
    probe_message_id: Option<String>,
    updated_at: i64,
}

fn open_ms(open_count: i64) -> i64 {
    (OPEN_BASE_MS << open_count.clamp(0, 10)).min(OPEN_MAX_MS)
}

async fn row<P: Platform>(p: &P, scope: &str) -> PResult<Option<Row>> {
    p.db()
        .first(Stmt::new(
            "SELECT consecutive_unknown, state, open_until, open_count, probe_message_id, updated_at FROM transport_breakers WHERE scope = ?1",
            vec![scope.into()],
        ))
        .await
}

/// Whether `message_id` may be claimed now.
pub async fn gate<P: Platform>(p: &P, scope: &str, message_id: &str, now: i64) -> PResult<Gate> {
    let Some(r) = row(p, scope).await? else {
        return Ok(Gate::Closed);
    };
    match r.state.as_str() {
        "open" if r.open_until.is_some_and(|u| u > now) => Ok(Gate::Open),
        "open" => {
            let m = p
                .db()
                .execute(Stmt::new(
                    "UPDATE transport_breakers SET state = 'half_open', probe_message_id = ?2, updated_at = ?3 WHERE scope = ?1 AND state = 'open' AND open_until <= ?3",
                    vec![scope.into(), message_id.into(), now.into()],
                ))
                .await?;
            Ok(if m.changes > 0 {
                Gate::Probe
            } else {
                Gate::Open
            })
        }
        "half_open" if r.probe_message_id.as_deref() == Some(message_id) => Ok(Gate::Probe),
        "half_open" if r.updated_at + PROBE_RETRY_MS <= now => {
            let m = p
                .db()
                .execute(Stmt::new(
                    "UPDATE transport_breakers SET probe_message_id = ?2, updated_at = ?3 WHERE scope = ?1 AND state = 'half_open' AND updated_at <= ?4",
                    vec![scope.into(), message_id.into(), now.into(), (now - PROBE_RETRY_MS).into()],
                ))
                .await?;
            Ok(if m.changes > 0 {
                Gate::Probe
            } else {
                Gate::Open
            })
        }
        "half_open" => Ok(Gate::Open),
        _ => Ok(Gate::Closed),
    }
}

/// Records one outcome; `true` when this outcome opened the breaker (`transport_breaker_open`).
pub async fn record<P: Platform>(p: &P, scope: &str, definitive: bool, now: i64) -> PResult<bool> {
    let r = row(p, scope).await?;
    let db = p.db();
    if definitive {
        if r.as_ref()
            .is_some_and(|r| r.consecutive_unknown != 0 || r.state != "closed")
        {
            db.execute(Stmt::new(
                "UPDATE transport_breakers SET consecutive_unknown = 0, state = 'closed', open_until = NULL, open_count = 0, probe_message_id = NULL, updated_at = ?2 WHERE scope = ?1",
                vec![scope.into(), now.into()],
            ))
            .await?;
        }
        return Ok(false);
    }
    let open = |count: i64| {
        Stmt::new(
            "INSERT INTO transport_breakers (scope, consecutive_unknown, state, open_until, open_count, probe_message_id, updated_at) VALUES (?1, 0, 'open', ?2, ?3, NULL, ?4) \
             ON CONFLICT (scope) DO UPDATE SET consecutive_unknown = 0, state = 'open', open_until = excluded.open_until, open_count = excluded.open_count, probe_message_id = NULL, updated_at = excluded.updated_at",
            vec![
                scope.into(),
                (now + open_ms(count)).into(),
                (count + 1).into(),
                now.into(),
            ],
        )
    };
    match r {
        Some(r) if r.state == "half_open" => {
            db.execute(open(r.open_count)).await?;
            Ok(true)
        }
        Some(r) if r.state == "open" => Ok(false),
        Some(r) if r.consecutive_unknown + 1 >= OPEN_AFTER => {
            db.execute(open(r.open_count)).await?;
            Ok(true)
        }
        _ => {
            db.execute(Stmt::new(
                "INSERT INTO transport_breakers (scope, consecutive_unknown, state, open_count, updated_at) VALUES (?1, 1, 'closed', 0, ?2) \
                 ON CONFLICT (scope) DO UPDATE SET consecutive_unknown = consecutive_unknown + 1, updated_at = excluded.updated_at",
                vec![scope.into(), now.into()],
            ))
            .await?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_period_doubles_to_an_hour() {
        assert_eq!(open_ms(0), 300_000);
        assert_eq!(open_ms(1), 600_000);
        assert_eq!(open_ms(5), 3_600_000);
    }
}
