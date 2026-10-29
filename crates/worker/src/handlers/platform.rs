//! The platform API: `POST /v1/platform/keys/{purpose}/rotate` (Security § 6.2, `platform:ops`,
//! audit-logged), the maintenance jobs, and `GET /v1/platform/status`.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde_json::json;

use crate::ctx::{Ctx, Reply};
use crate::db::audit;
use crate::errors::ApiError;
use crate::keyring::{self, Purpose};

pub async fn rotate_signing_key<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let purpose = match c.route.param("purpose") {
        "web_bot_auth" => {
            return Err(ApiError::new(
                ErrorCode::WebBotAuthDisabled,
                "Signed HTTP requests arrive with M25.",
            )
            .details(json!({ "purpose": "web_bot_auth" })));
        }
        p => Purpose::parse(p).ok_or_else(|| {
            ApiError::invalid(
                "purpose",
                "purpose is thread, link, cursor or web_bot_auth.",
            )
        })?,
    };
    let revoke = match c.query("revoke_previous") {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => return Err(ApiError::invalid("revoke_previous", "true or false.")),
    };
    let r = keyring::rotate(c.p.as_ref(), purpose, revoke).await?;
    c.p.db()
        .execute(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            None,
            Some(&c.key().key_id),
            "signing_key.rotate",
            "signing_key",
            &format!("{}:{}", purpose.as_str(), r.kid),
            &json!({ "revoke_previous": revoke }),
            &c.request_id,
            c.now,
        ))
        .await?;
    Ok(Reply::json(
        200,
        json!({
            "purpose": purpose.as_str(), "kid": r.kid, "created_at": rfc3339(r.created_at),
            "previous": { "kid": r.previous_kid, "verify_until": rfc3339(r.previous_verify_until), "revoked": r.revoked },
        }),
    ))
}

/// `POST /v1/platform/jobs` (J3): a `reparse`, `reembed` or `reindex` job over a tenant's mailboxes,
/// narrowed by `identity_ids`, `after` and `before`.
pub async fn job_create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let body = c.json()?;
    super::messages::only_fields(
        &body,
        &["kind", "tenant_id", "identity_ids", "after", "before"],
    )?;
    let kind = match body["kind"].as_str() {
        Some(k @ ("reparse" | "reembed" | "reindex")) => k.to_owned(),
        _ => return Err(ApiError::invalid("kind", "reparse, reembed or reindex.")),
    };
    let tenant = body["tenant_id"]
        .as_str()
        .ok_or_else(|| ApiError::invalid("tenant_id", "The tenant."))?;
    super::tenant_in_scope(c, tenant, true).await?;
    let time = |k: &str| -> Result<Option<i64>, ApiError> {
        match &body[k] {
            serde_json::Value::Null => Ok(None),
            serde_json::Value::String(s) => pylota_identity_core::days::parse_rfc3339(s)
                .map(Some)
                .ok_or_else(|| ApiError::invalid(k, "An RFC 3339 time.")),
            _ => Err(ApiError::invalid(k, "An RFC 3339 time.")),
        }
    };
    let params = json!({
        "identity_ids": body["identity_ids"], "after": time("after")?, "before": time("before")?,
    });
    let id = crate::jobs::create(c.p.as_ref(), &kind, Some(tenant), &params).await?;
    let j = job_row(c, &id).await?;
    Ok(Reply::json(202, j))
}

async fn job_row<P: Platform>(c: &Ctx<P>, id: &str) -> Result<serde_json::Value, ApiError> {
    #[derive(serde::Deserialize)]
    struct J {
        id: String,
        kind: String,
        tenant_id: Option<String>,
        status: String,
        result_json: Option<String>,
        created_at: i64,
        completed_at: Option<i64>,
    }
    let j: J = c
        .p
        .db()
        .first(pylota_identity_platform::d1::Stmt::new(
            "SELECT id, kind, tenant_id, status, result_json, created_at, completed_at FROM jobs WHERE id = ?1 AND kind NOT IN ('erasure','export')",
            vec![id.into()],
        ))
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::JobNotFound))?;
    Ok(json!({
        "id": j.id, "kind": j.kind, "tenant_id": j.tenant_id, "status": j.status,
        "created_at": rfc3339(j.created_at), "completed_at": j.completed_at.map(rfc3339),
        "result": j.result_json.as_deref().and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok()),
    }))
}

