//! Search ([Search § 2]): the front half. A validated request becomes a [`Plan`] (scope, compiled query,
//! cursor); the mode runs its legs against the mailboxes in scope ([`Fan`]), the ranked list is paged
//! with the cursor rules, and the page is assembled with snippets, `why`, facets and coverage under the
//! 256 KB byte cap. Every mailbox call re-applies visibility; nothing here reads mail text directly.
//!
//! [Search § 2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

pub mod contacts;
pub mod facets;
pub mod hybrid;
pub mod join;
pub mod related;
pub mod semantic;
pub mod wire;

use std::cell::RefCell;
use std::collections::HashMap;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::rfc3339;
use pylota_identity_core::query::{CompiledQuery, Tz};
use pylota_identity_core::search::cursor::{self as scursor, Boundary, CursorV1};
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{Clock as _, ObjectClient as _, Platform};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::db::{identities as idb, tenants as tdb};
use crate::errors::ApiError;
use crate::keyring::{self, Purpose};
use crate::mailbox::MailboxRequest;
use crate::mailbox::search::{DetailsIn, KeywordIn, KeywordOut, SearchOp};
use crate::rpc::{RpcResult, envelope_for};
use wire::{Cand, Compiled, Detail};

/// The response byte cap ([Search § 5.9]).
pub const BYTE_CAP: usize = 262_144;
/// Each identity's deadline at tenant scope, from the start of the fan-out ([F15]).
pub const DEADLINE_MS: i64 = 900;
/// Mailbox calls in flight at once at tenant scope.
pub const IN_FLIGHT: usize = 20;
/// Candidate list lengths: keyword, hybrid, semantic.
pub const KEYWORD_CANDIDATES: u32 = 1_000;
pub const HYBRID_CANDIDATES: u32 = 200;
/// The slack band of keyword and hybrid cursors (0.02).
pub const EPS_KEYWORD: u32 = 20_000;
/// At most this many `why` entries.
pub const MAX_WHY: usize = 8;
/// Month buckets sent for the `month` facet (ten years; the facet keeps the 24 newest with messages).
const MONTHS: u32 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Keyword,
    Semantic,
    Hybrid,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Keyword => "keyword",
            Mode::Semantic => "semantic",
            Mode::Hybrid => "hybrid",
        }
    }
}

/// The identities a search reads, sorted by ID; their index is the high part of `ord` at tenant scope.
pub struct Scope {
    pub tenant: tdb::Row,
    pub targets: Vec<idb::Row>,
    pub tenant_scope: bool,
}

/// A validated request.
pub struct Plan {
    pub scope: Scope,
    pub mode: Mode,
    pub thread: bool,
    pub limit: u32,
    pub snippet_chars: u32,
    pub facets: bool,
    pub include_quarantined: bool,
    pub require_mode: bool,
    pub compiled: CompiledQuery,
    pub wire: Compiled,
    pub parsed: String,
    pub as_of: i64,
    pub cursor: Option<CursorV1>,
    pub qh: String,
    pub tz: Tz,
    pub request_id: String,
    /// The `direction`, `after` and `before` request filters, for the Vectorize pre-filter.
    pub direction: Option<String>,
    pub after_ms: Option<i64>,
    pub before_ms: Option<i64>,
}

impl Plan {
    pub fn ord(&self, idx: usize, rowid: i64) -> u64 {
        let rowid = u64::try_from(rowid).unwrap_or(0);
        if self.scope.tenant_scope {
            ((idx as u64) << 40) | rowid
        } else {
            rowid
        }
    }

    pub fn eps(&self) -> u32 {
        if self.mode == Mode::Semantic {
            0
        } else {
            EPS_KEYWORD
        }
    }
}

/// One candidate of the merged list.
#[derive(Clone, Debug)]
pub struct Item {
    pub idx: usize,
    pub ord: u64,
    pub c: Cand,
    pub score: f64,
    /// `semantic:` and `rerank:` reasons, added after the mailbox's own.
    pub cosine: Option<f64>,
    pub rerank: Option<f64>,
}

impl Item {
    pub fn key(&self) -> Boundary {
        Boundary {
            score_q: scursor::score_q(self.score),
            date: self.c.msg_date,
            ord: self.ord,
        }
    }
}

