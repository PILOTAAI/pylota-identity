//! Search routes ([Search § 2]): `POST /v1/identities/{id}/search`, `POST /v1/tenants/{id}/search` and
//! `GET …/messages/{message_id}/related`. The handler authenticates scope, rate-limits, validates the
//! body and builds the plan; `crate::search` runs it. Agentic search is M11: until then
//! `mode: "agentic"` answers `422 agentic_disabled` (after the `search:agentic` permission check).
//!
//! [Search § 2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::parse_rfc3339;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{
    Background as _, Clock as _, ControlDb as _, ObjectClient as _, Platform,
};
use serde_json::Value;

use super::messages::{review, valid_label};
use super::{identity_in_scope, tenant_in_scope};
use crate::ctx::{Ctx, Reply};
use crate::db::identities as idb;
use crate::errors::ApiError;
use crate::quota::{QuotaRequest, UsageMetric};
use crate::rpc::envelope_for;
use crate::search::{self, Mode, Params, Scope};

/// At most this many identities at tenant scope.
const MAX_IDENTITIES: usize = 100;
const FIELDS: &[&str] = &[
    "q",
    "mode",
    "filters",
    "group_by",
    "limit",
    "snippet_chars",
    "facets",
    "include_quarantined",
    "cursor",
    "require_mode",
    "budget",
    "stream",
];

pub async fn identity<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let body = c.json()?;
    if body.get("identity_ids").is_some() {
        return Err(ApiError::invalid(
            "identity_ids",
            "identity_ids is for tenant search.",
        ));
    }
    let scope = Scope {
        tenant: t,
        targets: vec![idn],
        tenant_scope: false,
    };
    search(c, scope, body).await
}

pub async fn tenant<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), false).await?;
    let body = c.json()?;
    // Never the system identity, never a deleted one (A15, Search § 10).
    let mut all: Vec<idb::Row> = c.p.db().query(idb::searchable(&t.id)).await?;
    all.sort_by(|a, b| a.id.cmp(&b.id));
    let targets = match &body["identity_ids"] {
        Value::Null => all,
        Value::Array(ids) => {
            if ids.len() > MAX_IDENTITIES {
                return Err(scope_too_large());
            }
            let mut out = Vec::new();
            for (i, id) in ids.iter().enumerate() {
                let Some(id) = id.as_str() else {
                    return Err(ApiError::invalid(
                        &format!("identity_ids[{i}]"),
                        "An identity ID.",
                    ));
                };
                match all.iter().find(|r| r.id == id) {
                    Some(r) if !out.iter().any(|o: &idb::Row| o.id == r.id) => out.push(r.clone()),
                    Some(_) => {}
                    None => return Err(ApiError::not_found(ErrorCode::IdentityNotFound)),
                }
            }
            out.sort_by(|a, b| a.id.cmp(&b.id));
            out
        }
        _ => {
            return Err(ApiError::invalid(
                "identity_ids",
                "An array of identity IDs.",
            ));
        }
    };
    if targets.len() > MAX_IDENTITIES {
        return Err(scope_too_large());
    }
    let scope = Scope {
        tenant: t,
        targets,
        tenant_scope: true,
    };
    search(c, scope, body).await
}

fn scope_too_large() -> ApiError {
    ApiError::new(
        ErrorCode::ScopeTooLarge,
        "Tenant search reaches at most 100 identities; pass identity_ids.",
    )
}

/// The request's structured `filters`.
struct Filters {
    direction: Option<String>,
    labels: Vec<String>,
    after_ms: Option<i64>,
    before_ms: Option<i64>,
}

fn filters(body: &Value) -> Result<Filters, ApiError> {
    let f = &body["filters"];
    if !(f.is_null() || f.is_object()) {
        return Err(ApiError::invalid("filters", "An object."));
    }
    let direction = match &f["direction"] {
        Value::Null => None,
        v => match v.as_str() {
            Some(d @ ("inbound" | "outbound")) => Some(d.to_owned()),
            _ => {
                return Err(ApiError::invalid(
                    "filters.direction",
                    "inbound or outbound.",
                ));
            }
        },
    };
    let labels = match &f["labels"] {
        Value::Null => Vec::new(),
        Value::Array(a) => {
            let mut out = Vec::new();
            for (i, l) in a.iter().enumerate() {
                match l.as_str() {
                    Some(l) if valid_label(l) => out.push(l.to_owned()),
                    _ => {
                        return Err(ApiError::invalid(
                            &format!("filters.labels[{i}]"),
                            "Not a label.",
                        ));
                    }
                }
            }
            out
        }
        _ => return Err(ApiError::invalid("filters.labels", "An array of labels.")),
    };
    let time = |name: &str| -> Result<Option<i64>, ApiError> {
        match &f[name] {
            Value::Null => Ok(None),
            v => v.as_str().and_then(parse_rfc3339).map(Some).ok_or_else(|| {
                ApiError::invalid(&format!("filters.{name}"), "Not an RFC 3339 time.")
            }),
        }
    };
    let after_ms = time("after")?;
    let before_ms = time("before")?;
    Ok(Filters {
        direction,
        labels,
        after_ms,
        before_ms,
    })
}