/// `GET /v1/platform/jobs/{job_id}`: any job but erasure and export jobs.
pub async fn job_get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    Ok(Reply::json(200, job_row(c, c.route.param("job_id")).await?))
}

/// `GET /v1/platform/status` (`audit:read`, Observability § 5.5–5.6): the freeze flag, the switches,
/// the alert address, the firing state alerts, the master-key slots with the re-seal progress
/// (Security § 6.2), capacity and agent sign-up. Capacity's D1 and Vectorize figures are measured by
/// the evaluator's capacity check and are `null` until it has run.
pub async fn status<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    #[derive(serde::Deserialize)]
    struct Sw {
        name: String,
        state: String,
    }
    #[derive(serde::Deserialize)]
    struct N {
        n: i64,
    }
    let cfg = c.p.config();
    let rows: Vec<Sw> =
        c.p.db()
            .query(pylota_identity_platform::d1::Stmt::new(
                "SELECT name, state FROM ops_switches",
                vec![],
            ))
            .await?;
    let switch = |name: &str, default: &str| {
        rows.iter()
            .find(|r| r.name == name)
            .map_or(default.to_owned(), |r| r.state.clone())
    };
    let firing: Vec<serde_json::Value> = crate::ops::alerts::load_state(c.p.as_ref())
        .await
        .map(|(s, _)| s.keys)
        .unwrap_or_default()
        .into_iter()
        .map(|(k, s)| json!({ "alert": k, "severity": s.severity.as_str(), "fired_at": rfc3339(s.fired_at) }))
        .collect();
    let tenants =
        c.p.db()
            .first::<N>(pylota_identity_platform::d1::Stmt::new(
                "SELECT COUNT(*) AS n FROM tenants WHERE status <> 'erased'",
                vec![],
            ))
            .await?
            .map_or(0, |n| n.n);
    let console = cfg.console.as_ref();
    let configured = console.is_some_and(|k| k.agent_signup);
    let agent_switch = switch("agent_signup", "on");
    let reason = if !configured {
        Some("signup_not_open")
    } else if agent_switch == "off" {
        Some("switch_off")
    } else {
        None
    };
    Ok(Reply::json(
        200,
        json!({
            "frozen": cfg.frozen,
            "frozen_since": cfg.frozen_since,
            "switches": {
                "read_only": switch("read_only", "off"),
                "free_sending": switch("free_sending", "on"),
                "emergency_prune": switch("emergency_prune", "off"),
                "agent_signup": agent_switch,
            },
            "alert_email": if cfg.alert_email.is_some() { "configured" } else { "missing" },
            "firing": firing,
            "master_key": crate::ops::reseal::status(c.p.as_ref()).await?,
            "capacity": {
                "d1_bytes": null, "d1_pct": null, "vectors": null, "vectors_pct": null,
                "tenants": tenants, "namespaces_pct": tenants as f64 * 100.0 / 50_000.0,
                "measured_at": null,
            },
            "agent_signup": {
                "configured": configured,
                "active": reason.is_none(),
                "reason": reason,
                "switch": agent_switch,
                "daily_cap": console.map_or(1, |k| k.agent_signup_daily_cap.max(1)),
            },
        }),
    ))
}

#[derive(serde::Deserialize)]
struct DlqRow {
    id: String,
    queue: String,
    kind: Option<String>,
    tenant_id: Option<String>,
    first_seen_at: i64,
    redriven_at: Option<i64>,
    redrive_count: i64,
}

impl DlqRow {
    fn json(&self) -> serde_json::Value {
        json!({
            "id": self.id, "queue": self.queue, "kind": self.kind, "tenant_id": self.tenant_id,
            "first_seen_at": rfc3339(self.first_seen_at), "redriven_at": self.redriven_at.map(rfc3339),
            "redrive_count": self.redrive_count,
        })
    }
}

fn queue_of(name: &str) -> Option<pylota_identity_platform::queues::QueueName> {
    pylota_identity_platform::queues::QueueName::ALL
        .into_iter()
        .find(|q| {
            q.queue_name() == name && *q != pylota_identity_platform::queues::QueueName::Phone
        })
}

