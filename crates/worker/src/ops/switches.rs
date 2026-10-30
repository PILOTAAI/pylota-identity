//! The deployment-wide switches (Observability § 5.6): one `ops_switches` row per switch, a missing
//! row meaning the default. Readers cache a switch for 60 seconds per isolate, so a change takes effect
//! everywhere within a minute; writers (`PUT /v1/platform/switches` and the containment rules) write
//! the row and the audit row `ops.switch` in one batch.

use std::cell::RefCell;

use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{Clock as _, ControlDb as _, Ids as _, Platform, PlatformError};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::audit;

/// The switches the API sets, with their defaults.
pub const SWITCHES: &[(&str, &str)] = &[
    ("read_only", "off"),
    ("free_sending", "on"),
    ("emergency_prune", "off"),
    ("agent_signup", "on"),
];

const CACHE_MS: i64 = 60_000;

thread_local! {
    static CACHE: RefCell<Vec<(&'static str, String, i64)>> = const { RefCell::new(Vec::new()) };
}

pub fn default_of(name: &str) -> Option<&'static str> {
    SWITCHES.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

/// The switch's state, cached for 60 seconds; the default when D1 does not answer.
pub async fn state<P: Platform>(p: &P, name: &'static str) -> String {
    let now = p.clock().now_ms();
    if let Some(s) = CACHE.with(|c| {
        c.borrow()
            .iter()
            .find(|(n, _, at)| *n == name && now - at < CACHE_MS)
            .map(|(_, s, _)| s.clone())
    }) {
        return s;
    }
    #[derive(Deserialize)]
    struct S {
        state: String,
    }
    let fallback = default_of(name).unwrap_or("off").to_owned();
    let s = match p
        .db()
        .first::<S>(Stmt::new(
            "SELECT state FROM ops_switches WHERE name = ?1",
            vec![name.into()],
        ))
        .await
    {
        Ok(row) => row.map_or(fallback, |r| r.state),
        Err(_) => return fallback,
    };
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|(n, _, _)| *n != name);
        c.push((name, s.clone(), now));
    });
    s
}

/// Sets a switch and writes `ops.switch`; answers the stored row as the API's `Switch`.
pub async fn set<P: Platform>(
    p: &P,
    name: &'static str,
    state: &str,
    set_by: &str,
    reason: &str,
    actor_key_id: Option<&str>,
    request_id: &str,
) -> Result<Value, PlatformError> {
    let now = p.clock().now_ms();
    p.db()
        .batch(vec![
            Stmt::new(
                "INSERT INTO ops_switches (name, state, set_by, reason, actor_key_id, set_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT (name) DO UPDATE SET state = excluded.state, set_by = excluded.set_by, reason = excluded.reason, \
                 actor_key_id = excluded.actor_key_id, set_at = excluded.set_at",
                vec![
                    name.into(),
                    state.into(),
                    set_by.into(),
                    reason.into(),
                    actor_key_id.map(str::to_owned).into(),
                    now.into(),
                ],
            ),
            audit::insert(
                &p.ids().new_id(IdPrefix::Audit),
                None,
                actor_key_id,
                "ops.switch",
                "switch",
                name,
                &json!({ "state": state, "set_by": set_by }),
                request_id,
                now,
            ),
        ])
        .await?;
    CACHE.with(|c| c.borrow_mut().retain(|(n, _, _)| *n != name));
    Ok(json!({
        "name": name, "state": state, "set_by": set_by, "reason": reason,
        "set_at": pylota_identity_core::days::rfc3339(now),
    }))
}

/// Whether `name` is away from its default (the evaluator's `containment:{rule}` level).
pub async fn away_from_default<P: Platform>(p: &P, name: &'static str) -> bool {
    let s = state(p, name).await;
    default_of(name).is_some_and(|d| d != s)
}