fn int(body: &Value, name: &str, default: i64, lo: i64, hi: i64) -> Result<i64, ApiError> {
    match &body[name] {
        Value::Null => Ok(default),
        v => match v.as_i64() {
            Some(n) if (lo..=hi).contains(&n) => Ok(n),
            _ => Err(ApiError::invalid(name, format!("{lo} to {hi}."))),
        },
    }
}

fn flag(body: &Value, name: &str, default: bool) -> Result<bool, ApiError> {
    match &body[name] {
        Value::Null => Ok(default),
        Value::Bool(b) => Ok(*b),
        _ => Err(ApiError::invalid(name, "true or false.")),
    }
}

async fn search<P: Platform>(c: &Ctx<P>, scope: Scope, body: Value) -> Result<Reply, ApiError> {
    let Some(obj) = body.as_object() else {
        return Err(ApiError::invalid("", "The body must be a JSON object."));
    };
    let key = c.key();
    if body["mode"] == "agentic" {
        return agentic(c, scope, body).await;
    }
    crate::ratelimit::check(c.p.as_ref(), RlBucket::Search, &key.key_id).await?;
    for k in obj.keys() {
        if !FIELDS.contains(&k.as_str()) && k != "identity_ids" {
            return Err(ApiError::invalid(k, "Unknown field."));
        }
    }
    let q = match &body["q"] {
        Value::String(q) if q.chars().count() <= 1_024 => q.clone(),
        Value::String(_) => return Err(ApiError::invalid("q", "At most 1,024 characters.")),
        _ => {
            return Err(ApiError::invalid(
                "q",
                "A string; \"\" matches all messages.",
            ));
        }
    };
    let mode = match &body["mode"] {
        Value::Null => Mode::Hybrid,
        v => match v.as_str() {
            Some("keyword") => Mode::Keyword,
            Some("semantic") => Mode::Semantic,
            Some("hybrid") => Mode::Hybrid,
            _ => return Err(ApiError::invalid("mode", "keyword, semantic or hybrid.")),
        },
    };
    let thread = match &body["group_by"] {
        Value::Null => false,
        v => match v.as_str() {
            Some("message") => false,
            Some("thread") => true,
            _ => return Err(ApiError::invalid("group_by", "message or thread.")),
        },
    };
    let limit = int(&body, "limit", 10, 1, 50)? as u32;
    let snippet_chars = int(&body, "snippet_chars", 240, 40, 1_000)? as u32;
    let facets = flag(&body, "facets", true)?;
    let review = review(c);
    // Without quarantine:review the flag is filtered silently (F7).
    let include_quarantined = flag(&body, "include_quarantined", false)? && review;
    let require_mode = flag(&body, "require_mode", false)?;
    let cursor = match &body["cursor"] {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        _ => return Err(ApiError::invalid("cursor", "A cursor string.")),
    };
    let Filters {
        direction,
        labels,
        after_ms,
        before_ms,
    } = filters(&body)?;
    let started = c.p.clock().now_ms();
    let tenant_id = scope.tenant.id.clone();
    let quota_do = scope.tenant.quota_do_id.clone();
    let plan = search::plan(
        c.p.as_ref(),
        scope,
        Params {
            q,
            mode,
            thread,
            limit,
            snippet_chars,
            facets,
            include_quarantined,
            review,
            require_mode,
            cursor,
            direction,
            labels,
            after_ms,
            before_ms,
            key_id: key.key_id.clone(),
            request_id: c.request_id.clone(),
            safe_why: false,
        },
    )
    .await?;
    let out = search::run(c.p.as_ref(), &plan).await?;
    // Accounting (step 10): one search; the query text is never logged (FR-PRV-6).
    let env = envelope_for(
        c.p.as_ref(),
        Some(&tenant_id),
        None,
        &c.request_id,
        None,
        QuotaRequest::RecordUsage {
            metric: UsageMetric::Search,
            n: 1,
        },
    );
    // After the response: the search is answered whether or not the count lands.
    let owner = c.p.clone();
    c.p.background().wait_until(async move {
        let _ = owner
            .objects()
            .call::<_, Value>(DoClass::Quota, &quota_do, &env)
            .await;
    });
    let hash = pylota_identity_core::search::cursor::query_hash(
        &c.p.secrets().hash_key,
        body["q"].as_str().unwrap_or(""),
    );
    crate::log::info(
        "search",
        &[
            ("tenant_id", tenant_id.into()),
            ("mode", out["query"]["mode"].as_str().unwrap_or("").into()),
            ("ms", (c.p.clock().now_ms() - started).into()),
            (
                "hits",
                (out["hits"].as_array().map_or(0, Vec::len) as i64).into(),
            ),
            (
                "degraded",
                out["degraded"].as_bool().unwrap_or(false).into(),
            ),
            ("query_hash", hash.into()),
        ],
    );
    Ok(Reply::json(200, out))
}