/// Orders by the cursor key, `(score_q, date, ord)` descending.
pub fn sort_items(items: &mut [Item]) {
    items.sort_by(|a, b| {
        let (ka, kb) = (a.key(), b.key());
        (kb.score_q, kb.date, kb.ord).cmp(&(ka.score_q, ka.date, ka.ord))
    });
}

// ---- Building a plan --------------------------------------------------------------------------------

/// A request after its body was read: everything a [`Plan`] is built from.
pub struct Params {
    pub q: String,
    pub mode: Mode,
    pub thread: bool,
    pub limit: u32,
    pub snippet_chars: u32,
    pub facets: bool,
    /// Already narrowed to keys with `quarantine:review`.
    pub include_quarantined: bool,
    pub review: bool,
    pub require_mode: bool,
    pub cursor: Option<String>,
    pub direction: Option<String>,
    pub labels: Vec<String>,
    pub after_ms: Option<i64>,
    pub before_ms: Option<i64>,
    pub key_id: String,
    pub request_id: String,
}

/// The tenant's time zone (UTC when the stored name is not an IANA zone).
pub fn tz_of(t: &tdb::Row) -> Tz {
    t.timezone.parse().unwrap_or(Tz::UTC)
}

/// The category names `category:` accepts: the custom list of the policy, else the built-in one.
fn categories(policy: &Value) -> Vec<String> {
    match pylota_identity_core::policy::get_path(policy, "triage.categories")
        .and_then(Value::as_array)
    {
        Some(list) if !list.is_empty() => list
            .iter()
            .filter_map(|c| c["name"].as_str().or_else(|| c.as_str()).map(str::to_owned))
            .collect(),
        _ => pylota_identity_core::triage_rules::BUILT_IN_CATEGORIES
            .iter()
            .map(|c| (*c).to_owned())
            .collect(),
    }
}

/// The reference packs of `policy.search.refs_packs` (`core` when unset).
pub fn packs(policy: &Value) -> Vec<pylota_identity_core::refs::Pack> {
    use pylota_identity_core::refs::Pack;
    pylota_identity_core::policy::get_path(policy, "search.refs_packs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().and_then(Pack::from_name))
                .collect()
        })
        .unwrap_or_else(|| vec![Pack::Core])
}

/// The `400 invalid_query` of a parse error.
pub fn query_error(e: &pylota_identity_core::query::QueryError) -> ApiError {
    ApiError::new(ErrorCode::InvalidQuery, "The query could not be parsed.").details(json!({
        "position": e.position,
        "expected": e.expected,
        "found": e.found,
    }))
}

