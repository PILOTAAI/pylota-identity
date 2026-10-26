# Configuration

There are three layers:

1. **Deployment configuration**: the Worker's bindings, variables and secrets, written into
   `deploy/wrangler.toml` by `idvy setup` and uploaded by `idvy deploy`.
2. **Tenant policy**: a JSON document per tenant, managed through the API. Defaults come from
   `PM_DEFAULT_POLICY` or the built-in defaults below.
3. **CLI configuration**: `~/.config/pylota-identity/config.toml` on the machine that runs `idvy`.

## Bindings

| Binding | Type | Name created by setup | Notes |
|---|---|---|---|
| `DB` | D1 | `pylota-identity` | Created with the chosen jurisdiction. It cannot be moved later |
| `BLOBS` | R2 | `pylota-identity-blobs` | Same jurisdiction. Lifecycle rules: delete `inbound-staging/` after 15 days (longer than the 14-day dead-letter retention), and abort incomplete multipart uploads after 1 day |
| `MAILBOX` | Durable Object namespace | class `IdentityMailbox` | SQLite-backed |
| `DOMAINS` | Durable Object namespace | class `DomainMonitor` | SQLite-backed |
| `JOBS` | Durable Object namespace | class `JobRunner` | SQLite-backed |
| `QUOTA` | Durable Object namespace | class `TenantQuota` | SQLite-backed |
| `SES_CONTROL` | Durable Object namespace | class `SesControl` | SQLite-backed. One object per deployment, used only when SES is configured: the SES control-plane token bucket |
| `NOTIFY` | Durable Object namespace | class `Notifier` | SQLite-backed. One object per tenant: coalescing, schedules and caps of notification email ([Notifications](../project/design/notifications.md#8-notifier-object)) |
| `EVENTS` | Durable Object namespace | class `TenantEvents` | SQLite-backed. One object per tenant: the tenant's event log, durable consumers and WebSocket streams ([Event feed](../project/design/event-feed.md)) |
| `Q_INBOUND` | Queue producer and consumer | `pm-inbound` (DLQ `pm-inbound-dlq`) | Batch 10, max retries 10, `retry_delay` 120 s, so ten deliveries of a message that keeps failing span about 18 minutes and its third attempt runs headers-only ([Inbound › Degraded mode](../project/design/inbound.md#the-pm-inbound-consumer)) |
| `Q_OUTBOUND` | Queue producer and consumer | `pm-outbound` (DLQ `pm-outbound-dlq`) | Batch 10, max retries 100. They count only unexpected errors: provider rate-limit, quota and relay back-offs re-enqueue a new message with a delay, so they never use them up; every back-off ends `failed` (`quota_exhausted`) 24 hours after submit |
| `Q_DELIVERY` | Queue consumer and producer | `pm-delivery-events` (DLQ `pm-delivery-events-dlq`) | Batch 10, max retries 20 (the G8 retry schedule, every 60 s for 15 minutes, needs at least 16). Fed by Email Sending event subscriptions; the producer is used only to redrive dead-lettered items |
| `Q_WEBHOOKS` | Queue producer and consumer | `pm-webhooks` (DLQ `pm-webhooks-dlq`) | Batch 20, max retries 13 |
| `Q_INDEX` | Queue producer and consumer | `pm-index` (DLQ `pm-index-dlq`) | Batch 10, max retries 10 |
| `Q_SCREEN` | Queue producer and consumer | `pm-screen` (DLQ `pm-screen-dlq`) | Batch 10, `max_batch_timeout` 1 s, max retries 3. Screening jobs, kept apart from `pm-index` so a backlog there cannot push held mail past its deadline ([Screening hold §5](../project/design/screening.md#5-the-screen-job)) |
| `Q_PHONE` | Queue producer and consumer | `pm-phone` (DLQ `pm-phone-dlq`) | Provider events, executions, reconciles, SMS dispatch and configuration. Created by `idvy setup` when `PM_PHONE` is `on`, or later ([Phone numbers](../project/design/phone-numbers.md)) |
| `VECTORS` | Vectorize | `pm-mail-chunks` | 1024 dimensions, cosine, 8 metadata indexes |
| `VECTORS_NEXT` | Vectorize | the new index of a re-embed | Only while an embedding-model change is re-embedding; `idvy deploy` adds and removes it ([Search design › Index lifecycle](../project/design/search.md#7-index-lifecycle)) |
| `AI` | Workers AI | – | Embeddings, rerank, triage, planner, `toMarkdown` |
| `EMAIL` | `send_email` | – | No address restrictions; the Worker enforces policy |
| `RL_API` | Rate limiting | – | 600 per 60 s, keyed by API key ID |
| `RL_TENANT` | Rate limiting | – | 1,800 per 60 s, keyed by tenant ID: every request made with a tenant or identity key of the tenant, on top of `RL_API` ([Security § 10](../project/design/security.md#10-rate-limiting-and-abuse)) |
| `RL_PARTNER_API` | Rate limiting | – | 1,800 per 60 s, keyed by partner ID: every request made with a partner key of the partner, on top of `RL_API` |
| `RL_SEARCH` | Rate limiting | – | 120 per 60 s, keyed by API key ID |
| `RL_AGENTIC` | Rate limiting | – | 20 per 60 s, keyed by API key ID |
| `RL_SEND` | Rate limiting | – | 120 per 60 s, keyed by identity ID |
| `RL_AI` | Rate limiting | – | 60 per 60 s, keyed by tenant ID. Asked before every Workers AI text-generation call (triage model, agentic planning and answer calls), so triage and agentic search together take at most a fifth of the account's 300 a minute per tenant ([Search › Indexing pipeline](../project/design/search.md#6-indexing-pipeline-pm-index), [F16](../project/edge-cases.md)). Screening's classifier asks the same binding under the key `screen:{tenant_id}`, a separate 60 a minute that triage and agentic search cannot spend ([Screening hold § 5.2](../project/design/screening.md#52-steps)) |
| `RL_EMBED` | Rate limiting | – | 600 per 60 s, keyed by tenant ID. Asked before every embedding request (an `Embed` batch of up to 16 texts, or a query embedding); the account limit is 3,000 a minute ([F16](../project/edge-cases.md)) |
| `RL_SIGNIN` | Rate limiting | – | 10 per 60 s, keyed by client network (`CF-Connecting-IP`; an IPv6 address by its /64 prefix). Applies to `POST /console/sign-in`, `/console/sign-in/link`, `/console/sign-in/code`, `/console/sign-in/verify`, `/console/sign-up` and `/console/waitlist`, and `GET /console/oauth/{provider}/start` ([Cloud sign-up](../project/design/cloud-signup.md#10-abuse-and-safety-on-cloud)), and `POST /v1/agent-signup` on the API host |
| `RL_SIGN` | Rate limiting | – | 600 per 60 s, keyed by identity ID. Agent assertions and signed HTTP requests together ([Agent signing keys](../project/design/agent-keys.md#6-permissions-limits-and-plans)) |
| `RL_PARTNER` | Rate limiting | – | 10 per 60 s, keyed by partner ID. `POST /v1/tenants` and `POST /v1/tenants/{tenant_id}/invitations` called with a partner key, across all of the partner's keys ([Security § 10](../project/design/security.md#10-rate-limiting-and-abuse)) |
| `METRICS` | Analytics Engine dataset | `pylota_identity_metrics` | Metrics and alerts ([Observability](../project/design/observability.md#3-metrics)). Holds IDs and counts only |
| `BACKUP` | R2 | the value of `PM_BACKUP_BUCKET` | Only when `PM_BACKUP_BUCKET` is set. Same jurisdiction as `BLOBS` |

Every work queue has a dead-letter queue with a consumer (batch 100) that records items in D1
(`dlq_items`). That makes twelve queues in all.

The Durable Object migrations (`new_sqlite_classes`: `IdentityMailbox`, `DomainMonitor`, `JobRunner`,
`TenantQuota`, `SesControl`, `Notifier` and `TenantEvents`, seven classes) are part of the generated `wrangler.toml` and are
versioned with the release ([Rust workspace › Generated wrangler.toml](../project/design/rust-workspace.md#8-generated-wranglertoml)). The generated file also turns invocation logs off and traces off in
production (`[observability.logs] invocation_logs = false`), because invocation logs record request URLs
and email recipients ([Observability › Signals](../project/design/observability.md#1-signals)).

Cron triggers:

- `* * * * *`: address retirement, the platform-event outbox sweep, restarting jobs left `queued`, the
  state-alert evaluator (with alert email, the automatic containment rules, and every 15th run the
  capacity checks), the master-key re-seal sweep (500 values per run), the SES inbound backstop (draining `PM_SES_INBOUND_QUEUE_URL`, when set), and
  minting the Durable Object IDs that only the Worker can mint for rows written outside it: the system
  identity's mailbox, the `DomainMonitor` of a domain row with `monitor_do_id = ''` (the platform domain,
  and domains added with `idvy domains add --local-token`), the `Notifier` of a tenant row with
  `notify_do_id = ''` (then `NotifierRequest::Init`), the `TenantEvents` object of a tenant row with
  `events_do_id = ''` (`EventsRequest::Init` follows on first use), and the `SesControl` object when SES is
  configured.
  Retrying stuck sends, transport claims and uncertain-send bookkeeping run in each mailbox's own alarms,
  not in the cron.
- `*/15 * * * *`: domain health scheduling, retention, usage roll-up, the
  nightly backup job once per UTC day when `PM_BACKUP_BUCKET` is set, and the new-workspace send-ramp
  evaluation once per UTC day (`crons/signup_ramp.rs`; it finds ramped Free workspaces with
  `PM_BILLING=stripe`, and ramped tenants of partners on any deployment), and the expiry of provisional
  agent sign-ups (`crons/agent_signup.rs`).

## Variables

| Variable | Default | Meaning |
|---|---|---|
| `PM_PLATFORM_DOMAIN` | – (required) | The shared mail domain. It must be a zone apex in this account |
| `PM_API_HOST` | – (required) | The host that serves the REST API (`/v1/*`, including signed links `/v1/links/*`), MCP (`/mcp`), `/openapi.json`, `/health`, `/.well-known/*` (the security contact, identity JWKS and the Web Bot Auth key directory), `/hooks/*` and the Stripe webhook (`/billing/stripe/webhook`), for example `mail.example.com`. It is the issuer (`iss`) of agent assertions. It also serves the console unless `PM_CONSOLE_HOST` names another host |
| `PM_JURISDICTION` | `eu` | `eu` or `default`. Applied to D1, R2 and Durable Objects at creation, where `eu` is Cloudflare's jurisdiction: the European Union only ([R2 data location](https://developers.cloudflare.com/r2/reference/data-location/), read 2026-10-09). For the SES region checked by `idvy setup ses`, `eu` means "EU or UK": the UK has an EU adequacy decision under the GDPR (European Commission [adequacy decisions](https://commission.europa.eu/law/law-topic/data-protection/international-dimension-data-protection/adequacy-decisions_en), renewed 19 December 2025, read 2026-10-09), so `eu-west-2` (London) is accepted |
| `PM_CF_ACCOUNT_ID` | – (written by setup) | The Cloudflare account ID. Needed by the Worker's own Cloudflare REST calls (zone onboarding, event subscriptions, the Email Sending suppression list) |
| `PM_ENV` | `production` | `production`, `staging` or `local`. Shown in `/health` and logs |
| `PM_EMBED_MODEL` | `@cf/baai/bge-m3` | Changing it starts a background re-embed (see [Search design › Index lifecycle](../project/design/search.md#7-index-lifecycle)) |
| `PM_EMBED_MODEL_PREVIOUS` | unset | Only during a re-embed: the old model, used for semantic reads until the new index is complete. `idvy deploy` sets and removes it |
| `PM_RERANK_MODEL` | `@cf/baai/bge-reranker-base` | Set to `none` to disable reranking |
| `PM_AGENT_MODEL` | `@cf/qwen/qwen3.8-27b` | The function-calling model for agentic search |
| `PM_TRIAGE_MODEL` | `@cf/openai/gpt-oss-20b` | A JSON-output model for triage |
| `PM_SCREEN_MODEL` | `@cf/openai/gpt-oss-20b` | The JSON-output model of the screening classifier ([Screening hold §5.3](../project/design/screening.md#53-the-model-call)) |
| `PM_AI_GATEWAY` | unset | Optional AI Gateway ID. Model calls go through it for logging and caching |
| `PM_TRUSTED_AUTHSERV_ID` | empty (set by `idvy setup`) | The `Authentication-Results` authserv-id stamped by Cloudflare's MX. `idvy setup` runs the mail test as its last step and writes the authserv-id it observed here; `idvy doctor --mail-test` prints it too. While it is empty, SPF cannot be read (Email Workers do not see the client IP), so a sender whose DMARC policy is `quarantine` or `reject` and who aligns only through SPF gets the verdict `unverified` and is quarantined as `auth_unverified`, never `fail`. Headers from any other authserv-id are always ignored |
| `PM_DOH_RESOLVERS` | `https://cloudflare-dns.com/dns-query,https://dns.google/resolve` | Two independent resolvers for domain checks and DKIM keys |
| `PM_SES_REGION` | unset | The AWS region of Amazon SES, for both directions: with the secrets `PM_SES_ACCESS_KEY_ID` and `PM_SES_SECRET_ACCESS_KEY` it enables the SES transport, and with the three `PM_SES_INBOUND_*` variables also SES receiving. `idvy setup ses` requires one of the 22 regions that receive mail ([SES endpoints](https://docs.aws.amazon.com/general/latest/gr/ses.html#ses_inbound_endpoints), read 2026-10-09), and a region in the EU or the UK when `PM_JURISDICTION=eu` unless it was run with `--allow-non-eu` |
| `PM_SES_SNS_TOPIC_ARN` | unset | The only SNS topic whose notifications `POST /hooks/ses` accepts. Required with the SES transport |
| `PM_SES_INBOUND_BUCKET` | unset | The S3 bucket of the receipt rule `pm-deliver`. Set with the two below, it enables `inbound = ses` (`dns_records`, and `smtp_relay` with `inbound: ses`). Without all three, those domains get `422 transport_unavailable` (`ses_receiving_not_configured`) ([Domains on any DNS host › Deployment set-up for SES](../project/design/domain-connections.md#42-deployment-set-up-for-ses)) |
| `PM_SES_INBOUND_TOPIC_ARN` | unset | The only SNS topic whose notifications `POST /hooks/ses/inbound` accepts |
| `PM_SES_INBOUND_QUEUE_URL` | unset | The SQS backstop queue subscribed to the same topic, drained by the every-minute cron |
| `PM_SES_RULE_SET` | `pylota-identity` | The active SES receipt rule set. It holds `pm-deliver` and the `pm-retired-{n}` rules, which only the `SesControl` object edits ([Domains on any DNS host § 4.6](../project/design/domain-connections.md#46-retired-and-unknown-recipients)) |
| `PM_CF_SUBDOMAIN_SETUP` | `off` | `on` allows the `delegated_subdomain` method (Cloudflare Enterprise accounts only, once spike S10 has passed). While it is `off`, that method gets `422 transport_unavailable` (`subdomain_setup_disabled`) |
| `PM_WEB_BOT_AUTH` | `off` | `on` publishes the Web Bot Auth key directory at `/.well-known/http-message-signatures-directory` and allows signed HTTP requests (`POST …/http-signatures`), for tenants whose policy has `web_bot_auth.allowed: true`. Turn it on only once spike S13 has passed; while it is `off`, those requests and the `web_bot_auth` key rotation get `422 web_bot_auth_disabled` and the directory `404` ([Agent signing keys](../project/design/agent-keys.md#5-signed-http-requests-web-bot-auth), [Deploy › Signed HTTP requests](../self-hosting.md#signed-http-requests-web-bot-auth)) |
| `PM_IDENTITY_KEY_OVERLAP_DAYS` | `7` | Days a rotated identity signing key stays `retiring`: still published in the identity's JWKS, no longer signing ([Agent signing keys › Keys](../project/design/agent-keys.md#2-keys)) |
| `PM_DAILY_SEND_QUOTA` | unset | The account's Email Sending daily quota, copied from the Cloudflare dashboard, less the daily volume of anything else that sends from the same account (on Pylota Identity Cloud, Pylota's own mail). Cloudflare does not expose it to the Worker. When set, an alert fires at 80% of it; when unset, the alert fires on the first quota error ([G3](../project/edge-cases.md)). When set it also turns on the shared-domain breaker (at 60% of the day's quota, Free and ramped workspaces stop sending; at 90%, every tenant does) and sizes the system identity's day for the system-mail budgets ([Cloud sign-up §10.2–10.3](../project/design/cloud-signup.md#102-system-mail-budgets)). Pylota Identity Cloud must set it, because its account's quota is shared with Pylota |
| `PM_BACKUP_BUCKET` | unset | Name of a second R2 bucket. When set, setup creates it in the same jurisdiction, binds it as `BACKUP`, and a nightly job copies new `t/` objects into it ([Privacy design](../project/design/privacy.md#54-optional-r2-backup-copy)). Off by default; Pylota Identity Cloud sets `pylota-identity-backup` |
| `PM_ALERT_EMAIL` | the `mailto:` address of `PM_SECURITY_CONTACT`, if it is one | The operator's address for alert email: each page alert at once and every 6 hours while it fires, and a daily summary of ticket alerts, sent through the system identity ([Observability › Alert email](../project/design/observability.md#55-alert-email-and-the-external-heartbeat)). When neither is set, `GET /v1/platform/status` reports `alert_email: missing` and doctor's `alerts` check fails |
| `PM_BREAK_GLASS_KEY_ID` | unset | The `key_…` ID of the long-lived platform key kept only in the operator's break-glass record. When set, every request authenticated with it pages (`break_glass_key_used`) and writes the audit row `key.break_glass_used` ([J36](../project/edge-cases.md)) |
| `PM_HEARTBEAT_KEY_ID` | unset | The `key_…` ID of the platform key that the external heartbeat workflow uses. When set, the `heartbeat_missing` alert fires once that key has not been used for an hour |
| `PM_FREEZE` | `off` | `on` freezes the whole deployment for a restore: inbound mail gets a temporary failure, queued work and alarms wait, and only platform keys can use the API. Set only by `idvy ops freeze` and `idvy ops unfreeze` ([Observability › Restore from PITR](../project/design/observability.md#restore-from-pitr)). A variable, so that a D1 restore cannot undo it. `idvy ops freeze` also writes `PM_FREEZE_SINCE` (RFC 3339), which `GET /v1/platform/status` reports as `frozen_since` |
| `PM_SCANNER_URL` | unset | Optional malware scanner endpoint for inbound attachments and uploads (see [Inbound](../project/design/inbound.md#attachment-safety) and [Uploads](../project/design/uploads.md#43-classification-and-the-scanner)) |
| `PM_SECURITY_CONTACT` | unset | Served in `/.well-known/security.txt`. When it is a `mailto:` URI and `PM_ALERT_EMAIL` is unset, alert email goes to that address |
| `PM_EVAL_REST` | `off` | Evaluation runs only: `on` sends every Workers AI and Vectorize call through the REST API with `PM_CF_API_TOKEN`, so `wrangler dev --local` needs no remote binding ([Testing § 9.2](../project/design/testing.md#92-running)). Read only when `PM_ENV = "local"`; ignored otherwise |
| `PM_LOG_LEVEL` | `info` | `error`, `warn`, `info` or `debug`. Content is never logged at any level |
| `PM_DEFAULT_POLICY` | `{}` | JSON merged over the built-in tenant policy defaults |
| `PM_CONSOLE` | `on` | `on` serves the console at `/console`; `off` removes its routes |
| `PM_QUARANTINE_KEY_RELEASE` | `on` | `on`: API keys with `quarantine:review` may release quarantined mail (`POST …/release`). `off`: only a signed-in person can, in the console, and API keys get `403 permission_denied` (FR-CON-6), except on a tenant whose policy has `quarantine.key_release: true` ([Tenant policy](#tenant-policy)). The same rule decides whether API keys may take the other decisions reserved for people: loosening a guard field of the policy and approving a service account ([Workspace policy §3](../project/design/workspace-policy.md#3-decisions-reserved-for-people)), and approving a draft (on a tenant with `drafts.key_approval: true`, a platform key or the tenant's own partner key may approve even with `off`). Pylota Identity Cloud sets `off`. With `PM_CONSOLE=off` it is always treated as `on` |
| `PM_CONSOLE_HOST` | the value of `PM_API_HOST` | The host that serves the console. When it differs from `PM_API_HOST`, console paths answer only on this host and API paths only on `PM_API_HOST`; anything else gets `404`, and no cookie is set or read on the API host. Every console POST must carry `Origin: https://{PM_CONSOLE_HOST}` (CSRF defence in depth), and console links in mail use that origin. Both host variables are compared with the request's `Host` as written, so they may carry a port, which only the local test harness uses (`console.localhost:8799`). It is read even with `PM_CONSOLE=off`, because invitation links use it |
| `PM_SIGNUP` | `closed` | Self-serve sign-up: `closed` (people join by invitation or `idvy setup --owner-email`), `waitlist` (double opt-in, invited in batches with `idvy waitlist invite`) or `open` ([Cloud sign-up](../project/design/cloud-signup.md#6-sign-up)) |
| `PM_SYSTEM_FROM` | `Pylota Identity <no-reply@{PM_PLATFORM_DOMAIN}>` | The display name and address of the **system identity**, which `idvy setup` creates on the default tenant and which sends sign-in, invitation and notification mail through the platform domain. Its local part may be a reserved name; it is never listed to tenants ([Identities and domains › The system identity](../project/design/identity-domains.md#the-system-identity)). Read even with `PM_CONSOLE=off` |
| `PM_DOCS_URL` | `https://pylotamail.com/docs` | The base URL of the published docs, used to build each error's `docs` link (`{PM_DOCS_URL}/reference/errors#code-{code}`). `https`, at most 200 bytes, no query or fragment; a trailing `/` is removed. `idvy setup` and `idvy deploy` refuse an invalid value; the Worker falls back to the default and logs `docs_url_invalid` |
| `PM_NOTIFICATIONS` | `on` | `on` sends notification email to people: usage alerts, new-mail notifications and the daily "needs a person" email, each as their preferences allow. `off` sends only `account` emails ([Notifications](../project/design/notifications.md)). Read even with `PM_CONSOLE=off` |
| `PM_TERMS_URL`, `PM_PRIVACY_URL`, `PM_DPA_URL` | unset | Terms of Service, Privacy Policy and Data Processing Addendum, linked from the sign-up checkbox. Required when `PM_SIGNUP` is not `closed` |
| `PM_TERMS_VERSION` | unset | The terms version stored on the user (`users.terms_version`) when they accept. Required when `PM_SIGNUP` is not `closed` |
| `PM_MAILBOX_PROVIDER_DOMAINS` | unset | Comma-separated organisational domains, in addition to the built-in list of shared mailbox providers that ships with each release, that a service-ledger entry may not name as an extra sender domain ([E15](../project/edge-cases.md)) |
| `PM_SIGNUP_BLOCKED_DOMAINS` | unset | Comma-separated domains refused at sign-up, before any mail is sent, in addition to the built-in list of disposable-mail domains that ships with each release |
| `PM_AGENT_SIGNUP` | `off` | `on` allows agent self-sign-up (`POST /v1/agent-signup`), only while `PM_SIGNUP` is `open`, billing is on and the console is on ([Agent self-sign-up](../project/design/agent-signup.md#1-when-agent-sign-up-is-on)). Pylota Identity Cloud keeps it `off` until gate D |
| `PM_AGENT_SIGNUP_DAILY_CAP` | `200` | Agent sign-ups per UTC day for the whole deployment, 1–10,000 |
| `PM_OAUTH_GOOGLE_CLIENT_ID` | unset | With the secret `PM_OAUTH_GOOGLE_CLIENT_SECRET`, enables "Continue with Google" |
| `PM_OAUTH_GITHUB_CLIENT_ID` | unset | With the secret `PM_OAUTH_GITHUB_CLIENT_SECRET`, enables "Continue with GitHub" |
| `PM_BILLING` | `off` | `off` (no plan checks and no usage alerts; only the daily caps in tenant policy apply) or `stripe` (plans, metering, Stripe checkout and portal) |
| `PM_PLAN_CATALOG` | built-in Cloud catalog | JSON plan catalog (see [Billing design](../project/design/billing.md#plan-catalog)), including each plan's Stripe price ID, the top-up price IDs and maximum quantity, and the three Customer Portal configuration IDs; `cargo xtask stripe-setup` writes it ([Billing › Stripe account setup](../project/design/billing.md#stripe-account-setup)) |
| `PM_BILLING_GRACE_DAYS` | `7` | Days a `past_due` workspace keeps its plan before Free limits apply |
| `PM_PHONE` | `off` | `on` enables phone numbers: the routes, `POST /hooks/telnyx`, the `pm-phone` consumer and the phone cron steps. With `off` every phone route answers `404 route_not_found` and `/hooks/telnyx` answers `503`. Turning it off never releases a number ([Phone numbers § 30](../project/design/phone-numbers.md#30-rollout-telemetry-and-rollback)) |
| `PM_TELNYX_API_BASE` | `https://api.telnyx.com/v2` | `https://api.telnyx.com/v2` or `https://api.telnyx.eu/v2`, checked at isolate start; no other host is accepted |
| `PM_TELNYX_ACCOUNT_ID` | – (required with `PM_PHONE=on`) | The Telnyx organisation ID, stored as `provider_account_id` on every phone row and checked against `GET /v2/balance` at isolate start |
| `PM_TELNYX_PUBLIC_KEY` | – (required with `PM_PHONE=on`) | The account's webhook public key (Ed25519, base64). Not secret. Without it `/hooks/telnyx` answers `503 unavailable` (`not_configured`) |
| `PM_PHONE_CATALOGUE` | built-in | JSON catalogue of countries and number types, validated at deploy and at isolate start; how the operator turns a row on or off ([Phone numbers § 3](../project/design/phone-numbers.md#3-the-country-catalogue)) |
| `PM_PHONE_RATE_CARD` | built-in | JSON rate card of GBP prices, versioned ([Phone numbers § 18.1](../project/design/phone-numbers.md#181-prices-currency-and-the-rate-card)) |
| `PM_PHONE_KYC_RETENTION_DAYS` | `365` | Days KYC documents are kept after the tenant's last number, until counsel sets a period per country |
| `PM_VAULT` | `off` | `on` enables the [vault](../guides/vault.md). With `off` every vault and consumer route answers `422 vault_disabled`; hidden `vault_code` mail stays hidden. Production keeps it `off` until the vault's last milestone is accepted |
| `PM_VAULT_REFUSED_DOMAINS` | unset | Comma-separated registrable domains added to the built-in refused list (`account.gov.uk`): no vault item may use an origin there |
| `PM_VAULT_CLEARANCE_COOKIES` | unset | Comma-separated cookie names added to the built-in clearance list (`cf_clearance`): dropped from every session state |

## Secrets

`idvy setup` generates the random ones (32 bytes from the OS CSPRNG, base64) and uploads them as Worker
secrets. Without `--print-secrets` they go only to the Worker (through Wrangler, on stdin), never to
stdout, a file or a log; with it they are printed once to stdout.

| Secret | Required | Used for (one purpose each; no secret is derived from another) |
|---|---|---|
| `PM_MASTER_KEY` | yes | Master-key slot `a`. AES-256-GCM encryption at rest of webhook secrets, identity signing keys, the Worker-generated thread, link and cursor keys and the Web Bot Auth deployment key, SMTP relay credentials, TOTP secrets and recovery codes, and OAuth PKCE verifiers ([Data model › Notes](../project/design/data-model.md#notes)) |
| `PM_MASTER_KEY_B` | no (written by the first `idvy secrets rotate-master`) | Master-key slot `b`. `PM_MASTER_KEY` is slot `a`. After a rotation one slot holds the active key and the other the previous one, kept for D1 restores ([Security design](../project/design/security.md#62-rotation-procedures)) |
| `PM_MASTER_KEY_ACTIVE` | no (absent means `a`) | `a` or `b`: the slot whose key seals new values. A secret, so that the CLI switches it with `wrangler secret put`. The Worker refuses to start when the named slot is empty |
| `PM_KEY_PEPPER` | yes | HMAC-SHA256 of API key secrets |
| `PM_HASH_KEY` | yes | Pseudonymisation: address tombstones, suppression hashes, log and query hashes |
| `PM_CF_API_TOKEN` | for some domain methods (for every deployment if a spike S6 REST fallback is taken) | Runtime automation of tenant domains (zone onboarding and creation, literal routing rules, event subscriptions), and the REST fallbacks for Vectorize and Workers AI if spike S6 fails ([Rust workspace §7](../project/design/rust-workspace.md#7-wasm-bindgen-externs)). Required on the Worker for the `cloudflare_zone`, `nameservers` (a token that can create zones) and `delegated_subdomain` methods; without it they get `422 cf_token_required`. `idvy domains add --local-token` with your own token can then add an apex `cloudflare_zone` domain only (catch-all, no literal rules). `dns_records`, `send_only` and `smtp_relay` need no Cloudflare token. Scope it to named zones (the platform domain's zone and every zone listed in a tenant's `domains.cloudflare_zones`); only a deployment that offers `nameservers` or `delegated_subdomain` needs All zones. Its permissions are in [Deploy › Create a Cloudflare API token](../self-hosting.md#2-create-a-cloudflare-api-token) |
| `PM_SES_ACCESS_KEY_ID`, `PM_SES_SECRET_ACCESS_KEY` | no | Amazon SES in both directions: sending (`dns_records`, `send_only`, failover), creating domain identities, and receiving (S3 objects, the SQS backstop, receipt-rule updates). `idvy setup ses` creates the IAM user with exactly one policy |
| `PM_STRIPE_SECRET_KEY` | only with `PM_BILLING=stripe` | Stripe API calls: create a Customer, create and retrieve Checkout Sessions, create Customer Portal sessions, read subscriptions with their latest invoice, retrieve a disputed charge, and cancel subscriptions (workspace deletion, duplicates, a lost dispute). A restricted key with exactly Checkout Sessions Write, Customer portal Write, Customers Write, Subscriptions Write, Invoices Read and Charges Read ([Billing › Stripe integration](../project/design/billing.md#stripe-integration)) |
| `PM_STRIPE_WEBHOOK_SECRET` | only with `PM_BILLING=stripe` | Verifying the `Stripe-Signature` header on `/billing/stripe/webhook` |
| `PM_OAUTH_GOOGLE_CLIENT_SECRET`, `PM_OAUTH_GITHUB_CLIENT_SECRET` | only with the matching client ID | The OAuth code exchange for Google and GitHub sign-in |
| `PM_TELNYX_API_KEY` | only with `PM_PHONE=on` (uploaded by `idvy setup phone`, read from stdin; generated by the operator in the Telnyx portal) | Every provider call. A leak lets someone buy numbers and send on the operator's account until it is rotated in the portal and here |

The vault adds no secret: its keys are generated by the Worker and sealed under `PM_MASTER_KEY`
([Security § 6.1](../project/design/security.md#61-inventory)).

Rotating `PM_KEY_PEPPER` (`idvy setup --rotate-pepper`) invalidates every API key, so it is a
break-glass action. Rotating
the master key uses `idvy secrets rotate-master`, which writes a new key into the slot that is not
active, switches `PM_MASTER_KEY_ACTIVE` to it and follows the Worker's re-seal until every stored value
is sealed with it. The previous key stays in its slot for D1 restores; a rotation within 30 days of the
previous one needs `--discard-previous`. No secret is ever read back from the Worker by the CLI
([Security design](../project/design/security.md#62-rotation-procedures)).

### Thread and link keys

The keys that sign thread tokens, signed links, search cursors and Web Bot Auth requests are not Worker
secrets. The Worker generates them (32 random bytes each), stores them in D1 `signing_keys` sealed under
`PM_MASTER_KEY`, and puts their key ID (kid) in everything it signs: one character for `thread`, `link`
and `cursor`, and the RFC 7638 thumbprint of the public key for `web_bot_auth`. No API, CLI command or log
ever returns a private key; the `web_bot_auth` public key is published in the key directory.

| Purpose | Signs | Rotate with | Old kid keeps verifying for |
|---|---|---|---|
| `thread` | Thread tokens in `Reply-To` sub-addresses | `POST /v1/platform/keys/thread/rotate` | 90 days |
| `link` | Signed attachment and export links, console sign-in, invitation and session tokens, OAuth state hashes | `POST /v1/platform/keys/link/rotate` | 7 days (the longest link lifetime) |
| `cursor` | Search cursors (`next_cursor`) | `POST /v1/platform/keys/cursor/rotate` | 24 hours (the cursor lifetime) |
| `web_bot_auth` | Signed HTTP requests (Web Bot Auth) and the key directory; only with `PM_WEB_BOT_AUTH=on` | `POST /v1/platform/keys/web_bot_auth/rotate` | 7 days (still listed in the key directory) |

All four need a platform key with `platform:ops` ([REST API](api.md#platform-operations)); the CLI is
`idvy keys rotate thread|link|cursor|web_bot_auth`. Identity signing keys are not in this table: each
identity has its own, rotated through `POST /v1/identities/{identity_id}/keys/rotate`
([Agent signing keys](../project/design/agent-keys.md)). Add `?revoke_previous=true` (`--revoke-previous`) after a suspected
leak: the previous kid is deleted at once instead of verifying for its window, so what it signed stops
working (thread tokens fall back to header threading; open links, sign-in tokens, invitations, sessions,
OAuth flows and cursors fail). Then rotate `PM_MASTER_KEY`.

## Tenant policy

Stored per tenant. Two routes deep-merge a policy write: `PATCH /v1/tenants/{tenant_id}` with
`{ "policy": { … } }`, which needs `tenants:manage`, so a platform key changes any tenant's policy and a
partner key the policy of the tenants its partner's keys created ([REST API › Partners](api.md#partners));
and `PATCH /v1/tenants/{tenant_id}/policy`, which needs `policy:write`, a permission tenant keys can hold
too, so a workspace changes its own policy ([REST API › Tenants](api.md#tenants)). Console owners and
admins change it on the policy page ([Workspace policy](../project/design/workspace-policy.md)). Partner
keys and workspaces may change only some fields, and some only downwards
([Who may change a field](#who-may-change-a-field)), so no partner or workspace can spend the shared
sending reputation or the AI budget of a Cloud deployment. This is the full document with defaults:

```json
{
  "identity_daily_send_cap": 500,
  "tenant_daily_send_cap": 5000,
  "max_recipients": 10,
  "send_allowlist_only": false,
  "large_attachments": "refuse",
  "link_ttl_hours": 72,
  "ai_disclosure": { "mode": "none", "text": "This message was written with the help of an AI assistant." },
  "auto_reply": {
    "allowed": true,
    "max_automatic_exchanges": 2
  },
  "quarantine": {
    "on_auth_fail": true,
    "spam_threshold": 0.8,
    "unsolicited_otp": true,
    "key_release": false
  },
  "inbound": {
    "per_sender_per_hour": 60,
    "unauthenticated_per_hour": 120,
    "per_tenant_per_hour": 2000,
    "extract_attachment_text": ["pdf", "office", "text", "html"],
    "extract_image_text": false,
    "ses_bounce_retired": true,
    "trusted_arc_sealers": ["google.com", "microsoft.com"]
  },
  "retention": {
    "raw_days": 90,
    "message_days": null,
    "events_days": 30
  },
  "triage": {
    "enabled": true,
    "daily_model_cap": 2000,
    "categories": null,
    "rules": []
  },
  "search": {
    "agentic_enabled": true,
    "agentic_daily_cap": 500,
    "agentic_max_steps": 6,
    "agentic_max_seconds": 8,
    "refs_packs": ["core"],
    "custom_refs": []
  },
  "webhook_text_bytes": 16384,
  "abuse": {
    "complaint_rate_pause": 0.003,
    "bounce_rate_pause": 0.05,
    "tenant_complaint_rate_pause": 0.001,
    "tenant_bounce_rate_pause": 0.05
  },
  "domains": {
    "allow_create_zone": false,
    "cloudflare_zones": []
  },
  "web_bot_auth": {
    "allowed": false
  },
  "accounts": {
    "require_approval": false
  },
  "vault": {
    "allowed": false,
    "key_approval": false,
    "min_approval": "grant",
    "codes_into_fills_only": false,
    "max_items": 1000,
    "session_max_ttl_days": 30,
    "approval_deadline_seconds": 600
  },
  "phone": {
    "allowed": false,
    "key_approval": false,
    "countries": null,
    "max_numbers_per_identity": 5,
    "max_numbers_per_tenant": 20,
    "auto_approve": { "enabled": false },
    "spend": { "tenant_monthly_cap_pence": null, "identity_monthly_cap_pence": null,
               "registration_cap_pence": null },
    "sms": { "identity_daily_cap": 200, "tenant_daily_cap": 2000, "tenant_monthly_cap": null,
             "allowed_destination_countries": null, "brand_name": null },
    "inbound": { "per_sender_per_hour": 30, "per_identity_per_hour": 600 },
    "calls": { "outbound": "approval", "allowed_destination_countries": null, "concurrent_limit": 2,
               "max_minutes": 15 },
    "recording": "off",
    "disclosure": { "calls": "required", "first_sms": "required", "call_text": null, "sms_text": null },
    "verification": { "redact": true }
  },
  "drafts": {
    "require_approval": "never",
    "key_approval": false
  },
  "screening": {
    "mode": "monitor",
    "on_unavailable": "quarantine",
    "deadline_seconds": 120
  },
  "events": { "labels_changed": false },
  "domain_fallback": true
}
```

| Field | Notes |
|---|---|
| `tenant_daily_send_cap` | With `PM_BILLING=stripe`, a new workspace on the Free plan has an effective cap of min(this value, 50) until `tenants.ramp_lifted_at` is set: for its first 7 days, then until the daily evaluation (the `*/15` cron's `crons/signup_ramp.rs`) finds its bounce and complaint rates under the `abuse` thresholds. A paid plan lifts it once its invoice is paid, and a disputed payment restores it. A tenant a partner's key created follows the same ramp whatever `PM_BILLING` and its billing mode (`exempt` included), unless a platform key set the partner's `ramp_exempt`; an `exempt` tenant has no plan, so only the daily evaluation lifts it. The system identity is never counted against this cap ([Cloud sign-up › New-workspace send ramp](../project/design/cloud-signup.md#101-new-workspace-send-ramp)) |
| `max_recipients` | 1–49. Cloudflare allows 50 recipients per message, and one is kept for the hidden journal copy of Message-ID strategy B ([Outbound design](../project/design/outbound.md#message-id-of-outbound-mail-spike-s7)) |
| `large_attachments` | `refuse`, or `link` (expiring signed links, `link_ttl_hours` 1–168). Applies to attachments from uploads too |
| `ai_disclosure.mode` | `none`, `footer` (appended to text and HTML) or `header` (`X-AI-Generated: true`) |
| `auto_reply.max_automatic_exchanges` | Automatic replies allowed per thread before a human must act ([D6](../project/edge-cases.md)) |
| `inbound.per_sender_per_hour`, `inbound.unauthenticated_per_hour`, `inbound.per_tenant_per_hour` | Inbound volume caps; the excess is stored `throttled`, never triaged, embedded or evented, and not counted in `storage_gb`. The per-sender cap counts mail that fails authentication under `unauth:` + the address, so a forged `From` cannot use a real sender's allowance; the other two count only senders who are not known correspondents ([Inbound › Inbound volume caps](../project/design/inbound.md#inbound-volume-caps-d5-d13), [D5](../project/edge-cases.md), [D13](../project/edge-cases.md)) |
| `inbound.trusted_arc_sealers` | ARC sealers (the `d=` of their `ARC-Seal`, A-label registrable domains, at most 20) whose verified seal can rescue mail that fails DMARC: verdict `arc_rescued`, not quarantined for authentication, never a source of verification codes ([Parity additions § 6](../project/design/parity-additions.md#6-trusted-arc-sealers-fr-in-15)). `["google.com", "microsoft.com"]` by default, the two verified on 2026-10-17; `[]` turns it off. Only a platform key can set it |
| `inbound.ses_bounce_retired` | `true` bounces mail to retired addresses on SES-receiving domains with `550 5.1.6`, through SES receipt rules; `false` drops it without a bounce ([Domains on any DNS host › Retired and unknown recipients](../project/design/domain-connections.md#46-retired-and-unknown-recipients)) |
| `quarantine.unsolicited_otp` | Quarantine password-reset and OTP mail that no `wait` asked for ([E5](../project/edge-cases.md)) |
| `quarantine.key_release` | `true` lets a platform key or the partner key of the tenant's own partner (never a tenant or identity key) release this tenant's quarantined mail even when `PM_QUARANTINE_KEY_RELEASE` is `off`, and take the other decisions reserved for people (loosening a guard field, approving a service account other than one it requested; [Workspace policy §3](../project/design/workspace-policy.md#3-decisions-reserved-for-people)). `false` by default. Only a platform key, or the partner key of the tenant's own partner, can set it (a tenant key cannot call `PATCH /v1/tenants/{tenant_id}`: `403 permission_denied`; with `policy:write` it gets `403 scope_denied` on `PATCH …/policy`). With `PM_QUARANTINE_KEY_RELEASE=on` it changes nothing. On Pylota Identity Cloud, Pylota's partner key sets it to `true` on each operator's tenant ([J14](../project/edge-cases.md), [J16](../project/edge-cases.md)) |
| `retention.message_days` | `null` keeps parsed messages indefinitely. A number deletes messages, attachments, index rows and vectors after that age, except held threads |
| `retention.events_days` | 1–365, default 30. Webhook delivery rows, the event index and the event payloads kept for replay are deleted after this many days. Webhook replay reaches back 30 days from an event's `occurred_at`, or this many days if fewer ([Privacy design › Retention](../project/design/privacy.md#52-steps-of-a-tenant-retention-job)). The tenant's event log keeps events this many days after they were appended, within its size budget ([Event feed § 2.5](../project/design/event-feed.md#25-retention-and-the-size-budget)) |
| `triage.daily_model_cap` | Model-backed triage runs per day in the tenant's time zone; past it, triage is rules-only until local midnight. It applies whatever the billing mode, so an `exempt` workspace still has a ceiling on model spend ([Triage § 12](../project/design/triage.md#12-cost-controls)). Screening's classifier counts against the same number on its own counter, at most a tenth of it per authenticated sender address (all unauthenticated mail is one sender) and at most four fifths for senders that are not known correspondents; past it a held message is quarantined as `screening_unavailable` whatever `screening.on_unavailable` says ([Screening hold § 5.2](../project/design/screening.md#52-steps)) |
| `triage.categories` | `null` uses the built-in list. Otherwise an array of up to 20 `{ "name": "pcn", "description": "Penalty charge notices from councils" }`, which replaces it |
| `triage.rules` | Deterministic rules. See [Triage](../guides/triage.md#rules) |
| `search.refs_packs` | `core` (amounts, phones, emails, domains, dates, invoice and order numbers) and optional `uk_vehicle` (plates, PCNs). There is no built-in pack for booking references: add them with `custom_refs` |
| `search.custom_refs` | Up to 20 `{ "name": "booking", "pattern": "BK-\\d{4,6}", "normalise": "upper" }`. Patterns use the `regex` crate syntax: linear time, no back-references, compiled size capped at 64 KB |
| `domains.allow_create_zone` | Lets the tenant's own keys, and its partner key, use the `nameservers` method, which creates a Cloudflare zone. `false` by default, and Pylota Identity Cloud keeps it `false`: Cloud's Worker token is limited to named zones and cannot create zones ([ADR 0010](../project/adr/0010-cloud-in-the-existing-cloudflare-account.md)). Without it, the request gets `422 transport_unavailable` (`zone_creation_not_allowed`). Platform keys may use the method when the Worker's token can create zones (it needs All zones, [Deploy › step 2](../self-hosting.md#2-create-a-cloudflare-api-token)). Only a platform key can set it |
| `domains.cloudflare_zones` | Zones of the deployment's Cloudflare account, by name (A-label apex, lower case, up to 50), that the tenant's own keys and its partner key may use with the `cloudflare_zone` method, besides the zones this deployment created for the tenant (`nameservers`, `delegated_subdomain`). A listed zone grants names strictly under it; its apex stays platform-only. An entry written `=name` (for example `=notify.pylota.io`) grants that one host name and nothing under it: list a zone you also use yourself only this way ([H18](../project/edge-cases.md)). `replace_mx` is platform-only on listed zones and exact names; a tenant may use it only on a zone created for it. `[]` by default. A zone created for another tenant, or one under the zones of `PM_PLATFORM_DOMAIN`, `PM_API_HOST` or `PM_CONSOLE_HOST`, is refused even when listed (`403 scope_denied`, `details.reason = "zone_not_allowed"`). Platform keys may use any zone. Each listed zone must also be in the zone list of the Worker's `PM_CF_API_TOKEN`. Dropping a zone while the tenant still has domains in it is refused with `409 domain_in_use` (`details.reason = "zone_has_domains"`) ([H13](../project/edge-cases.md)). Only a platform key can set it ([Identities and domains › Zone permission](../project/design/identity-domains.md#zone-permission)) |
| `web_bot_auth.allowed` | Lets the tenant's identities obtain signed HTTP requests (Web Bot Auth). `false` by default, and until it is `true` those requests get `403 policy_denied`. Only a platform key can set it: a tenant or partner key cannot turn it on. It has no effect while `PM_WEB_BOT_AUTH` is `off` ([Agent signing keys](../project/design/agent-keys.md#5-signed-http-requests-web-bot-auth)) |
| `domain_fallback` | `false` fails sends on a failing domain instead of using the platform address. A domain or tenant paused for its complaint or bounce rate never falls back ([G12](../project/edge-cases.md)) |
| `abuse.tenant_complaint_rate_pause`, `abuse.tenant_bounce_rate_pause` | Rates over the last 7 UTC days, with at least 500 outcomes, at which the tenant's sending, or one of its sending domains, is paused (`sending_paused_at`). The complaint default, 0.001, is Amazon SES's review rate ([Outbound › Tenant and domain auto-pause](../project/design/outbound.md#tenant-and-domain-auto-pause-g12)). Shown, with the current rate and headroom, by `GET /v1/metrics/rates` |
| `accounts.require_approval` | `true` turns on the [service sign-up ledger](../project/design/service-accounts.md) gate: a verification code or link from a service reaches the tenant's agents only when an approved ledger entry matches the mail, otherwise the message is quarantined `account_unapproved`, and `wait` with `kind=verification` needs an approved entry (`403 policy_denied`, `details.reason = "account_not_approved"`). `false` by default; Pylota Identity Cloud sets it to `true` in `PM_DEFAULT_POLICY`. Lower-only, with `false` as the looser value, so a partner or workspace cannot turn it off where the deployment default or a ceiling has it on |
| `drafts.require_approval` | `never`, `new_recipients` (approval when a recipient is neither known nor internal to the workspace), `external` (approval when a recipient is not internal) or `always`. It applies to draft sends and to direct sends, which get `403 approval_required` when approval is needed; an identity's `send_policy.require_approval` can only make it stricter. A guard field: a tenant key may loosen it only where API keys may take decisions reserved for people ([Drafts §3](../project/design/drafts.md#3-approval-policy)) |
| `drafts.key_approval` | `true` lets a platform key or the partner key of the tenant's own partner (never a tenant or identity key), holding `drafts:approve`, approve this tenant's drafts even when `PM_QUARANTINE_KEY_RELEASE` is `off`. `false` by default. Only a platform key or the tenant's own partner key can set it; `quarantine.key_release` does not imply it. Pylota sets it on its operators' tenants |
| `screening.mode` | `off`, `monitor` (default: mail is delivered at once and screened alongside, with a `message.screened` event) or `hold` (mail waits, hidden from agents, in status `screening` until its verdict). Guard field ([Screening hold §2](../project/design/screening.md#2-policy)) |
| `screening.on_unavailable` | `quarantine` (default: no verdict by the deadline holds the message as `screening_unavailable`) or `deliver`. `deliver` applies only when the service failed (`model_unavailable`, `ai_rate_limited`, `deadline_exceeded`); a reached model cap and invalid classifier output are always held. Guard field |
| `screening.deadline_seconds` | 30–900, default 120: the longest a held message waits for its verdict. Guard field: a tenant key may lower it only while `on_unavailable` is `quarantine` |
| `events.labels_changed` | `true` emits `message.labels_changed` for label changes, delivered only to endpoints that name it. `false` by default |
| `phone.allowed` | Phone numbers may be used. Lower-only; on Pylota Identity Cloud only a platform key or the tenant's partner key turns it on |
| `phone.key_approval` | A platform key or the tenant's partner key may approve phone requests, decide port-outs and set or loosen the standing budget, with `on_behalf_of`. Only those keys set it |
| `phone.countries` | Countries the tenant may buy in. Lower-only |
| `phone.max_numbers_per_identity`, `phone.max_numbers_per_tenant` | Live numbers, counting pending purchases (at most 20 and 200). Lower-only |
| `phone.auto_approve` | The standing purchase budget: `enabled`, `countries`, `number_types`, `max_numbers_per_identity`, `max_new_numbers_per_month`, `max_setup_pence_per_number`, `max_monthly_pence_per_number`, `expiring_only`; `version`, `set_by` and `set_at` are written by the service. A guard object: setting it or loosening any field is a decision reserved for people; tightening needs `phone:manage` ([Phone numbers § 5.7](../project/design/phone-numbers.md#57-the-standing-purchase-budget)) |
| `phone.spend.tenant_monthly_cap_pence`, `phone.spend.identity_monthly_cap_pence` | Monthly phone spend caps. Lower-only |
| `phone.spend.registration_cap_pence` | A monthly cap for one-off registration fees (the US 10DLC brand and campaign, toll-free verification), which count here instead of the monthly caps. Lower-only |
| `phone.sms.*` | Segment caps per day and month; the destination countries (each also needs the sending number's row to have tested it); the brand name in auto-responses and disclosures |
| `phone.inbound.*` | Inbound SMS throttles per sender and per identity |
| `phone.calls.outbound` | `off`, `approval` (default) or `allowed`. A guard field |
| `phone.calls.*` | Destination countries, concurrency and maximum call length. Lower-only |
| `phone.recording` | `off` or `with_announcement`. A guard field |
| `phone.disclosure.*` | AI disclosure on calls and the first SMS of a conversation (guard fields), and their texts |
| `phone.verification.redact` | Codes shown as `[code]` in reads and events. A guard field |
| `vault.allowed` | `true` lets the tenant use the vault. `false` by default. Platform-only: only a platform key sets it; turning it off closes every open use ([Vault § 15](../project/design/vault.md#15-kill-switches-and-incident-response)) |
| `vault.key_approval` | `true` lets a platform key or the partner key of the tenant's own partner (never a tenant or identity key), holding `vault:approve`, approve this tenant's vault grants and uses, naming the person with `on_behalf_of`. `false` by default. Only a platform key or the tenant's own partner key can set it. Pylota sets it on its operators' tenants |
| `vault.min_approval` | The floor of every item's approval level: `grant` (default), `per_run` or `per_use`. Guard field: tightening means a level at or above the current one |
| `vault.codes_into_fills_only` | `true` withholds every verification message (mail and SMS) from agents and delivers codes only into vault phases; `wait` with `kind=verification` answers `403 policy_denied`. `false` by default. Guard field (`true` is stricter) |
| `vault.max_items` | Vault items per tenant, 1–10,000, default 1,000. Lower-only |
| `vault.session_max_ttl_days` | The longest session TTL, 1–30 days, default 30. Lower-only |
| `vault.approval_deadline_seconds` | How long a use waits for a person, 60–900 seconds, default 600. Lower-only |

On Pylota Identity Cloud, `PM_DEFAULT_POLICY` sets `phone.spend` to `{2000, 1000, 6000}`. `null` for a country
list means every catalogue country.

**Provisional agent sign-ups.** While a tenant is a provisional agent sign-up, `core::policy::effective`
applies a fixed overlay after the stored policy, taking the stricter value of each field: both daily send
caps 10, `max_recipients` 1, `send_allowlist_only`, `quarantine.on_auth_fail`, `quarantine.unsolicited_otp`
and `accounts.require_approval` `true`, `quarantine.spam_threshold` 0.5, `screening.mode` `hold` with
`screening.on_unavailable` `quarantine`, `auto_reply.allowed`,
`inbound.extract_image_text`, `triage.enabled` and `search.agentic_enabled` `false`,
`triage.daily_model_cap` 50, inbound caps 20, 20 and 100, `large_attachments` `refuse`. It is not stored and
ends with verification ([Agent self-sign-up §4.2](../project/design/agent-signup.md#42-policy-overlay)).
`drafts.require_approval` is not in it: step 16a of a send does not apply to a provisional tenant, whose sends
reach only its human.

### Who may change a field

Every policy write is checked against this table: the `policy` of `POST /v1/tenants`, of
`PATCH /v1/tenants/{tenant_id}`, of `PATCH /v1/tenants/{tenant_id}/policy` and of the console's policy page.
Only the fields present in the write are compared; one refused field refuses the whole write and nothing
is stored. Platform keys may set every field. A **workspace writer** is a tenant key holding
`policy:write` or a console owner or admin of the workspace ([Workspace policy](../project/design/workspace-policy.md)).
Identity keys cannot write the policy at all: they can hold neither `tenants:manage` nor `policy:write`.

| Class | Fields | A partner key | A workspace writer |
|---|---|---|---|
| Platform-only | `web_bot_auth.allowed`, `domains.allow_create_zone`, `domains.cloudflare_zones`, `inbound.trusted_arc_sealers`, `vault.allowed` | `403 scope_denied` with `details.field` | `403 scope_denied` with `details.field` and `details.reason = "not_writable"`  |
| Platform or own partner | `quarantine.key_release`, `drafts.key_approval`, `phone.allowed`, `phone.key_approval`, `vault.key_approval` | May set it on the tenants of its own partner, at creation and later | `403 scope_denied` with `details.field` and `details.reason = "not_writable"` ([J14](../project/edge-cases.md))  |
| Lower-only | `identity_daily_send_cap`, `tenant_daily_send_cap`, `max_recipients`, `auto_reply.allowed`, `auto_reply.max_automatic_exchanges`, `inbound.per_sender_per_hour`, `inbound.unauthenticated_per_hour`, `inbound.per_tenant_per_hour`, `inbound.extract_image_text`, `retention.raw_days`, `retention.events_days`, `triage.enabled`, `triage.daily_model_cap`, `search.agentic_enabled`, `search.agentic_daily_cap`, `search.agentic_max_steps`, `search.agentic_max_seconds`, `abuse.complaint_rate_pause`, `abuse.bounce_rate_pause`, `abuse.tenant_complaint_rate_pause`, `abuse.tenant_bounce_rate_pause`, `accounts.require_approval`, `phone.countries`, `phone.max_numbers_per_identity`, `phone.max_numbers_per_tenant`, `phone.spend.*`, `phone.sms.identity_daily_cap`, `phone.sms.tenant_daily_cap`, `phone.sms.tenant_monthly_cap`, `phone.sms.allowed_destination_countries`, `phone.calls.allowed_destination_countries`, `phone.calls.concurrent_limit`, `phone.calls.max_minutes`, `vault.max_items`, `vault.session_max_ttl_days`, `vault.approval_deadline_seconds` | May set a value at or below the field's ceiling; above it, `403 scope_denied` with `details.field` | May set a value at or below its workspace ceiling; above it, `403 scope_denied` with `details.field`, `details.reason = "above_ceiling"`, `details.ceiling` and `details.ceiling_source` ([J23](../project/edge-cases.md))  |
| Guard | `send_allowlist_only`, `quarantine.on_auth_fail`, `quarantine.spam_threshold`, `quarantine.unsolicited_otp`, `drafts.require_approval`, `screening.mode`, `screening.on_unavailable`, `screening.deadline_seconds`, `phone.auto_approve` (setting it or loosening any field is a decision reserved for people), `phone.calls.outbound`, `phone.recording`, `phone.disclosure.calls`, `phone.disclosure.first_sms`, `phone.verification.redact`, `vault.min_approval`, `vault.codes_into_fills_only` | May set any valid value | A person may set any valid value. A tenant key may only tighten them (a value at least as strict as the current one), unless API keys may take decisions reserved for people on this tenant; otherwise `403 permission_denied` with `details.field` and `details.reason = "person_required"` ([J25](../project/edge-cases.md))  |
| Free | Every other field: `large_attachments`, `link_ttl_hours`, `ai_disclosure`, `inbound.extract_attachment_text`, `inbound.ses_bounce_retired`, `retention.message_days`, `triage.categories`, `triage.rules`, `search.refs_packs`, `search.custom_refs`, `webhook_text_bytes`, `domain_fallback`, `events.labels_changed` | May set any valid value | May set any valid value |

- **Ceiling.** A lower-only field's ceiling for a partner key is the more restrictive of the deployment
  default (the built-in defaults above merged with `PM_DEFAULT_POLICY`) and the tenant's platform ceiling,
  the value a platform key last set on that field, at creation or by `PATCH` (kept in
  `tenants.policy_ceilings_json`): min(deployment default, platform ceiling). A platform key's `null` for
  the field removes its platform ceiling. A higher number is always the looser value, the `abuse`
  thresholds included (a higher rate makes auto-pause more lenient), and so are longer retention, more
  steps or seconds, and more automatic exchanges.
- **Workspace ceiling.** For a workspace writer the ceiling is also bounded by the **partner ceiling**: for a
  tenant a partner's key created, the value that partner's key last set on the field, at creation or later
  (kept in `tenants.partner_ceilings_json`; the partner key's `null` removes it). So a partner's tenant can
  lower what its partner set, and raise it back to that value, never above it ([J26](../project/edge-cases.md)).
  `details.ceiling_source` says which bound decided: `deployment`, `platform` or `partner`.
- **Switches.** For `auto_reply.allowed`, `inbound.extract_image_text`, `triage.enabled` and
  `search.agentic_enabled`, `true` is the looser value (it sends more mail or spends Workers AI). For
  `accounts.require_approval`, `false` is the looser value (it lets codes from unapproved services reach
  agents). A partner key or workspace writer may always set the stricter value, and the looser one only when
  every bound that applies to it (the deployment default, the platform ceiling, and for a workspace writer
  the partner ceiling) allows it.
- **`null` from a partner key or workspace writer** on a lower-only field resets it to the deployment
  default, so it is compared as that value: refused when a stricter ceiling applies.
- **Guard fields** have no ceilings. Tightening means `true` for the three switches and a value at or below
  the current `quarantine.spam_threshold`. For `drafts.require_approval`, tightening means a level at or
  above the current one in the order `never` < `new_recipients` < `external` < `always`. For
  `screening.mode`, tightening is a value at least as strict in the order `off` < `monitor` < `hold`; for
  `screening.on_unavailable`, `quarantine`; for `screening.deadline_seconds`, any value while the effective
  `on_unavailable` after the write is `quarantine`, otherwise a value at least the current one. API keys may take decisions reserved for people when
  `PM_QUARANTINE_KEY_RELEASE` is `on`, `PM_CONSOLE` is `off`, or the tenant's `quarantine.key_release` is
  `true` ([Workspace policy §3](../project/design/workspace-policy.md#3-decisions-reserved-for-people)).
- Ceilings are checked when a value is written. Changing `PM_DEFAULT_POLICY` later does not rewrite
  stored values; a platform or partner key that wants a tenant lower sets the field.
- **An identity's `send_policy.daily_cap`** (`POST …/identities`, `PATCH /v1/identities/{identity_id}`)
  may not exceed the tenant's effective `identity_daily_send_cap` for any key but a platform key
  (`403 scope_denied`, `details.field = "send_policy.daily_cap"`), so a lower-only cap cannot be raised
  one identity at a time.
- **An identity's `send_policy.require_approval`** can only be stricter than the tenant's; a looser value
  than the identity's current one gets `403 scope_denied` from an identity key and `403 permission_denied`
  (`person_required`) from a tenant key unless keys may take decisions reserved for people.
- **`quarantine.on_auth_fail: false`** is a guard value, and it removes a guarantee: mail whose authentication
  verdict is `fail` or `unverified` is then stored as `received` and evented to agents with that verdict,
  instead of being quarantined (rules 3 and 3a of
  [Inbound › Quarantine decision](../project/design/inbound.md#quarantine-decision)). A partner or workspace
  that turns it off accepts that its agents must check `verdict` themselves.

## CLI configuration

```toml
# ~/.config/pylota-identity/config.toml
[profiles.default]                 # the profile idvy setup and idvy login write unless --profile names another
url = "https://mail.example.com"
key = "pmk_live_…"                 # or key_command = "op read op://vault/pylota-identity/key"
identity = "bookings.acme@agents.example"   # default for mail commands
account_id = "0123456789abcdef0123456789abcdef"   # Cloudflare account ID, stored by idvy setup

[profiles.staging]
url = "https://mail-staging.example.com"
key_env = "PYLOTA_IDENTITY_STAGING_KEY"
```

Precedence, highest first: command-line flags (`--url`, `--key`, `--profile`, `--account-id`), then the
environment (`PYLOTA_IDENTITY_URL`, `PYLOTA_IDENTITY_KEY`, `PYLOTA_IDENTITY_PROFILE`, `CLOUDFLARE_ACCOUNT_ID`), then
the profile in the file. Without `--profile` or `PYLOTA_IDENTITY_PROFILE`, the profile is `default_profile`
when the file sets it, else the one named `default`. `idvy setup` and `idvy login` write the profile
named by `--profile`, default `default`; `PYLOTA_IDENTITY_PROFILE` and `default_profile` do not change it.
The file is created with mode `0600`, and `idvy` refuses to read it (exit 3) if its group or other users
have any access to it, or if another user owns it. `idvy mcp stdio` takes its key only from
`PYLOTA_IDENTITY_KEY` or the profile; `--key` is refused.

The commands that use `CLOUDFLARE_API_TOKEN` are listed in
[CLI › Commands that use your Cloudflare token](cli.md#commands-that-use-your-cloudflare-token). The
token is never stored. The account ID comes from `--account-id` (a global flag), else
`CLOUDFLARE_ACCOUNT_ID`, else the profile's `account_id` (written by `idvy setup`; not a secret), else
`PM_CF_ACCOUNT_ID` in `deploy/wrangler.toml`.
