# Observability and SLOs

Binding for implementation. This page defines what the deployment records about itself (logs, metrics,
alert state), how service-level objectives are measured, which alerts fire and how they reach a person,
the dead-letter queue consumers, `/health` and `idvy doctor`, and the runbooks. Nothing here may record
mail content or clear-text addresses ([Security › Logging rules](security.md#12-logging-rules)).

| | |
|---|---|
| Requirements | FR-OPS-3, FR-OPS-4, FR-PRV-6, FR-DLV-3, FR-DLV-5, FR-DOM-9, FR-IDN-6…8, FR-CON-14, FR-CON-15, FR-BILL-13, NFR-REL-1…4, NFR-PERF-1…6, NFR-PRV-1, NFR-OPS-2, NFR-OPS-3 |
| Edge cases | [D4](../edge-cases.md), [D5](../edge-cases.md), [G3](../edge-cases.md), [G8](../edge-cases.md), [I5](../edge-cases.md), [J4](../edge-cases.md), [J5](../edge-cases.md), [J6](../edge-cases.md), [J8](../edge-cases.md), [N1](../edge-cases.md), [N4](../edge-cases.md), [N10](../edge-cases.md), [O24](../edge-cases.md), [O25](../edge-cases.md), [I6](../edge-cases.md), [J30](../edge-cases.md)–[J33](../edge-cases.md) |
| Code | `crates/worker/src/log.rs`, `crates/worker/src/metrics.rs`, `crates/worker/src/ops/` (alert evaluator, alert email, containment rules, switches, freeze, restore, DLQ consumer), `crates/core/src/slo.rs` (alert rule evaluation, pure), `crates/cli/src/ops/` (`idvy ops`), `.github/workflows/heartbeat.yml` |

## 1. Signals

| Signal | Where | Retention | Holds |
|---|---|---|---|
| Structured logs | Workers Logs (one JSON object per `console.log` line) | 7 days (Cloudflare) | IDs, codes, counts, durations, pseudonyms (section 2) |
| Metrics | Workers Analytics Engine dataset `pylota_identity_metrics`, binding `METRICS` | 3 months (Cloudflare) | Counters and observations with low-cardinality labels (section 3) |
| Alert state | D1 `audit_log` rows `alert.fired` / `alert.resolved`, plus an `alert_fired` metric and, for page alerts, alert email (section 5.5) | Life of the deployment | Alert key, severity, values |
| Events | Webhooks (`domain.failing`, `quota.warning`, `webhook.disabled`, `erasure.failed`, `identity.paused`, …) | [Webhook events](../../reference/events.md) | Tenant-facing conditions |
| Traces | Workers traces | 7 days (Cloudflare) | Staging only (below) |
| Exact counters | D1 `usage_daily`, `TenantQuota` | [Privacy](privacy.md#2-data-inventory) | Usage and caps |

**Analytics Engine from Rust.** `workers-rs` 0.8.7 exposes the binding: `Env::analytics_engine(name)`
returns `AnalyticsEngineDataset`, written with `write_data_point(&AnalyticsEngineDataPoint)` or
`AnalyticsEngineDataPointBuilder::new().indexes(..).add_blob(..).add_double(..).write_to(&dataset)`
(docs.rs, worker 0.8.7, read 2026-10-09). Limits: 20 blobs, 20 doubles and one index per data point,
16 KB of blobs, an index of at most 96 bytes, 250 data points per invocation (Analytics Engine limits,
read 2026-10-09). `wrangler dev` does not write local data to Analytics Engine, so with
`PM_ENV = "local"` the metrics sink writes each data point as a log line instead (`event = "metric"`),
which the integration tests read.

**Required Worker configuration.** The generated `wrangler.toml` must contain:

```toml
[observability]
enabled = true
head_sampling_rate = 1

[observability.logs]
invocation_logs = false        # invocation logs record request URLs and the email recipient (FR-PRV-6)

[observability.traces]
enabled = false                # production; staging sets true with head_sampling_rate = 0.1

[[analytics_engine_datasets]]
binding = "METRICS"
dataset = "pylota_identity_metrics"
```

Cloudflare's invocation log for a `fetch` is the method and full URL, and for the email handler it is
the recipient address (Workers Logs docs, read 2026-10-09). URLs such as
`/v1/identities/lookup?address=…` contain addresses, so invocation logs are off. Automatic traces record
URLs and handler attributes the same way, so traces run only on staging, whose traffic is synthetic.
Workers Issues needs Wrangler 4.134.0 or later. The pinned 4.139.0 supports it, but v1 does not depend on it.

`METRICS` is listed in [Configuration › Bindings](../../reference/configuration.md#bindings) and in the
template in [Rust workspace](rust-workspace.md#8-generated-wranglertoml).

## 2. Structured logs

### 2.1 Schema

`worker::log` writes one JSON object per line. Only these fields exist; values are typed (section 12 of
[Security](security.md#12-logging-rules)):

| Field | Type | Present | Meaning |
|---|---|---|---|
| `ts` | RFC 3339, ms | always | Platform clock |
| `level` | `error` \| `warn` \| `info` \| `debug` | always | Filtered by `PM_LOG_LEVEL` |
| `event` | snake_case name | always | Section 2.2 |
| `env` | `production` \| `staging` \| `local` | always | `PM_ENV` |
| `version`, `commit` | string | always | Build metadata |
| `handler` | `fetch` \| `email` \| `queue` \| `scheduled` \| `alarm` \| `rpc` | always | Entry point |
| `request_id` | `req_…` | always | Generated per invocation ([Design conventions](index.md#3-time-randomness-and-ids)); returned in `Request-Id`; carried in RPC envelopes and queue bodies |
| `tenant_id`, `identity_id` | opaque ID | when known | `tenant_id` is the tenant pseudonym: slugs and names are never logged |
| `key_id` | `key_…` | authenticated requests | Never the key string |
| `route`, `method`, `status` | pattern, verb, int | `fetch` | The matched route **pattern**, never the raw path or query |
| `code` | error or reason code | on failure | `ErrorCode` or a reason from [Errors](../../reference/errors.md) |
| `queue`, `attempt` | name, int | queue handlers | |
| `message_id`, `thread_id`, `job_id`, `domain_id`, `event_id`, `delivery_id`, `export_id`, `erasure_id`, `user_id` | opaque IDs | when relevant | `user_id` is the person's `usr_` ID (console requests, notifications), never their address |
| `address_ph` | `ph_` + 16 hex | when an address must be correlated | Pseudonym of the address the event concerns (counterparty or envelope recipient): `HMAC-SHA256(PM_HASH_KEY, address)` truncated |
| `transport`, `provider_code`, `smtp_code` | short codes | outbound and delivery | `E_RATE_LIMIT_EXCEEDED`, `550`, `5.1.1`; never `smtpResponse` text |
| `mcp_method`, `tool` | JSON-RPC method, MCP tool name | MCP requests | Never the arguments ([MCP](mcp.md#7-limits-logging-and-safety)) |
| `client` | `ts_sdk` \| `rust_sdk` \| `cli` \| `mcp_bridge` \| `other` | `fetch` | From the user agent ([SDKs, skills and agent tooling § 16](sdk-and-skills.md#16-observability)) |
| `query_hash` | 16 hex | search requests and MCP search tools | `hex(HMAC-SHA256(PM_HASH_KEY, q))[..16]` ([MCP](mcp.md#7-limits-logging-and-safety)) |
| `count`, `bytes`, `duration_ms` | int | when relevant | |
| `detail` | `^[a-z0-9_.:-]{1,64}$` | optional | Machine string only |

```json
{"ts":"2026-10-09T10:12:03.412Z","level":"info","event":"inbound_accepted","env":"production",
 "version":"1.0.0","commit":"abc1234","handler":"email","request_id":"req_01J9Z5…",
 "tenant_id":"ten_01J9…","identity_id":"idn_01J9…","message_id":"msg_01J9…","bytes":48213,"duration_ms":41}
```

### 2.2 Event names

| Event | Level | Emitted when |
|---|---|---|
| `http_request` | info (5xx: error) | Every `fetch` response |
| `inbound_accepted`, `inbound_staged`, `inbound_rejected`, `inbound_tempfail` | info / warn | `email()` outcome; `inbound_rejected` carries the SMTP code |
| `inbound_processed` | info | `pm-inbound` consumer committed or deduplicated a message |
| `outbound_transport` | info / warn | A transport call and its classified outcome |
| `delivery_event`, `delivery_orphaned` | info / warn | A provider event applied or parked ([G8](../edge-cases.md)) |
| `webhook_attempt` | info / warn | One delivery attempt |
| `index_job`, `triage_result` | info / warn | Indexing and triage outcomes |
| `search_request`, `agentic_request` | info | Mode, scope, duration, status; the query only as `query_hash` |
| `job_step`, `job_failed` | info / error | JobRunner transitions |
| `domain_check`, `domain_transition` | info / warn | DomainMonitor |
| `dlq_item` | error | Dead-letter consumer recorded an item |
| `alert_fired`, `alert_resolved` | error / info | Alert evaluator transitions (the audit actions are `alert.fired` and `alert.resolved`) |
| `rpc_owner_mismatch` | error | Durable Object owner check failed ([Design conventions](index.md#5-internal-durable-object-rpc)) |
| `config_invalid` | error | Required variable or secret missing or malformed |
| `rpc_intent_stuck` | warn | A Durable Object call recorded in `rpc_intents` is still failing more than an hour after its D1 change: `tenant_id`, `identity_id`, `detail` = `init` or the event type, `code` ([Design conventions § 9](index.md#9-durable-object-calls-after-a-d1-change)) |
| `secrets_reseal_progress` | info | Master-key rotation sweep ([Security](security.md#62-rotation-procedures)) |
| `identity_key_changed` | info | An identity key was created (explicitly or on first signing), rotated or revoked: `identity_id`, `key_id` or `user_id` of the actor, `detail` = `create`, `rotate` or `revoke`; never key material ([Agent signing keys](agent-keys.md)) |
| `signature_minted` | info | An agent assertion or HTTP signature was minted: `identity_id`, `key_id`, `detail` = `assertion` or `http_signature`. Never the token, the signature, the audience, `ext`, the URL or the headers ([Security › Logging rules](security.md#12-logging-rules)) |
| `notification_sent`, `notification_failed`, `notification_deferred` | info / warn / info | The `Notifier` submitted a notification email, had it refused or saw it fail, or held an item back: `tenant_id`, `user_id`, `message_id` once known, `code` on failure, `detail` = the kind (`usage`, `new_mail`, `needs_person`, `account`) or the deferral reason. Never an address or the email's text ([Notifications](notifications.md)) |
| `notifier_handoff_failed` | warn | The `pm-webhooks` consumer could not hand a new-mail event to the tenant's Notifier; the delivery work is unaffected: `tenant_id`, `event_id`, `code` ([Webhooks](webhooks.md#handing-new-mail-to-the-notifier)) |
| `notification_unsubscribe` | info | `POST /console/notifications/unsubscribe`: `tenant_id` and `user_id` when the token verified, `detail` = the kind, `code` = `expired` or `invalid` otherwise; never the token |
| `usage_alert` | info | `TenantQuota` asked the Notifier for a usage alert: `tenant_id`, `detail` = `{feature}:{threshold}` |
| `calendar_parsed` | info | `pm-inbound` step 10a read a calendar object: method, `object_status`, `verified`, reason, part count, `tz_resolution` values, bytes. Never the `UID`, text or addresses ([Calendar invitations § 14](calendar-invitations.md#14-observability)) |
| `calendar_send_refused` | info | A send, reply or calendar reply with a calendar object was refused: method, error code, `details.reason` |
| `mcp_tools_corrupt` | warn | A key's stored `mcp_tools` list cannot be parsed: the key sees no MCP tools (fail closed) and REST is unaffected; `key_id` ([SDKs, skills and agent tooling § 10](sdk-and-skills.md#10-failure-handling)) |
| `docs_url_invalid` | warn | Once per isolate: `PM_DOCS_URL` is malformed, so the default is used for error `docs` links ([Parity additions § 10.1](parity-additions.md#101-the-field)) |
| `retention_expire_flush` | info | A mailbox with `retention.message_hours` wrote its daily expiry audit row ([Parity additions § 7](parity-additions.md#7-per-identity-message-retention-fr-prv-7)) |
| `label_event_trimmed` | info | An erasure removed a message from a pending label event ([Parity additions § 4](parity-additions.md#4-label-change-events-fr-wh-6)) |
| `panic` | error | Panic hook; source location only |
| `metric` | debug | `PM_ENV = "local"` only: a metrics data point |

## 3. Metrics

### 3.1 Data point layout

Every metric is written to `METRICS` with the same layout, so one SQL shape reads them all:

| Column | Holds |
|---|---|
| `index1` | `tenant_id`, or `platform` for deployment-level metrics. This is the sampling key |
| `blob1` | Metric name |
| `blob2` | `PM_ENV` |
| `blob3` … `blob8` | Labels, in the order listed for the metric in section 3.2 (unused positions empty) |
| `double1` | Value: the count for counters; the observed value for observations (milliseconds, bytes) |

- **Counters** are aggregated in memory per invocation by `(index1, name, labels)` and written once at
  the end of the invocation with `double1 = count`. **Observations** (durations, sizes) are one data
  point each. A per-invocation cap of 240 points protects the platform limit; the overflow is counted in
  `metrics_dropped_total`.
- Labels are low-cardinality codes. Message, thread and identity IDs are never labels; `domain_id` and
  `tenant_id` are, because their counts are bounded by configuration.
- Writes never fail a request: an error from the binding is logged once per isolate and ignored.

Reading (Analytics SQL API dataset `events.analyticsEngine."pylota_identity_metrics"`, account scope; sample
weights are applied to `COUNT`, `SUM` and `AVG` automatically, per the SQL API datasets page read
2026-10-09):

```sql
SELECT blob4 AS domain_id, SUM(double1) AS value
FROM events.analyticsEngine."pylota_identity_metrics"
WHERE accountTag = '<ACCOUNT_TAG>'
  AND timestamp >= NOW() - INTERVAL '1' HOUR
  AND blob1 = 'bounces_total' AND blob2 = 'production'
GROUP BY domain_id
```

### 3.2 Catalogue

| Metric | Kind | Labels (`blob3`, `blob4`, …) | Emitted by |
|---|---|---|---|
| `http_requests_total` | counter | route, method, status_class, code | `fetch` |
| `http_ms` | observation | route | `fetch` |
| `rate_limited_total` | counter | bucket | `fetch` |
| `inbound_received_total` | counter | result: `accepted`, `staged`, `rejected_unknown`, `rejected_retired`, `rejected_suspended`, `tempfail_suspended`, `tempfail_storage`, `tempfail_frozen` (a restore freeze; excluded from NFR-REL-2 like suspensions) | `email()` |
| `inbound_r2_retries_total` | counter | – | `email()` |
| `inbound_processed_total` | counter | outcome: `stored`, `deduplicated`, `quarantined`, `hidden`, `throttled`, `dsn_applied`, `parse_degraded`, `screening` (stored and held for screening, [Screening hold § 22](screening.md#22-observability)) | `pm-inbound` |
| `inbound_ingest_ms` | observation | – | `pm-inbound`: `received_at` → commit |
| `inbound_lost_total` | counter | – | Global retention `staging` step: a staging object still unrouted 14 days after it was written, a day before the lifecycle rule deletes it ([J7](../edge-cases.md)) |
| `inbound_raw_missing_total` | counter | – | `pm-inbound`: a pointer whose raw object is missing and whose message is not in the mailbox ([Inbound](inbound.md)) |
| `inbound_orphan_raw_total`, `inbound_staged_unroutable_total` | counter | – | `email()` and `pm-inbound` ([Inbound](inbound.md)) |
| `inbound_dropped_total` | counter | reason (`unknown_recipient`, `tenant_suspended`, `identity_gone`, `not_ses_domain`), source (`routing`, `ses`) | `email()`, `pm-inbound` ([Inbound](inbound.md)); on SES domains unknown recipients are dropped without a bounce ([Domains on any DNS host §4.6](domain-connections.md#46-retired-and-unknown-recipients)) |
| `ses_sns_rejected_total` | counter | endpoint (`delivery` for `/hooks/ses`, `inbound` for `/hooks/ses/inbound`), reason (`version`, `signature`, `cert_host`, `topic`, `timestamp`) | Both SNS endpoints and the SQS backstop: a message refused with `403 invalid_signature` ([N1](../edge-cases.md)) |
| `ses_auth_disagreement_total` | counter | check (`dkim`, `dmarc`) | `pm-inbound`: SES's verdict differs from our own check on the same message |
| `ses_object_lost_total` | counter | – | `pm-inbound`: an S3 object was missing while its `ses_ingest` row was still `queued` ([N4](../edge-cases.md)) |
| `scanner_error_total`, `auth_dns_cache_miss_total` | counter | – | `pm-inbound` ([Inbound](inbound.md)) |
| `backscatter_total` | counter | – | `pm-inbound` ([D4](../edge-cases.md)) |
| `inbound_throttled_total` | counter | cap (`per_sender`, `unauthenticated`, `per_tenant`) | mailbox ([D5](../edge-cases.md), [D13](../edge-cases.md)) |
| `thread_token_invalid_total`, `thread_token_previous_key_total`, `thread_token_unchecked_total` | counter | – | mailbox ([Threading](threading.md)) |
| `quarantine_total` | counter | reason (including `steering_suspected` and `screening_unavailable`) | mailbox |
| `send_api_requests_total` | counter | operation, result (`accepted`, `deduplicated`, or the error code) | `fetch` |
| `send_api_ms` | observation | operation | `fetch`, accepted sends only |
| `outbound_queue_to_transport_ms` | observation | transport | `pm-outbound`, first transport attempt |
| `transport_outcomes_total` | counter | transport (`cloudflare`, `ses`, `smtp`, `simulator`), outcome (`accepted`, `rejected`, `retry`, `uncertain`), provider_code (for `smtp`, the SMTP reply code) | `pm-outbound` |
| `recipients_submitted_total` | counter | transport, domain_id | `pm-outbound` |
| `bounces_total` | counter | domain_id, bounce_type | `pm-delivery-events` |
| `complaints_total` | counter | domain_id | `pm-delivery-events` |
| `delivery_events_total` | counter | transport, type | `pm-delivery-events` |
| `delivery_orphaned_total` | counter | – | `pm-delivery-events` ([G8](../edge-cases.md)) |
| `delivery_unknown_type_total`, `delivery_unroutable_total` | counter | – | `pm-delivery-events` ([Outbound](outbound.md)) |
| `reconcile_ambiguous_total` | counter | – | mailbox ([Outbound](outbound.md)) |
| `uncertain_total`, `reconciled_total` | counter | transport | mailbox |
| `fallback_sends_total` | counter | domain_id | mailbox |
| `suppressed_recipients_total` | counter | reason | mailbox |
| `provider_quota_errors_total` | counter | transport, provider_code | `pm-outbound` ([G3](../edge-cases.md)) |
| `backup_objects_total` | counter | result (`copied`, `skipped`, `error`, `revoked`: a copy deleted again because its source was gone from `BLOBS`) | JobRunner `backup` job ([Privacy](privacy.md#54-optional-r2-backup-copy)) |
| `alert_emails_total` | counter | severity, result (`sent`, `refused` with the submit's error code, `capped`) | Alert evaluator: alert email through the system identity (section 5.5) |
| `containment_total` | counter | rule | The code that applied an automatic containment rule (section 5.6) |
| `webhook_attempts_total` | counter | result (`succeeded` or the attempt's error code) | `pm-webhooks` ([Webhooks › Metrics](webhooks.md#metrics)) |
| `webhook_delivery_latency_ms` | observation | event_class (`inbound` for `message.received` and `message.quarantined`, `other`), first_attempt (`succeeded`, `failed`) | `pm-webhooks`: the event's `occurred_at` to the first successful attempt, written once per delivery |
| `webhook_dead_total` | counter | event_class | `pm-webhooks`: a delivery's 13th attempt failed (not written for `endpoint_disabled`) |
| `webhook_disabled_total` | counter | reason | `pm-webhooks` |
| `outbox_undispatched_age_ms` | observation | owner (`mailbox`, `domain`, `job`) | outbox dispatch |
| `webhook_ssrf_blocked_total` | counter | – | `pm-webhooks`, webhook create and update |
| `identity_keys_total` | counter | op (`create`, `rotate`, `revoke`) | `fetch` and console: the identity-key handlers; a key created lazily by a first signing request counts as `create` ([Agent signing keys](agent-keys.md#2-keys)) |
| `signatures_total` | counter | kind (`assertion`, `http_signature`), result (`ok`, or the error code, for example `rate_limited`, `identity_paused`, `policy_denied`, `web_bot_auth_disabled`) | `fetch`: `POST …/assertions` and `POST …/http-signatures` |
| `well_known_requests_total` | counter | endpoint (`identity_jwks`, `directory`), status_class | `fetch`: `GET /.well-known/jwks/{identity_id}.json` and `GET /.well-known/http-message-signatures-directory` |
| `notifications_sent_total` | counter | kind (`usage`, `new_mail`, `needs_person`, `account`, `digest`) | `Notifier`: an email accepted by the outbound pipeline (`202`) ([Notifications](notifications.md#8-notifier-object)) |
| `notifications_failed_total` | counter | kind, reason (the error code of a refused or unfinished submit, for example `unavailable` or `timeout`; or the message's `reason` when an accepted notification ends `failed` or `rejected`, for example `domain_failing_no_fallback`) | `Notifier`, for submits; the system identity's mailbox, for accepted notifications that end `failed` or `rejected`. A bounce or complaint is not counted here: it pauses the person's preferences ([O17](../edge-cases.md)) |
| `notifications_deferred_total` | counter | reason (`cap_person`, `cap_workspace`, `paused`, `platform_domain`, `system_mail_blocked`) | `Notifier`: an item folded into the person's `digest` by a daily cap ([O24](../edge-cases.md)), skipped while the person's preferences are paused, kept for the hourly retry while the platform domain is `failing` ([O25](../edge-cases.md)), or kept for the hourly retry after the system identity's submit was refused with `429 daily_cap_reached`, `409 identity_paused` or `409 domain_not_ready` ([Notifications §7](notifications.md#7-when-system-mail-cannot-be-sent)) |
| `notification_unsubscribes_total` | counter | kind (empty when the token cannot be read), result (`ok`, `expired`, `invalid`) | `fetch`: `POST /console/notifications/unsubscribe` ([O18](../edge-cases.md)) |
| `usage_alerts_total` | counter | feature (`inboxes`, `sends`, `triage`, `custom_domains`, `storage_gb`, `seats`), threshold (`80`, `100`) | `TenantQuota`: a `NotifierRequest::UsageThreshold` sent, once per threshold per period, or after the 24-hour cooldown for counts ([Notifications §4](notifications.md#4-usage-alerts)) |
| `quota_hold_denied_total` | counter | feature | `TenantQuota`: a `Hold`, or the `sends` hold of `Reserve`, denied; the caller answers `402 billing_limit` ([Billing › Hold](billing.md#hold)) |
| `quota_hold_expired_total` | counter | feature | `TenantQuota` alarm `alarm:holds`: a hold released because it expired unsettled, meaning a request died without settling ([W6](../edge-cases.md)) |
| `quota_consumed_without_hold_total` | counter | – | `TenantQuota` `Settle`: units consumed with no matching hold, for example a reconciled uncertain send ([W5](../edge-cases.md)) |
| `quota_count_drift_total` | counter | feature (`inboxes`, `custom_domains`, `seats`) | `TenantQuota` `Reconcile`: a count corrected from D1 by the hourly roll-up ([Billing › Reconciliation against D1](billing.md#reconciliation-against-d1)) |
| `stripe_webhook_total` | counter | type (the Stripe event type), outcome (`applied`, `ignored_stale`, `ignored_erased`, `ignored_unresolved`, `cancelled_after_erasure`, `duplicate`, or the `error:` code) | `fetch`: `POST /billing/stripe/webhook`, once per verified event ([Billing › Webhook endpoint](billing.md#webhook-endpoint)) |
| `stripe_webhook_rejected_total` | counter | – | `fetch`: a Stripe webhook refused with `400 invalid_request` by signature verification ([W14](../edge-cases.md)) |
| `stripe_api_errors_total` | counter | call (`customer_create`, `checkout_create`, `checkout_retrieve`, `portal_create`, `subscriptions_list`, `charge_retrieve`, `subscription_cancel`) | worker: a Stripe API call that failed with a network error, a timeout or a non-`2xx` answer ([Billing › Stripe integration](billing.md#stripe-integration)) |
| `stripe_duplicate_subscriptions_total` | counter | – | `fetch`: the webhook's state application cancelled a duplicate subscription ([Billing › Applying state](billing.md#applying-state), [W39](../edge-cases.md)) |
| `billing_increase_held_total` | counter | feature (`plan`, or the top-up feature) | `fetch`: the webhook's state application kept an increase back because the subscription's latest invoice is not `paid` ([Billing › Allowances and periods](billing.md#allowances-and-periods), [W37](../edge-cases.md)) |
| `system_mail_budget_denied_total` | counter | class (`signin`, `invitation`, `notification`, `signup`, `account`), scope (`recipient`, `network`, `asn`, `tenant`, `class`, `role`, `day`) | `TenantQuota` of the default tenant: a system-identity send refused by a budget ([Cloud sign-up › System mail budgets](cloud-signup.md#102-system-mail-budgets), [W44](../edge-cases.md)) |
| `ses_control_throttled_total` | counter | – | SES control-plane callers: SES answered `ThrottlingException` or `TooManyRequestsException` although `SesControl` granted the slot ([Domains on any DNS host §4.8](domain-connections.md#48-ses-api-rate-one-request-per-second)) |
| `search_requests_total` | counter | mode, scope, result (`ok`, `degraded`, `partial`, code) | `fetch` |
| `search_ms` | observation | mode, scope, fanout (`1`, `2-10`, `11-100`) | `fetch` |
| `agentic_requests_total` | counter | status | `fetch` |
| `agentic_ms`, `agentic_first_evidence_ms` | observation | – | `fetch` |
| `ai_calls_total` | counter | purpose (`embed`, `rerank`, `triage`, `planner`, `markdown`), result | worker |
| `index_jobs_total` | counter | kind, result | `pm-index` |
| `vector_count_drift` | observation | – (the value is `index_count − Σ embedded_rows`) | Nightly reconciliation cron ([Search › Nightly reconciliation](search.md#66-nightly-reconciliation)) |
| `triage_total` | counter | status | `pm-index` |
| `job_steps_total` | counter | kind, step, result | JobRunner |
| `erasure_ms` | observation | scope | JobRunner: `created_at` → completion |
| `retention_purged_total` | counter | store (`raw`, `messages`, `vectors`, `events`) | JobRunner. Per-identity message retention has its own `retention_expired_total` (below) |
| `domain_checks_total` | counter | resolver, outcome | DomainMonitor |
| `domain_transitions_total` | counter | from, to | DomainMonitor |
| `mailbox_size_bytes` | observation | – | mailbox, hourly at most |
| `dlq_items` | observation | queue | Alert evaluator, every minute: open items per queue ([J8](../edge-cases.md)) |
| `rpc_owner_mismatch_total` | counter | class | Durable Objects |
| `alert_fired` | counter | alert, severity | Alert evaluator |
| `panics_total`, `config_invalid_total`, `metrics_dropped_total` | counter | – | any |
| `inbound_dictionary_guard_total`, `role_mail_failed_total` | counter | `role_mail_failed_total`: reason | `email()`: misses refused by the dictionary guard ([A16](../edge-cases.md)); role mail that could not be relayed ([D14](../edge-cases.md)) |
| `dsn_untrusted_total` | counter | reason (`provider_events`, `verdict`, `alignment`, `original_from`) | `pm-inbound`: DSN entries that changed nothing ([D11](../edge-cases.md)) |
| `r2_orphans_deleted_total` | counter | – | the mailbox's daily orphan sweep ([I9](../edge-cases.md)) |
| `loopback_refused_total`, `stale_claim_outcome_total` | counter | – | `pm-inbound` (a loopback pointer across tenants), `pm-outbound` (a transport outcome with a stale claim token, [G2](../edge-cases.md)) |
| `index_job_unimplemented_total`, `ai_deferred_total`, `ai_rate_limited_total`, `triage_model_cap_total` | counter | kind or task | `pm-index`: job kinds before their milestone; jobs deferred by `RL_AI` or `RL_EMBED`; Workers AI rate-limit answers; triage runs made rules-only by the daily model cap ([F16](../edge-cases.md), [D13](../edge-cases.md)) |
| `drafts_written_total` | counter | op (`create`, `update`, `delete`) | mailbox ([Drafts § 18](drafts.md#18-observability)) |
| `draft_sends_total` | counter | origin (`api`, `approval`, `console`, `alarm`), outcome (`sent`, `pending_approval`, `scheduled`, `refused`, `failed`, `transient`) | mailbox |
| `draft_send_failures_total` | counter | reason (the `failure.reason` codes) | mailbox |
| `draft_decisions_total` | counter | decision (`approve`, `reject`, `expire`), actor (`key`, `person`, `system`), send | mailbox |
| `draft_approval_rerequested_total` | counter | cause (`edit`, `digest_changed`, `approver_invalid`) | mailbox |
| `draft_fire_lag_ms` | observation | – | mailbox: the fire's start minus `send_at` |
| `draft_claims_expired_total` | counter | origin | mailbox |
| `drafts_alarm_missed_total` | counter | – | `crons/drafts_due.rs` |
| `draft_index_write_failures_total` | counter | – | mailbox |
| `send_approval_required_total` | counter | level | `fetch` (step 16a refusals) |
| `event_log_append_total` | counter | result (`appended`, `staged`, `duplicate`, `refused_erased`, `skipped_no_tenant`, `skipped_system`, `skipped_unavailable`, `skipped_tenant_erasing`, `skipped_too_old`) | `pm-webhooks` (`Fanout`) and the `TenantEvents` object ([Event feed § 16](event-feed.md#16-observability)) |
| `event_log_append_failures_total` | counter | reason (`unavailable`, `timeout`, `payload_read`) | `pm-webhooks` |
| `event_log_lag_ms` | observation | – | `TenantEvents`: `appended_at − occurred_at` for each appended event |
| `event_log_pruned_total` | counter | reason (`retention`, `size_limit`, `erasure`, `message_deleted`, `reset`) | `TenantEvents` (`message_deleted`: an `EraseMessages` from a mailbox's `log_erase` purpose) |
| `event_log_erase_failures_total` | counter | – | mailbox: a `log_erase` call that failed and will be retried ([Event feed § 11.3](event-feed.md#113-erasure)) |
| `event_log_payload_bytes` | observation | – | `TenantEvents` daily maintenance |
| `event_feed_reads_total` | counter | surface (`feed`, `consumer`, `mcp`), result (`ok`, `empty`, `long_poll_timeout`, or the error code) | `fetch` |
| `event_consumer_backlog` | observation | state (`active`, `expired`, `reset_required`) | `TenantEvents` daily maintenance: each consumer's `positions_behind` |
| `event_stream_connections` | observation | – | `TenantEvents`, at each revalidation alarm |
| `event_stream_opened_total` | counter | auth (`key`, `token`), result (`ok`, or the refusal's code) | `fetch` and `TenantEvents` |
| `event_stream_closed_total` | counter | code, by (`server`, `client`) | `TenantEvents` |
| `event_stream_sent_total` | counter | – | `TenantEvents`: events sent |
| `event_stream_tokens_total` | counter | result (`minted`, `redeemed`, `expired`, `refused`) | `fetch` and `TenantEvents` |
| `event_stream_revalidation_failures_total` | counter | – | `TenantEvents`: a revalidation query that failed |
| `screening_verdict_total` | counter | mode, status, reason | mailbox, at every final verdict ([Screening hold § 22](screening.md#22-observability)) |
| `screening_latency_ms` | observation | mode | mailbox: `completed_at − started_at` |
| `screening_deadline_lag_ms` | observation | – | mailbox `screen` alarm: run time − `deadline_at` for each expired row |
| `screening_model_calls_total` | counter | outcome (`valid`, `invalid`, `error`, `timeout`, `length`) | `pm-screen` |
| `screening_model_capped_total` | counter | scope (`tenant`, `sender`) | `pm-screen`: `CountScreenModel` answered `CapReached` |
| `screening_model_ms` | observation | – | `pm-screen` |
| `screening_deferred_total` | counter | cause (`rl_ai`, `quota`, `rpc`, `attachment_wait`) | `pm-screen` |
| `screening_queue_wait_ms` | observation | – | `pm-screen`: commit time → first run |
| `screening_late_signal_total` | counter | outcome (`requarantined`, `flagged`, `ignored_released`) | mailbox |
| `screening_late_verdict_total` | counter | verdict | `pm-screen` |
| `screening_deadline_extended_total` | counter | – | mailbox, after a freeze |
| `screening_feedback_total` | counter | screening_status, verdict | feedback handler |
| `screening_test_header_ignored_total` | counter | – | `pm-inbound`: a test header on mail that may not use it |
| `calendar_objects_total` | counter | method, object_status | `pm-inbound`, step 10a ([Calendar invitations § 14](calendar-invitations.md#14-observability)) |
| `calendar_verification_total` | counter | method, result (`verified` or the reason) | `pm-inbound`, step 10a |
| `calendar_tz_resolution_total` | counter | resolution (`utc`, `iana`, `windows`, `prefixed`, `vtimezone`, `unknown`, `floating`, `date`) | `pm-inbound`, step 10a |
| `calendar_events_total` | counter | trigger (`ingest`, `release`, `screening`, `reparse`) | mailbox |
| `calendar_sends_total` | counter | method, result (`accepted`, or the error code) | `fetch` (send, reply, draft send) |
| `calendar_replies_total` | counter | partstat, result (`accepted`, or the error code) | `fetch` (calendar reply) |
| `calendar_vtimezones_added_total` | counter | – | `fetch` |
| `calendar_properties_removed_total` | counter | – | `fetch` (one per removed extension property) |
| `uploads_created_total` | counter | – | `handlers/uploads.rs` ([Uploads § 14](uploads.md#14-observability)) |
| `upload_puts_total` | counter | result (`ready`, `already_ready`, `rejected_risk`, `rejected_infected`, `size_mismatch`, `checksum_mismatch`, `incomplete`, `in_progress`, `expired`, `not_found`, `unavailable`) | `handlers/links.rs` |
| `upload_bytes` | observation | – | `handlers/links.rs`, per `ready` upload |
| `upload_put_ms` | observation | – | `handlers/links.rs`: request start → response |
| `upload_classify_wait_total` | counter | result (`immediate`, `waited`, `timed_out`) | `uploads/classify.rs` |
| `upload_scanner_error_total` | counter | – | `uploads/classify.rs` |
| `uploads_expired_total` | counter | reason (`expired`, `too_many_attempts`, `object_missing`) | `mailbox/uploads.rs` |
| `upload_attachments_total` | counter | mode (`inline`, `link`) | `mailbox/submit.rs` |
| `upload_copy_ms` | observation | – | `uploads/copy.rs`, per copied upload |
| `metrics_requests_total` | counter | route (`rates`, `events`), scope (`identity`, `tenant`, `domain`), status_class | `handlers/metrics.rs` ([Deliverability metrics § 10](metrics.md#10-observability)) |
| `metrics_ms` | observation | route | `handlers/metrics.rs` |
| `metrics_fanout_partial_total` | counter | – | `metrics/events.rs`, once per partial response |
| `rates_tripped_reads_total` | counter | action (`pause_identity`, `pause_tenant`, `pause_domain`, `operator_alert`) | `metrics/rates.rs`, per tripped window returned |
| `agent_signups_total` | counter | result (`created`, `closed`, `rate_limited`, `limit_network`, `limit_asn`, `limit_human`, `limit_deployment`, `username_taken`, `invalid`, `no_mail_domain`, `dns_unavailable`) | `signup/create.rs` and `signup/admit.rs` ([Agent self-sign-up § 21](agent-signup.md#21-observability)); no `source` label |
| `agent_signup_codes_total` | counter | result (`verified`, `wrong`, `burned`, `locked`, `expired`, `superseded`) | `signup/verify.rs` |
| `agent_signup_verified_total` | counter | via (`code`, `link`, `claim`) | `signup/verify.rs` |
| `agent_signup_ended_total` | counter | reason (`expired`, `abandoned`, `refused`) | `signup/end.rs`, `crons/agent_signup.rs` |
| `agent_signup_mail_total` | counter | result (`queued`, `not_sent`, `delivered`, `bounced`, `complained`) | `signup/mail.rs`, `signup/outcomes.rs` |
| `agent_signup_refusals_total` | counter | reason (`refused`, `complaint`, `lifted`) | `signup/end.rs`, `signup/outcomes.rs` |
| `api_client_requests_total` | counter | client (`ts_sdk`, `rust_sdk`, `cli`, `mcp_bridge`, `other`), client_version (`major.minor` from the product token, or `unknown`) | `fetch`, from `User-Agent` or `X-Pylota-Identity-Client`; MCP requests are already counted by `mcp_method` ([SDKs, skills and agent tooling § 16](sdk-and-skills.md#16-observability)) |
| `messages_batch_total` | counter | op (`get`, `update`), result (`ok` or the error code) | `fetch` ([Parity additions § 11](parity-additions.md#11-observability)) |
| `messages_batch_ids` | observation | op | `fetch`: IDs per request |
| `label_events_total` | counter | source (`message`, `thread`, `batch`) | mailbox |
| `list_decisions_total` | counter | direction, scope (`identity`, `tenant`), kind | `pm-inbound`, `fetch` (send), `pm-outbound` |
| `arc_rescue_total` | counter | result (`rescued`, `off`, `chain`, `untrusted_sealer`, `from_unsigned`, `no_dmarc_pass`, `stale`, `no_binding`), source (`routing`, `ses`) | `pm-inbound` |
| `retention_expired_total` | counter | store (`messages`, `attachments`, `r2_objects`, `vectors`, `events`, `drafts`) | mailbox |
| `retention_expiry_lag_ms` | observation | – | mailbox: due time → commit |
| `retention_expiry_errors_total` | counter | step (`vectors`, `objects`, `commit`) | mailbox |
| `webhook_header_updates_total` | counter | result | `fetch` |
| `zone_exports_total` | counter | method | `fetch` |

## 4. Service-level objectives

Windows are rolling 30 days unless stated. "Good" and "total" are counted from the metrics above.

| ID | Objective | SLI: good / total | Notes |
|---|---|---|---|
| NFR-REL-1 | 0 acknowledged inbound messages lost | `inbound_lost_total` + `inbound_raw_missing_total` + `ses_object_lost_total` = 0 | Any non-zero value pages |
| NFR-REL-2 | ≥ 99.9% of valid inbound accepted | (`accepted` + `staged`) / (`accepted` + `staged` + `tempfail_storage`) from `inbound_received_total` | Rejections of unknown, retired and suspended addresses are correct behaviour and excluded |
| NFR-REL-3 | Inbound accepted → webhook delivered: p95 ≤ 30 s, p99 ≤ 120 s | share of `webhook_delivery_latency_ms{event_class=inbound, first_attempt=succeeded}` ≤ 30,000 (target 95%) and ≤ 120,000 (target 99%) | Measured to delivery, as the PRD states. Deliveries whose first attempt failed at the endpoint are excluded from the SLI so an integrator's outage does not burn the service budget; they stay visible on the dashboard |
| NFR-REL-4 | ≥ 99.99% of webhooks delivered within 24 h | count of `webhook_delivery_latency_ms` ≤ 86,400,000 / (count of `webhook_delivery_latency_ms` + `webhook_dead_total`) | `webhook_dead_total` counts only deliveries whose 13th attempt failed; a delivery ended because its endpoint was disabled (attempt error `endpoint_disabled`) is not counted, so disabled endpoints are excluded |
| NFR-PERF-1 | Send API p95 ≤ 500 ms | share of `send_api_ms` ≤ 500 (target 95%) | |
| NFR-PERF-2 | Queued → transport p95 ≤ 60 s | share of `outbound_queue_to_transport_ms` ≤ 60,000 (target 95%) | First attempt only; quota back-off is measured by `provider_quota_errors_total` |
| NFR-PERF-3 | Keyword search, one identity, p95 ≤ 200 ms | `search_ms{mode=keyword, scope=identity}` ≤ 200 (95%) | |
| NFR-PERF-4 | Hybrid search, one identity, p95 ≤ 800 ms | `search_ms{mode=hybrid, scope=identity}` ≤ 800 (95%) | |
| NFR-PERF-5 | Tenant search over ≤ 10 identities, p95 ≤ 1 s | `search_ms{scope=tenant, fanout ∈ {1, 2-10}}` ≤ 1,000 (95%) | |
| NFR-PERF-6 | Agentic p95 ≤ 8 s; first evidence ≤ 1.5 s | `agentic_ms` ≤ 8,000 and `agentic_first_evidence_ms` ≤ 1,500 (95%) | |
| NFR-PRV-1 | Erasure ≤ 24 h, receipt always produced | `erasure_ms` ≤ 86,400,000 (100%) | `erasure_overdue` alert at 20 h |
| NFR-OPS-2 | RPO ≤ 1 min (indexes), ≤ 15 min (blobs); RTO ≤ 4 h | Restore drills on staging ([Restore from PITR](#restore-from-pitr)) | See the R2 note in that runbook |

Measured outside production: NFR-QUAL-1…3 by the evaluation harness, NFR-SEC-1 by the attack suite,
NFR-SEC-2 by `cargo xtask build-worker`, NFR-OPS-1 by deploy rehearsals ([Testing](testing.md)).
NFR-COST-1 is checked by reviewing Cloudflare usage after a week of idling on staging.

## 5. Alerts

### 5.1 How alerts reach the operator

Pylota Identity Cloud is run by one person, who is also its developer: there is no second person and no
on-call rota. Alerting is therefore built so that nothing depends on someone watching a dashboard, on a
single delivery path, or on the deployment itself still running ([ADR 0015](../adr/0015-solo-operator.md),
NFR-OPS-3, [J32](../edge-cases.md)).

| Class | Evaluated by | Delivered by |
|---|---|---|
| **A: metric alerts** | Cloudflare Custom Alerts (beta), which run a SQL API query on a schedule with threshold, anomaly or SLO detection and deliver to email, webhooks or PagerDuty (Cloudflare Notifications docs, read 2026-10-09). Workers Analytics Engine datasets are queryable through the SQL API. Used only for the burn-rate and latency rules of section 5.2 and the rate rules marked A in section 5.3: none of them is the only signal of a condition that needs action | The deployer's chosen destination (on Cloud: `PM_ALERT_EMAIL`) |
| **B: state alerts** | The Worker's alert evaluator (section 5.4), every minute, from exact state in D1 and the objects | An `alert.fired` audit row, an `error` log line and one `alert_fired` data point, **and alert email** (section 5.5). Doctor and the heartbeat read them through `GET /v1/platform/status` |
| **C: event alerts** | The service, as part of normal behaviour | Webhook events to the integrator's endpoints |
| **Heartbeat** | A scheduled GitHub Actions workflow outside Cloudflare (section 5.5) | A failed workflow run, which GitHub notifies to the owner by email |

Custom Alerts are created in the Cloudflare dashboard from the queries in `deploy/observability/alerts/`;
no creation API was found in the documentation on 2026-10-09, so `idvy doctor` cannot check that they
exist. That is why no condition that needs action relies on a Custom Alert alone: every page-severity
condition is a state alert (class B), which the Worker emails itself and the heartbeat checks.

**Severities.** `page`: emailed at once, again every 6 hours while it fires, and it fails the heartbeat.
`ticket`: listed in one summary email a day (08:00 UTC) while it fires; it never wakes anyone.

### 5.2 Burn-rate rules

Availability SLOs use multi-window burn rates. A Custom Alert with SLO detection fires when
`(1 − bad/total) × 100` is below its target over both the short and the long window, so each rule's
target is `100 × (1 − burn × (1 − SLO))`.

| SLO | Burn | Long / short window | Custom Alert target | Severity |
|---|---|---|---|---|
| NFR-REL-2 (99.9%) | 14.4× | 1 h / 5 min | 98.56 | page |
| NFR-REL-2 (99.9%) | 6× | 6 h / 30 min | 99.40 | page |
| NFR-REL-2 (99.9%) | 3× | 24 h / 2 h | 99.70 | ticket |
| NFR-REL-4 (99.99%) | 14.4× | 1 h / 5 min | 99.856 | page |
| NFR-REL-4 (99.99%) | 6× | 6 h / 30 min | 99.94 | page |
| NFR-REL-4 (99.99%) | 3× | 24 h / 2 h | 99.97 | ticket |

Latency SLOs (95% and 99% targets) have budgets too large for those burn rates, so they use two rules
each: **page** when the good share is below 90% (95% targets) or 97% (99% targets) over 1 h and 5 min,
and **ticket** when it is below the target over 6 h and 30 min. Each rule needs at least 100 events in
the window (the Custom Alert "minimum event count").

### 5.3 Alert list

| Alert | Class | Condition | Severity | Runbook |
|---|---|---|---|---|
| `dlq:{queue}` | B | The oldest open `dlq_items` row of a queue is older than 15 minutes ([J8](../edge-cases.md)); this includes `dlq:pm-screen` for the screening queue's backstop jobs ([Screening hold § 22](screening.md#22-observability)) | page | [DLQ growth](#dlq-growth) |
| `vector_drift` | B | Tonight's and the previous night's reconciliation both put `drift_pct` more than 1 away from zero ([Search › Nightly reconciliation](search.md#66-nightly-reconciliation)) | ticket | Re-run the reconciliation; if the drift persists, start a `reembed` job for each affected tenant (`POST /v1/platform/jobs` with `{ "kind": "reembed", "tenant_id": … }`, and `identity_ids` to limit it to the identities whose `index_reconcile` rows show the gap; `platform:ops`). A `reindex` job rebuilds only the keyword index and does not touch Vectorize |
| `bounce_rate:{tenant_id}:{domain_id}` | B | Bounced outcomes / all outcomes > 2% in the current UTC hour, with ≥ 50 outcomes, for one tenant's sends through one domain (the platform domain counts per tenant). Evaluated by `TenantQuota` from its per-domain hourly outcome counters each time `RecordOutcome` runs ([Outbound › Abuse auto-pause](outbound.md#abuse-auto-pause-fr-dlv-3)), so one rule covers every domain without a Custom Alert per domain. Above 10% with ≥ 200 outcomes the tenant's sending is paused automatically (section 5.6) | page | [Bounce spike](#bounce-spike) |
| `complaint_rate:{tenant_id}:{domain_id}` | B | Complained outcomes / all outcomes > 0.1% over the 24 UTC hours ending with the current hour, with ≥ 200 outcomes, for one tenant and domain, evaluated the same way. Above 0.6% with ≥ 1,000 outcomes the tenant's sending is paused automatically (section 5.6) | page | [Complaint spike](#complaint-spike) |
| `inbound_reject_spike` | A | Anomaly detection on `inbound_received_total{result=rejected_unknown}`: spike, 15-minute evaluation window, 24 h baseline, minimum 50 events | ticket | [Domain failing](#domain-failing) (routing checks) |
| `inbound_tempfail` | A | `inbound_received_total{result=tempfail_storage}` > 0 over 5 minutes | page | [DLQ growth](#dlq-growth) (storage path) |
| `inbound_lost` | B | `inbound_lost_total` or `inbound_raw_missing_total` > 0 | page | [Restore from PITR](#restore-from-pitr) (re-ingest step) |
| `ses_object_lost` | B | `ses_object_lost_total` > 0: an S3 object was deleted before every recipient was ingested ([N4](../edge-cases.md)) | page | [SES account and receiving](#ses-account-and-receiving) |
| `ses_sending_paused` | B | The 15-minute SES platform check finds account sending paused. SES sends are held meanwhile, never moved to the platform domain, and end `failed` after 24 hours ([N10](../edge-cases.md)) | page | [SES account and receiving](#ses-account-and-receiving) |
| `ses_rule_missing` | B | The 15-minute SES platform check finds the receipt rule set `PM_SES_RULE_SET` inactive or without the rule `pm-deliver` (only when SES receiving is configured) | page | [SES account and receiving](#ses-account-and-receiving) |
| `ses_identities_90pct` | B | SES identities in the region reach 9,000, 90% of the 10,000 per Region ([quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html), read 2026-10-10). Counted as in [Domains on any DNS host § 4.3](domain-connections.md#43-dns_records): `domains` rows with `ses_region` set and not `removed`, plus onboarding journal rows that hold an SES identity, plus the platform identity, or the region's total from the daily `ListEmailIdentities` when that is larger. There is no `quota.warning` event for this | ticket | [SES account and receiving](#ses-account-and-receiving) |
| `ses_rule_sync_failed` | B | `SesControl` could not make the `pm-retired-{n}` rules match a change set after three verified attempts; it keeps the change set and retries every 15 minutes ([N32](../edge-cases.md)) | ticket | [SES account and receiving](#ses-account-and-receiving) |
| `webhook_failing:{webhook_id}` | B | `consecutive_failures` ≥ 10 on an enabled endpoint | ticket | [Integrator API down](#integrator-api-down) |
| `webhook_disabled:{webhook_id}` | B + C | Endpoint disabled with `failing` (`webhook.disabled` event) | ticket | [Integrator API down](#integrator-api-down) |
| `provider_quota` | A | `provider_quota_errors_total` > 0 over 15 minutes: the first quota error ([G3](../edge-cases.md)) | page | [Quota exhausted](#quota-exhausted) |
| `provider_quota_80` | B | Only when `PM_DAILY_SEND_QUOTA` is set: the default tenant's `deployment_day` (recipients accepted today, UTC, for the Cloudflare transport by every tenant and the system identity, counted inline, [Cloud sign-up § 10.3](cloud-signup.md#103-shared-domain-breaker)), reach 80% of it ([G3](../edge-cases.md)) | ticket | [Quota exhausted](#quota-exhausted) |
| `quota_warning` | C | `quota.warning` at 80% and 100% of a tenant or identity daily send cap | – (tenant-facing) | [Quota exhausted](#quota-exhausted) |
| `domain_failing:{domain_id}` | B + C | Domain enters `failing` or `suspended` (`domain.failing`, `domain.suspended`) | ticket | [Domain failing](#domain-failing) |
| `platform_domain_failing` | B | The platform domain enters `failing`, or shows `dns_unresolvable` or `dnssec_bogus`: all system mail and every fallback send depend on it ([H9](../edge-cases.md)) | page | [Domain failing](#domain-failing) (the platform domain is never suspended; `POST /v1/domains/{id}/reprove` re-reads its zone) |
| `domain_remove_failed:{domain_id}` | B | A `domain_remove` job is `failed` after 10 attempts on one step; the domain stays `removing` and the job restarts once a day ([H12](../edge-cases.md)) | ticket | [Domain failing](#domain-failing) (read the job's `last_error`; `DELETE /v1/domains/{id}` restarts it at once) |
| `delivery_events_manual:{domain_id}` | B | A Cloudflare-transport domain with `sending` has no event subscription (`delivery_events: "manual"`); its sends use the platform address meanwhile ([H17](../edge-cases.md)) | ticket | Run `idvy domains subscribe <domain>` |
| `inbound_throttled` | A | `inbound_throttled_total` > 100 over 1 h: one or more senders exceed `inbound.per_sender_per_hour` and their excess is stored `throttled` ([D5](../edge-cases.md)) | ticket | [Abusive identity](#abusive-identity) (the affected mailbox's `rate_windows` rows name the sender; add a receive-block if it is abuse) |
| `stripe_webhook_errors` | B | Only with `PM_BILLING=stripe`: at least one `billing_events` row with `outcome` starting `error:` received in the last hour (the detail lists each `type` and code) | ticket | [Billing design › Stripe webhook](billing.md#stripe-integration) (fix the endpoint's event list, or the customer mismatch) |
| `mailbox_size:{identity_id}` | B | Mailbox SQLite size > 70% of 10 GB (7,516,192,768 bytes), reported by the mailbox's size check (at most hourly, after a write; [Data model › Mailbox notes](data-model.md#mailbox-notes)) | ticket | [Abusive identity](#abusive-identity) (archive or split) |
| `abuse_pause:{identity_id}` | B + C | An identity paused with `abuse_threshold` (`identity.paused`) | ticket | [Abusive identity](#abusive-identity) |
| `signup_ramp_review:{tenant_id}` | B | Only with `PM_BILLING=stripe`: the third failed daily evaluation of a new Free workspace's send ramp (audit `tenant.ramp_held`); nothing is suspended automatically ([Cloud sign-up › New-workspace send ramp](cloud-signup.md#101-new-workspace-send-ramp)) | ticket | [Abusive identity](#abusive-identity) (review the workspace's identities; suspend the tenant if it is abuse) |
| `system_mail_blocked` | B | The Notifier's submit through the system identity was refused with `429 daily_cap_reached`, `409 identity_paused` or `409 domain_not_ready` (the code is in the detail); the items are kept and retried hourly ([Notifications §7](notifications.md#7-when-system-mail-cannot-be-sent)) | page | [Domain failing](#domain-failing) for `domain_not_ready`; otherwise read the system identity with a platform key and resume it or raise its `send_policy.daily_cap`. Sign-in and invitation mail, and alert email (section 5.5), are blocked by the same refusal; the heartbeat still reports it |
| `billing_dispute:{tenant_id}` | B | Only with `PM_BILLING=stripe`: a charge of the workspace's customer is disputed (`billing_accounts.dispute_open_at` is set); its sends are stopped until the dispute closes ([Billing › Disputes and refunds](billing.md#disputes-and-refunds), [W38](../edge-cases.md)) | page | [Billing dispute](#billing-dispute) |
| `shared_domain_breaker` | B | Only when `PM_DAILY_SEND_QUOTA` is set: the `*/15` cron set stage 1 (60% of the day's quota) or stage 2 (90%) of the shared-domain breaker ([Cloud sign-up › Shared-domain breaker](cloud-signup.md#103-shared-domain-breaker), [W46](../edge-cases.md)); it resolves when the row is deleted at 00:00 UTC | ticket (stage 1), page (stage 2) | [Quota exhausted](#quota-exhausted) |
| `system_mail_budget` | A | `system_mail_budget_denied_total` ≥ 1,000 over 1 h: system mail refused by its budgets ([Cloud sign-up › System mail budgets](cloud-signup.md#102-system-mail-budgets), [W44](../edge-cases.md)) | ticket | [Abusive identity](#abusive-identity) (the `scope` label says whether one tenant's invitations, one network or one ASN is the cause; suspend a tenant that invites strangers) |
| `billing_cancel_failed:{tenant_id}` | B | Tenant erasure's `cancel_billing` step failed for the third time ([Privacy › Tenant scope](privacy.md#66-tenant-scope)) | page | [Erasure failure](#erasure-failure) (cancel the customer's subscriptions in the Stripe Dashboard; the step's next attempt then finds none and the erasure continues) |
| `billing_cancelled_after_erasure:{tenant_id}` | B | A Stripe webhook for an erasing or erased workspace showed a live subscription, and the handler cancelled it ([Billing › Webhook handling](billing.md#webhook-endpoint)) | ticket | Check in the Stripe Dashboard that the subscription is canceled and that no invoice was paid after the workspace was deleted; refund any that was |
| `erasure_stalled:{erasure_id}` | B | A step of the erasure job failed three times in a row; the job keeps retrying until `deadline_at − 4 h` ([Privacy § 4](privacy.md#4-jobrunner)). The detail names the step and error code | page | [Erasure failure](#erasure-failure) |
| `erasure_failed:{erasure_id}` | B + C | Erasure request `failed` (`erasure.failed`): its step still failed at the end of its retry window, and no request resumes it yet. It resolves once a resumption exists (`resumes_id`), which raises its own alerts if it fails too | page | [Erasure failure](#erasure-failure) |
| `erasure_overdue:{erasure_id}` | B | Erasure still `queued` or `running` after `deadline_at − 4 h` (20 hours after the first request) | page | [Erasure failure](#erasure-failure) |
| `job_failed:{kind}` | B | A job of that kind ended `failed` in the last 7 days and no later job of the same kind and tenant has completed. Page for `retention` (personal data kept past its retention), `domain_remove` (provider resources left behind) and `restore_reconcile`; ticket for `backup`, `export`, `reembed`, `reparse` and `reindex`. The detail lists the job IDs, tenants, failed steps and error codes | page / ticket | [Job failed](#job-failed) |
| `break_glass_key_used` | B | Only when `PM_BREAK_GLASS_KEY_ID` is set: a request authenticated with that key. Fired by the authentication path itself, with the audit row `key.break_glass_used` (route, IP hash), not by the evaluator, so it pages on the first use ([J36](../edge-cases.md)) | page | [Break-glass record](#57-break-glass-record) |
| `heartbeat_missing` | B | Only when `PM_HEARTBEAT_KEY_ID` is set: that key's `last_used_at` (before its first use, its `created_at`) is more than 1 hour old, so the external heartbeat (section 5.5) has stopped: the workflow was disabled, GitHub Actions is not running, or the key was revoked | page | [Heartbeat](#heartbeat) |
| `capacity_70:{resource}` | B | `resource` is `d1` (the D1 database's size against 10 GB, read as 10,000,000,000 bytes), `vectors` (the nightly reconciliation's `index_count` against 20,000,000 vectors) or `namespaces` (tenants that are not erased, one Vectorize namespace each, against 50,000) at 70% or more ([Limits](../../reference/limits.md#storage), [J30](../edge-cases.md), [J31](../edge-cases.md)) | ticket | [Capacity](#capacity) |
| `capacity_80:{resource}` | B | The same at 80% or more | page | [Capacity](#capacity) |
| `containment:{rule}` | B | An automatic containment rule of section 5.6 acted: `read_only`, `partner_suspended`, `tenant_paused`, `free_sending_off`, `emergency_prune` or `agent_signup_off`. The detail names the target and the values that triggered it. It stays firing while the rule's effect lasts (section 5.4), so the heartbeat sees it too | page | [Automatic containment](#automatic-containment) |
| `rpc_owner_mismatch` | B | `rpc_owner_mismatch_total` ≥ 1. The deployment turns read-only at once (section 5.6) | page | [Compromised key](#compromised-key) (treat as a security incident) |
| `uncertain_spike` | A | `transport_outcomes_total{outcome=uncertain}` > 5 over 15 minutes | page | [Email Sending outage](#email-sending-outage) |
| `transport_breaker_open:{scope}` | B | A transport's circuit breaker opened after 3 unknown outcomes in a row; queued messages wait unclaimed ([Outbound › Transport circuit breaker](outbound.md#transport-circuit-breaker-j28), [J28](../edge-cases.md)) | page | [Email Sending outage](#email-sending-outage) |
| `sending_pause:{tenant_id or domain_id}` | B | A tenant or a tenant domain was sending-paused for its complaint or bounce rate ([Outbound › Tenant and domain auto-pause](outbound.md#tenant-and-domain-auto-pause-g12), [G12](../edge-cases.md)) | page | [Abusive identity](#abusive-identity) (review the workspace's recent sends; resume with a platform key only when the cause is fixed) |
| `ai_backlog` | B | `ai_deferred_total` > 1,000 over 15 minutes, or `ai_rate_limited_total` > 20 over 5 minutes ([F16](../edge-cases.md)) | ticket | Compare the deferrals per tenant with the Workers AI limits; lower a tenant's `triage.daily_model_cap`, or ask Cloudflare for higher limits |
| `delivery_orphaned` | A | `delivery_orphaned_total` > 10 over 1 h | ticket | [Email Sending outage](#email-sending-outage) |
| `panics` | A | `panics_total` > 0 over 5 minutes | ticket | [Parser bug](#parser-bug) |
| `config_invalid` | A | `config_invalid_total` > 0 | page | `idvy doctor` |
| `webhook_secret_unavailable` | A | `webhook_attempts_total{result=secret_unavailable}` > 0 over 15 minutes (a sealed secret no longer opens: wrong or rotated `PM_MASTER_KEY`, [Webhooks](webhooks.md#secrets)) | page | [Security › Rotation procedures](security.md#62-rotation-procedures) (`PM_MASTER_KEY`) |
| `webhook_ssrf_blocked` | A | `webhook_ssrf_blocked_total` > 20 over 1 h | ticket | [Integrator API down](#integrator-api-down) (an endpoint's DNS now points at a blocked range) |
| `notification_send_failures` | A | `notifications_failed_total` > 0 in each of 3 consecutive hours, or > 20 in one hour | ticket | [Domain failing](#domain-failing) for the platform domain first (system mail has no fallback, [Notifications §7](notifications.md#7-when-system-mail-cannot-be-sent)), then [Email Sending outage](#email-sending-outage) |
| `drafts_overdue` | B | The watchdog finds a `scheduled` draft more than 10 minutes past its due time (`next_attempt_at` while it waits after a transient condition, else `send_at`) in two consecutive minutes ([Drafts § 18](drafts.md#18-observability)) | page | [Drafts overdue](#drafts-overdue) |
| `draft_index_stuck` | B | A mailbox reports a `draft_sync` row older than 15 minutes (the size check of its next write reports it, as `mailbox_size` is reported) | ticket | Check D1 health and `draft_index_write_failures_total`; the rows retry with back-off |
| `draft_send_interrupted` | A | `draft_send_failures_total{reason=send_interrupted}` > 0 over 1 hour | ticket | Read the logs of the claims that expired (`draft_claims_expired_total`); a repeating crash in composition is a bug |
| `event_log_append_failing` | A | `event_log_append_failures_total` > 0 in each of 5 consecutive minutes: a tenant's log refuses appends, and that tenant's webhooks wait with it ([Event feed § 16](event-feed.md#16-observability)). The existing `dlq:pm-webhooks` alert is the backstop for `Fanout` messages whose append retries ran out | page | [Event log](#event-log) |
| `event_log_lag` | A | The share of `event_log_lag_ms` ≤ 10,000 is below 95% over 1 hour (ticket), or the share ≤ 60,000 is below 90% over 15 minutes (page); at least 100 events in the window | ticket / page | [Event log](#event-log) |
| `event_log_size:{tenant_id}` | B | Reported by the `TenantEvents` maintenance: `log_bytes` passed 4,200,000,000 (70% of the budget), the database size passed 8,000,000,000 bytes, or the size budget pruned the log in the last 24 hours | ticket | [Event log](#event-log) |
| `event_stream_errors` | A | `event_stream_closed_total` with code `1011` or `1013` above 50 in 5 minutes | ticket | [Event log](#event-log) |
| `screening_unavailable:{tenant_id}` | B | Reported by an identity's mailbox when, in the current UTC hour, at least 10 of its hold-mode verdicts are `unavailable` and they are at least 20% of its verdicts (counted in mailbox meta `screen_hour`) ([Screening hold § 22](screening.md#22-observability)) | page | [Screening unavailable](#screening-unavailable) |
| `screening_model_failing` | B | Reported by the `pm-screen` consumer when the default tenant's `TenantQuota` counts at least 20 verdicts `unavailable` for `model_unavailable` or `invalid_output` across the deployment in the current UTC hour (`CountScreenFailure`): an outage or a model regression spread over identities that each stay under the bar above | page | [Screening unavailable](#screening-unavailable) |
| `screening_model_cap:{tenant_id}` | B | Reported by the `pm-screen` consumer when the tenant's `screen_model` count passes 80% of `triage.daily_model_cap`, and again when it reaches it | ticket | [Screening unavailable](#screening-unavailable) |
| `screening_stuck:{identity_id}` | B | Reported by `ScreenExpire` (the backstop, 30 seconds after the deadline) when it finds the row still `pending`, or by the mailbox's daily maintenance when a `pending` row is more than 60 seconds past its deadline: the alarm did not end the hold | page | [Screening unavailable](#screening-unavailable) |
| `screening_deadline_lag` | A | p95 of `screening_deadline_lag_ms` above 30 s over 15 minutes, with at least 20 points | ticket | [Screening unavailable](#screening-unavailable) |
| `screening_queue_wait` | A | p95 of `screening_queue_wait_ms` above 20 s over 15 minutes, with at least 20 points | ticket | [Screening unavailable](#screening-unavailable) |
| `screening_suspect_spike` | A | Anomaly detection on `screening_verdict_total{status=suspect}` per tenant: spike, 1-hour window, 7-day baseline, at least 20 verdicts | ticket | [Screening unavailable](#screening-unavailable) (a campaign, or a regression that makes false positives) |
| `calendar_parse_failures` | A | `calendar_objects_total{object_status="invalid"}` is more than 20% of `calendar_objects_total` over 24 hours, with at least 50 objects ([Calendar invitations § 14](calendar-invitations.md#14-observability)) | ticket | [Parser bug](#parser-bug) (a reader regression, or a sender's new format; add the case to the corpus) |
| `calendar_raw_refused` | A | `calendar_sends_total{result="transport_unavailable"}` > 0 over 1 hour | ticket | Read the S1 result in [Design › Spikes](index.md#spikes); until the raw path works, calendar mail needs an SES or SMTP domain |
| `upload_object_missing` | B | `uploads_expired_total{reason=object_missing}` > 0 while no restore freeze is in effect: an upload's object disappeared without a restore ([Q100](../edge-cases.md)) | ticket | Compare the mailbox's `uploads` rows with `BLOBS.list` of the identity's `up/` prefix and the orphan sweep's log |
| `agent_signup_spike` | B | The evaluator counts `agent_signup.create` audit rows: the last hour has at least 20 and at least three times the hourly average of the previous 7 days ([Agent self-sign-up § 21](agent-signup.md#21-observability)) | ticket | [Agent sign-up abuse](#agent-sign-up-abuse) |
| `agent_signup_cap_80` | B | Today's (UTC) `sysmail:signup:deployment` counter reaches 80% of `PM_AGENT_SIGNUP_DAILY_CAP` | ticket | [Agent sign-up abuse](#agent-sign-up-abuse) (decide whether to raise the variable) |
| `retention_expiry_overdue:{identity_id}` | B | The oldest due message of a mailbox with `retention.message_hours` is more than 1 hour past due, reported by its expiry run or its daily maintenance ([Parity additions § 11](parity-additions.md#11-observability)) | ticket; page at 24 hours | [Erasure failure](#erasure-failure) (read `retention_expiry_errors_total{step}`; an R2 or Vectorize outage clears itself) |
| `arc_rescue_refused_spike` | A | `arc_rescue_total{result=no_binding\|stale}` > 50 in 1 hour: someone may be replaying sealed mail | ticket | [Abusive identity](#abusive-identity) |
| SLO burn rules | A | Section 5.2 | page / ticket | The runbook of the failing path |

### 5.4 The state alert evaluator

The `* * * * *` cron runs `ops::alerts::evaluate`:

1. Read the conditions:
   - `dlq_items`: per queue, `COUNT(*)` and `MIN(first_seen_at)` of open rows; also written as the
     `dlq_items` metric;
   - `webhook_endpoints` with `enabled = 1 AND consecutive_failures >= 10`, and those disabled with
     `failing` in the last minute;
   - `domains` in `failing` or `suspended` (for the platform domain, also `dns_unresolvable` or
     `dnssec_bogus` among its issues: `platform_domain_failing`); `domains` in `removing` whose last
     `domain_remove` job is `failed`; Cloudflare-transport `domains` with `sending = 1`, no
     `event_subscription_id` and not `removing` or `removed`;
   - `erasure_requests` with `status = 'failed'` and no request whose `resumes_id` is its ID (indexed
     by `erasure_requests_resumes`), or `status IN ('queued','running')` and `deadline_at − 4 h` passed;
   - `jobs` other than erasure with `status = 'failed'` and `updated_at` in the last 7 days, with no later
     `completed` job of the same `kind` and `tenant_id` (`job_failed:{kind}`);
   - when `PM_HEARTBEAT_KEY_ID` is set: that `api_keys` row's `last_used_at` (or, before its first use, `created_at`) older than
     1 hour, or the row revoked or missing (`heartbeat_missing`);
   - every 15th run (every 15 minutes): capacity. D1's size is the `meta.size_after` that D1 returns with
     every query result ([D1 result object](https://developers.cloudflare.com/d1/worker-api/return-object/),
     read 2026-10-10), taken from the result of the evaluator's own tenant-count query below; the vector count is `index_count` on the
     latest `index_reconcile` summary row; the namespace count is
     `SELECT COUNT(*) FROM tenants WHERE status <> 'erased'`. The three values are kept in the `alert_fired`
     detail and shown by `GET /v1/platform/status`; the automatic rules of section 5.6 that depend on
     them run here too;
   - when `PM_DAILY_SEND_QUOTA` is set: the default tenant's `deployment_day` (`QuotaRequest::DeploymentStatus`,
     [Cloud sign-up § 10.3](cloud-signup.md#103-shared-domain-breaker)) against 80% of the quota. It is
     counted inline at accept, system mail included, so the alert has no roll-up lag; SES and SMTP sends are
     not counted, because they do not use the Cloudflare quota;
   - when SES is configured: `SELECT COUNT(*) FROM domains WHERE ses_region IS NOT NULL AND state <>
     'removed'`, plus the `domain_onboarding` rows whose journal holds an SES identity, plus one for the
     platform identity, or the last daily `ListEmailIdentities` total when larger, against 9,000
     (`ses_identities_90pct`);
   - conditions reported by objects and crons since the last run. The reporting code writes the
     `alert.fired` audit row itself:

     | Condition | Reported by, and writer of its `alert.fired` row |
     |---|---|
     | `mailbox_size` | The identity's mailbox, from its size check ([Data model › Mailbox notes](data-model.md#mailbox-notes)) |
     | `abuse_pause` | The delivery-event consumer (`consumers/delivery.rs`), when its auto-pause update changed a row ([Outbound › Abuse auto-pause](outbound.md#abuse-auto-pause-fr-dlv-3)) |
     | `sending_pause` | The delivery-event consumer, when its tenant or domain pause update changed a row |
     | `transport_breaker_open` | The outbound consumer, when it opens a breaker |
     | `ai_backlog` | The `pm-index` consumer, from its deferral and rate-limit counts |
     | `rpc_owner_mismatch` | The Durable Object whose owner check failed ([Design conventions](index.md#5-internal-durable-object-rpc)) |
     | `inbound_lost` | The global retention `staging` step (`inbound_lost_total`) and the `pm-inbound` consumer (`inbound_raw_missing_total`) |
     | `ses_object_lost` | The `pm-inbound` consumer's SES source |
     | `system_mail_blocked` | The Notifier, and the alert evaluator when an alert email's submit is refused (section 5.5) |
     | `billing_cancel_failed` | The tenant erasure job, on the third failed `cancel_billing` attempt |
     | `erasure_stalled` | The erasure job, on the third consecutive failed attempt of a step |
     | `bounce_rate`, `complaint_rate` | The delivery-event consumer, when `QuotaRequest::RecordOutcome` answers that a domain's rate crossed the threshold ([Outbound › Abuse auto-pause](outbound.md#abuse-auto-pause-fr-dlv-3)) |
     | `containment` | The code that applied the rule (section 5.6) |
     | `billing_cancelled_after_erasure` | The Stripe webhook handler, when it cancels a live subscription of an erasing or erased workspace ([Billing › Webhook endpoint](billing.md#webhook-endpoint)) |
     | `vector_drift` | The `*/15` cron's reconciliation drift evaluation, when this run's and the previous run's `drift_pct` are both more than 1 from zero ([Search › Nightly reconciliation](search.md#66-nightly-reconciliation)) |
     | `signup_ramp_review` | The daily ramp evaluation (`crons/signup_ramp.rs`) |
     | `billing_dispute` | The Stripe webhook handler, when `charge.dispute.created` sets `dispute_open_at`; resolved when `charge.dispute.closed` clears it ([Billing › Disputes and refunds](billing.md#disputes-and-refunds)) |
     | `shared_domain_breaker` | The breaker cron (`crons/send_breaker.rs`), when it writes a stage; resolved when it deletes the row ([Cloud sign-up › Shared-domain breaker](cloud-signup.md#103-shared-domain-breaker)) |
     | `ses_sending_paused`, `ses_rule_missing` | The 15-minute SES platform check, which reads `GetAccount` and the receipt rule set |
     | `ses_rule_sync_failed` | `SesControl`, after the third failed verification of a change set |
     | `drafts_overdue`, `draft_index_stuck` | The drafts watchdog (`crons/drafts_due.rs`) and the identity's mailbox ([Drafts § 18](drafts.md#18-observability)) |
     | `event_log_size` | The tenant's `TenantEvents` object, from its daily maintenance ([Event feed § 16](event-feed.md#16-observability)) |
     | `screening_unavailable`, `screening_stuck` | The identity's mailbox (verdict counts per hour; the daily maintenance and ScreenExpire) |
     | `screening_model_failing`, `screening_model_cap` | The `pm-screen` consumer, from the answers of `CountScreenFailure` (default tenant) and `CountScreenModel` |
     | `upload_object_missing` | The identity's mailbox, when it expires an upload whose object is missing and no restore freeze is in effect ([Uploads § 14](uploads.md#14-observability)) |
     | `retention_expiry_overdue` | The identity's mailbox, from its expiry run or its daily maintenance ([Parity additions § 11](parity-additions.md#11-observability)) |
2. Read the current state: for each alert key, the latest `audit_log` row with
   `action IN ('alert.fired', 'alert.resolved') AND target_id = <alert key>`.
3. Transition, with pure rules in `core::slo`: a true condition on a key that is not firing writes
   `alert.fired` (`details_json = { "severity", "values" }`), logs `alert_fired`, writes one
   `alert_fired` point and, for `page`, sends the alert email (section 5.5). A firing key re-notifies
   (another `alert_fired` point and, for `page`, another email; no audit row) every 6 hours. A firing key
   whose condition has been false on two consecutive runs writes `alert.resolved`. A condition reported
   by an object or cron is true only in the run that reads the report; its key resolves after two runs
   without a new report, with three exceptions that stay firing while their cause lasts:
   `erasure_stalled` and `billing_cancel_failed`, which the erasure job resolves itself (it writes
   `alert.resolved`) when the step next succeeds or the job ends; and `containment:{rule}`, which the
   evaluator re-reads as a level on every run: true while a switch is away from its default (`read_only`
   on, `free_sending` off, `emergency_prune` on, `agent_signup` off); for `partner_suspended`, while the partner is `suspended`
   and its latest status audit row is `partner.auto_suspend`; for `tenant_paused`, while the tenant's
   `sending_paused_at` is set with `sending_pause_reason = 'abuse_burst'`.
4. Alert rows use `tenant_id` of the affected tenant, or NULL for deployment-level alerts. They contain
   IDs and numbers only.

### 5.5 Alert email and the external heartbeat

**Alert email.** `PM_ALERT_EMAIL` names the operator's address (default: the address of
`PM_SECURITY_CONTACT` when that is a `mailto:` URI). The evaluator sends each page alert, and once a day
at 08:00 UTC a summary of every firing ticket alert, as an ordinary `transactional` send from the system
identity on the default tenant ([Identities › The system identity](identity-domains.md#the-system-identity)),
through the normal outbound pipeline with `Idempotency-Key: alert-{alert_key}-{fired_at}` (for a
re-notification, `-{n}`). The subject is `[pylota-identity {env}] {severity}: {alert_key}`; the body holds the
alert key, severity, values, the time it fired, the runbook's URL in the published docs and the
`idvy` command that shows more. Like every alert row it holds IDs and numbers only, never content or
addresses. At most 20 alert emails are sent per hour; above that one email says how many more are
firing. They count against the system identity's daily cap like any of its sends, and as class `account`
of the system-mail budgets (`QuotaRequest::SystemMail { class: Account }`,
[Cloud sign-up § 10.2](cloud-signup.md#102-system-mail-budgets)), which only the day limits, so
notifications and role relays can never crowd them out; when a send is
refused, `alert_emails_total{result=refused}` counts it, `system_mail_blocked` fires, and the heartbeat
still reports the page alert.

Alert email is a second channel beside Custom Alerts, and it shares a weakness with all system mail: it
cannot leave while the platform domain is failing or the system identity is refused
(`system_mail_blocked`), nor while the deployment is frozen for a restore, when every send waits. The
`read_only` switch does not hold it back: sends from the system identity are exempt (section 5.6). The
heartbeat below does not depend on the deployment sending anything.

**The heartbeat.** `.github/workflows/heartbeat.yml` in `PILOTAAI/pylota-identity` runs on GitHub's
infrastructure, outside the Cloudflare account it watches:

- `on: schedule` with `*/15 * * * *` (GitHub's shortest interval is 5 minutes; schedules run on the
  default branch, in UTC, and can be delayed under load: [Events that trigger workflows](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows),
  read 2026-10-10), plus `workflow_dispatch`.
- It installs the released `idvy` binary of the deployed version, verifies its signature, and runs
  `idvy doctor --json --check health --check alerts` against production, with `PYLOTA_IDENTITY_API_URL` and
  `PYLOTA_IDENTITY_KEY` from the GitHub Environment `ops` (secret `HEARTBEAT_KEY`).
- Once a day (the 06:00 UTC run) it also runs `idvy doctor --json --mail-test --check mail_test` (only that check) with the secret
  `HEARTBEAT_MAIL_TEST_KEY`, so mail flow end to end is proven daily.
- The job fails when `doctor` exits non-zero, when `GET /health` does not answer within 20 seconds, when a
  page alert is firing, when `alert_email` is `missing` in `GET /v1/platform/status` (no alert address
  configured), or when the deployment has been frozen for more than 4 hours. GitHub sends the failure
  notification of a scheduled workflow to the user who created it, or who last changed its cron
  ([Notifications for workflow runs](https://docs.github.com/en/actions/concepts/workflows-and-actions/notifications-for-workflow-runs),
  read 2026-10-10): the owner, by email, as long as GitHub Actions notifications are on in the owner's
  settings.
- The two keys are created on the production deployment and stored by the owner:
  `HEARTBEAT_KEY` is a platform key holding only `audit:read` (it reads `GET /v1/platform/status`); its ID
  is set as `PM_HEARTBEAT_KEY_ID`. `HEARTBEAT_MAIL_TEST_KEY` is an **identity key of `idvy-doctor`** with
  exactly the permissions `--mail-test` needs (`messages:send`, `messages:read`, `search:read`,
  `erasure:manage`; [CLI and setup §10](cli.md#10-doctor)), never a tenant key of the default tenant, which
  could create bare platform addresses ([A18](../edge-cases.md)). Both expire after
  90 days. A rotation keeps a key's `expires_at` ([Security § 4.5](security.md#45-rotation)), so every
  quarter (the break-glass review, section 5.7) the owner **replaces** them: creates a new platform key
  (`idvy keys create --level platform --permissions audit:read --expires-in 90d`), sets
  `PM_HEARTBEAT_KEY_ID` to its ID under `[vars]` and runs `idvy deploy` (a variable change only),
  stores it as `HEARTBEAT_KEY` (`gh secret set HEARTBEAT_KEY --env ops`), runs the workflow once
  (`gh workflow run heartbeat.yml`), and only then revokes the old key. The mail-test key is replaced the
  same way, without the redeploy. A key that expires anyway stops the heartbeat, which
  `heartbeat_missing` reports.

**Watching the watcher.** GitHub disables scheduled workflows in a public repository after 60 days with
no repository activity, and a failed or disabled run reports nothing on its own. The Worker therefore
checks the heartbeat from its side: every request authenticated by `HEARTBEAT_KEY` moves its
`last_used_at` (at most once a minute, [Security §4.3](security.md#43-last_used_at)), and
`heartbeat_missing` pages by alert email when it is more than an hour old. Each channel covers the other
one's failure: a dead deployment fails the heartbeat; a dead heartbeat raises an alert email.

### 5.6 Automatic containment

With one operator, a condition that needs a safe action at 03:00 cannot wait for a person. These rules
act on their own, fail closed, write an audit row and page through `containment:{rule}`. Each is undone
only by a platform key, after the cause is understood. They are evaluated where their data is: the
alert evaluator for D1 conditions, the delivery-event consumer for outcome rates.

| Rule | Trigger | Action | Undone by |
|---|---|---|---|
| `read_only` | Any `rpc_owner_mismatch` (a Durable Object received a request for another owner: a possible isolation bug) | The `ops_switches` row `read_only` becomes `on`: every request that is not a `GET` from a non-platform key gets `503 unavailable` with `details.reason = "read_only"`, and the outbound consumer re-enqueues sends with a 5-minute delay instead of sending them (they stay `queued`; nothing is canceled), except sends from the system identity, so sign-in, invitation and alert email still leave. Inbound mail is still accepted and stored. Audit `ops.switch` | `PUT /v1/platform/switches` `{"name": "read_only", "state": "off"}` |
| `partner_suspended` | For one partner, from `audit_log`: 5 or more `identity.auto_pause` rows in the last hour spread over at least 3 of its tenants (one abusive tenant is contained by its own identity pauses and by `tenant_paused`, not by suspending every tenant of the partner), or 3 or more of its tenants sending-paused automatically (`tenant.auto_pause`, by `tenant_paused` below or by the 7-day rates of [Outbound › Tenant and domain auto-pause](outbound.md#tenant-and-domain-auto-pause-g12)) in the last 24 hours. A stolen partner key used to send abuse shows up this way within the hour; key creation alone is not a trigger, because a partner onboarding many tenants creates many keys legitimately | `partners.status = 'suspended'` (every key of the partner and of its tenants gets `403 partner_suspended`, deliveries to their endpoints are held, inbound is still stored: [J13](../edge-cases.md)). Audit `partner.auto_suspend` with the trigger | `PATCH /v1/partners/{partner_id}` `{"status": "active"}` |
| `tenant_paused` | For one tenant and domain: bounced > 10% of outcomes in the current UTC hour with ≥ 200 outcomes, or complained > 0.6% over the 24 UTC hours ending with the current hour with ≥ 1,000 outcomes (the windows of [Deliverability metrics § 3.1](metrics.md#31-the-windows)). That is above both the alert thresholds (2% and 0.1%) and the identity auto-pause thresholds (5% of 200 and 0.3% of 1,000, [Outbound](outbound.md#abuse-auto-pause-fr-dlv-3)), so the identities causing it are normally paused first and this rule catches abuse spread over many identities. Outcomes of the system identity are not counted ([Outbound › Per-domain outcome counters](outbound.md#per-domain-outcome-counters)) | The tenant's sending pause (`tenants.sending_paused_at`, `sending_pause_reason = 'abuse_burst'`): sends get `409 sending_paused` and queued sends are held, never canceled. It is not a suspension: inbound mail is still accepted, keys still work, and the system identity's mail (sign-in, invitation and alert email) still leaves. Audit `tenant.auto_pause` | `PATCH /v1/tenants/{tenant_id}` `{"sending_paused": false}` (platform key) |
| `free_sending_off` | Only with `PM_BILLING=stripe` or partners with ramped tenants: in the last hour, 5 or more distinct workspaces on the Free plan or still in the new-workspace send ramp had an identity auto-paused for abuse. Pauses come only from counted outcomes, so strangers who write in cannot cause them ([G13](../edge-cases.md)); someone with five Free workspaces of their own can, and that stops Free and ramped sending until the operator lifts it. That is accepted: Free sending is best effort, the page names the five workspaces, and a partner with `ramp_exempt = 1` is never affected | The `ops_switches` row `free_sending` becomes `off`: every send from a workspace on the Free plan or still ramped is refused at submit with `403 policy_denied` (`details.reason = "free_sending_off"`), and queued ones are re-enqueued with the `Paused` back-off. Paid workspaces and partner tenants that are not ramped are unaffected | `PUT /v1/platform/switches` `{"name": "free_sending", "state": "on"}` |
| `agent_signup_off` | Over the last 24 hours of agent sign-up mail outcomes ([Agent self-sign-up § 13.2](agent-signup.md#132-the-automatic-switch)): bounced > 5% with ≥ 50 outcomes; or complained > 0.5% with ≥ 200 outcomes; or 5 or more complaints, whatever the count. Evaluated in the delivery consumer, where the outcomes are | The `ops_switches` row `agent_signup` becomes `off` (`set_by = 'auto'`): new sign-ups, resends and replacements get `403 signup_closed` (`paused`), and every send of a provisional tenant gets `403 policy_denied` (`signup_paused`). Verification by code, link and claim, refusals, abandonment and expiry continue. Audit `ops.switch` | `PUT /v1/platform/switches` `{"name": "agent_signup", "state": "on"}` |
| `emergency_prune` | `capacity` for `d1` at 90% or more | The `ops_switches` row `emergency_prune` becomes `on`: the next tenant retention runs use a 7-day cutoff for `event_index` and `webhook_deliveries` whatever `retention.events_days` says, so webhook replay reaches back 7 days at most meanwhile ([J30](../edge-cases.md)) | `PUT /v1/platform/switches` `{"name": "emergency_prune", "state": "off"}` |

`ops_switches` ([Data model](data-model.md#1-d1-control-plane)) holds one row per switch: `read_only`,
`free_sending`, `emergency_prune` and `agent_signup`. Readers cache a row for 60 seconds per isolate, so a
switch takes effect within a minute everywhere. `GET /v1/platform/status` shows every switch, and the
operator can also set one by hand (`idvy ops switch <name> on|off`), for example `free_sending off` during a
sign-up abuse wave, or `agent_signup off`, which stops new agent sign-ups and their mail.

`GET /v1/platform/status` also has an `agent_signup` block: `configured`, `active`, the `reason` when inactive
(`signup_not_open`, `billing_off`, `console_off` or `switch_off`), the switch, today's sign-ups against
`PM_AGENT_SIGNUP_DAILY_CAP`, and the last 24 hours' outcome rates
([Agent self-sign-up § 21](agent-signup.md#21-observability)).

The other automatic safe actions already in the design: identity auto-pause on complaint and bounce
rates ([Outbound](outbound.md#abuse-auto-pause-fr-dlv-3)), erasure retries until 20 hours after the request
([Privacy § 4](privacy.md#4-jobrunner)), domain fallback to the platform address, held webhook deliveries
for a suspended partner, and quarantine of suspicious inbound mail.

### 5.7 Break-glass record

Every credential that the operator would need to recover Cloud is written down in one record, kept in
two places: the operator's password manager (a vault named `pylota-identity-break-glass`) and a printed,
sealed copy stored away from the operator's usual workplace. No credential lives only on one laptop. The
record lists, for each item, where it is and how to use it:

| Item | Where it is kept |
|---|---|
| Cloudflare account login, its two-factor device and recovery codes | Password manager; recovery codes also on the printed copy |
| The break-glass platform key (`break-glass`: the session permissions, 400 days; its ID is `PM_BREAK_GLASS_KEY_ID`) | The `pylota-identity-break-glass` vault and the printed copy only, never a `idvy` profile; any use pages ([J36](../edge-cases.md)) |
| The `idvy` profile and the current day's session platform key | Password manager (`key_command` reads it, [CLI › Configuration](cli.md#2-configuration-and-credentials)); each day's key is minted with the previous one, or with the break-glass key after a lapse |
| The minisign release key and its password (section 11 of [Security](security.md#11-supply-chain)) | The encrypted removable drive, and a second drive in the sealed copy's location; password in the password manager |
| GitHub account (owner of `PILOTAAI`), two-factor device and recovery codes | Password manager and printed copy |
| AWS root login and the SES IAM user | Password manager |
| Stripe account login and two-factor recovery | Password manager |
| Domain registrar for `pylotamail.com` | Password manager |
| `PM_HASH_KEY`, `PM_KEY_PEPPER` and the master-key slots | Never written down: they live only as Worker secrets and cannot be read back ([Security §6.1](security.md#61-inventory)). The record says so, and that losing the Worker means rotating them |

`idvy doctor` cannot check this record. The owner reviews it every quarter, when the heartbeat keys are
rotated, and writes the date in the incident log. The review rehearses the break-glass path: it mints a
1-day session key with the break-glass key (the expected `break_glass_key_used` page confirms the alert
works) and revokes it, and it replaces the break-glass key when fewer than 120 days remain (a new key,
the record and `PM_BREAK_GLASS_KEY_ID` updated, the old key revoked). Session keys live one day, so
without this key a gap of more than a day would leave no key that can mint another, and the only way back
would be `idvy setup --rotate-pepper`, which invalidates every key of every tenant
([Security §6.2](security.md#62-rotation-procedures), [J36](../edge-cases.md)).

### 5.8 When a store is full

There is one D1 database (10 GB, which cannot be raised) and one Vectorize index (20,000,000 vectors,
50,000 namespaces) for the whole deployment ([Limits › Storage](../../reference/limits.md#storage)). The
capacity alerts (section 5.3) give warning; this is what happens if a limit is reached anyway
([J30](../edge-cases.md), [J31](../edge-cases.md)):

- **D1 full.** D1 refuses writes with "Exceeded maximum DB size" ([Debug D1](https://developers.cloudflare.com/d1/observability/debug-d1/),
  read 2026-10-10). `platform::db` maps that error to `DbError::Full`. A request whose write fails this
  way answers `503 unavailable` with `details.reason = "storage_full"`; nothing was written, so a retry
  with the same `Idempotency-Key` is safe. Reads keep working, except reads of mail content by
  platform and partner keys, which must write their `mail.read` audit row first and so get
  `503 unavailable` (`storage_full`) too ([Security § 3.6](security.md#36-tb6-operators-of-the-deployment)). `email()` writes only to R2 and the queue,
  so inbound mail is still accepted; the inbound consumer commits the message to its mailbox, and the
  outbox dispatch, whose `event_index` and `webhook_deliveries` inserts fail, keeps the events in the
  mailbox outbox and retries every 5 minutes (they are durable there). Sends already queued continue,
  because the mailbox, not D1, records them. `capacity_80:d1` is already firing, and the
  `emergency_prune` switch (section 5.6) is already on from 90%.
- **Vectorize full.** While `capacity_80:vectors` or `capacity_80:namespaces` fires, the `pm-index`
  consumer treats an upsert error as "index full": it leaves the chunks `pending`, acks the job (no
  retry, nothing dead-lettered), and counts `index_jobs_total{result=index_full}`. The exact error text
  of a full index is not documented (not verified on 2026-10-10); spike S6 records it if it can. Keyword
  search is unaffected; hybrid search reports `degraded: true` with the semantic coverage it has. The
  nightly reconciliation re-enqueues `pending` rows, so they are embedded once there is room.

## 6. Dashboards

The SQL for each panel is kept in `deploy/observability/dashboards/` and runs against the Analytics SQL
API, so it works from any tool that can call that API.

| Dashboard | Panels |
|---|---|
| Overview | Requests and 5xx rate by route; SLO compliance per objective (section 4) with remaining error budget; firing alerts (from `alert_fired`) |
| Inbound | Accepted, staged, rejected and temp-failed per hour; quarantine reasons; verdict mix (with `arc_rescued`); screening verdicts, latency and queue wait; calendar objects by status and verification result; `inbound_ingest_ms` and `webhook_delivery_latency_ms{event_class=inbound}` p50/p95/p99 by `first_attempt`; backscatter and throttling; drops by reason and source; SES: SNS rejections, auth disagreements, lost objects |
| Outbound and delivery | Sends by transport and outcome; bounce and complaint rate per domain; uncertain and reconciled; fallback sends per domain; provider quota errors; delivery orphans; draft sends by origin and outcome and `draft_fire_lag_ms`; uploads by `PUT` result, `upload_put_ms` and `upload_copy_ms` p50/p95/p99, and inline against linked attachments; `metrics_ms` p50/p95/p99 and the partial share of event counts |
| Webhooks | Attempts by result and error code; dead deliveries; endpoints over 10 consecutive failures; attempt latency; event log appends, lag and size, feed reads by surface, consumer backlog, open streams and close codes |
| Identity and notifications | Signing calls by kind and result; identity-key operations; JWKS and directory requests by status class; notifications sent, failed and deferred by kind and reason; unsubscribes by result; usage alerts by feature and threshold; agent sign-ups by result, verifications by path, ends by reason and the outcome rates |
| Client versions | Requests by `client` and `client_version`, and the share of SDK requests whose minor is more than three behind the deployment's ([SDKs, skills and agent tooling § 16](sdk-and-skills.md#16-observability)); no alert pages on it |
| Search and AI | Latency per mode and scope; degraded and partial share; agentic status mix; AI call failures by purpose; index job failures |
| Jobs and privacy | Erasure durations and status; retention purges per store; DLQ open items per queue |
| Domains | Domains per state and per method; transitions; check outcomes per resolver; SES identities against the 10,000 per Region |

## 7. Health checks

### 7.1 `GET /health`

- No authentication, no dependency calls, no tenant data, no alert state.
- `200 {"status": "ok", "version": "1.0.0", "commit": "abc1234", "env": "production"}` when the
  isolate's configuration is valid. `env` is `PM_ENV`, which [Configuration](../../reference/configuration.md#variables)
  says is shown here. When SES is configured, the body also has `"ses_region": "eu-west-2"` (the value
  of `PM_SES_REGION`), so anyone can see where AWS processes mail
  ([Domains on any DNS host §11](domain-connections.md#11-privacy-and-jurisdiction)). While the
  deployment is frozen for a restore (`PM_FREEZE = "on"`, [Restore from PITR](#restore-from-pitr)), the
  body also has `"frozen": true`; the status stays `ok`, because the Worker is doing what it was told.
- `200 {"status": "degraded", …}` when the Worker runs with a feature off because its configuration is
  incomplete: `"ses": "sns_topic_missing"` (SES credentials and region set, `PM_SES_SNS_TOPIC_ARN`
  missing: the SES transport is off), `"billing": "stripe_secrets_missing"` (`PM_BILLING=stripe`
  without its secrets: billing is not started) or `"cf_api_token": "missing"` (a Cloudflare REST fallback
  is in use without `PM_CF_API_TOKEN`, [Rust workspace §7](rust-workspace.md#7-wasm-bindgen-externs))
  ([Rust workspace › Startup rules](rust-workspace.md#61-errors-and-configuration)). The `Health` schema
  of `openapi.yaml` lists these fields.
- `503` with the error envelope (`unavailable`) when the configuration is invalid (a required variable
  or secret is missing or malformed, or an optional variable is malformed; `config_invalid` is logged
  with the variable's name, and the body's `details.config_invalid` names it). Every other handler
  returns the same.
- It is a liveness check. Dependency health is `idvy doctor`'s job.

### 7.2 `idvy doctor`

`doctor` runs every check, prints one line per check with `pass`, `warn` or `fail`, and a fix for each
failure (FR-OPS-3). Output format and exit codes are defined in [CLI and setup](cli.md). The checks:

| Check | Fails when | Fix printed |
|---|---|---|
| `dns.platform` | MX, SPF, DKIM or DMARC for the platform domain missing or different from the provider API's expected records, on either resolver | The exact record to add |
| `routing.catch_all` | The platform domain's catch-all rule does not target the Worker | The API call or dashboard step |
| `sending.domains` | A sending domain is not onboarded, or has `preview_enabled = true` ([Privacy](privacy.md#3-jurisdiction-and-residency)) | Onboarding step; `PATCH … {"preview_enabled": false}` |
| `sending.event_subscriptions` | A sending domain has no event subscription to `pm-delivery-events` (`delivery_events: "manual"`, the spike S9 fallback), or a subscription is left over from a removed domain | `idvy domains subscribe <domain>`; for a left-over one, `wrangler queues subscription delete <id>` |
| `bindings` | A resource in the generated `wrangler.toml` is missing; D1 or R2 jurisdiction differs from `PM_JURISDICTION`; a queue lacks its dead-letter consumer; a bound Vectorize index (`VECTORS`, and `VECTORS_NEXT` during a re-embed) is not cosine with the eight metadata indexes, or its dimensions differ from those of the model named in its description (`embed_model=…`): 1,024 for `@cf/baai/bge-m3`, otherwise the length of a probe embedding from that model, as the re-embed step does ( [Search §7.3](search.md#73-re-embed-job-embedding-model-change)); `METRICS` missing | The resource to create or `idvy setup` |
| `secrets` | A required secret is missing (names only; values are never read), or the slot named by `PM_MASTER_KEY_ACTIVE` has no secret. `warn` while a master-key rotation is unfinished (`master_key.remaining` > 0 in `GET /v1/platform/status`) | The secret to set, or `idvy secrets rotate-master --resume` |
| `observability` | `invocation_logs` is not `false`, or traces are enabled with `PM_ENV = production` | The `wrangler.toml` lines |
| `worker.version` | The deployed version differs from the CLI's | `idvy upgrade` |
| `health` | `GET /health` is not `200`, or reports `degraded` (a `warn` that names the feature that is off), or `frozen: true` (a `warn`) | Section 7.1; the missing variable or secret; `idvy ops unfreeze` once a restore is done |
| `agent_signup` (when `PM_AGENT_SIGNUP = "on"`; otherwise `skip`) | Never fails. `warn` when the feature is configured but inactive (`agent_signup.active: false` in `GET /v1/platform/status`; the reason is printed) or when the `agent_signup` switch is `off` ([Agent self-sign-up § 21](agent-signup.md#21-observability)) | The reason's fix; `idvy ops switch agent_signup on` once the cause is fixed |
| `alerts` | Any page alert is firing, the deployment has been frozen for more than 4 hours, or `alert_email` is `missing` (`GET /v1/platform/status`, which needs a platform key with `audit:read`). A firing ticket alert is a `warn` | The alert's runbook; set `PM_ALERT_EMAIL` |
| `dlq` | Open `dlq_items` exist | `idvy dlq list` (`GET /v1/platform/dlq`) |
| `quota` | Provider quota errors in the last 24 hours (Analytics Engine SQL API); `warn` when `PM_DAILY_SEND_QUOTA` is unset, and `warn` (never `fail`) when the operator's token lacks Account Analytics · Read, so the errors cannot be counted | [Quota exhausted](#quota-exhausted); the permission in [Deploy › step 2](../../self-hosting.md#2-create-a-cloudflare-api-token) |
| `web_bot_auth` (when `PM_WEB_BOT_AUTH = "on"`; otherwise `skip`) | `GET /.well-known/http-message-signatures-directory` does not answer `200` with `Content-Type: application/http-message-signatures-directory+json`, lists no key or more than three, or lacks a valid `http-message-signatures-directory` signature for each listed key ([Agent signing keys §3.2](agent-keys.md#32-web-bot-auth-key-directory)) | `idvy keys rotate web_bot_auth`; [Deploy › Signed HTTP requests](../../self-hosting.md#signed-http-requests-web-bot-auth) |
| `ses` (when `PM_SES_REGION` is set) | `GetAccount`: production access not enabled, or account sending paused; with SES receiving configured, the active receipt rule set is not `PM_SES_RULE_SET` or lacks `pm-deliver`; `PM_SES_REGION` cannot receive mail; the region holds 10,000 identities (new SES domains are refused with `ses_identity_limit`); a resource `setup ses` created no longer carries this deployment's `pylota-identity:api-host` tag, or `pm-deliver` writes to another bucket ([N31](../edge-cases.md)). `warn` at 9,000 or more identities (`ses_identities_90pct`), and when the region is outside the EU and the UK under `PM_JURISDICTION=eu` ([CLI › Doctor](cli.md#10-doctor)) | [SES account and receiving](#ses-account-and-receiving) |
| `domains.orphans` | Never fails. `warn` for a `zone_claims` row still `pending` an hour after it was written, or a `domain_onboarding` row older than two hours: the hourly cleanup did not remove it ([Identities and domains › Cleanup after a failed add](identity-domains.md#cleanup-after-a-failed-add)) | Names the zone (when one was created after the claim) to delete in the dashboard if no domain uses it |
| `routing.leftover_subdomains` (only if spike S9 showed that a subdomain's routing cannot be removed on its own) | Never fails. `warn` for a routing subdomain that no domain row names | The dashboard step: Email Routing › Settings › Subdomains |
| `cloudflare.zones` | Never fails. Prints the account's zone count, and `warn`s above 1,000, because the zone limit of a non-Enterprise account is not documented ([Domains on any DNS host §3.2](domain-connections.md#32-nameservers)) | Ask Cloudflare to confirm the account's zone limit |
| `security_txt` | `PM_SECURITY_CONTACT` unset (`warn`), or `Expires` within 30 days | Set the variable; upgrade |
| `mail_test` (`--mail-test`) | A message from the platform domain to a platform address does not arrive within 120 s with `verdict: pass` | Prints the observed authserv-id for `PM_TRUSTED_AUTHSERV_ID` |

## 8. Dead-letter queues

Every work queue has a dead-letter queue with a consumer (FR-OPS-4,
[Rust workspace](rust-workspace.md#8-generated-wranglertoml)): `pm-inbound-dlq`, `pm-outbound-dlq`,
`pm-delivery-events-dlq`, `pm-webhooks-dlq`, `pm-index-dlq`, `pm-screen-dlq`
([Screening hold § 5.1](screening.md#51-queue)), each with `max_batch_size = 100`.

### 8.1 Consumer

For each message:

1. Insert into `dlq_items` (`INSERT OR IGNORE` on `(queue, message_id)`, so a repeat is absorbed): a new
   `dlq_` ID, the source queue, the Cloudflare message ID, the body as received, its SHA-256, the
   `tenant_id` and `kind` named by the body if any, and `first_seen_at = now`.
2. Log `dlq_item` (queue, message ID, tenant, `kind` from the body; no body text) and count it.
3. `ack()`. A failed insert leaves the message for the dead-letter queue's own retry.

The alert evaluator turns open items into `dlq_items` metrics and the `dlq:{queue}` alert
([J8](../edge-cases.md)).

### 8.2 Table

`dlq_items` is defined in [Data model](data-model.md#1-d1-control-plane): a `dlq_` ID, the queue, the
Cloudflare message ID (unique per queue), the body as received, its SHA-256, the tenant and kind, and the
redrive bookkeeping. Rows are deleted after 14 days by the global retention job
([Privacy](privacy.md#53-global-retention-job)).

<a id="83-idvy-ops-dlq"></a>

### 8.3 Listing and redriving

Dead-letter items are read and redriven through the platform API, with a platform key holding
`platform:ops` ([REST API › Platform operations](../../reference/api.md#platform-operations)). The CLI
wraps it as `idvy dlq list` and `idvy dlq redrive` ([CLI and setup](cli.md)).

- `GET /v1/platform/dlq` lists items (filters `queue`, `status`, `tenant_id`). It returns the queue,
  kind, tenant and timestamps, never the stored body: pointers can carry envelope addresses.
- `POST /v1/platform/dlq/{dlq_id}/redrive` first checks that SHA-256 of `body_json` still equals
  `body_sha256`. A mismatch (the row was edited or damaged) is refused with `500 internal_error`, logged
  as `dlq_body_mismatch` and nothing is published. Otherwise it publishes the stored body back to its source queue through the
  Worker's own producer binding (`Q_INBOUND`, `Q_OUTBOUND`, `Q_DELIVERY`, `Q_WEBHOOKS`, `Q_INDEX` or `Q_SCREEN`), then
  sets `redriven_at` and increments `redrive_count` in the same request, and writes an `audit_log` row
  (`dlq.redrive`). No Cloudflare API token is involved. Every consumer is idempotent
  ([Design conventions](index.md#7-idempotent-queue-consumers)), so a redrive is safe to repeat.

## 9. Runbooks

| Runbook | Typical alert |
|---|---|
| [Bounce spike](#bounce-spike) | `bounce_rate:{tenant_id}:{domain_id}` |
| [Complaint spike](#complaint-spike) | `complaint_rate:{tenant_id}:{domain_id}` |
| [Quota exhausted](#quota-exhausted) | `provider_quota`, `provider_quota_80`, `quota_warning`, `shared_domain_breaker` |
| [Email Sending outage](#email-sending-outage) | `uncertain_spike`, `transport_breaker_open`, `delivery_orphaned`, outbound burn rules, `notification_send_failures` |
| [Domain failing](#domain-failing) | `domain_failing:{domain_id}`, `inbound_reject_spike`, `notification_send_failures` |
| [SES account and receiving](#ses-account-and-receiving) | `ses_object_lost`, `ses_sending_paused`, `ses_rule_missing`, `ses_identities_90pct` |
| [DLQ growth](#dlq-growth) | `dlq:{queue}`, `inbound_tempfail` |
| [Integrator API down](#integrator-api-down) | `webhook_failing`, `webhook_disabled` |
| [Parser bug](#parser-bug) | `panics`, `calendar_parse_failures`, reports of mis-parsed mail |
| [Compromised key](#compromised-key) | Report, unusual usage, `rpc_owner_mismatch` |
| [Abusive identity](#abusive-identity) | `abuse_pause`, `sending_pause`, `mailbox_size`, `system_mail_budget`, `arc_rescue_refused_spike` |
| [Erasure failure](#erasure-failure) | `erasure_stalled`, `erasure_failed`, `erasure_overdue`, `billing_cancel_failed`, `retention_expiry_overdue` |
| [Billing dispute](#billing-dispute) | `billing_dispute:{tenant_id}` |
| [Job failed](#job-failed) | `job_failed:{kind}` |
| [Heartbeat](#heartbeat) | `heartbeat_missing`, a failed heartbeat workflow run |
| [Capacity](#capacity) | `capacity_70:{resource}`, `capacity_80:{resource}` |
| [Automatic containment](#automatic-containment) | `containment:{rule}` |
| [Drafts overdue](#drafts-overdue) | `drafts_overdue` |
| [Event log](#event-log) | `event_log_append_failing`, `event_log_lag`, `event_log_size`, `event_stream_errors` |
| [Screening unavailable](#screening-unavailable) | `screening_unavailable`, `screening_model_failing`, `screening_model_cap`, `screening_stuck`, `screening_deadline_lag`, `screening_queue_wait`, `screening_suspect_spike` |
| [Agent sign-up abuse](#agent-sign-up-abuse) | `agent_signup_spike`, `agent_signup_cap_80`, `containment:agent_signup_off` |
| [Restore from PITR](#restore-from-pitr) | Data corruption, a bad migration, `inbound_lost` |

The runbooks are written for one operator working alone: every step is a command or an API call, the
automatic rules of section 5.6 have already made the deployment safe where they can, and nothing waits
for a second person. Every runbook ends by checking that the alert resolved and recording what was done
in the incident log: a private document kept next to the break-glass record (section 5.7), one entry per
incident with the alert key, the times, the commands run and any follow-up.

### Bounce spike

1. **Diagnose.** Overview and Outbound dashboards: which domain, which identities. Read
   `GET /v1/identities/{id}/messages?status=bounced` for samples; group `deliveries.smtp_code` and
   `bounce_type`. Hard bounces from one recipient domain usually mean stale addresses; soft bounces with
   `4.7.x` mean throttling or reputation.
2. **Mitigate.** Above 10% with ≥ 200 outcomes in an hour the tenant's sending is already paused (`containment:tenant_paused`,
   section 5.6). Otherwise pause the sending identities (`PATCH /v1/identities/{id} {"status": "paused"}`)
   if the integrator is sending to a bad list. Hard bounces already create suppressions (FR-DLV-2). If a
   recipient provider is throttling, lower `identity_daily_send_cap` for the affected tenant.
3. **Verify.** The bounce rate falls below 2% over the next hour; resume identities.

### Complaint spike

1. **Diagnose.** Which identities and message kinds. Check that marketing mail carries consent and
   unsubscribe headers (FR-OUT-8) and that the AI disclosure policy is applied.
2. **Mitigate.** Identities above 0.3% complaints over their last 1,000 sends are already paused
   (FR-DLV-3), and a tenant above 0.6% with ≥ 1,000 outcomes in 24 hours is already suspended (section 5.6). Pause the
   rest of the affected identities; suspend the tenant if the content is abusive
   (`PATCH /v1/tenants/{id} {"status": "suspended"}`). Complaint suppressions are permanent.
3. **Verify.** No new complaints for 24 hours before resuming, then watch the rate for a week.

### Quota exhausted

1. **Diagnose.** `provider_quota_errors_total` by `provider_code`: `E_DAILY_LIMIT_EXCEEDED` (daily
   quota) or `E_RATE_LIMIT_EXCEEDED` (rate). Cloudflare applies the daily quota per account and raises
   it automatically over time; it is not exposed to the Worker as a number (Email Service limits page,
   read 2026-10-09). With `PM_DAILY_SEND_QUOTA` set to the figure shown in the dashboard,
   `provider_quota_80` warns at 80%; without it, `provider_quota` fires on the first quota error. Update
   the variable when Cloudflare raises the quota.
2. **Mitigate.** Nothing is lost: definitely-not-sent messages stay `queued` and back off for up to
   24 hours ([G3](../edge-cases.md)). Request a higher limit from Cloudflare; move urgent domains to SES
   ([Email Sending outage](#email-sending-outage)) if they are pre-verified there. For a tenant's own
   cap (`quota.warning`, `429 daily_cap_reached`), raise `identity_daily_send_cap` or
   `tenant_daily_send_cap` in the tenant policy. For `shared_domain_breaker`, find the workspaces with
   the most sends today in `usage_daily`; suspend any that is sending spam (FR-TEN-3). The breaker ends
   by itself at 00:00 UTC; do not raise `PM_DAILY_SEND_QUOTA` above the figure Cloudflare shows to end
   it early.
3. **Verify.** `transport_outcomes_total{outcome=accepted}` resumes; no message reaches
   `failed: quota_exhausted`.

### Email Sending outage

1. **Diagnose.** Rising `uncertain` and `retry` outcomes or `E_INTERNAL_SERVER_ERROR` across all
   domains; check Cloudflare's status page. Uncertain messages are never resent automatically
   (FR-OUT-2).
2. **Mitigate ([J5](../edge-cases.md)).** For each affected domain that has a verified SES identity
   (`domains.ses_identity` set and its Easy DKIM records published), switch its transport with a
   platform key:

   ```bash
   curl -X PATCH https://mail.example.com/v1/domains/dom_01JA… \
     -H "Authorization: Bearer $PYLOTA_IDENTITY_KEY" -H "Content-Type: application/json" \
     -d '{"transport": "ses"}'
   ```

   The change is audit-logged and starts a health check, which re-checks alignment for the new
   transport. Domains without SES fall back per policy only if they are failing; otherwise their mail
   waits in the queue.
3. **Resolve uncertain sends.** After the outage, `message.reconciled` events settle most of them
   (FR-DLV-4). For the rest, the integrator checks with the recipient or its own records and calls
   `POST …/messages/{id}/resolve`.
4. **Verify.** Switch transports back (`{"transport": "cloudflare"}`) once Cloudflare reports recovery,
   and watch `delivery_events_total`.

### Domain failing

1. **Diagnose.** `GET /v1/domains/{domain_id}/health` lists `issues` with the record and fix;
   `GET /v1/domains/{domain_id}/records` shows expected versus observed per resolver. A single resolver
   disagreeing never changes state ([H7](../edge-cases.md)).
2. **Mitigate.** Sending already uses the identity's platform address (`sent_via_fallback`, FR-DOM-6).
   Give the operator the exact record from `issues[].fix`. For `suspended` (nameservers, ownership
   TXT or registration changed), issue a new ownership value with `POST /v1/domains/{id}/reprove`.
   For `inbound_reject_spike`, check whether a domain's routing points elsewhere or a sender is guessing
   addresses (dictionary attack); both are visible in `inbound_received_total` by result.
   For `notification_send_failures`, check the platform domain first: system mail (sign-in,
   invitations, notifications) has no fallback, so while it is `failing` notification sends fail
   (`reason` `domain_failing_no_fallback`) or are held back
   (`notifications_deferred_total{reason=platform_domain}`). The Notifier keeps the items and retries
   hourly for 24 hours ([O25](../edge-cases.md)); fixing the platform domain within that time loses nothing. If
   the platform domain is `healthy`, follow [Email Sending outage](#email-sending-outage).
3. **Verify.** `POST /v1/domains/{id}/verify` twice, a minute apart; the state returns to `healthy` and
   `domain.recovered` is emitted.

### SES account and receiving

1. **Diagnose.** `idvy doctor --check ses` shows production access, the sending status, the receipt
   rule set and the identity count. For `ses_object_lost`, the `ses_ingest` row with status `lost` names
   the object and recipient: both the SNS push and the SQS backstop failed to get the message ingested
   before the 14-day lifecycle rule deleted it. Look for `pm-inbound` dead-letter items and backstop
   cron errors in that period.
2. **Mitigate.**
   - `ses_sending_paused`: SES sends are already held (they never fall back to the platform domain), and
     end `failed` (`quota_exhausted`) 24 hours after submit. Follow AWS's instructions in the SES console
     to have sending resumed (the steps are AWS's; verify them at the time). For a domain that must keep
     sending meanwhile, the transport runbook's failover to Cloudflare is a decision for the owner
     ([J5](../edge-cases.md)).
   - `ses_rule_missing`: run `idvy setup ses` again. It is idempotent, adds `pm-deliver` to the active
     rule set and never deactivates another set. Until then, mail to SES domains does not reach the
     Worker.
   - `ses_identities_90pct`: the limit of 10,000 identities per Region can be raised only through the AWS
     account manager ([quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html), read 2026-10-09).
     Ask for it now, or point new customers at `nameservers` or `cloudflare_zone`. At 10,000, creating a
     domain that needs an SES identity fails with `422 transport_unavailable`
     (`details.reason = "ses_identity_limit"`).
   - `ses_object_lost`: the message cannot be recovered (the sender's server got a success reply). Tell
     the affected tenant, then fix why ingestion stalled (DLQ growth, a failing backstop).
3. **Verify.** `idvy doctor --check ses` passes and the alert resolves; for a lost object, new mail to
   the same recipient arrives.

### DLQ growth

1. **Diagnose.** `idvy dlq list --queue <queue>` (`GET /v1/platform/dlq?queue=…`): the `kind` and
   tenant of each item, and the matching `error` log lines (by `request_id` or `message_id`).
   `inbound_tempfail` means R2 writes are failing in `email()` ([J1](../edge-cases.md)): senders are
   retrying, nothing is lost.
2. **Mitigate.** Fix the cause (a bug: deploy the fix; a dependency outage: wait). Then
   `idvy dlq redrive --queue <queue>`, which calls `POST /v1/platform/dlq/{dlq_id}/redrive` for each
   open item. Items are kept for 14 days.
3. **Verify.** Redriven items leave the open set; `dlq:{queue}` resolves; for inbound items, the
   messages appear in their mailboxes.

### Integrator API down

1. **Diagnose.** `GET /v1/webhooks/{webhook_id}/deliveries?status=failed` shows error codes
   (`timeout`, `tls`, `dns`, `status_5xx`, `ssrf_blocked`). Deliveries retry for about 72 hours
   ([J4](../edge-cases.md)).
2. **Mitigate.** Nothing to do while the endpoint is down. After 100 consecutive failures over at least
   24 hours the endpoint is disabled. When the integrator is back: re-enable
   (`PATCH /v1/webhooks/{id} {"enabled": true}`) and replay what died:

   ```bash
   curl -X POST https://mail.example.com/v1/webhooks/whk_01J9…/replay \
     -H "Authorization: Bearer $PYLOTA_IDENTITY_KEY" -H "Content-Type: application/json" \
     -d '{"since": "2026-10-08T00:00:00Z", "until": "2026-10-09T00:00:00Z", "status": "dead"}'
   ```

   Replay reaches back 30 days from each event's `occurred_at` (or the tenant's
   `retention.events_days`, if shorter); older events cannot be replayed.
3. **Verify.** Replayed deliveries succeed; consumers deduplicate on `webhook-id`.

### Parser bug

1. **Diagnose.** Reproduce with the raw message (`GET …/messages/{id}/raw`, while within `raw_days`)
   against `crates/conformance`; add the case to the corpus with addresses rewritten to RFC 2606 names.
2. **Fix.** Release with the fix and an incremented `parser_version`.
3. **Re-parse ([J3](../edge-cases.md)).** Start a `reparse` job for each affected tenant with a platform
   key holding `platform:ops`: `POST /v1/platform/jobs` with `{"kind": "reparse", "tenant_id": "ten_…",
   "after": "<first affected date>"}` ([REST API › Platform operations](../../reference/api.md#platform-operations)).
   It re-parses affected messages from raw with the new `parser_version` and re-emits their events with
   `reprocessed: true` ([Inbound](inbound.md#re-parsing-j3)). Follow it with
   `GET /v1/platform/jobs/{job_id}`. Messages past `raw_days` cannot be re-parsed and are counted in the
   job's result.
4. **Verify.** Spot-check re-parsed messages and their `message.received` events with `reprocessed`.

### Compromised key

1. **Contain ([J6](../edge-cases.md)).** Revoke at once: `DELETE /v1/keys/{key_id}`. Revoke its
   descendants too: list keys and revoke every key whose `created_by_key_id` chain leads to the
   compromised key (revocation does not cascade).
2. **Investigate.** `GET /v1/audit-events?actor_key_id=key_…` lists the key's administrative actions.
   Sends are not audit rows (each is recorded by its message, events and delivery log): list the
   outbound messages of the identities the key reaches, and query Workers Logs for `key_id = <key>`
   over the last 7 days (route, status, `message_id`). Check webhooks created by the key (URLs pointing
   somewhere unexpected) and keys it created. If the key held `identities:sign`, its `signature_minted`
   lines name the identities it signed as; an assertion lives at most 10 minutes and a signed request at
   most 5, and the identity's private key was never exposed, so no identity key needs rotating for this
   alone.
3. **Remediate.** Rotate integrator secrets that may have been read through the key (webhook secrets
   with `rotate-secret`). Cancel queued sends made by the key (`POST …/cancel`). If the key was a
   platform key, review every tenant; if it was a partner key, review its partner's tenants
   (`GET /v1/tenants?partner_id=`), and to contain the partner at once, suspend it
   (`PATCH /v1/partners/{partner_id}` with `status: suspended`, a platform key): every key of the partner
   and every API key of its tenants then gets `403 partner_suspended`, so nothing sends for those tenants,
   their inbound mail is still stored, and deliveries to the partner's and its tenants' endpoints are held
   until it is `active` again ([J13](../edge-cases.md)). Before reactivating, rotate the partner's keys
   and check its endpoints' URLs, because held deliveries go out on reactivation. For
   `rpc_owner_mismatch`, treat it as a possible isolation bug: the deployment is already read-only for
   non-platform keys (`containment:read_only`, section 5.6). Capture the logged IDs, open a private
   security advisory, deploy the fix, and only then set the switch back
   (`idvy ops switch read_only off`).
4. **Verify.** Requests with the old key return `401 key_revoked`.

### Abusive identity

1. **Diagnose.** `identity.paused` with `reason: abuse_threshold` carries the complaint and bounce
   metrics. Review recent outbound messages and recipients.
2. **Mitigate.** Keep the identity paused (inbound continues). Resume only with a tenant, partner or platform key
   (only a platform key on a tenant a partner's key created, [J17](../edge-cases.md)) after the cause is fixed (`PATCH … {"status": "active"}`, audit-logged). For a whole tenant,
   suspend it. For `mailbox_size` (above 70% of 10 GB), set `retention.message_days` for the tenant or
   split traffic across identities; raw MIME and attachments are already in R2.
3. **Verify.** Rates stay below the thresholds for a week after resuming.

### Erasure failure

1. **Diagnose.** `GET /v1/erasure-requests/{id}` shows the status and the receipt so far.
   `erasure_stalled` and `erasure.failed` name the `step` and `error`; find `job_step` and `job_failed`
   log lines by `job_id`. The request's `deadline_at` is when NFR-PRV-1's 24 hours end.
2. **Mitigate.**
   - `erasure_stalled` (the job is still retrying, hourly at most, until `deadline_at − 4 h`): fix the
     cause (an outage: wait for it; a bug: deploy the fix). The job's next attempt continues from its
     cursor; nothing needs resubmitting.
   - `billing_cancel_failed`: the job is still retrying. Cancel the customer's subscriptions in the Stripe
     Dashboard (immediately, without proration or refund); the step's next attempt finds none left. Check
     `stripe_api_errors_total{call=subscription_cancel}` for the cause. Do it at once: until the
     subscriptions are canceled Stripe can still charge the workspace.
   - `erasure_failed` (the retry window ended with the step still failing): fix the cause, then run
     `idvy erasure retry <era_id>`. It sends a new request for the same scope and target, which resumes
     the failed job at its failed step and keeps `deadline_at`, for every scope
     ([Privacy § 6.1](privacy.md#61-request)); a counterparty erasure needs `--address` again, because the
     failed job deleted its copy. The alert resolves once the resumption exists; the receipt is
     cumulative.
   - `erasure_overdue` with no stall: the job is progressing but slowly (a very large tenant). Watch the
     receipt counts grow; there is nothing to restart.
   - **`retention_expiry_overdue`.** Read `retention_expiry_errors_total{step}` for the identity. `vectors` or
     `objects` errors clear when Vectorize or R2 recovers; `commit` errors are a bug: capture the logged IDs
     and open a private issue. Expiry resumes by itself at the next alarm.
3. **Verify.** The request (or its resumption) is `completed` (or `completed_with_holds`) with zero probe
   hits.

### Job failed

1. **Diagnose.** The alert detail lists the job IDs. `GET /v1/platform/jobs/{job_id}` returns any job
   that is not an erasure or an export, with its `result` (failed step and error code); find `job_step`
   and `job_failed` log lines by `job_id`.
2. **Mitigate.** Fix the cause. Then:
   - `retention`: the next day's run of the tenant's retention job retries the purge. Data past its
     retention is kept until a run completes;
   - `backup`: the next night's run copies everything uploaded since the last run that started, so a
     missed night is caught up;
   - `domain_remove`: restart the removal as [Identities and domains › Domain removal](identity-domains.md#domain-removal)
     says, so no routing rule, sending subdomain or SES identity is left behind;
   - `export`: the requester asks again (`POST /v1/exports`);
   - `reembed`, `reparse`, `reindex`, `restore_reconcile`: start the job again with the same body
     (`POST /v1/platform/jobs`).
3. **Verify.** A later job of the same kind and tenant completes; the alert resolves.

### Heartbeat

1. **Diagnose.** For a failed heartbeat run, its log shows which `doctor` check failed; follow that
   check's fix or alert runbook. For `heartbeat_missing` (no heartbeat for an hour), check in this order:
   GitHub Actions status; whether the workflow is disabled (`gh workflow view heartbeat.yml`); the
   account's Actions spending limit and billing, which can stop every run before it starts; the
   `HEARTBEAT_KEY` (an expired or revoked key gets `401`).
2. **Mitigate.** Re-enable the workflow (`gh workflow enable heartbeat.yml`; the person who re-enables a
   scheduled workflow becomes the one GitHub notifies), replace an expired key as section 5.5 describes (a rotation keeps the expiry), or fix the billing. Until the heartbeat runs again, run
   `idvy doctor --check health --check alerts` by hand twice a day.
3. **Verify.** A heartbeat run passes and `heartbeat_missing` resolves.

### Capacity

1. **Diagnose.** `idvy ops status` shows D1's size, the vector count and the tenant count with their
   percentages. For D1, `wrangler d1 info pylota-identity` confirms the size; the largest tables are normally
   `event_index` and `webhook_deliveries` (about four rows per inbound message).
2. **Mitigate.**
   - `capacity_70:*` (ticket): write the sharding ADR now. For D1 the first step is planned: move
     `event_index` and `webhook_deliveries`, which are read only per tenant or per endpoint and join
     nothing else, into a second D1 database (binding `DB_EVENTS`), then split by tenant if needed. For
     Vectorize: new tenants get their namespace in a second index (`pm-mail-chunks-s2`), recorded per
     tenant. Check that retention runs are completing (`job_failed:retention`).
   - `capacity_80:*` (page): carry out the ADR's first step. For D1, lower the default
     `retention.events_days` in `PM_DEFAULT_POLICY` meanwhile. At 90% the `emergency_prune` switch is
     turned on automatically ([J30](../edge-cases.md)).
   - Full: what happens is in [J30](../edge-cases.md) and [J31](../edge-cases.md); no mail is lost, but
     API writes (D1) or semantic indexing (Vectorize) stop until there is room.
3. **Verify.** The percentage falls below the threshold; the alert resolves after two runs.

### Automatic containment

1. **Diagnose.** The `containment:{rule}` email and the audit row (`ops.switch`, `partner.auto_suspend`,
   `tenant.auto_pause`) name the rule, the target and the values that triggered it.
   `idvy ops status` shows every switch.
2. **Mitigate.**
   - `read_only`: follow [Compromised key](#compromised-key) for `rpc_owner_mismatch`. Keep the switch on
     until the cause is fixed and deployed.
   - `partner_suspended`: review the partner's tenants and its keys' recent actions
     (`GET /v1/audit-events?actor_key_id=…`). If a key is compromised, follow
     [Compromised key](#compromised-key); rotate the partner's keys before reactivating it.
   - `tenant_paused`: follow [Bounce spike](#bounce-spike) or [Complaint spike](#complaint-spike);
     lift the pause (`PATCH /v1/tenants/{tenant_id}` `{"sending_paused": false}`) only once the cause is
     fixed.
   - `free_sending_off`: review the paused identities of Free and ramped workspaces; suspend abusive
     workspaces; then `idvy ops switch free_sending on`.
   - `emergency_prune`: follow [Capacity](#capacity); turn it off once D1 is below 80%.
   - `agent_signup_off`: follow [Agent sign-up abuse](#agent-sign-up-abuse).
3. **Verify.** The switch or status is back, the alert resolved, and the trigger has not recurred within a
   day.

### Drafts overdue

([Drafts § 18](drafts.md#18-observability))

1. **Diagnose.** Read `drafts_alarm_missed_total` and `draft_fire_lag_ms`: a missed alarm that the watchdog
   repaired raises only the first.
2. **Mitigate.** If the watchdog's pokes fail, the mailbox is erroring: read its logs by `identity_id`. A
   draft past its late limit is already `failed` with `send_window_missed` and in the workspace's "Needs a
   person"; nothing sends it without an explicit act.
3. **Verify.** The alert resolved and `draft_fire_lag_ms` is back to seconds.

### Event log

([Event feed § 16](event-feed.md#16-observability))

1. **Diagnose.** Find the tenants: `event_log_append_failures_total` and `event_log_lag_ms` by `index1`;
   `idvy dlq list --queue pm-webhooks`. Check the object: `idvy events list --tenant <tenant> --limit 1`
   with a platform key. Repeated `503 unavailable` means the object is failing; an `rpc_owner_mismatch` alert
   at the same time is a security incident ([Compromised key](#compromised-key)).
2. **Mitigate.**
   - Once the object answers, redrive the dead-letter items (`idvy dlq redrive`): each appends once and fans
     out once.
   - If it cannot recover (a schema or data fault), rebuild it: `idvy jobs start event_log_rebuild
     --tenant <tenant>` ([Event feed § 9.3](event-feed.md#93-rebuilding-a-log)), and tell the tenant, or its
     partner, that cursors and consumers must be reset.
   - For `event_log_size`: compare the tenant's inbound volume with its `webhook_text_bytes`. Lowering
     `webhook_text_bytes` shrinks new events; the budget keeps pruning the oldest. When the database size,
     not `log_bytes`, raised it, the 1,024-byte row charge is too low for this log: record the ratio of
     database size to `log_bytes` and raise the charge before the object fills.
   - For `event_stream_errors`: `1013` points at D1 (revalidation), `1011` at the object (read its error
     logs).
3. **Verify.** Appends succeed, the lag is back under 10 seconds, and the alert resolved.

### Screening unavailable

([Screening hold § 22](screening.md#22-observability))

1. **Diagnose.** Read the reasons in `screening_verdict_total` (model errors, `ai_rate_limited`, `model_cap`,
   `deadline_exceeded`) and the queue wait (`screening_queue_wait_ms`).
2. **Mitigate.**
   - For a Workers AI outage (`screening_model_failing`, or many `model_unavailable` reasons), tell affected
     tenants (their held mail is safe and releasable) and, where a tenant asks, switch its
     `screening.on_unavailable` to `deliver` with a platform or partner key until the outage ends, then back.
   - For `model_cap` or `screening_model_cap`, never switch to `deliver`: it does not apply to `model_cap`
     holds, and a flood that drained the cap may be meant to come before steering mail. Find the sender keys
     that spent it (`screening_model_capped_total{scope}`, the tenant's inbound volume), block them with a
     receive-block entry, or raise `triage.daily_model_cap` with a platform or partner key when the volume is
     legitimate.
   - For `invalid_output` across tenants, suspect a model change: check `PM_SCREEN_MODEL` and
     `screening_model_calls_total{outcome="length"}`.
   - For `screening_stuck`, the backstop has already expired the row, so no mail is waiting. A repeated
     condition for one identity means its mailbox's `screen` alarm purpose keeps failing: read the Worker's
     error logs for that `identity_id` and fix the handler, while the backstop, the next write to the mailbox
     and the daily maintenance keep ending holds meanwhile.
   - If `dlq:pm-screen` fires as well, redrive the dead-lettered backstop jobs
     ([§8.3](#83-listing-and-redriving)).
   - For `screening_suspect_spike`: look for one campaign (many senders, one pattern) or a regression that
     makes false positives (feedback with `false_positive` rising).
3. **Verify.** The alerts resolved and new verdicts are `clear` or `suspect` within their deadline.

### Agent sign-up abuse

([Agent self-sign-up § 21](agent-signup.md#21-observability))

1. **Diagnose.** List the last hour's sign-ups with `idvy agent-signups list --since <RFC 3339 time an
   hour ago>` (CLI times are RFC 3339 or a date); look for one network or source, then list that network
   with `--network <hash>`. For `containment:agent_signup_off`, read the bounce and complaint counts of
   sign-up mail and look at the sign-ups of the last day.
2. **Mitigate.**
   - `agent_signup_spike`: if it is abuse, `idvy ops switch agent_signup off`.
   - `agent_signup_cap_80`: decide whether to raise `PM_AGENT_SIGNUP_DAILY_CAP`.
   - `containment:agent_signup_off`: fix the cause, then switch it back on
     (`idvy ops switch agent_signup on`).
3. **Verify.** The alert resolved and the outcome rates in `GET /v1/platform/status` are back under the
   thresholds of [Agent self-sign-up § 13.2](agent-signup.md#132-the-automatic-switch).

### Billing dispute

1. **Diagnose.** The alert names the workspace. The audit row `billing.dispute_opened` holds the dispute
   and charge IDs; open the dispute in the Stripe Dashboard and read its reason. A stolen card
   (`fraudulent`) on a new workspace with fresh identities is the usual pattern.
2. **Mitigate.** Sends are already stopped and the workspace is on the default plan. If it looks like
   fraud, suspend the tenant (`PATCH /v1/tenants/{id} {"status": "suspended"}`) and accept the dispute in
   the Dashboard. Otherwise submit evidence in the Dashboard before Stripe's deadline. Nothing else is
   changed by hand: `charge.dispute.closed` restores the plan when the dispute is won, and cancels every
   subscription when it is lost ([Billing › Disputes and refunds](billing.md#disputes-and-refunds)).
3. **Verify.** After the close event, `billing_accounts.dispute_open_at` is `NULL`, the audit row
   `billing.dispute_closed` exists, and the alert resolved.

### Restore from PITR

D1 has Time Travel (30 days on Workers Paid) and SQLite-backed Durable Objects have point-in-time
recovery (30 days). R2 has no point-in-time recovery, no object versioning and no bucket replication
(`PutBucketVersioning` and `PutBucketReplication` are listed as not implemented on the R2 S3 API
compatibility page, last updated 2026-07-31, read 2026-10-09). The only copy of a deleted blob is the
optional backup bucket ([Privacy › R2 backup copy](privacy.md#54-optional-r2-backup-copy)).

A D1 Time Travel restore replaces the **whole** database, for every tenant, with its state at `T`
(D1 Time Travel docs, read 2026-10-09). Everything written after `T` would be lost or undone: keys,
identities, domains, webhook endpoints, signing and identity keys, the SES ingestion ledger,
suppressions, member removals, session revocations, suspensions, address and key tombstones, domain
removals and erasure records. So the runbook exports the database before it restores, and afterwards
re-applies every change made after `T` except what the restore is meant to undo ([I6](../edge-cases.md)).
The tooling is built by M17 Completion ([Build plan](../build-plan.md#m17--observability-and-operations-track-3)):
`idvy ops freeze|unfreeze|restore`, `POST /v1/platform/identities/{identity_id}/restore`, the
`restore_reconcile` job and `POST /v1/platform/erasure-requests/{erasure_id}/reapply`.

1. **Scope.** Decide what to restore: D1, one or more mailboxes, or both. Pick the target time `T`, just
   before the damage, and write down what the restore must undo (the tables, columns or rows the damage
   touched).
2. **Freeze the whole deployment:** `idvy ops freeze`. It sets `PM_FREEZE = "on"` under `[vars]` in the
   rendered `wrangler.toml` and redeploys the deployed bundle (a variable change only,
   [CLI and setup § 20](cli.md#20-ops)). The flag lives in the Worker's configuration, not in D1, so the
   D1 restore cannot undo it. Tenants are **not** suspended: suspension pauses identities, and the
   outbound consumer cancels the queued sends of a paused identity. While frozen:
   - `email()` throws before it reads anything, so every sending server gets a temporary failure and
     retries later (the [J1](../edge-cases.md) mechanism; counted as `tempfail_frozen`, which the
     NFR-REL-2 SLI and the `inbound_tempfail` alert leave out); nothing is accepted that a restore could
     lose;
   - `POST /hooks/ses`, `POST /hooks/ses/inbound` and `POST /billing/stripe/webhook` answer `503`, so SNS
     and Stripe retry; the SQS backstop cron does nothing, and S3 keeps raw SES mail for up to 14 days;
   - every API and console request answers `503 unavailable` (`details.reason = "frozen"`,
     `Retry-After: 300`) except `GET /health` and requests made with a platform key, which the runbook
     and the heartbeat use; the operator makes no other change while frozen;
   - every queue consumer re-enqueues each message with a 300-second delay and acks it, so no retry is
     used up and no send is canceled;
   - the crons do nothing but log `frozen`, and every Durable Object alarm re-arms itself 5 minutes later
     without doing its work (sends, job steps, domain checks, notifications, hold expiry), except the
     `JobRunner` alarms of `restore_reconcile` jobs and of erasure requests whose reason starts with
     `reapply_after_restore:` (step 5).
3. **Restore D1 and replay what came after `T`:**
   `idvy ops restore d1 --at <T> [--exclude <table>[.<column>]]… [--exclude-row <table>:<key>]…`.
   It refuses unless `/health` reports `frozen: true`, then:
   1. exports the whole database as it is now
      (`wrangler d1 export pylota-identity --remote --output <dir>/restore-<ts>/before.sql`, through the
      pinned Wrangler as every CLI call). An export blocks
      other requests to the database while it runs ([Import and export data](https://developers.cloudflare.com/d1/best-practices/import-export-data/),
      read 2026-10-10), which the freeze makes harmless;
   2. restores: `wrangler d1 time-travel info pylota-identity --timestamp=<T>`, then
      `wrangler d1 time-travel restore pylota-identity --bookmark=<bookmark>`. The restore is destructive and
      in place, and prints a bookmark that undoes it (D1 Time Travel docs, read 2026-10-09); the CLI
      writes that bookmark to `<dir>/restore-<ts>/undo-bookmark` and prints it;
   3. exports the restored database the same way (`after.sql`);
   4. loads both exports into in-memory SQLite and compares every table row by row on its primary key: a
      row only in `before` was created after `T` (insert it), a row only in `after` was deleted after `T`
      (delete it), a row that differs was changed after `T` (update it to the `before` values). It skips
      `schema_migrations`, every `--exclude` table or column and every `--exclude-row`: exactly what the
      restore is meant to undo. A column that `before` has and the restored schema lacks (a migration ran
      after `T`) is reported; re-apply the fixed migration with `idvy deploy` first, then run
      `idvy ops restore d1 --replay-only <dir>/restore-<ts>`;
   5. writes `replay.sql` in foreign-key order (parents inserted before children, children deleted
      before parents) and a summary of inserted, updated and deleted rows per table, prints the summary
      and asks for confirmation (`--yes` in scripts), then applies it with
      `wrangler d1 execute pylota-identity --remote --file <dir>/restore-<ts>/replay.sql` (a file of up to
      5 GiB, per the same page).

   The replay brings back, among the rest, every erasure request and its receipt, every revoked key,
   suppression, tombstone, member removal, session revocation, suspension and domain removal made after
   `T`, and the `ses_ingest` rows of mail received through SES after `T`. A D1 restore to before a
   finished master-key rotation still opens every sealed value: the previous key stays in its slot for
   30 days ([Security § 6.2](security.md#62-rotation-procedures)).
4. **Restore a mailbox:** `idvy ops restore mailbox <identity> --at <T>` calls
   `POST /v1/platform/identities/{identity_id}/restore` (`platform:ops`; `409 not_frozen` unless the
   deployment is frozen). Inside the object: `ctx.storage.getBookmarkForTime(T)`, then
   `ctx.storage.onNextSessionRestoreBookmark(bookmark)` (which returns an undo bookmark), then abort the
   object so it restarts restored (Durable Objects SQLite storage API, read 2026-10-09). `workers-rs`
   0.8.7 does not wrap these methods (docs.rs, read 2026-10-09), so `ops/restore.rs` adds externs on
   `Storage::as_raw()` ([ADR 0001](../adr/0001-rust-on-workers.md)). The response and the audit row
   `mailbox.restore` carry the undo bookmark; the same call with `{"bookmark": …}` instead of `at` undoes
   the restore. The PITR API is not available in local development, so drills run on staging.
5. **Reconcile and re-apply erasures** (R2 is not rewound). For each restored mailbox the CLI starts a
   `restore_reconcile` job (`POST /v1/platform/jobs` with `{"kind": "restore_reconcile", "tenant_id": …,
   "identity_ids": [ … ], "after": "<T>"}`), which:
   - re-queues the pointer of every inbound message received after `T` whose `raw.eml` still exists in R2
     and whose message is missing from the restored mailbox (ingest deduplicates on `raw_sha256`); the
     consumers process them after the freeze;
   - lists `t/{ten}/i/{idn}/out/` objects uploaded after `T` whose message is missing from the restored
     mailbox, and for each re-inserts the message from the stored MIME with status `uncertain` and flag
     `reprocessed`, plus an `idempotency` row from the object's `idem_key_sha256`, `fingerprint` and
     `operation` metadata, with `response_json` built from the re-inserted row. A retry with the same
     Idempotency-Key then replays instead of sending again, and the tenant resolves each `uncertain`
     message as usual. A refused send leaves no such object: the sent copy is written only after policy,
     quota and the thread lock succeeded, and deleted if the accept transaction then fails
     ([Outbound › Reservation](outbound.md#reservation-inside-the-mailbox-fr-out-1-g1)). Only that failure
     followed by a failed delete could leave one; the person resolving the re-inserted `uncertain`
     messages checks each against the provider's events and resolves such a message `not_sent`.

   Then, for every erasure request that completed after `T` and touched a restored mailbox (read from
   D1, which step 3 brought up to date), the CLI calls
   `POST /v1/platform/erasure-requests/{erasure_id}/reapply`, which re-runs it from the stored row with
   reason `reapply_after_restore:{era_id}`: message and thread scope from `identity_id` and `target_id`,
   identity scope from `identity_id`, tenant scope over the identities its receipt lists (the tenant
   stays `erased`), counterparty scope by `counterparty_hash` ([Privacy § 11](privacy.md#11-what-remains-after-deletion)).
   The CLI waits until every re-applied request and reconcile job has completed.
6. **Unfreeze and verify:** `idvy ops unfreeze`, then `idvy doctor --mail-test`. Queued work drains,
   senders' retries arrive, and the alert evaluator resumes. Last, `idvy ops restore cleanup` deletes
   the working files, which hold every D1 row ([Privacy § 11](privacy.md#11-what-remains-after-deletion)).

RPO and RTO (NFR-OPS-2): D1 and Durable Object recovery is continuous, which meets the 1-minute RPO for
indexes. R2 objects are written once, before the row that points to them, and deleted only by
retention and erasure; R2's durability covers infrastructure loss, which meets the 15-minute RPO for
blobs. Against a bug that deletes objects, nothing protects blobs by default; with `PM_BACKUP_BUCKET`
set (as on Cloud), the nightly copy limits the loss to objects created since the last run (RPO 24
hours). The 4-hour RTO, including the two exports of a full-size database, is rehearsed in the staging
drill.

## 10. Tests

| Test | Proves | Covers |
|---|---|---|
| `it::ops::j8_dlq_consumer` | A message forced into each dead-letter queue is recorded in `dlq_items`, counted, alerts after 15 minutes of fake time, is listed by `GET /v1/platform/dlq` without its body, and is redriven by `POST /v1/platform/dlq/{dlq_id}/redrive`; a non-platform key gets `403` | [J8](../edge-cases.md), FR-OPS-4 |
| `it::ops::provider_quota_80` | With `PM_DAILY_SEND_QUOTA` set, the evaluator fires `provider_quota_80` at 80% of the day's sends; unset, only the first quota error fires `provider_quota` | [G3](../edge-cases.md) |
| `it::ops::restore_rebuilds_ledger` | After a simulated mailbox restore, the `restore_reconcile` job re-inserts a send made after the restore point as `uncertain`, which then replays with its original key instead of sending again, and re-queues an inbound message received after it | NFR-OPS-2 |
| `it::ops::freeze_holds_everything` | With `PM_FREEZE = "on"`: `email()` gives a temporary failure; the SES and Stripe hooks answer `503`; tenant, partner and identity keys get `503 unavailable` (`frozen`) while platform keys work; a queued send stays `queued` (never `canceled`) and is re-enqueued with a delay; mailbox, domain and Notifier alarms re-arm without acting; `restore_reconcile` and `reapply_after_restore:` jobs still run; `/health` shows `frozen: true`; `POST /v1/platform/identities/{id}/restore` gets `409 not_frozen` when not frozen | section 9 (Restore from PITR), [I6](../edge-cases.md) |
| `it::ops::restore_replays_changes` | With the D1 export and restore steps faked by two SQLite snapshots (before and after a restore to `T`), the replay re-inserts an API key, an identity, a domain, a webhook endpoint, a signing key, an identity key and a `ses_ingest` row created after `T`; re-applies a suppression added, a member removed, a session revoked, a tenant suspended, an address tombstoned and a domain removed after `T`; leaves an `--exclude`d column at its restored value; and orders statements so no foreign key fails | [I6](../edge-cases.md) |
| `it::ops::j30_d1_capacity` | With the D1 size faked through `meta.size_after`: `capacity_70:d1` (ticket) at 70%, `capacity_80:d1` (page, emailed) at 80%, the `emergency_prune` switch and `containment:emergency_prune` at 90%, after which retention prunes `event_index` and `webhook_deliveries` at 7 days; a write failing with "Exceeded maximum DB size" answers `503 unavailable` (`storage_full`) while an inbound message is still accepted and its outbox event waits | [J30](../edge-cases.md) |
| `it::ops::j31_vector_capacity` | `capacity_70:vectors`, `capacity_80:vectors` and `capacity_70:namespaces` fire from the reconciliation's `index_count` and the tenant count; while `capacity_80:vectors` fires, a failing upsert leaves chunks `pending`, acks the index job without a retry and puts nothing in the dead-letter queue, and hybrid search reports `degraded` | [J31](../edge-cases.md) |
| `it::ops::j32_alert_channels` | A page alert sends one alert email through the system identity to `PM_ALERT_EMAIL` and another after 6 hours; ticket alerts arrive in one summary at 08:00 UTC; more than 20 emails in an hour collapse into one; `GET /v1/platform/status` reports `alert_email: missing` when no address is configured; `idvy doctor --check alerts` fails on a firing page alert and on `missing`; with the `read_only` switch on, an alert email from the system identity still leaves while a tenant's queued send is re-enqueued | [J32](../edge-cases.md), section 5.5 |
| `it::ops::j36_break_glass_key` | With `PM_BREAK_GLASS_KEY_ID` set and every session platform key expired, the break-glass key mints a new 1-day platform key; the request writes `key.break_glass_used` and fires `break_glass_key_used` (a page and an alert email) at once; any other key's use fires nothing; a revoked break-glass key gets `401`; with the variable unset nothing fires ([J36](../edge-cases.md)) | [J36](../edge-cases.md), section 5.7 |
| `it::ops::heartbeat_missing` | With `PM_HEARTBEAT_KEY_ID` set, `heartbeat_missing` fires when that key's `last_used_at` (or, never used, its `created_at`) is more than an hour old, or the key is revoked, and resolves after the key is used again; a replacement key the heartbeat has not used yet does not fire in its first hour | [J32](../edge-cases.md) |
| `it::ops::j33_auto_containment` | An `rpc_owner_mismatch` turns `read_only` on (non-platform writes `503 read_only`, queued sends re-enqueued and not canceled, inbound still stored); five auto-paused identities across three of one partner's tenants within an hour suspend the partner, and five in one tenant do not; a tenant above 10% bounces with ≥ 200 outcomes on one domain gets `sending_paused_at` with `sending_pause_reason = 'abuse_burst'` (sends `409 sending_paused`, queued sends held, inbound still stored), and its system identity's bounces count towards nothing; five auto-paused Free workspaces within an hour turn `free_sending` off (`403 policy_denied`, `free_sending_off`); each writes its audit row and pages `containment:{rule}`, which stays firing while the switch is set or the target is suspended and resolves two runs after a platform key undoes it; only a platform key undoes each | [J33](../edge-cases.md), section 5.6 |
| `it::ops::job_failed_alerts` | A retention job failing its tenth attempt fires `job_failed:retention` (page); a failed backup fires `job_failed:backup` (ticket); each resolves once a later job of the same kind and tenant completes | section 5.3 |
| `it::ops::domain_rate_state_alerts` | `RecordOutcome` with `domain_id` keeps hourly per-domain counters; 2 bounces in 51 outcomes within an hour fire `bounce_rate:{tenant_id}:{domain_id}` and 1 complaint in 200 outcomes within 24 hours fires `complaint_rate:{tenant_id}:{domain_id}`, for two domains of one tenant independently, with no Custom Alert; outcomes of the system identity are not counted | section 5.3 |
| `it::ops::platform_status` | `GET /v1/platform/status` with a platform key holding `audit:read` returns the freeze flag, the switches, `alert_email`, the firing alerts, the master-key slots with `remaining`, `activated_at` and `resealed_at`, and the capacity figures; a partner key gets `403` | section 5.5 |
| `it::events::append_failure_retries_then_dlq` ([Event feed § 20](event-feed.md#20-tests)) | With the object failing, no `Deliver` is queued for that tenant, another tenant in the batch is delivered, retries follow the age schedule, the messages reach `pm-webhooks-dlq` after 13 deliveries, and a redrive appends once and delivers once; `event_log_append_failing` fires | [R3](../edge-cases.md), section 5.3 |
| `it::events::size_budget_prune` | Above the budget the oldest events are deleted to 5,000,000,000 bytes of `log_bytes` (with a small budget in the test build), `floor_reason` is `size_limit`, `event_log_size` is reported, and a cursor behind the floor gets `410` with `size_limit`; a database size over the watch line reports `event_log_size` while `log_bytes` is under budget | [R8](../edge-cases.md), section 5.3 |
| `it::uploads::q100_restore_object_missing` ([Uploads](uploads.md)) | A `ready` upload with its object removed counts `uploads_expired_total{reason=object_missing}`, and the evaluator fires `upload_object_missing` only when no freeze is set | [Q100](../edge-cases.md), section 5.3 |
| `it::signup::platform_listing_and_status` ([Agent self-sign-up](agent-signup.md)) | `GET /v1/platform/status` and `idvy doctor` report the agent sign-up state; `agent_signup_spike` fires on a fixture of audit rows | FR-ASU-15, section 5.3 |
| `it::retention::z56_dependency_failure` ([Parity additions § 15](parity-additions.md#15-tests)) | Back-off, kept rows, `retention_expiry_errors_total` and the `retention_expiry_overdue` alert | [Z56](../edge-cases.md), section 5.3 |
| `it::logs::i5_no_content_in_logs` | No canary content or address in any captured log line, including `metric` lines | [I5](../edge-cases.md), FR-PRV-6 |
| `it::ops::metrics_emitted` | Each catalogued metric with its labels appears as `event = "metric"` lines for the flows that emit it; no metric carries a message or identity ID as a label | section 3 |
| `it::ops::alert_evaluator_transitions` | Fire on a true condition, one audit row, re-notify after 6 h, resolve after two false runs | section 5.4 |
| `it::ops::health_semantics` | `/health` needs no key, touches no binding, returns `503 unavailable` with an invalid configuration, and has `ses_region` exactly when `PM_SES_REGION` is set | section 7.1 |
| `it::ops::ses_alerts` | `ses_identities_90pct` fires at 9,000 counted identities; `ses_sending_paused` and `ses_rule_missing` fire from a fake `GetAccount` and rule set; the `ses` doctor check reports the same | section 5.3 |
| `it::ses::object_lost` | A lifecycle-deleted object sets the ledger row to `lost`, increments `ses_object_lost_total` and fires `ses_object_lost` | [N4](../edge-cases.md), NFR-REL-1 |
| `it::inbound::d4_backscatter_dropped` | `backscatter_total` increments | [D4](../edge-cases.md) |
| `it::inbound::d5_sender_throttle` | `inbound_throttled_total` increments, and 101 throttled messages in an hour meet the `inbound_throttled` alert condition | [D5](../edge-cases.md) |
| `it::send::g3_quota_backoff` | `provider_quota_errors_total` increments and the alert condition is met | [G3](../edge-cases.md) |
| `it::delivery::g8_race` | `delivery_orphaned_total` increments after the retry schedule | [G8](../edge-cases.md) |
| `it::notify::platform_domain_failing_retries` | With the platform domain `failing`, notification items are kept and retried hourly for 24 hours, `notifications_deferred_total{reason=platform_domain}` or `notifications_failed_total` increments, and `domain_failing:{domain_id}` fires for the platform domain | [O25](../edge-cases.md) |
| `it::notify::daily_caps` | The 51st notification for a person in a day goes to the digest and increments `notifications_deferred_total{reason=cap_person}` | [O24](../edge-cases.md) |
| `core::slo::burn_rate_targets` | The Custom Alert targets in section 5.2 follow from the formula | section 5.2 |
| `core::slo::alert_state_machine` | Transition rules are pure and deterministic | section 5.4 |
| `live::ops::metrics_reach_analytics_engine` | On staging, metrics written by a send are queryable through the SQL API | section 3 |
| `live::ops::restore_drill` | Staging drill of the whole runbook with `idvy ops`: freeze, D1 export, Time Travel restore and replay with one excluded table, a mailbox PITR restore, the reconcile job, re-applied erasures (one counterparty erasure by hash), unfreeze and the mail test, within 4 hours; mail sent to staging during the freeze arrives after it | NFR-OPS-2 |
| `live::ops::heartbeat_workflow` | On staging, the heartbeat workflow passes against a healthy deployment and fails when a page alert is forced to fire; the mail-test run passes | [J32](../edge-cases.md) |
| `it::ops::slo_from_metrics` | Each SLO row of section 4 (NFR-REL-1 to NFR-REL-4, NFR-PERF-1 to NFR-PERF-6, NFR-PRV-1) is computed by the SLO evaluator from metric lines that a scripted flow emitted, with the expected good and total counts | section 4 |
| `it::bench::send_api_p95` | 1,000 sends through the simulator in workerd: `send_api_ms` p95 ≤ 500 ms; reports the figure, CI warns above | NFR-PERF-1 |
| `it::bench::queue_to_transport_p95` | 1,000 queued sends: `outbound_queue_to_transport_ms` p95 ≤ 60 s | NFR-PERF-2 |
| `it::bench::hybrid_p95` | Hybrid search on the 50,000-message mailbox (bulk-seeded, nightly: [Testing § 6.9](testing.md#69-benchmarks)) with the fake AI at the recorded Workers AI latencies: p95 ≤ 800 ms (the real figure comes from staging in M20) | NFR-PERF-4 |
| `it::bench::tenant_fanout_p95` | Tenant search over 10 identities (bulk-seeded, nightly): p95 ≤ 1 s | NFR-PERF-5 |
| `it::bench::agentic_p95` | Agentic search with the scripted model at recorded latencies: p95 ≤ 8 s, first evidence ≤ 1.5 s | NFR-PERF-6 |
| `live::slo::inbound_to_webhook` | On staging, Gmail and Outlook mail to a webhook endpoint over the live run: p95 ≤ 30 s, p99 ≤ 120 s | NFR-REL-3 |
| `live::ops::idle_cost_review` | After a week of idling on staging, the Cloudflare usage report shows no compute beyond the cron and alarm invocations; recorded in the release notes | NFR-COST-1 |