/// Steps 5–7 of [Search § 2]: the cursor, the parse, date resolution and compilation, and the
/// request's structured filters ANDed onto the compiled filter.
///
/// [Search § 2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
pub async fn plan<P: Platform>(p: &P, scope: Scope, x: Params) -> Result<Plan, ApiError> {
    use pylota_identity_core::query::{
        CompileCtx, MSG_DATE, ParseCtx, compile, parse, print, require_semantic_text,
    };
    let policy = crate::policy::effective(p, &scope.tenant);
    let tz = tz_of(&scope.tenant);
    let cats = categories(&policy);
    let cat_refs: Vec<&str> = cats.iter().map(String::as_str).collect();
    let packs = packs(&policy);
    let query = parse(
        &x.q,
        &ParseCtx {
            categories: &cat_refs,
            ref_packs: &packs,
            custom_refs: &[],
            tz,
        },
    )
    .map_err(|e| query_error(&e))?;
    let parsed = print(&query);
    let mut ids: Vec<&str> = scope.targets.iter().map(|t| t.id.as_str()).collect();
    ids.sort_unstable();
    let filters = json!({
        "direction": x.direction, "labels": x.labels, "after": x.after_ms, "before": x.before_ms,
    })
    .to_string();
    let include = x.include_quarantined.to_string();
    let ids = ids.join(",");
    let qh = scursor::request_hash(&[
        &parsed,
        x.mode.as_str(),
        &filters,
        if x.thread { "thread" } else { "message" },
        &include,
        &ids,
        &x.key_id,
    ]);
    let now = p.clock().now_ms();
    let cursor = match &x.cursor {
        None => None,
        Some(token) => {
            let ks = keyring::keys(p, Purpose::Cursor).await?;
            let refs: Vec<(&str, &[u8; 32])> =
                ks.iter().map(|k| (k.kid.as_str(), &k.secret)).collect();
            let c = scursor::open(token, &refs, now).map_err(|e| match e {
                pylota_identity_core::cursor::CursorError::Expired => {
                    ApiError::new(ErrorCode::CursorExpired, "The cursor has expired.")
                }
                pylota_identity_core::cursor::CursorError::Invalid => {
                    ApiError::invalid("cursor", "The cursor is not valid.")
                }
            })?;
            if c.qh != qh {
                return Err(ApiError::invalid(
                    "cursor",
                    "This cursor belongs to a different query.",
                ));
            }
            Some(c)
        }
    };
    let as_of = cursor.as_ref().map_or(now, |c| c.as_of);
    let compiled = compile(
        &query,
        &CompileCtx {
            tz,
            now_ms: as_of,
            ref_packs: &packs,
            custom_refs: &[],
            quarantine_review: x.review,
        },
    );
    if x.mode == Mode::Semantic {
        require_semantic_text(&compiled).map_err(|e| query_error(&e))?;
    }
    let include_quarantined =
        x.include_quarantined || (compiled.forces_include_quarantined && x.review);
    let mut wire = Compiled::from(&compiled);
    let mut extra: Vec<String> = Vec::new();
    let mut push = |sql: String, param: Option<wire::Param>, wire: &mut Compiled| {
        extra.push(sql);
        wire.filter_params.extend(param);
    };
    if let Some(d) = &x.direction {
        push(
            "m.direction = ?".into(),
            Some(wire::Param::Text(d.clone())),
            &mut wire,
        );
        wire.why_filters.push(format!("in:{d}"));
    }
    for l in &x.labels {
        push(
            "EXISTS (SELECT 1 FROM labels l WHERE l.message_rowid = m.rowid AND l.label = ?)"
                .into(),
            Some(wire::Param::Text(l.clone())),
            &mut wire,
        );
        wire.why_filters.push(format!("label:{l}"));
    }
    if let Some(a) = x.after_ms {
        push(
            format!("{MSG_DATE} >= ?"),
            Some(wire::Param::Int(a)),
            &mut wire,
        );
    }
    if let Some(b) = x.before_ms {
        push(
            format!("{MSG_DATE} < ?"),
            Some(wire::Param::Int(b)),
            &mut wire,
        );
    }
    if !extra.is_empty() {
        let mut sql = String::new();
        if !wire.filter_sql.trim().is_empty() {
            sql.push('(');
            sql.push_str(&wire.filter_sql);
            sql.push_str(") AND ");
        }
        sql.push_str(&extra.join(" AND "));
        wire.filter_sql = sql;
    }
    Ok(Plan {
        scope,
        mode: x.mode,
        thread: x.thread,
        limit: x.limit,
        snippet_chars: x.snippet_chars,
        facets: x.facets,
        include_quarantined,
        require_mode: x.require_mode,
        compiled,
        wire,
        parsed,
        as_of,
        cursor,
        qh,
        tz,
        request_id: x.request_id,
        direction: x.direction,
        after_ms: x.after_ms,
        before_ms: x.before_ms,
    })
}

// ---- Fan-out ----------------------------------------------------------------------------------------

/// The mailboxes of a plan. At identity scope an error is the request's error; at tenant scope an
/// identity that errors or misses its deadline joins `failed` and is skipped from then on.
pub struct Fan<'a, P: Platform> {
    pub p: &'a P,
    pub plan: &'a Plan,
    start: i64,
    failed: RefCell<Vec<bool>>,
}

impl<'a, P: Platform> Fan<'a, P> {
    pub fn new(p: &'a P, plan: &'a Plan) -> Self {
        Self {
            p,
            plan,
            start: p.clock().now_ms(),
            failed: RefCell::new(vec![false; plan.scope.targets.len()]),
        }
    }

    pub fn failed_ids(&self) -> Vec<String> {
        self.failed
            .borrow()
            .iter()
            .zip(&self.plan.scope.targets)
            .filter(|(f, _)| **f)
            .map(|(_, t)| t.id.clone())
            .collect()
    }

    pub fn answered(&self, idx: usize) -> bool {
        !self.failed.borrow().get(idx).copied().unwrap_or(true)
    }