/// `GET /v1/identities/{identity_id}/messages/{message_id}/related`.
pub async fn related<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let key = c.key();
    crate::ratelimit::check(c.p.as_ref(), RlBucket::Search, &key.key_id).await?;
    let limit = match c.query("limit") {
        None => 10,
        Some(l) => match l.parse::<u32>() {
            Ok(n) if (1..=50).contains(&n) => n,
            _ => return Err(ApiError::invalid("limit", "1 to 50.")),
        },
    };
    let scope = Scope {
        tenant: t,
        targets: vec![idn],
        tenant_scope: false,
    };
    let out = search::related::related(
        c.p.as_ref(),
        scope,
        c.route.param("message_id"),
        limit,
        review(c),
        &key.key_id,
        &c.request_id,
        false,
    )
    .await?;
    Ok(Reply::json(200, out))
}

/// `GET /v1/identities/{identity_id}/contacts`.
pub async fn contacts<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    use crate::mailbox::search::{ContactsIn, SearchOp};
    use crate::search::contacts as ct;
    let (idn, t) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let key = c.key();
    crate::ratelimit::check(c.p.as_ref(), RlBucket::Search, &key.key_id).await?;
    let limit = c.limit()?;
    let q = ct::fold_q(c.query("q").unwrap_or(""))
        .ok_or_else(|| ApiError::invalid("q", "At most 100 characters."))?;
    const F: &[&str] = &["q"];
    let after = match c.after(F).await? {
        None => None,
        Some((_, k)) => Some(
            ct::decode_boundary(&k)
                .ok_or_else(|| ApiError::invalid("cursor", "The cursor is not valid."))?,
        ),
    };
    let scope = Scope {
        tenant: t,
        targets: vec![idn],
        tenant_scope: false,
    };
    let plan = search::plan(
        c.p.as_ref(),
        scope,
        Params {
            q: String::new(),
            mode: Mode::Keyword,
            thread: false,
            limit,
            snippet_chars: 240,
            facets: false,
            include_quarantined: false,
            review: review(c),
            require_mode: false,
            cursor: None,
            direction: None,
            labels: Vec::new(),
            after_ms: None,
            before_ms: None,
            key_id: key.key_id.clone(),
            request_id: c.request_id.clone(),
            safe_why: false,
        },
    )
    .await?;
    let fan = search::Fan::new(c.p.as_ref(), &plan);
    let rows: Vec<Option<Vec<Value>>> = fan
        .each(vec![Some(SearchOp::Contacts(ContactsIn {
            q,
            limit,
            review: review(c),
            after,
        }))])
        .await?;
    let rows = rows.into_iter().flatten().flatten().collect::<Vec<_>>();
    c.page(
        rows,
        limit,
        F,
        |r| (0, ct::encode_boundary(&ct::boundary(r))),
        ct::render,
    )
    .await
}

/// Fields of an `AgenticRequest` (and `identity_ids` on the tenant route).
const AGENTIC_FIELDS: &[&str] = &[
    "q",
    "mode",
    "budget",
    "stream",
    "filters",
    "include_quarantined",
    "require_mode",
    "identity_ids",
];