/// `GET /v1/platform/dlq` (Observability § 8.3): oldest first, never the stored body.
pub async fn dlq_list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let limit = c.limit()?;
    let filters = ["queue", "status", "tenant_id"];
    let queue = c.query("queue");
    if queue.is_some_and(|q| queue_of(q).is_none()) {
        return Err(ApiError::invalid("queue", "One of the work queues."));
    }
    let redriven = match c.query("status") {
        None | Some("open") => 0,
        Some("redriven") => 1,
        Some(_) => return Err(ApiError::invalid("status", "open or redriven.")),
    };
    let (kc, ki) = c
        .after(&filters)
        .await?
        .map_or((None, None), |(t, i)| (Some(t), Some(i)));
    let rows: Vec<DlqRow> = c
        .p
        .db()
        .query(pylota_identity_platform::d1::Stmt::new(
            "SELECT id, queue, kind, tenant_id, first_seen_at, redriven_at, redrive_count FROM dlq_items \
             WHERE (?1 IS NULL OR queue = ?1) AND (redriven_at IS NOT NULL) = (?2 = 1) AND (?3 IS NULL OR tenant_id = ?3) \
             AND (?4 IS NULL OR first_seen_at > ?4 OR (first_seen_at = ?4 AND id > ?5)) \
             ORDER BY first_seen_at, id LIMIT ?6",
            vec![
                queue.map(str::to_owned).into(),
                redriven.into(),
                c.query("tenant_id").map(str::to_owned).into(),
                kc.into(),
                ki.into(),
                i64::from(limit + 1).into(),
            ],
        ))
        .await?;
    c.p.db()
        .execute(audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            None,
            Some(&c.key().key_id),
            "dlq.list",
            "dlq",
            queue.unwrap_or("*"),
            &json!({ "count": rows.len().min(limit as usize) }),
            &c.request_id,
            c.now,
        ))
        .await?;
    c.page(
        rows,
        limit,
        &filters,
        |r| (r.first_seen_at, r.id.clone()),
        DlqRow::json,
    )
    .await
}

/// `POST /v1/platform/dlq/{dlq_id}/redrive`: the stored body, checked against its hash, back to its
/// source queue; then `redriven_at`, `redrive_count` and the audit row `dlq.redrive`.
pub async fn dlq_redrive<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    #[derive(serde::Deserialize)]
    struct Body {
        queue: String,
        body_json: String,
        body_sha256: String,
        tenant_id: Option<String>,
    }
    let id = c.route.param("dlq_id");
    let b: Body =
        c.p.db()
            .first(pylota_identity_platform::d1::Stmt::new(
                "SELECT queue, body_json, body_sha256, tenant_id FROM dlq_items WHERE id = ?1",
                vec![id.into()],
            ))
            .await?
            .ok_or_else(|| ApiError::not_found(ErrorCode::DlqItemNotFound))?;
    if pylota_identity_core::ses::sha256_hex(b.body_json.as_bytes()) != b.body_sha256 {
        crate::log::error("dlq_body_mismatch", &[("detail", id.to_owned().into())]);
        return Err(ApiError::internal());
    }
    let queue = queue_of(&b.queue).ok_or_else(ApiError::internal)?;
    let body: serde_json::Value =
        serde_json::from_str(&b.body_json).map_err(|_| ApiError::internal())?;
    pylota_identity_platform::QueueProducer::send(c.p.queues(), queue, &body, 0)
        .await
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "The queue did not accept the item."))?;
    c.p.db()
        .batch(vec![
            pylota_identity_platform::d1::Stmt::new(
                "UPDATE dlq_items SET redriven_at = ?2, redrive_count = redrive_count + 1 WHERE id = ?1",
                vec![id.into(), c.now.into()],
            ),
            audit::insert(
                &c.p.ids().new_id(IdPrefix::Audit),
                b.tenant_id.as_deref(),
                Some(&c.key().key_id),
                "dlq.redrive",
                "dlq_item",
                id,
                &json!({ "queue": b.queue }),
                &c.request_id,
                c.now,
            ),
        ])
        .await?;
    let row: DlqRow = c
        .p
        .db()
        .first(pylota_identity_platform::d1::Stmt::new(
            "SELECT id, queue, kind, tenant_id, first_seen_at, redriven_at, redrive_count FROM dlq_items WHERE id = ?1",
            vec![id.into()],
        ))
        .await?
        .ok_or_else(ApiError::internal)?;
    Ok(Reply::json(200, row.json()))
}