    async fn one<T: DeserializeOwned>(&self, idx: usize, op: SearchOp) -> Result<T, ApiError> {
        let idn = &self.plan.scope.targets[idx];
        let env = envelope_for(
            self.p,
            Some(&idn.tenant_id),
            Some(&idn.id),
            &self.plan.request_id,
            None,
            MailboxRequest::Search(op),
        );
        match self
            .p
            .objects()
            .call::<_, Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
            .await?
        {
            RpcResult::Ok(v) => serde_json::from_value(v).map_err(|_| ApiError::internal()),
            RpcResult::Err(e) => Err(ApiError::new(e.error.code, e.error.message)),
        }
    }

    /// One operation per identity (`None` skips it); answers in identity order.
    pub async fn each<T: DeserializeOwned>(
        &self,
        ops: Vec<Option<SearchOp>>,
    ) -> Result<Vec<Option<T>>, ApiError> {
        let tenant = self.plan.scope.tenant_scope;
        let futs: Vec<_> = ops
            .into_iter()
            .enumerate()
            .map(|(idx, op)| async move {
                let op = op.filter(|_| {
                    self.answered(idx) && !self.plan.scope.targets[idx].mailbox_do_id.is_empty()
                })?;
                if !tenant {
                    return Some(self.one::<T>(idx, op).await);
                }
                let left = DEADLINE_MS - (self.p.clock().now_ms() - self.start);
                let late = || ApiError::new(ErrorCode::Timeout, "missed the deadline");
                if left <= 0 {
                    return Some(Err(late()));
                }
                let sleep = self.p.clock().sleep(u32::try_from(left).unwrap_or(0));
                Some(
                    join::before(self.one::<T>(idx, op), sleep)
                        .await
                        .unwrap_or_else(|| Err(late())),
                )
            })
            .collect();
        let width = if tenant { IN_FLIGHT } else { 1 };
        let mut out = Vec::new();
        for (idx, r) in join::bounded(futs, width).await.into_iter().enumerate() {
            out.push(match r {
                None => None,
                Some(Ok(v)) => Some(v),
                Some(Err(e)) if !tenant => return Err(e),
                Some(Err(_)) => {
                    self.failed.borrow_mut()[idx] = true;
                    None
                }
            });
        }
        Ok(out)
    }
}

// ---- Legs -------------------------------------------------------------------------------------------

pub struct KeywordLeg {
    pub items: Vec<Item>,
    pub facets: Option<Value>,
    pub facet_rows: Vec<Vec<i64>>,
}

/// The keyword leg: each mailbox's candidates merged by `kw_score`, then date, then `ord`.
pub async fn keyword_leg<P: Platform>(
    fan: &Fan<'_, P>,
    limit: u32,
    months: Option<Vec<(String, i64, i64)>>,
    facet_rows: bool,
) -> Result<KeywordLeg, ApiError> {
    let plan = fan.plan;
    let ops = plan
        .scope
        .targets
        .iter()
        .map(|_| {
            Some(SearchOp::Keyword(KeywordIn {
                c: plan.wire.clone(),
                as_of: plan.as_of,
                include_quarantined: plan.include_quarantined,
                limit,
                months: months.clone(),
                facet_rows,
            }))
        })
        .collect();
    let outs: Vec<Option<KeywordOut>> = fan.each(ops).await?;
    let mut items = Vec::new();
    let mut facet_sets = Vec::new();
    let mut rows = Vec::new();
    for (idx, o) in outs.into_iter().enumerate() {
        let o = o.unwrap_or_default();
        facet_sets.extend(o.facets);
        rows.push(o.facet_rows);
        for c in o.cands {
            items.push(Item {
                idx,
                ord: plan.ord(idx, c.rowid),
                score: c.kw,
                c,
                cosine: None,
                rerank: None,
            });
        }
    }
    items.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.c.msg_date.cmp(&a.c.msg_date))
            .then(b.ord.cmp(&a.ord))
    });
    items.truncate(limit as usize);
    Ok(KeywordLeg {
        items,
        facets: months.map(|_| facets::merge(&facet_sets)),
        facet_rows: rows,
    })
}

