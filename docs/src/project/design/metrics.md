# Deliverability metrics

Binding for implementation. Two read-only routes tell an agent, an operator or an integrator how close a
workspace is to the automatic sending pauses, and what its mail has been doing. `GET /v1/metrics/rates`
returns **exactly the windows the pauses use**, with each window's threshold, its source, the current
counts and the headroom ("pauses above 0.3% complaints over the last 1,000 outcomes; now 0.1%; 3 more
complaints pause it"), computed by `TenantQuota` from the same rows and with the same function that
decides a pause, so the figure shown and the decision taken can never disagree. `GET /v1/metrics/events`
returns counts of message events per hour or per day for an identity, a tenant or a domain, from counters
each mailbox writes in the transaction that appends the event, so they equal the events webhooks deliver.
Neither route repeats `GET /v1/usage` (plan allowances) or `GET /v1/usage/daily` (metered units per day),
which stay as they are ([§2](#2-what-the-routes-are-for-and-how-they-relate-to-usage)).

| | |
|---|---|
| Requirements | FR-MET-1 to FR-MET-10 (this page), FR-DLV-3, FR-DLV-6, FR-BILL-11 |
| Edge cases | [Y1](../edge-cases.md)–[Y19](../edge-cases.md) (section Y continues from Y20 with [SDKs, skills and agent tooling](sdk-and-skills.md)) |
| Code | `crates/core/src/abuse.rs` (the window catalogue, `evaluate`, `headroom`, thresholds in parts per million; pure), `crates/core/src/metrics.rs` (event-count types, bucket and range arithmetic; pure), `crates/worker/src/metrics/{mod.rs, rates.rs, events.rs}`, `handlers/metrics.rs`, `quota/rates.rs` (`QuotaRequest::GetRates`), the `RecordOutcome` threshold argument in `consumers/delivery.rs` and `quota/outcomes.rs`, `mailbox/metric_counts.rs` (the counter writes beside every outbox append, the read, the pruning in `alarm:maintenance`), `console/pages/deliverability.rs`, `mcp/tools.rs`, `crates/cli/src/commands/metrics.rs` |
| Tables | Mailbox `metric_counts` (new). `TenantQuota` unchanged: `outcomes`, `counters` and `outcome_events` are read as they are. D1: none ([Data model](data-model.md)) |
| Contracts | `GET /v1/metrics/rates` and `GET /v1/metrics/events` ([REST API](../../reference/api.md)); the permission `metrics:read`; `QuotaRequest::GetRates` and the `thresholds` field of `QuotaRequest::RecordOutcome`; `400 query_range_too_wide` and `422 scope_too_large` ([Errors](../../reference/errors.md)); the MCP tools `mail_get_rates` and `mail_get_event_counts`; no event, no policy field |
| Limits | [Limits › Deliverability metrics](../../reference/limits.md#deliverability-metrics): hourly buckets for 192 hours and at most 168 per request; daily buckets for 92 days and at most 92 per request; at most 100 identities per fan-out |
| Built in | [M35](../build-plan.md#m35--deliverability-metrics-after-m9-m13-m14-m15-m16-and-m21) |

## 1. What exists today

- **The pauses** ([Outbound › Abuse auto-pause](outbound.md#abuse-auto-pause-fr-dlv-3)). `TenantQuota`
  keeps each identity's last 1,000 outcomes (`outcomes`), the tenant's outcome counters per UTC day, and
  each sending domain's outcome counters per UTC hour (`counters`). `RecordOutcome` updates them for
  every counted delivery outcome and decides: the identity pause (FR-DLV-3), the tenant and domain pauses
  over 7 UTC days (FR-DLV-6, [G12](../edge-cases.md)), the burst pause of the tenant, and the two operator
  alerts per domain. A first reply to a stranger is recorded in `outcome_events` only and counts toward
  nothing ([G13](../edge-cases.md)). The rates exist only inside that object: an agent learns about them
  when a send fails with `409 identity_paused` or `409 sending_paused`.
- **Usage** ([REST API › Usage and audit](../../reference/api.md#usage-and-audit)).
  `GET /v1/usage` returns the plan and the state of each allowance; `GET /v1/usage/daily` returns metered
  units per UTC day for a tenant (`inbound`, `outbound`, `sends`, `triage`, `search`, `agentic`,
  `assertions`, `http_signatures`, `ai_neurons`, `storage_bytes`), from `usage_daily`.
- **Operator metrics** ([Observability § 3](observability.md#3-metrics)). Analytics Engine, dataset
  `pylota_mail_metrics`, sampled per tenant, with no identity labels; read by the operator through the
  Analytics SQL API, never by tenants.
- **Domain health** ([Identities and domains](identity-domains.md)): DNS and authentication state per
  domain, `GET /v1/domains/{domain_id}`. Not a rate.

What is missing is a tenant-facing view of the rates before a pause, and per-hour activity counts by
identity and domain. AgentMail offers `GET /v0/metrics/events` (counts per event type in buckets),
`/metrics/usage` and bounce and complaint rates, at organisation, pod and inbox scope.

## 2. What the routes are for, and how they relate to usage

| Question | Route | Source | Exact? |
|---|---|---|---|
| How close am I to being paused, and by what rule? | `GET /v1/metrics/rates` | `TenantQuota` (the pause state) and D1 (who is paused now) | Yes: the same rows and function as the decision |
| What did my mail do, per hour or day, per identity or domain? | `GET /v1/metrics/events` | Mailbox `metric_counts` | Yes: written with each event |
| What does my plan allow, and how much is used? | `GET /v1/usage` (unchanged) | `TenantQuota` allowances | Yes |
| What metered units did I use per day? | `GET /v1/usage/daily` (unchanged) | D1 `usage_daily` | Yes |
| How is the deployment doing? | Operator dashboards (unchanged) | Analytics Engine | Sampled |

The routes do not overlap. `usage/daily` counts what is billed (`sends` are accepted recipients against
the allowance; `inbound` and `outbound` are metered messages), per tenant and per day; `metrics/events`
counts what happened to messages (sent, delivered, bounced, …), per identity, domain or tenant, per hour
or day. AgentMail's `/metrics/usage` series (storage bytes, inbox and domain counts) map to `/v1/usage`
(`storage_gb`, `inboxes`, `custom_domains`) and `/v1/usage/daily` (`storage_bytes` per day); no new usage
route is added, and the [REST API](../../reference/api.md) cross-links the three.

**Why not Analytics Engine for the counts.** Analytics Engine can sample data points per index, both
when they are written and when they are queried, and each returned row carries `_sample_interval` for
weighting ([Analytics Engine SQL API](https://developers.cloudflare.com/analytics/analytics-engine/sql-api/),
updated 2026-10-02, read 2026-10-17); results of the Analytics SQL API datasets are adaptively sampled with
the weights applied (Analytics SQL API datasets page, read 2026-10-17). It keeps data for three months,
and a data point has one index of at most 96 bytes, 20 blobs and 20 doubles
([Analytics Engine limits](https://developers.cloudflare.com/analytics/analytics-engine/limits/),
updated 2026-04-23, read 2026-10-17). So it is unfit here:

- **Sampling.** A sampled count next to an exact pause threshold is wrong exactly when it matters: with
  1,000 outcomes and a pause at more than 3 complaints, a weighted estimate of "2.6" or "4.1" is not an
  answer. The pauses themselves never read Analytics Engine.
- **Identity scope.** The dataset's sampling key is `tenant_id` and identity IDs are never labels
  ([Observability § 3.1](observability.md#31-data-point-layout)); adding them would break the
  cardinality rule the operator metrics depend on.
- **Access.** Reading it needs the account-level Analytics SQL API; tenants must never get a path to
  another tenant's points.

**Why not the event feed.** The per-tenant event feed ([Event feed and live stream](event-feed.md)) holds
the same events, but it is fed by the webhook consumer after the mailbox commits, so a count derived from
it lags the mail and depends on the webhook pipeline being healthy, and its retention follows
`retention.events_days`. A counter written in the mailbox transaction that appends the event is exact
and immediate, and costs two small upserts.

**Why not `TenantQuota` for the counts.** It is one object per tenant on the hot path of every send's
reservation. Inbound mail never touches it today; counting every inbound event there would put inbound
volume on that object. The mailbox already runs the transaction that appends each event.

## 3. Rates

### 3.1 The windows

Every rule that can pause sending, or alert the operator, is one **window**. The catalogue is a constant
in `core::abuse`; `RecordOutcome` and `GetRates` both iterate it, so a window cannot exist in one and not
the other.

| `id` | Scope listed at | Counts | Window | Rule | Threshold (`threshold_source`) | Minimum outcomes | `action` |
|---|---|---|---|---|---|---|---|
| `identity_complaints` | identity | complained outcomes | the identity's last 1,000 outcomes; denominator always 1,000 | `gt` | `policy.abuse.complaint_rate_pause` (default 0.003) | – | `pause_identity` |
| `identity_bounces` | identity | bounced outcomes | the identity's last 200 outcomes; denominator always 200 | `gt` | `policy.abuse.bounce_rate_pause` (default 0.05) | – | `pause_identity` |
| `tenant_complaints_7d` | tenant | complained / outcomes | the 7 UTC days ending with the current day | `ge` | `policy.abuse.tenant_complaint_rate_pause` (default 0.001) | 500 | `pause_tenant` (`abuse_threshold`) |
| `tenant_bounces_7d` | tenant | bounced / outcomes | as above | `ge` | `policy.abuse.tenant_bounce_rate_pause` (default 0.05) | 500 | `pause_tenant` (`abuse_threshold`) |
| `domain_complaints_7d` | domain (tenant domains only) | complained / outcomes through the domain | the hourly windows of the 7 UTC days ending with the current day | `ge` | `policy.abuse.tenant_complaint_rate_pause` | 500 | `pause_domain` |
| `domain_bounces_7d` | domain (tenant domains only) | bounced / outcomes through the domain | as above | `ge` | `policy.abuse.tenant_bounce_rate_pause` | 500 | `pause_domain` |
| `burst_bounces_1h` | domain; also tenant, once per domain | bounced / outcomes through the domain | the current UTC hour | `gt` | 0.10 (`fixed`) | 200 | `pause_tenant` (`abuse_burst`) |
| `burst_complaints_24h` | domain; also tenant, once per domain | complained / outcomes through the domain | the 24 UTC hours ending with the current hour | `gt` | 0.006 (`fixed`) | 1,000 | `pause_tenant` (`abuse_burst`) |
| `alert_bounces_1h` | domain | bounced / outcomes through the domain | the current UTC hour | `gt` | 0.02 (`fixed`) | 50 | `operator_alert` (`bounce_rate:{tenant_id}:{domain_id}`) |
| `alert_complaints_24h` | domain | complained / outcomes through the domain | the 24 UTC hours ending with the current hour | `gt` | 0.001 (`fixed`) | 200 | `operator_alert` (`complaint_rate:{tenant_id}:{domain_id}`) |

The thresholds, minimums and comparisons are those of [Outbound](outbound.md#abuse-auto-pause-fr-dlv-3)
and [Observability § 5.3](observability.md#53-alert-list). Two things the existing text leaves open are
pinned here, and the same pins apply to `RecordOutcome` ([Outbound](outbound.md)):

- **Boundaries.** Every window is relative to the UTC time at which it is evaluated (`now`, read from
  `TenantQuota`'s `platform::Clock` when the request runs, for `RecordOutcome` and `GetRates` alike; no
  caller passes it), not to the outcome's `at`. "The current UTC hour" is the hourly counter window whose
  key is `now`'s hour; "the 24 UTC hours" are that window and the 23 before it; "the 7 UTC days" are
  `now`'s day and the 6 before it (for the domain, the hourly windows whose date is one of those days).
  An outcome is still counted in the window of its own `at`, as today; a late event therefore counts in
  an earlier window, never in a later one.
- **Arithmetic.** Thresholds are held in parts per million (`u32`; 0.003 is 3,000) and every comparison
  is done in integers: `gt` is `events × 1,000,000 > threshold_ppm × denominator`, `ge` is `≥`. A policy
  value is converted once, rounded to the nearest part per million. No floating-point value decides a
  pause, so the JSON a client reads and the decision cannot drift apart.

**Where each window is listed.**

- **Identity scope:** `identity_complaints` and `identity_bounces`.
- **Tenant scope:** `tenant_complaints_7d` and `tenant_bounces_7d`, and `burst_bounces_1h` and
  `burst_complaints_24h` once for each domain (the platform domain included) that has outcomes in the
  last 24 UTC hours of this tenant's counters, each with its `domain_id`: the burst rule pauses the
  tenant, so a tenant view must show it. For an identity key, the burst windows of a domain none of its
  addresses use carry `domain_id: null`: the identity still sees how close the tenant pause is, and the
  rates routes never name a domain they would answer `404` for at domain scope (§6).
- **Domain scope:** for a tenant domain, all six domain windows; for the platform domain, the burst and
  alert windows only (the platform domain is never domain-paused, and its counters in this tenant's
  `TenantQuota` hold only this tenant's outcomes, so they reveal nothing about other tenants).

### 3.2 `GET /v1/metrics/rates`

Query: `scope` (`identity`, `tenant` or `domain`; default `identity` for an identity key, `tenant`
otherwise), `identity_id` (identity scope; an identity key may omit it), `domain_id` (domain scope),
`tenant_id` (required for platform and partner keys, as for `GET /v1/usage`; ignored otherwise).

```json
{
  "scope": "identity",
  "tenant_id": "ten_01J9Z4K8V4QW7X2M5N6P8R0T1Y",
  "identity_id": "idn_01J9Z4M2P6R8T0V2X4Z6B8D0F2",
  "domain_id": null,
  "evaluated_at": "2026-10-17T09:12:44Z",
  "exempt": false,
  "sending": {
    "paused": false,
    "stops": []
  },
  "windows": [
    {
      "id": "identity_complaints",
      "metric": "complaint_rate",
      "action": "pause_identity",
      "domain_id": null,
      "window": { "kind": "last_outcomes", "size": 1000, "from": null, "to": null },
      "comparison": "gt",
      "threshold": 0.003,
      "threshold_source": "policy.abuse.complaint_rate_pause",
      "min_outcomes": null,
      "outcomes": 640,
      "events": 1,
      "denominator": 1000,
      "rate": 0.001,
      "evaluated": true,
      "tripped": false,
      "headroom": 3,
      "summary": "Pauses this identity above 0.3% complaints over its last 1,000 outcomes. Now 0.1% (1 of 1,000; 640 outcomes recorded). 3 more complaints pause it."
    },
    {
      "id": "identity_bounces",
      "metric": "bounce_rate",
      "action": "pause_identity",
      "domain_id": null,
      "window": { "kind": "last_outcomes", "size": 200, "from": null, "to": null },
      "comparison": "gt",
      "threshold": 0.05,
      "threshold_source": "policy.abuse.bounce_rate_pause",
      "min_outcomes": null,
      "outcomes": 200,
      "events": 4,
      "denominator": 200,
      "rate": 0.02,
      "evaluated": true,
      "tripped": false,
      "headroom": 7,
      "summary": "Pauses this identity above 5% bounces over its last 200 outcomes. Now 2% (4 of 200). 7 more bounces pause it."
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `evaluated_at` | The `now` of the evaluation; every window is relative to it |
| `exempt` | `true` only for the system identity (`is_system = 1`), whose outcomes are recorded but never pause it, and which only platform keys can read. Its windows are returned with `tripped` computed and the summary "Exempt: never paused" |
| `sending.paused` | Whether this scope itself is paused now (identity `status = 'paused'`; tenant or domain `sending_paused_at` set), read from D1 in the same request |
| `sending.stops` | Every pause that stops sends from this scope now: `{ "scope", "id", "reason", "paused_at" }`. For an identity: its own pause, its tenant's, and that of each tenant domain one of its `active` or `retiring` addresses is on (sends from it stop at policy step 14a); for a domain: its own and its tenant's; for a tenant: its own |
| `window.kind` | `last_outcomes` (`size`), `utc_days` (`from`, `to`: the first and last day's start, RFC 3339) or `utc_hours` (`from`, `to`: the first and last hour's start) |
| `outcomes` | Outcomes in the window (for `last_outcomes`, rows present, at most `size`) |
| `events` | Outcomes of the window's kind (complaints or bounces) in the window |
| `denominator` | `size` for `last_outcomes` windows; `outcomes` for the others |
| `rate` | `events / denominator`, rounded to 6 decimal places for display; `0` when the denominator is 0. Decisions use `events` and `denominator` (§3.1) |
| `evaluated` | `false` while `outcomes` is below `min_outcomes`: the rule cannot trip yet |
| `tripped` | The rule's predicate is true at `evaluated_at`. A tripped window of a scope that is not yet paused means the next counted outcome is decided against it |
| `headroom` | The fewest further outcomes of the window's kind, recorded in a row with nothing else recorded, that would make the rule trip at `evaluated_at` (below); fewer further outcomes, of any kinds, cannot trip it at that instant; `0` when tripped; `null` when no number of them could trip it (a threshold of 1 with `gt`, or a threshold of 1 with `ge` once the window holds an outcome that is not of its kind) |
| `summary` | One plain-English sentence built from the fields above, for a model or a person |

**Headroom.** `core::abuse::headroom(window_state, rule)` returns the smallest `n ≥ 0` for which the
rule's own predicate is true after `n` simulated outcomes of the window's kind, found by a binary search
over `0..=bound` with the predicate itself, never with a separate formula. The predicate is monotone in
`n`: for a `last_outcomes` window the simulation drops the oldest rows as new ones arrive, exactly as
`RecordOutcome` does, so each step adds one event and removes at most one; for the other windows each step
adds one event and one outcome, which never lowers `events × 1,000,000 − threshold_ppm × outcomes` and never
undoes a minimum already met. The bound is:

- `size` for a `last_outcomes` window;
- for the other windows, `max(min_outcomes − outcomes, 0) + ⌊threshold_ppm × outcomes ÷ (1,000,000 −
  threshold_ppm)⌋ + 1` when `threshold_ppm` is below 1,000,000, and `max(min_outcomes − outcomes, 0)` when
  it is 1,000,000 (a threshold of 1); `min_outcomes` is 0 for a window without one.

Below 1,000,000 the predicate always holds at the bound. When it does not hold at the bound, no number of
outcomes can trip the rule and `headroom` is `null`: exactly a threshold of 1 with `gt`, or a threshold of
1 with `ge` on a window that holds an outcome not of its kind.

Headroom describes one instant, and at that instant it bounds any mix of outcomes: fewer than `headroom`
further outcomes, of any kinds, cannot trip the rule, because an outcome of the window's kind raises the
counted share at least as much as any other outcome. Between two reads it moves both ways:

- a delivered outcome never lowers it on a `last_outcomes` window or on a window that is `evaluated`;
- on a window below its minimum (`evaluated: false`), a delivered outcome counts toward the minimum and can
  lower it by one: a tenant with 100 outcomes and no complaint has headroom 400 against `ge` 0.001 and a
  minimum of 500, and after 399 deliveries it has headroom 1;
- an hour or day rolling out of a window removes its outcomes and can lower or raise it: when the hour or
  day that leaves held mostly deliveries, the same complaints become a larger share, and the 1-hour windows
  restart at every hour.

**Gating a batch.** Each recipient of a message adds at most one complaint and one hard bounce (its
terminal status changes only from `delivered` to `bounced` or `complained`, and from `bounced` to
`complained`, [Outbound](outbound.md#applying-an-event-to-a-message)), but it also adds other outcomes:
its delivery, and soft bounces (`other`). So, at one instant, a batch of `N` recipients:

- cannot trip a `last_outcomes` window, or a window that is `evaluated`, when `N` is smaller than its
  headroom, because the other outcomes never raise the share;
- is not bounded by the headroom of a window with `evaluated: false`, because the other outcomes count
  toward its minimum; such a window cannot trip while `(events + N) × 1,000,000` is below
  `threshold_ppm × min_outcomes` (`gt`: not above it), since a trip needs at least `min_outcomes` outcomes.

Both checks describe one instant, so a client reads the windows just before each batch (§11).

**Steps**, in order:

1. Authentication, `metrics:read`, level and owner checks ([§6](#6-security-and-visibility)). Scope
   parameters: a missing `identity_id` (non-identity key, identity scope) or `domain_id` (domain scope),
   or a parameter of another scope, is `400 invalid_request`.
2. Rate limit: `RL_API` (and `RL_TENANT` or `RL_PARTNER_API` as for every request).
3. D1, one batch (5-second deadline, `503 unavailable` on failure): the tenant's `policy_json`, its
   partner's ceilings and the deployment defaults (resolved by `core::policy::effective` into
   `AbuseThresholds`), `sending_paused_at` and `sending_pause_reason`; for identity scope the identity's
   `status`, `pause_reason`, `is_system`, and the ID and `sending_paused_at` of each tenant domain its
   `active` and `retiring` addresses are on; for domain scope the domain's `kind`, `sending_paused_at`;
   for tenant scope the tenant's domain IDs (to label burst windows) and, for an identity key, the
   domains of its addresses.
4. `QuotaRequest::GetRates { scope, thresholds }` on the tenant's `TenantQuota` (5-second deadline;
   `503 unavailable` on failure). It writes nothing.
5. Render: windows in catalogue order, `sending` from step 3.

### 3.3 One source of truth

`GetRates` is a new `QuotaRequest` variant:

```rust
// crates/worker/src/quota/mod.rs (QuotaRequest)
GetRates { scope: RateScope, thresholds: AbuseThresholds },   // reads only → RatesAnswer; now: the object's clock

// crates/core/src/abuse.rs
pub enum RateScope { Identity { identity_id: String }, Tenant, Domain { domain_id: String, platform: bool } }
pub struct AbuseThresholds {                    // effective values, parts per million
    pub complaint_rate_pause_ppm: u32, pub bounce_rate_pause_ppm: u32,
    pub tenant_complaint_rate_pause_ppm: u32, pub tenant_bounce_rate_pause_ppm: u32,
}
pub struct WindowState { pub id: WindowId, pub domain_id: Option<String>, pub outcomes: u64,
                         pub events: u64, pub denominator: u64, pub evicting: Vec<bool> /* last_outcomes only */ }
pub struct WindowEval { pub state: WindowState, pub rule: Rule, pub evaluated: bool,
                        pub tripped: bool, pub headroom: Option<u64> }
pub fn window_states(store: &impl OutcomeReader, scope: &RateScope, now: i64) -> Vec<WindowState>;
pub fn evaluate(states: &[WindowState], t: &AbuseThresholds) -> Vec<WindowEval>;
pub fn decide(evals: &[WindowEval]) -> OutcomeDecision;   // what RecordOutcome returns
pub struct RatesAnswer { pub evaluated_at: i64, pub windows: Vec<WindowEval> }   // evaluated_at: the now used
```

`RecordOutcome` becomes: record the outcome (unchanged), then
`decide(&evaluate(&window_states(…, now), &thresholds))` over the identity scope, the tenant scope and the
outcome's domain scope. `GetRates` is `evaluate(&window_states(…, now), &thresholds)` for the requested
scope. Both read the same tables (`outcomes`, `counters`) through the same `OutcomeReader`, in the same
object, with the same function, and both take `now` from the object's `platform::Clock` when the request
runs; `GetRates` returns it as `evaluated_at`. So, for any state:

- a window `GetRates` shows as `tripped` is one `decide` would act on at that instant, and the reverse;
- `headroom` complaints in a row, recorded through `RecordOutcome`, pause the scope (with no other
  outcome in between and no window rolling over), and one fewer does not, nor does any mix of fewer
  outcomes.

Both are tested as properties of `core::abuse` and end to end ([§14](#14-tests)).

**`RecordOutcome` gains its thresholds.** Today's `RecordOutcome { identity_id, domain_id, outcome, at,
event_id, counted }` names `policy.abuse.*` but carries no policy, and `TenantQuota` keeps none. It gains
`thresholds: AbuseThresholds`: the delivery consumer resolves them from the tenant's effective policy (a D1
read of the tenant's policy and its partner's ceilings, cached per isolate for 60 seconds, a Pylota Mail
choice) exactly as the rates handler does in step 3, with the same `core::policy::effective`. A policy change therefore applies to the next outcome
and to the next rates read alike (after at most the 60-second cache on the consumer side, which
[Y4](../edge-cases.md) states).

**The M5 interface.** [Outbound › TenantQuota](outbound.md#tenantquota) says the listing is "the frozen
interface the build plan's M5 stub declares". `GetRates` and the `thresholds` field are added to that
listing by M35; the sentence gains "except `GetRates` and `RecordOutcome.thresholds`, which M35 adds".

## 4. Event counts

### 4.1 What is counted

The mailbox increments a counter in the same `transaction_sync` that appends one of these events to its
outbox ([Design § 4, rule 4](index.md#4-durable-object-transactions)):

| Type | Counted when the mailbox appends | Unit |
|---|---|---|
| `received` | `message.received` without `reprocessed: true` | message |
| `quarantined` | `message.quarantined` without `reprocessed: true` | message |
| `sent` | `message.sent` | message |
| `delivered` | `message.delivered` | recipient |
| `deferred` | `message.deferred` | recipient event |
| `bounced` | `message.bounced` | recipient |
| `complained` | `message.complained` | recipient |
| `rejected` | `message.rejected` | message event |
| `failed` | `message.failed` | message |
| `uncertain` | `message.uncertain` | message |
| `suppressed` | `message.suppressed` | message |
| `canceled` | `message.canceled` | message |

A retried provider event that the mailbox deduplicates appends no event and counts nothing. Events not
listed (`message.released`, `message.triaged`, `message.reconciled`, the draft, screening and calendar
events of the other parity pages) are not counted; new types may be added within `/v1`, and clients must
ignore unknown keys. A message that leaves `screening` is counted when its `received` or `quarantined`
event is appended ([Screening hold](screening.md)).

**Domain of a count.** Each counter carries the DNS name of a domain (`''` when there is none):

- inbound types: the domain of the identity's address the message was routed to;
- `sent`, `delivered`, `deferred`, `bounced`, `complained`, `rejected`, `failed`, `uncertain`: the domain of
  the From address the message was sent with: the platform domain when the message has the flag
  `sent_via_fallback`, otherwise the domain of `from_address` (the same domain whose ID the delivery
  consumer passes to `RecordOutcome`);
- `suppressed` and `canceled` (never handed to a transport): the domain of `from_address` at submit.

**Counts and rates differ, by design.** `bounced` and `complained` here count every event, including
first replies to strangers and outcomes of the system identity, which the rate windows leave out
([G13](../edge-cases.md); Outbound › The system identity is exempt). Rates answer "how close is a pause";
event counts answer "what happened". The docs of both routes say so.

### 4.2 Storage

```sql
CREATE TABLE IF NOT EXISTS metric_counts (
  bucket TEXT NOT NULL,                  -- 'h:YYYY-MM-DDTHH' (UTC hour) or 'd:YYYY-MM-DD' (UTC day)
  type   TEXT NOT NULL CHECK (type IN ('received','quarantined','sent','delivered','deferred','bounced',
                                       'complained','rejected','failed','uncertain','suppressed','canceled')),
  domain TEXT NOT NULL DEFAULT '',       -- lower-case A-label DNS name; '' when none
  value  INTEGER NOT NULL,
  PRIMARY KEY (bucket, type, domain)
) WITHOUT ROWID;
```

Each counted event runs two statements in its transaction, for the hour and the day of the event's
`occurred_at`:

```sql
INSERT INTO metric_counts (bucket, type, domain, value) VALUES (?1, ?2, ?3, 1)
ON CONFLICT (bucket, type, domain) DO UPDATE SET value = value + 1;
```

The daily `alarm:maintenance` deletes `h:` rows older than 192 hours and `d:` rows older than 92 days.
The table holds at most 12 types × the identity's domains × (192 + 92) buckets: for an identity with three
domains, about 10,000 rows of a few dozen bytes. No row holds personal data: a bucket, an event type, one
of the tenant's own domains (or the platform domain) and a count. Every column has a writer (the outbox
append) and a reader (`MetricCounts`, below).

New `MailboxRequest` variant:

| Operation | Does |
|---|---|
| `MetricCounts { bucket: Hour \| Day, from: String, to: String, types: Vec<EventType>, domain: Option<String> }` | Sums `value` per bucket and type over `from ≤ bucket < to`, for one domain or all → `Vec<{ bucket, type, value }>` |

### 4.3 `GET /v1/metrics/events`

Query: `scope` (as for rates), `identity_id`, `domain_id`, `tenant_id` (as for rates), `bucket` (`hour` or
`day`, default `day`), `from` and `to` (RFC 3339; `from` is rounded down and `to` up to the bucket; `to`
is exclusive), `types` (comma-separated, default all twelve), `identity_ids` (tenant and domain scope: at
most 100, comma-separated).

Defaults: `bucket=hour` covers the 24 hours ending with the current hour; `bucket=day` the 30 days ending
with today.

```json
{
  "scope": "tenant",
  "tenant_id": "ten_01J9Z4K8V4QW7X2M5N6P8R0T1Y",
  "identity_id": null,
  "domain_id": null,
  "bucket": "day",
  "from": "2026-10-11T00:00:00Z",
  "to": "2026-10-18T00:00:00Z",
  "types": ["sent", "delivered", "bounced", "complained"],
  "identities": 4,
  "partial": false,
  "failed_identities": [],
  "data": [
    { "start": "2026-10-11T00:00:00Z", "counts": { "sent": 141, "delivered": 137, "bounced": 2, "complained": 0 } },
    { "start": "2026-10-12T00:00:00Z", "counts": { "sent": 0, "delivered": 0, "bounced": 0, "complained": 0 } }
  ],
  "totals": { "sent": 812, "delivered": 790, "bounced": 9, "complained": 1 }
}
```

`data` has one entry per bucket in the range, zeros included, oldest first. Steps, in order:

1. Authentication, `metrics:read`, level and owner checks (§6). An identity key may use only identity
   scope.
2. Rate limits: `RL_API` and `RL_SEARCH` (120 per 60 seconds per key), because tenant and domain scope
   fan out like a tenant search.
3. Range: `from < to` after rounding, else `400 invalid_request`. Hourly: at most 168 buckets, and `from`
   no earlier than the current hour minus 191 hours. Daily: at most 92 buckets, and `from` no earlier than
   today minus 91 days. Otherwise `400 query_range_too_wide` with `details.max_buckets`,
   `details.requested_buckets` and `details.earliest` (RFC 3339). A `to` in the future is allowed; its
   buckets are zero. An unknown `type` is `400 invalid_request`.
4. Identities: identity scope, the one identity. Tenant scope: as in
   [Search § 10](search.md#10-tenant-scope-fan-out) step 1 (`status IN ('active','paused') AND
   is_system = 0`, narrowed by `identity_ids`; a foreign ID in `identity_ids` is `404 identity_not_found`).
   Domain scope: the tenant's identities with at least one `addresses` row on the domain in any status
   (every identity, for the platform domain), with the same filter and narrowing. More than 100 → `422
   scope_too_large` (`details.identities`, and the fix "pass `identity_ids`, at most 100").
5. Fan-out: `MailboxRequest::MetricCounts` to each mailbox (with `domain` set for domain scope), at most
   20 in flight, each with a deadline of 2,000 ms from the start of the fan-out (a Pylota Mail choice:
   a counter read is one indexed query). An identity that errors or misses it is added to
   `failed_identities` and `partial` becomes `true`; the response is still `200`.
6. Sum per bucket and type; `totals` over the range.

## 5. Concurrency, consistency and failure handling

- **Rates are a snapshot of one object.** `GetRates` runs inside `TenantQuota` between other requests, so
  it sees a state `RecordOutcome` has fully committed; it never sees half an outcome. Two reads a second
  apart can differ because an outcome arrived or an hour rolled; `evaluated_at` says which instant a
  response describes.
- **Pauses are D1 facts.** A pause is applied by the consumer after `RecordOutcome` returns
  ([Outbound › Abuse auto-pause](outbound.md#abuse-auto-pause-fr-dlv-3)), so for a moment a window can
  read `tripped: true` while `sending.paused` is still `false`; the next read shows both. A resumed scope
  whose windows are still tripped is paused again by its next counted outcome, as today; the response
  shows `tripped: true` and `paused: false` in that state, which is the warning.
- **Event counts commit with the event.** A counter and its outbox row are one transaction: an event that
  is appended is counted, and a rolled-back one is not. A tenant total read during a fan-out mixes
  mailbox snapshots taken a few milliseconds apart; that is accepted, and the response is never cached.
- **Retries.** Both routes are `GET` and change nothing; a client retries freely.

| Failure | Behaviour |
|---|---|
| D1 unavailable or slow (rates step 3, events step 4) | `503 unavailable`, retryable |
| `TenantQuota` unavailable or past its 5-second deadline | `503 unavailable`; nothing is estimated |
| A mailbox errors or misses the 2,000 ms fan-out deadline (events) | `200` with that identity in `failed_identities` and `partial: true`; its late answer is discarded |
| Every mailbox of the fan-out fails | `200` with `partial: true`, every identity in `failed_identities` and zero counts. A client must check `partial` before trusting a zero |
| The counter upsert fails | It is inside the outbox transaction, so the whole transaction rolls back and the event's own retry path applies (the counter can never be missing for an appended event) |
| The pruning alarm fails | Rows are retried by the next daily maintenance; reads never return buckets outside the documented range, because the range check rejects them first |

## 6. Security and visibility

- **Permission.** `metrics:read`, a new permission every key level can hold
  ([REST API › Permissions](../../reference/api.md#permissions)). It is never implicit: unlike
  `usage:read`, a key created before M35 does not gain it, because rates and counts describe a whole
  workspace's activity.
- **Platform and partner keys** pass `tenant_id` (`400 invalid_request` without it, as for `GET /v1/usage`);
  a partner key reaches only its own tenants, and any other tenant answers `404 tenant_not_found`, as a
  missing one does.
- **Tenant keys** reach their own tenant, its identities and its domains, and the platform domain (whose
  counters in this tenant's `TenantQuota` and mailboxes hold only this tenant's mail).
- **Identity keys** reach:
  - rates at identity scope for their own identity, at tenant scope for their own tenant, and at domain
    scope for the platform domain and every domain one of their own addresses is on. The tenant and
    domain pauses stop the identity's own sends ([Outbound › Policy pipeline](outbound.md#policy-pipeline)
    steps 3a and 14a), so an agent must be able to see how close they are. These responses carry counts
    and rates only, never another identity's ID or mail, and at tenant scope a burst window of a domain
    none of their addresses use carries `domain_id: null` (§3.1);
  - event counts at identity scope for their own identity only. Tenant or domain scope gets
    `403 scope_denied` ([Security § 5.2](security.md#52-order-of-checks) step 3);
  - another identity's ID, or a domain none of their addresses use, answers `404 identity_not_found` or
    `404 domain_not_found`, as a missing one does.
- **The system identity** is readable only by platform keys (it lives in the default tenant, which no
  other key reaches), and is excluded from tenant-scope fan-out, as in tenant search.
- **Tenant states.** Reads work on a `suspended` tenant and for a `paused` identity: that is when an
  operator needs the rates most. On an `erasing` tenant, only platform and partner keys read, as for
  every read ([Security § 5.2](security.md#52-order-of-checks)).
- **Cross-tenant answers.** A foreign `tenant_id`, `identity_id`, `domain_id` or ID in `identity_ids`
  answers exactly as a missing one; the two routes join `it::security::cross_tenant_matrix`.
- **No content.** Neither route returns a subject, an address, a filename or any mail text; summaries are
  built from numbers and fixed strings.

## 7. Privacy

- **Rates** read `TenantQuota` state that already exists; nothing new is stored. Identity erasure's
  `ForgetIdentity` already deletes the identity's `outcomes`, so its identity windows disappear with it;
  the tenant and domain counters keep the outcomes, as today (they hold no identity).
- **Event counts** are aggregate: bucket, event type, a domain the tenant uses, and a number. They
  identify no message and no person, so message, thread and counterparty erasure leave them unchanged,
  as they leave the tenant's outcome counters ([Privacy § 6](privacy.md#6-erasure)). Identity erasure
  removes them with the mailbox (`WipeAll`, or the prefix delete); tenant erasure removes every mailbox.
  [Privacy § 11](privacy.md#11-what-remains-after-deletion) gains the row "Event counts per hour and day:
  kept 192 hours and 92 days, after message and counterparty erasure; numbers only".
- **Retention.** Counts follow their own pruning (192 hours hourly, 92 days daily), not
  `retention.events_days`: they are not event payloads, and a tenant that keeps payloads one day still
  needs its last week's bounce counts. A tenant with `retention.message_hours` (the identity field of
  [Parity additions](parity-additions.md)) keeps counts for the same 92 days.
- **Legal holds** do not apply: counts are not mail.
- **Subject-access export** leaves them out: they are not about a data subject.

## 8. Billing and limits

Reads are not metered and need no plan feature. Limits (Pylota Mail choices): hourly buckets kept
192 hours and at most 168 per request; daily buckets kept 92 days (the same reach as `GET /v1/usage/daily`)
and at most 92 per request; at most 100 identities per tenant or domain fan-out (as tenant search);
2,000 ms per mailbox; `RL_SEARCH` for events. Test tenants behave the same.

## 9. Events, console, MCP and CLI

**Events.** None. No new event type: a pause already shows as `identity.paused` (identity) or as the `409`
and the console (tenant and domain), and polling `rates` gives the warning before it. No notification kind
is added either; the console shows the windows and integrators poll.

**Console.** A new screen `/console/deliverability`, server-rendered with no script, for all roles:

- a banner per active pause (`sending.stops` of the tenant scope), with the resume rule ("a platform key
  resumes it");
- the tenant windows and, per domain, its windows: a table with the rule, the threshold, the current
  rate as "1 of 1,000" and as a percentage, and the headroom sentence; tripped rows first;
- per identity, its two identity windows, sorted by headroom, lowest first (identities are read with one
  `GetRates` per identity, at most 100, the first page of the inbox list);
- event counts per day for the last 30 days, tenant scope, as a table with a server-rendered SVG bar per
  row;
- the same identity windows on `/console/inboxes/{idn}`, and the domain windows on
  `/console/domains/{dom}`.

Rate and count values are numbers, so nothing on the screen is untrusted content.

**MCP.**

| Tool | Permission | REST | Rate limit |
|---|---|---|---|
| `mail_get_rates` | `metrics:read` | `GET /v1/metrics/rates` (`scope`, `identity`, `domain_id`) | – (`RL_API` only) |
| `mail_get_event_counts` | `metrics:read` | `GET /v1/metrics/events` (`scope`, `identity`, `domain_id`, `bucket`, `from`, `to`, `types`, `identity_ids`) | `RL_SEARCH` |

Both are read-only (`readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`,
`openWorldHint: false`). `mail_get_rates`'s description: "How close this identity, its workspace or a
domain is to an automatic sending pause: each rule, its threshold, the current rate and how many more
complaints or bounces would pause sending. Check it before sending to many recipients."
`identity` follows the MCP identity-argument rule ([MCP § 3](mcp.md#3-authentication-and-tool-filtering)).

**CLI.**

```text
pmail metrics rates [--identity ID | --domain ID] [--tenant ID] [--json]
pmail metrics events [--identity ID | --domain ID] [--tenant ID] [--bucket hour|day] [--from TIME]
                     [--to TIME] [--types sent,delivered,…] [--identity-ids ID,…] [--json]
```

`rates` prints one line per window (rule, threshold, now, headroom) with tripped windows first and exits
with status 16 (a new exit code; 3 means a configuration error) when any window is tripped or any `stops` entry exists, so a script can gate a bulk send on
it. `events` prints a table, one row per bucket, and a `partial` warning on stderr when the response is
partial. `--identity` and `--domain` send `scope=identity` and `scope=domain`; without either, the CLI sends
no `scope`, so the server's default applies (§3.2): an identity key reads its own identity, every other key
its tenant.

## 10. Observability

| Metric | Kind | Labels | Emitted by |
|---|---|---|---|
| `metrics_requests_total` | counter | route (`rates`, `events`), scope (`identity`, `tenant`, `domain`), status_class | `handlers/metrics.rs` |
| `metrics_ms` | observation | route | `handlers/metrics.rs` |
| `metrics_fanout_partial_total` | counter | – | `metrics/events.rs`, once per partial response |
| `rates_tripped_reads_total` | counter | action (`pause_identity`, `pause_tenant`, `pause_domain`, `operator_alert`) | `metrics/rates.rs`, per tripped window returned |

No new alert: the pauses and the domain alerts already page the operator
([Observability § 5.3](observability.md#53-alert-list)). The Outbound and delivery dashboard gains
`metrics_ms` p50/p95/p99 and the partial share.

## 11. Pylota agentic OS

**Today** (Pylota's integration spec, `docs/plans/pylota-mail-integration.md`, read-only): § 5.5's
monitoring computes bounce over 2% per hour and complaint over 0.1% per domain and per Pylota-owned
identity from Pylota's own `mail_outbound_messages`; § 5.7 and § 8.4 learn about a tenant or domain pause
only after it happens, from the synchronous `409` or from `mail-webhook-health` reading
`sending_paused_at` with K1 every 5 minutes. The shared identities `bookings`, `notices` and `reminders`
send for every operator, so one operator's complaints can pause them for all.

**With these routes:**

- **Reputation watch.** A new Pylota job, `mail-reputation-watch`, every 5 minutes with K1 (the partner
  key, `tenant_id` per call): `GET /v1/metrics/rates` at tenant scope for the platform, marketing and auth
  tenants, at domain scope for `notify.pylota.io`, `reminders.pylota.io` and `account.pylota.io`, and at
  identity scope for `bookings`, `notices`, `reminders`, `invites` and `waitlist`. It pages the owner when
  any pause window is `tripped`, when an `evaluated` window's rate is at least three quarters of its
  threshold, or when an identity window's `headroom` is 1; it opens a ticket at half the threshold.
  This replaces "detect after the pause" with "warn before it", and uses Cloud's own numbers instead of
  a recomputation that cannot see first-reply exclusions ([G13](../edge-cases.md)).
- **Per-operator dashboards.** Hourly, the same job reads `rates` at tenant scope and `events` at tenant
  scope (`bucket=day`, last 30 days) for each operator tenant, and stores the latest answer per tenant
  (`mail_tenant_health`: tenant, `evaluated_at`, the windows' `id`, `events`, `denominator`, `tripped`,
  `headroom`, and daily counts). Pylota's admin shows every operator; the operator app's "Email health"
  card shows its own tenant ("2 complaints this week; 3 more would pause your sending"). Nothing in it is
  mail content.
- **Agents.** The operator coordinator gets a read-only tool, `email_health`, answering "why did my email
  stop?" from the stored answer (or a live `rates` read with K2), with the pause rule and the resume path.
  The fleet-acquisition specialist (seller outreach, § 7.3 row 42) checks `rates` for its identity with
  K3 just before each batch; when the batch fails the batch check of [§3.2](#32-get-v1metricsrates) for
  either identity window (its headroom is not larger than the batch's recipient count: a batch of exactly
  `headroom` complaints would pause it), it sends nothing and asks for an operator's approval through the
  approval gate. It never gates on a stored answer, because headroom can fall between reads. The
  public-webchat coordinator never reads metrics.
- **Failures.** `503` or `partial: true` keep the previous stored answer and mark it stale after
  15 minutes; a `403 permission_denied` (a key minted before `metrics:read`) pages once and is fixed by
  re-minting the key.

**What Pylota's integration spec must add** (for its owner):

1. § 6: `metrics:read` on K1 (it must stay a superset of what it mints), K2 (operator dashboards and the
   coordinator's live read) and K3 (the acquisition check); not on K5–K7 (the watch job uses K1).
2. § 5.5 and § 8.4: the `mail-reputation-watch` job, its scopes, the paging rule above, and the
   note that Cloud's rate windows exclude first replies, so `mail_outbound_messages` figures differ.
3. § 5.7: "warn before the pause" for the shared identities and the platform tenant, with the reroute to
   the operator's own `bookings` identity started when headroom reaches the paging level, not only after
   the `409`.
4. § 4.6: the `mail_tenant_health` table (no mail content) and its retention (latest answer only).
5. § 7.3 row 42: the acquisition headroom check and its approval-gate fallback.
6. Tests: a tripped window pages; a stale answer is marked stale; the acquisition batch defers when
   headroom is not larger than the batch size, from a live read.

## 12. Functional requirements

To add to [PRD § 6](../prd.md) as a new group, "Deliverability metrics":

- **FR-MET-1** The service **must** return, for an identity, a tenant or a domain, every window that can
  pause that scope's sending (and, for a domain, the operator alert windows), each with its rule,
  threshold, threshold source, minimum outcomes, window bounds, outcomes, events, denominator, rate,
  whether it can trip yet, whether it is tripped, and the headroom in further outcomes of its kind.
- **FR-MET-2** Rates **must** be computed inside `TenantQuota` by the same function, over the same rows,
  that decides the pauses, with integer comparisons at one part per million, so that a window reported
  as tripped is exactly one the pause decision acts on, and the reported headroom is exactly the number
  of further outcomes of its kind that would trip it.
- **FR-MET-3** `RecordOutcome` **must** receive the tenant's effective abuse thresholds from its caller,
  and every window **must** be bounded by UTC hours or days relative to the evaluation time, the same in
  the decision and in the rates.
- **FR-MET-4** The rates response **must** state every pause in effect that stops the scope's sends, and
  **must** mark the system identity as exempt.
- **FR-MET-5** The service **must** count message events (received, quarantined, sent, delivered,
  deferred, bounced, complained, rejected, failed, uncertain, suppressed, canceled) per UTC hour and day,
  per identity and sending or receiving domain, in the same transaction that appends each event, and
  **must not** count reprocessed or deduplicated events.
- **FR-MET-6** `GET /v1/metrics/events` **must** return those counts at identity, tenant or domain scope,
  one entry per bucket including zeros, for at most 168 hourly buckets within the last 192 hours or at
  most 92 daily buckets within the last 92 days, refusing wider or older ranges with
  `400 query_range_too_wide`.
- **FR-MET-7** Tenant and domain event counts **must** fan out to at most 100 identities (narrowed by
  `identity_ids`, else `422 scope_too_large`) and **must** report `partial` and `failed_identities` when a
  mailbox does not answer in time.
- **FR-MET-8** Both routes **must** need the explicit permission `metrics:read`; an identity key **must**
  read rates only for its own identity, its tenant and the domains its addresses use, and event counts
  only for its own identity; a foreign ID **must** answer as a missing one.
- **FR-MET-9** Event counts **must** hold no personal data, survive message, thread and counterparty
  erasure, and be deleted with their identity and tenant.
- **FR-MET-10** The console **must** show the rates and headroom of the tenant, its domains and its
  identities, and daily event counts; the MCP server **must** offer `mail_get_rates` and
  `mail_get_event_counts`; the CLI **must** offer `pmail metrics rates|events`.

## 13. Edge cases

Section Y of the [edge-case register](../edge-cases.md), rows Y1–Y19 (Y20 onward belong to
[SDKs, skills and agent tooling](sdk-and-skills.md)):

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| Y1 | A young identity with fewer than 1,000 (or 200) recorded outcomes | The identity windows use the fixed denominators 1,000 and 200: one complaint in 40 outcomes is a rate of 0.1%, not 2.5%; `outcomes` shows the rows present; `headroom` is computed against the fixed denominator, and that many complaints through `RecordOutcome` pause the identity | S | `core::abuse::y_rates_match_decision`, `it::metrics::rates_identity_windows` |
| Y2 | A tenant or domain window below 500 outcomes (1,000 or 200 or 50 for the burst and alert windows) | `evaluated: false`, `tripped: false`; `headroom` counts the outcomes still needed, each simulated complaint or bounce also being an outcome, so a delivered outcome can lower it by one; fewer than `headroom` further outcomes, of any kinds, do not trip the rule | S | `core::abuse::y_rates_match_decision`, `core::abuse::headroom_properties`, `it::metrics::rates_tenant_and_domain` |
| Y3 | A rate exactly at a threshold: 1 complaint in 1,000 tenant outcomes (`ge` 0.001), 3 complaints in the identity's 1,000 (`gt` 0.003) | The tenant window is tripped and the identity window is not, in `GetRates` and in `RecordOutcome` alike, by integer comparison in parts per million | S | `core::abuse::y_rates_match_decision` |
| Y4 | The tenant's abuse policy is lowered (or raised within its ceiling) | The next rates read shows the new threshold and source; a window can read `tripped: true` while the scope is not paused; the next counted outcome is decided against the new value (after at most the consumer's 60-second policy cache) | S | `it::metrics::rates_policy_change` |
| Y5 | A scope is paused, then its windows fall back under the threshold (days roll out, or a platform key resumes it) | Paused and below: `sending.paused: true`, `tripped: false` (only a platform key resumes). Resumed and still above: `paused: false`, `tripped: true`, and the next counted outcome pauses it again | S | `it::metrics::rates_policy_change` |
| Y6 | Bounces and complaints of first replies to strangers ([G13](../edge-cases.md)) | Not in any rate window (they are recorded in `outcome_events` only); counted in event counts as `bounced` and `complained` | S | `it::metrics::y6_first_replies` |
| Y7 | A provider event retried, or a message reprocessed | Counted once in rates (`outcome_events`) and once in event counts (the mailbox appends no second event); `received` and `quarantined` with `reprocessed: true` are not counted | S | `it::metrics::y7_duplicates_and_reprocessed` |
| Y8 | The system identity | Readable only by platform keys; `exempt: true`; its outcomes are in its identity windows but not in the domain counters; excluded from tenant-scope fan-out | S | `it::metrics::rates_identity_windows` |
| Y9 | Domain scope on the platform domain | No 7-day domain windows (the platform domain is never domain-paused); burst and alert windows built from this tenant's own outcomes through it; event counts from every identity of the tenant | S | `it::metrics::rates_tenant_and_domain` |
| Y10 | A range wider than 168 hourly or 92 daily buckets, older than 192 hours or 92 days, with `from ≥ to`, unaligned times, or an unknown `type` | `400 query_range_too_wide` with `max_buckets`, `requested_buckets` and `earliest`; `from ≥ to` and unknown types `400 invalid_request`; unaligned times rounded to the bucket and echoed in `from` and `to` | S | `core::metrics::range_validation`, `it::metrics::y10_ranges` |
| Y11 | Tenant or domain scope over more than 100 identities without `identity_ids`, more than 100 IDs, or a foreign ID in `identity_ids` | `422 scope_too_large` with `details.identities`; a foreign or system-identity ID `404 identity_not_found`, as a missing one | S | `it::metrics::y11_fanout` |
| Y12 | A mailbox errors or misses the 2,000 ms deadline during the fan-out; every mailbox fails | `200`, that identity in `failed_identities`, `partial: true`, its late answer discarded; all failed: zero counts with `partial: true` | S | `it::metrics::y11_fanout` |
| Y13 | Identity keys: rates at identity, tenant and domain scope; events at tenant or domain scope; another identity; a domain none of its addresses use | Own identity, own tenant, the platform domain and its addresses' domains: `200`; at tenant scope, the burst windows of a domain none of its addresses use carry `domain_id: null`; at identity scope, `sending.stops` lists a pause of any domain its addresses are on. Events at tenant or domain scope: `403 scope_denied`. Another identity or domain: `404`, as missing | S | `it::metrics::scope_matrix` |
| Y14 | A key without `metrics:read`; a platform or partner key without `tenant_id`; a partner key naming another partner's tenant; a removed domain | `403 permission_denied` (`details.required: ["metrics:read"]`); `400 invalid_request`; `404 tenant_not_found`; `404 domain_not_found` | S | `it::metrics::scope_matrix` |
| Y15 | A read near an hour or day boundary; an outcome whose event arrives after its hour ended | Windows are relative to `evaluated_at`: at 10:00:00 UTC the current hour is the 10:00 window and the 1-hour windows restart; the late outcome counts in its own `at` hour, in rates and event counts alike, never in a later bucket | S | `core::abuse::window_boundaries`, `it::metrics::y15_boundaries` |
| Y16 | An identity is erased or deleted | Its identity windows and its event counts are gone (`404 identity_not_found`); tenant and domain rate windows keep its outcomes; tenant event totals no longer include it | S | `it::metrics::y16_erasure` |
| Y17 | Message, thread or counterparty erasure | Event counts and rate windows unchanged; no count identifies the erased mail | S | `it::metrics::y16_erasure` |
| Y18 | `TenantQuota` or D1 unavailable | `503 unavailable` for rates, retryable; events are not affected by `TenantQuota`; nothing is estimated or served from a cache | S | `it::metrics::y18_dependencies` |
| Y19 | A suspended tenant, a paused identity, an erasing tenant, a test tenant | Suspended and paused: reads work and `sending.stops` lists the pauses; erasing: platform and partner keys only; test tenants: the same behaviour | S | `it::metrics::y19_tenant_states` |

## 14. Tests

| Test | Proves | Covers |
|---|---|---|
| `core::abuse::y_rates_match_decision` | Property test over random outcome histories and thresholds: for every window, `tripped` from `evaluate` equals the predicate `decide` acts on; after `headroom` simulated outcomes of the window's kind through the same `RecordOutcome` code path the rule trips, and after `headroom − 1` it does not; integer comparison at the exact boundaries of Y3; fixed denominators of Y1; minimums of Y2 | [Y1](../edge-cases.md)–[Y3](../edge-cases.md), FR-MET-2 |
| `core::abuse::headroom_properties` | Property test over random window states, thresholds from 0 to 1 and both comparisons: the predicate holds at the §3.2 bound unless `headroom` is `null`, and `null` occurs exactly for a threshold of 1 with `gt`, or with `ge` on a window holding an outcome not of its kind; every mix of fewer than `headroom` further outcomes leaves the rule untripped; one delivered outcome never lowers headroom on a `last_outcomes` or `evaluated` window and lowers it by at most one below the minimum; a batch of `N` recipients, each adding up to one complaint, one hard bounce and any number of other outcomes, never trips a window that passes the batch check of §3.2; the worked example of §3.2 (100 outcomes, 399 deliveries, headroom 400 then 1) | [Y2](../edge-cases.md), FR-MET-2 |
| `core::abuse::window_boundaries` | The pinned windows: current hour, 24 hours, 7 UTC days, relative to `now`; a late outcome in an earlier window; the catalogue is the single list both callers iterate | [Y15](../edge-cases.md), FR-MET-3 |
| `core::metrics::range_validation` | Rounding of `from` and `to`, the bucket counts, the earliest allowed bucket, the defaults, `query_range_too_wide` details | [Y10](../edge-cases.md) |
| `it::metrics::rates_identity_windows` | Through the delivery consumer: three complaints in an identity's outcomes read `rate` 0.003, `tripped: false`, `headroom` 1; one more complaint pauses the identity and the next read shows `tripped: true`, `sending.paused: true` with `reason: "abuse_threshold"`; the system identity reads `exempt: true` with a platform key and is not paused | [Y1](../edge-cases.md), [Y8](../edge-cases.md) |
| `it::metrics::rates_tenant_and_domain` | Tenant windows under 500 outcomes are `evaluated: false`; at 500 with 1 complaint the tenant window is tripped; burst windows are listed per domain at tenant scope; the platform domain has no 7-day windows | [Y2](../edge-cases.md), [Y9](../edge-cases.md) |
| `it::metrics::rates_policy_change` | Lowering `abuse.tenant_complaint_rate_pause` changes the next read's threshold and source; a tripped, unpaused tenant is paused by the next counted outcome; a paused tenant whose 7 days roll under the threshold stays paused; after a platform resume the read shows `paused: false`, `tripped: true` | [Y4](../edge-cases.md), [Y5](../edge-cases.md) |
| `it::metrics::y6_first_replies` | A complaint on a first reply leaves every rate window unchanged and adds 1 to `complained` in event counts | [Y6](../edge-cases.md) |
| `it::metrics::y7_duplicates_and_reprocessed` | The same provider event delivered twice adds 1 to the rate window and 1 to `bounced`; a reprocessed inbound message adds nothing to `received` | [Y7](../edge-cases.md) |
| `it::metrics::events_counts_match_outbox` | For a scripted day of inbound and outbound mail across two identities and two domains, every count equals the number of matching events appended to the outboxes, per hour, day, identity and domain; a fallback send counts under the platform domain | FR-MET-5 |
| `it::metrics::y10_ranges` | Each invalid range gets the listed code and details; a valid 168-hour range returns 168 entries with zeros | [Y10](../edge-cases.md) |
| `it::metrics::y11_fanout` | 101 identities without `identity_ids`: `422`; with 100 IDs: `200`; a foreign ID: `404`; one mailbox delayed past 2,000 ms: `partial: true` with that ID in `failed_identities`; all failing: zero counts and `partial: true` | [Y11](../edge-cases.md), [Y12](../edge-cases.md) |
| `it::metrics::scope_matrix` | The two routes join `it::security::cross_tenant_matrix`; every cell of Y13 and Y14 | [Y13](../edge-cases.md), [Y14](../edge-cases.md), NFR-SEC-1 |
| `it::metrics::y15_boundaries` | With the clock at 09:59:59 and 10:00:00 UTC, the 1-hour windows and the hourly event buckets move as specified; a delivery event for 09:58 processed at 10:01 counts in the 09:00 bucket | [Y15](../edge-cases.md) |
| `it::metrics::y16_erasure` | After a counterparty erasure, counts and windows are unchanged; after identity erasure, its rates and events answer `404`, tenant windows keep its outcomes and tenant event totals drop by its counts | [Y16](../edge-cases.md), [Y17](../edge-cases.md) |
| `it::metrics::y18_dependencies` | Fault-injected `TenantQuota` and D1 failures give `503 unavailable`; events still answer while `TenantQuota` is down | [Y18](../edge-cases.md) |
| `it::metrics::y19_tenant_states` | Reads on a suspended tenant and a paused identity work and list the stops; on an erasing tenant a tenant key is refused and a partner key reads; a test tenant reads the same shapes | [Y19](../edge-cases.md) |
| `it::metrics::record_outcome_thresholds` | `RecordOutcome` receives `thresholds` resolved from the effective policy (tenant value, partner ceiling, deployment default), and a policy change applies to the next outcome after the cache window | FR-MET-3 |
| `it::console::deliverability_page` | All roles see the screen; pauses first, tripped rows first, headroom sentences; works with no script; the inbox and domain pages show their windows | §9 |
| `it::mcp::metrics_tools` | `mail_get_rates` and `mail_get_event_counts` are listed only for keys with `metrics:read`, map to their routes and return the same errors; an identity key asking for tenant-scope counts gets `scope_denied` | §9 |
| `cli::metrics::rates_and_events` | Against recorded fakes: `pmail metrics rates` prints tripped windows first and exits 16 when a window is tripped or a pause exists; `events` warns on partial; without `--identity` or `--domain` neither command sends `scope` | §9 |
| `live::metrics::bounce_moves_rates` | On staging: a bounce to a non-existent mailbox adds 1 to `bounced` in event counts and to `identity_bounces.events`, and the headroom drops by 1 | FR-MET-1, FR-MET-5 |

## 15. Milestone

### M35 · Deliverability metrics (after M9, M13, M14, M15, M16 and M21)

(In `build-plan.md` this heading is level 2.)

**Files:** `crates/core/src/abuse.rs` (the window catalogue, `evaluate`, `decide`, `headroom`, parts per
million; pure), `crates/core/src/metrics.rs`, `crates/worker/src/metrics/{mod.rs, rates.rs, events.rs}`,
`handlers/metrics.rs`, `quota/rates.rs`, `mailbox/metric_counts.rs`, `console/pages/deliverability.rs`,
`crates/cli/src/commands/metrics.rs`. Mailbox schema: `metric_counts`, in mailbox schema v1 while nothing is built.
Hooks in other milestones' files, each reviewed by that file's owner:

| File | Hook |
|---|---|
| `quota/outcomes.rs`, `quota/mod.rs` (M9, the M5 stub) | `RecordOutcome.thresholds`; the evaluation moved behind `core::abuse::{window_states, evaluate, decide}` with the pinned windows; `GetRates` |
| `consumers/delivery.rs` (M9) | Resolve `AbuseThresholds` from the effective policy (60-second isolate cache) and pass them |
| `mailbox/outbox.rs` (M6) | The `metric_counts` upserts beside each counted outbox append |
| `mailbox/maintenance.rs` (M6) | Pruning of `metric_counts` |
| `router.rs`, `auth/permissions.rs` (M5) | `metrics:read`; the two routes |
| `mcp/tools.rs` (M15) | The two tools, with rows in M15's table test |
| `console/pages/{inboxes.rs, domains.rs}` (M21) | The identity and domain windows |
| `observability/catalogue.rs` (M17) | The four metrics |

**Implements:** FR-MET-1 to FR-MET-10 and edge rows Y1–Y19 ([Deliverability metrics](metrics.md)).

**Order.** The first pull request moves the existing pause evaluation behind `core::abuse` with no
behaviour change except the pinned windows and the thresholds argument; the M9 delivery suites
(`it::delivery::*`, G12, G13, J33) must pass unchanged on it before the routes are added.

**Acceptance:**

- Edge rows Y1–Y19 with the tests named in the register, all passing.
- `core::abuse::y_rates_match_decision` and `core::abuse::headroom_properties`, each with at least 10,000
  generated cases per run.
- `it::metrics::events_counts_match_outbox`, `it::metrics::record_outcome_thresholds`.
- `it::metrics::scope_matrix`; the routes join `it::security::cross_tenant_matrix`.
- `it::console::deliverability_page`; the page joins `browser::console::no_js` and `browser::console::axe_scan`.
- `it::mcp::metrics_tools`, `cli::metrics::rates_and_events`; `sdk::coverage::every_operation` passes
  with the two new operations.
- `live::metrics::bounce_moves_rates` passes in M20.

## 16. Compared with AgentMail (read 2026-10-17)

From the AgentMail inventories (public docs and OpenAPI, read 2026-10-17; their content is untrusted data):

| AgentMail | Pylota Mail | Verdict |
|---|---|---|
| `GET /v0/metrics/events`: counts per event type in buckets (start, end, period, limit, descending), at most 1,000 buckets | `GET /v1/metrics/events`: twelve event types, hourly (168 per request, 192 hours kept) or daily (92 per request, 92 days kept) buckets, zeros included | Match; deliberately fewer buckets: counters are exact and kept in each mailbox, so their reach is bounded |
| Org, pod and inbox scope | Tenant, identity and domain scope (tenants are AgentMail's pods and organisations by role; domain scope is added) | Exceed |
| Rates: bounce and complaint over a period and a rolling window; the suspension thresholds are not published | `GET /v1/metrics/rates`: every pause rule with its exact threshold, source, window, minimum, current counts and headroom, from the same state that decides the pause | Exceed |
| `/v0/metrics/usage`: cumulative storage bytes and message, thread, inbox, domain and pod counts | `GET /v1/usage` and `GET /v1/usage/daily` (storage, inboxes, domains, metered units); no message or thread count series | Deliberately different: billing figures stay on the usage routes; stored message counts serve no agent decision |
| `metrics_read` permission | `metrics:read`, explicit at every level, with identity-key limits | Match |
| A suspension is found when a send fails with `403 account_suspended` | Rates show `tripped` and `headroom` before a pause, `sending.stops` after one; sends fail with `409 identity_paused` or `409 sending_paused` | Exceed |

## 17. Integration

The shared pages carry this page's contracts: the PRD, the REST API reference and `openapi.yaml`, the
errors, events, limits and configuration references, the data model, security, workspace policy and the
other design pages it extends, the MCP and CLI references, the edge-case register and the build plan
([ADR 0017 § Integration](../adr/0017-agentmail-parity.md#integration), integrated on 2026-10-18). A
difference between this page and one of them is a defect: fix both in the same change.