/// `mode: "agentic"` ([Search § 2, § 11.1, § 11.2 Init]): `search:agentic`, the policy switch,
/// `RL_AGENTIC`, the body and budget, then the tenant's daily cap, counted once; then the run, as JSON
/// or as server-sent events.
async fn agentic<P: Platform>(c: &Ctx<P>, scope: Scope, body: Value) -> Result<Reply, ApiError> {
    use crate::quota::CountAgenticAnswer;
    use crate::search::agentic::{self, Agent, Request, sse::Sse};
    let key = c.key();
    if !key.permissions.iter().any(|p| p == "search:agentic") {
        return Err(ApiError::permission_denied(&["search:agentic"]));
    }
    let policy = crate::policy::effective(c.p.as_ref(), &scope.tenant);
    let pol = |path: &str| pylota_identity_core::policy::get_path(&policy, path).cloned();
    if pol("search.agentic_enabled").and_then(|v| v.as_bool()) == Some(false) {
        return Err(ApiError::new(
            ErrorCode::AgenticDisabled,
            "Agentic search is turned off for this tenant.",
        ));
    }
    crate::ratelimit::check(c.p.as_ref(), RlBucket::Agentic, &key.key_id).await?;
    if let Some(k) = body
        .as_object()
        .and_then(|o| o.keys().find(|k| !AGENTIC_FIELDS.contains(&k.as_str())))
    {
        return Err(ApiError::invalid(k, "Unknown field."));
    }
    let q = match &body["q"] {
        Value::String(q) if !q.trim().is_empty() && q.chars().count() <= 1_024 => q.clone(),
        _ => {
            return Err(ApiError::invalid(
                "q",
                "The question, 1 to 1,024 characters.",
            ));
        }
    };
    let policy_steps = pol("search.agentic_max_steps")
        .and_then(|v| v.as_i64())
        .unwrap_or(6);
    let policy_secs = pol("search.agentic_max_seconds")
        .and_then(|v| v.as_i64())
        .unwrap_or(8);
    let budget = &body["budget"];
    if !(budget.is_null() || budget.is_object()) {
        return Err(ApiError::invalid("budget", "An object."));
    }
    let bounded = |name: &str, lo: i64, hi: i64, policy: i64| -> Result<i64, ApiError> {
        match &budget[name] {
            Value::Null => Ok(policy),
            v => match v.as_i64() {
                // A larger value is lowered to the policy's, not refused.
                Some(n) if (lo..=hi).contains(&n) => Ok(n.min(policy)),
                _ => Err(ApiError::invalid(
                    &format!("budget.{name}"),
                    format!("{lo} to {hi}."),
                )),
            },
        }
    };
    let max_steps = bounded("max_steps", 2, 10, policy_steps.clamp(2, 10))? as u8;
    let max_seconds = bounded("max_seconds", 3, 30, policy_secs.clamp(3, 30))? as u8;
    let review = review(c);
    let include_quarantined = flag(&body, "include_quarantined", false)? && review;
    let require_mode = flag(&body, "require_mode", false)?;
    let stream = flag(&body, "stream", false)?
        && c.header("accept")
            .is_some_and(|a| a.contains("text/event-stream"));
    let Filters {
        direction,
        labels,
        after_ms,
        before_ms,
    } = filters(&body)?;
    // Init: one count per validated request against the tenant's daily cap.
    let cap = pol("search.agentic_daily_cap")
        .and_then(|v| v.as_u64())
        .unwrap_or(500) as u32;
    let env = envelope_for(
        c.p.as_ref(),
        Some(&scope.tenant.id),
        None,
        &c.request_id,
        None,
        QuotaRequest::CountAgentic {
            tz: scope.tenant.timezone.clone(),
            cap,
        },
    );
    let counted =
        c.p.objects()
            .call::<_, CountAgenticAnswer>(DoClass::Quota, &scope.tenant.quota_do_id, &env)
            .await?;
    match counted {
        crate::rpc::RpcResult::Ok(CountAgenticAnswer::Ok { .. }) => {}
        crate::rpc::RpcResult::Ok(CountAgenticAnswer::CapReached { resets_at }) => {
            return Err(ApiError::new(
                ErrorCode::AgenticBudgetExhausted,
                "The tenant's agentic searches for today are spent.",
            )
            .details(serde_json::json!({
                "resets_at": pylota_identity_core::days::rfc3339(resets_at),
                "cap": cap,
            })));
        }
        crate::rpc::RpcResult::Err(e) => return Err(ApiError::new(e.error.code, e.error.message)),
    }
    let req = Request {
        q,
        direction,
        labels,
        after_ms,
        before_ms,
        include_quarantined,
        review,
        max_steps,
        max_seconds,
    };
    let nonce = agentic::nonce(c.p.as_ref());
    if !stream {
        let run = Agent::new(
            c.p.clone(),
            scope,
            req,
            &key.key_id,
            &c.request_id,
            nonce,
            None,
        )
        .run()
        .await;
        if require_mode && run.status == agentic::answer::Status::Degraded {
            return Err(search::degraded_error());
        }
        return Ok(Reply::json(200, run.body));
    }
    let (body, sink) = pylota_identity_platform::stream::channel()?;
    let sse = std::rc::Rc::new(Sse::new(sink, c.p.clock().now_ms()));
    let agent = Agent::new(
        c.p.clone(),
        scope,
        req,
        &key.key_id,
        &c.request_id,
        nonce,
        Some(sse.clone()),
    );
    let p = c.p.clone();
    c.p.background().wait_until(async move {
        let work = async {
            let run = agent.run().await;
            sse.done(p.as_ref(), &run.body).await;
        };
        search::join::both(work, sse.keep_alive(p.as_ref())).await;
    });
    Ok(Reply {
        status: 200,
        body: None,
        raw: None,
        headers: vec![
            (
                "content-type".into(),
                "text/event-stream; charset=utf-8".into(),
            ),
            ("x-accel-buffering".into(), "no".into()),
        ],
        stream: Some(body),
    })
}