/// `Σ covered / Σ eligible` over the identities that answered, 3 decimals (1.0 when nothing is
/// eligible).
pub async fn coverage<P: Platform>(fan: &Fan<'_, P>) -> Result<f64, ApiError> {
    let model = semantic::model_tag(fan.p);
    let ops = fan
        .plan
        .scope
        .targets
        .iter()
        .map(|_| {
            Some(SearchOp::Coverage {
                as_of: fan.plan.as_of,
                model: model.clone(),
            })
        })
        .collect();
    let outs: Vec<Option<crate::mailbox::search::Coverage>> = fan.each(ops).await?;
    let (mut e, mut c) = (0i64, 0i64);
    for o in outs.into_iter().flatten() {
        e += o.eligible;
        c += o.covered;
    }
    Ok(if e == 0 {
        1.0
    } else {
        round3(c as f64 / e as f64)
    })
}

pub fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

// ---- Running a plan ---------------------------------------------------------------------------------

/// What a mode produced before paging.
pub struct Ranked {
    pub items: Vec<Item>,
    pub facets: Option<Value>,
    pub coverage: Option<f64>,
    pub degraded: bool,
}

/// Runs the plan and answers the response body.
pub async fn run<P: Platform>(p: &P, plan: &Plan) -> Result<Value, ApiError> {
    let fan = Fan::new(p, plan);
    let first = plan.cursor.is_none();
    let months = (first && plan.facets)
        .then(|| pylota_identity_core::query::month_buckets(plan.tz, plan.as_of, MONTHS));
    let ranked = match plan.mode {
        Mode::Keyword => {
            let leg = keyword_leg(&fan, KEYWORD_CANDIDATES, months, false).await?;
            Ranked {
                items: leg.items,
                facets: leg.facets,
                coverage: None,
                degraded: false,
            }
        }
        Mode::Semantic => match semantic::semantic_mode(&fan, months).await? {
            Some(r) => r,
            None => {
                // The semantic leg failed: keyword results instead.
                let leg = keyword_leg(&fan, KEYWORD_CANDIDATES, months_again(plan), false).await?;
                Ranked {
                    items: leg.items,
                    facets: leg.facets,
                    coverage: Some(coverage(&fan).await?),
                    degraded: true,
                }
            }
        },
        Mode::Hybrid => hybrid::hybrid_mode(&fan, months).await?,
    };
    assemble(&fan, ranked).await
}

fn months_again(plan: &Plan) -> Option<Vec<(String, i64, i64)>> {
    (plan.cursor.is_none() && plan.facets)
        .then(|| pylota_identity_core::query::month_buckets(plan.tz, plan.as_of, MONTHS))
}

/// The refusal of `require_mode` when the requested mode could not run.
pub fn degraded_error() -> ApiError {
    ApiError::new(
        ErrorCode::SearchDegraded,
        "The requested search mode is unavailable.",
    )
}

/// One entry of the page: a message, or a thread's best message.
struct Entry {
    item: Item,
}

async fn assemble<P: Platform>(fan: &Fan<'_, P>, ranked: Ranked) -> Result<Value, ApiError> {
    let plan = fan.plan;
    let mut items = ranked.items;
    sort_items(&mut items);
    if plan.thread {
        // A thread's key is its best message's key; each thread appears once.
        let mut seen = std::collections::HashSet::new();
        items.retain(|i| seen.insert((i.idx, i.c.thread_seq)));
    }
    let eps = plan.eps();
    let after: Vec<Item> = match &plan.cursor {
        Some(c) => items
            .iter()
            .filter(|i| scursor::after(c, &i.key(), eps))
            .cloned()
            .collect(),
        None => items.clone(),
    };
    let more = after.len() > plan.limit as usize;
    let page: Vec<Entry> = after
        .into_iter()
        .take(plan.limit as usize)
        .map(|item| Entry { item })
        .collect();

    // Details, per identity.
    let mut per: Vec<Vec<(i64, Option<String>)>> = vec![Vec::new(); plan.scope.targets.len()];
    for e in &page {
        per[e.item.idx].push((e.item.c.rowid, e.item.c.vector_id.clone()));
    }
    let ops = per
        .iter()
        .map(|items| {
            (!items.is_empty()).then(|| {
                SearchOp::Details(DetailsIn {
                    items: items.clone(),
                    c: plan.wire.clone(),
                    snippet_chars: plan.snippet_chars,
                })
            })
        })
        .collect();
    let details: Vec<Option<Vec<Detail>>> = fan.each(ops).await?;
    let mut by: HashMap<(usize, i64), Detail> = HashMap::new();
    for (idx, d) in details.into_iter().enumerate() {
        for d in d.unwrap_or_default() {
            by.insert((idx, d.rowid), d);
        }
    }
    let page: Vec<Entry> = page
        .into_iter()
        .filter(|e| fan.answered(e.item.idx))
        .collect();

    // Thread rows.
    let mut threads: HashMap<(usize, i64), Value> = HashMap::new();
    if plan.thread {
        let mut seqs: Vec<Vec<i64>> = vec![Vec::new(); plan.scope.targets.len()];
        for e in &page {
            seqs[e.item.idx].push(e.item.c.thread_seq);
        }
        let ops = seqs
            .iter()
            .map(|s| (!s.is_empty()).then(|| SearchOp::Threads { seqs: s.clone() }))
            .collect();
        let rows: Vec<Option<Vec<Value>>> = fan.each(ops).await?;
        for (idx, r) in rows.into_iter().enumerate() {
            for t in r.unwrap_or_default() {
                if let Some(seq) = t["seq"].as_i64() {
                    threads.insert((idx, seq), t);
                }
            }
        }
    }

    let hits: Vec<Value> = page
        .iter()
        .map(|e| {
            let d = by.remove(&(e.item.idx, e.item.c.rowid)).unwrap_or_default();
            let idn = &plan.scope.targets[e.item.idx];
            let why = why(&e.item, &d, plan);
            if plan.thread {
                thread_hit(
                    plan,
                    idn,
                    &e.item,
                    &d,
                    why,
                    threads.get(&(e.item.idx, e.item.c.thread_seq)),
                )
            } else {
                message_hit(idn, &e.item, d, why)
            }
        })
        .collect();

    let coverage = ranked.coverage;
    let keys: Vec<Boundary> = page.iter().map(|e| e.item.key()).collect();
    let body = |n: usize, hits: &[Value], next: Option<String>, truncated: bool| {
        let mut b = json!({
            "query": { "parsed": plan.parsed, "mode": plan.mode.as_str() },
            "hits": hits[..n],
            "facets": ranked.facets,
            "next_cursor": next,
            "truncated": truncated,
            "semantic_coverage": coverage,
            "degraded": ranked.degraded,
            "as_of": rfc3339(plan.as_of),
        });
        if plan.scope.tenant_scope {
            let failed = fan.failed_ids();
            b["partial"] = Value::Bool(!failed.is_empty());
            b["failed_identities"] = json!(failed);
        }
        b
    };
    let key = keyring::current(fan.p, Purpose::Cursor).await?;
    let next_after = |n: usize| -> Option<String> {
        let last = *keys.get(n.checked_sub(1)?)?;
        Some(next_cursor(
            plan,
            &items,
            &keys[..n],
            last,
            &key.kid,
            &key.secret,
            fan.p.clock().now_ms(),
        ))
    };
    let full = body(
        hits.len(),
        &hits,
        if more { next_after(hits.len()) } else { None },
        false,
    );
    if serde_json::to_vec(&full).map_or(0, |v| v.len()) <= BYTE_CAP {
        return Ok(full);
    }
    // The byte cap: the largest prefix that fits, continuing right after it.
    let fits = |n: usize, hits: &[Value]| {
        serde_json::to_vec(&body(n, hits, next_after(n), true)).map_or(usize::MAX, |v| v.len())
            <= BYTE_CAP
    };
    let (mut lo, mut hi) = (0usize, hits.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid, &hits) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 && !hits.is_empty() {
        let mut one = hits[..1].to_vec();
        let cut: String = one[0]["snippet"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(40)
            .collect();
        one[0]["snippet"] = Value::String(cut);
        return Ok(body(1, &one, next_after(1), true));
    }
    Ok(body(lo, &hits, next_after(lo), true))
}

/// The cursor after a page whose keys are `page` and whose last key is `last`.
fn next_cursor(
    plan: &Plan,
    all: &[Item],
    page: &[Boundary],
    last: Boundary,
    kid: &str,
    secret: &[u8; 32],
    now: i64,
) -> String {
    let eps = plan.eps();
    let band = |k: &Boundary| k.score_q <= last.score_q.saturating_add(eps) && k.ord != last.ord;
    let prev: &[u64] = plan.cursor.as_ref().map_or(&[], |c| c.seen.as_slice());
    let mut seen: Vec<(u32, u64)> = all
        .iter()
        .map(Item::key)
        .filter(|k| band(k) && prev.contains(&k.ord))
        .chain(page.iter().copied().filter(band))
        .map(|k| (k.score_q.abs_diff(last.score_q), k.ord))
        .collect();
    seen.sort_unstable();
    seen.dedup_by_key(|s| s.1);
    seen.truncate(scursor::MAX_SEEN);
    let c = CursorV1 {
        v: 1,
        as_of: plan.as_of,
        issued_at: now,
        qh: plan.qh.clone(),
        last,
        seen: seen.into_iter().map(|s| s.1).collect(),
    };
    scursor::encode(kid, secret, &c)
}

/// The mailbox's reasons, then `fuzzy:`, `semantic:` and `rerank:`, `attachment_text_unavailable` last;
/// at most 8.
fn why(item: &Item, d: &Detail, plan: &Plan) -> Vec<String> {
    const UNAVAILABLE: &str = "attachment_text_unavailable";
    let mut out: Vec<String> = d
        .why
        .iter()
        .filter(|w| *w != UNAVAILABLE)
        .cloned()
        .collect();
    if item.c.fuzzy {
        for (t, ..) in &plan.wire.terms {
            out.push(format!("fuzzy:\"{t}\""));
        }
    }
    if let Some(c) = item.cosine {
        out.push(format!("semantic:{c:.2}"));
    }
    if let Some(r) = item.rerank {
        out.push(format!("rerank:{r:.2}"));
    }
    let mut dedup = Vec::new();
    for w in out {
        if !dedup.contains(&w) {
            dedup.push(w);
        }
    }
    if d.why.iter().any(|w| w == UNAVAILABLE) {
        dedup.truncate(MAX_WHY - 1);
        dedup.push(UNAVAILABLE.to_owned());
    }
    dedup.truncate(MAX_WHY);
    dedup
}

pub(crate) fn message_hit(idn: &idb::Row, item: &Item, d: Detail, why: Vec<String>) -> Value {
    let c = &item.c;
    json!({
        "message_id": c.id,
        "thread_id": c.thread_id,
        "identity_id": idn.id,
        "date": rfc3339(c.sent_at.unwrap_or(c.received_at)),
        "direction": c.direction,
        "from": c.from_address.as_ref().map(|a| json!({ "address": a, "name": c.from_name.clone().unwrap_or_default() })),
        "subject": c.subject,
        "snippet": d.snippet,
        "score": round3(item.score.clamp(0.0, 1.0)),
        "why": why,
        "attachment_hits": d.attachment_hits,
        "trust": {
            "verdict": c.verdict.as_deref().unwrap_or("none"),
            "known_sender": c.known_sender == Some(1),
            "quarantined": c.status == "quarantined",
        },
    })
}

fn thread_hit(
    plan: &Plan,
    idn: &idb::Row,
    item: &Item,
    d: &Detail,
    why: Vec<String>,
    t: Option<&Value>,
) -> Value {
    let empty = json!({});
    let t = t.unwrap_or(&empty);
    let participants: Vec<Value> = t["participants_json"]
        .as_str()
        .and_then(|p| serde_json::from_str::<Vec<Value>>(p).ok())
        .unwrap_or_default()
        .into_iter()
        .take(50)
        .map(|p| json!({ "address": p["address"], "name": p["name"].as_str().unwrap_or("") }))
        .collect();
    let mut h = json!({
        "thread_id": item.c.thread_id,
        "subject": t["subject"].as_str().unwrap_or(""),
        "participants": participants,
        "message_count": t["message_count"].as_i64().unwrap_or(1),
        "last_at": rfc3339(t["last_at"].as_i64().unwrap_or(item.c.received_at)),
        "snippet": d.snippet,
        "why": why,
        "top_message_id": item.c.id,
    });
    if plan.scope.tenant_scope {
        h["identity_id"] = Value::String(idn.id.clone());
    }
    h
}
