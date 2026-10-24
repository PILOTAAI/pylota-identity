# Limits

Some limits come from Cloudflare, some from Amazon SES (only for domains that use it) and some from
Pylota Identity. Cloudflare's and Amazon's were read from their documentation on 2026-10-09 and can change.
`idvy doctor` reports the ones it can observe.

## Mail

| Limit | Value | Source | What happens |
|---|---|---|---|
| Inbound message size | 25 MiB through Email Routing; 40 MB, including headers, through Amazon SES (domains with `inbound = ses`) | Cloudflare Email Routing; [SES quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html), read 2026-10-09 | Never reaches the Worker: Cloudflare rejects it, and SES stores at most 40 MB in S3 |
| Outbound message size, encoded, including attachments | 5 MiB | Cloudflare Email Sending | `413 message_too_large`, or a signed link if `large_attachments: link` |
| Recipients per message (`to` + `cc` + `bcc`) | 49, default policy 10. Cloudflare allows 50; one is kept for the hidden journal copy of Message-ID strategy B | Cloudflare / Pylota Identity / policy | `400 too_many_recipients` |
| Subject length | 998 characters | RFC 5322 / Cloudflare | `400 invalid_request` |
| Custom headers on a send | 16 KB total; at most 20 non-`X-` headers, service-set ones included; values ≤ 2,048 bytes. Names, matched case-insensitively: `X-` names matching `^X-[A-Za-z0-9_-]+$` (≤ 100 bytes), or `Importance`, `Priority`, `Sensitivity`, `Keywords`, `Comments`, `Organization` (sent in that casing). Values of `Importance`: `high`, `normal`, `low`; `Priority`: `normal`, `non-urgent`, `urgent`; `Sensitivity`: `personal`, `private`, `company-confidential` | Cloudflare (Email headers reference, read 2026-10-10) | Checked when the request arrives: `400 header_not_allowed` for a name, `400 invalid_request` for a value |
| Attachments per send | 32 (REST); 10 per call in the MCP tool `mail_send`; each inline or from an upload | Pylota Identity | `400 invalid_request` |
| Inbound MIME nesting depth | 32 | Pylota Identity | Deeper parts are kept raw; flag `parse_degraded` |
| Inbound MIME parts | 500 | Pylota Identity | Further parts are kept raw; flag `parse_degraded` |
| Attachment text extracted | 20 MB input, 200 pages, 2 MB text | Pylota Identity | `text_status: unavailable` beyond it |
| Archive expansion checked | ratio ≤ 100:1, ≤ 100 MB | Pylota Identity | Larger means `risk: archive_bomb`, quarantined |
| Local part length | 64 characters, including the thread token | RFC 5321 | Username plus suffix at most 40 |
| References kept on our replies | 20: the first plus the 19 most recent | Pylota Identity | The ones between are trimmed ([C2](../project/edge-cases.md)) |
| Daily sending | Account quota, set and raised by Cloudflare. It is not exposed to the Worker, and the limits page does not say whether it counts messages or recipients, so the alert counts accepted recipients plus the strategy-B journal copy (`cf_recipients`) | Cloudflare | Queue backs off for up to 24 hours. An alert fires at 80% of `PM_DAILY_SEND_QUOTA` when you set it to your quota, otherwise on the first quota error ([G3](../project/edge-cases.md)) |
| Stored message row (Durable Object SQLite) | 1,900,000 bytes per message row, inbound and outbound: `text` 512 KiB, `html_sanitized` 1 MiB, `extracted_text` 256 KiB, `calendar_json` 256 KiB, `references_json` 200 msg-ids and 32 KiB, `to` and `cc` 200 addresses each, DSN recipients 100; when the sum would pass the budget, `html_sanitized` is cut first, then `text`, then `extracted_text` | Cloudflare (2 MB per row, string or BLOB; 100 bound parameters per query; 100 KB per statement; 100 columns per table: [Durable Objects limits](https://developers.cloudflare.com/durable-objects/platform/limits/), read 2026-10-10) / Pylota Identity | Flag `body_truncated`; the full message stays in the raw MIME ([Inbound › Storage caps](../project/design/inbound.md#storage-caps)) |
| Inbound volume per identity | 60 an hour per sender (`unauth:` + address when authentication fails), 120 an hour across unauthenticated senders who are not known correspondents | Policy (`inbound.*`) | Stored `throttled`: not triaged, embedded or evented, not counted in `storage_gb` ([D5](../project/edge-cases.md), [D13](../project/edge-cases.md)) |
| Inbound volume per tenant | 2,000 an hour from senders who are not known correspondents | Policy (`inbound.per_tenant_per_hour`) | As above |
| Unknown recipients per domain | 20 misses a second | Pylota Identity | Further mail to unknown addresses on the domain gets a temporary failure for the rest of the second ([A16](../project/edge-cases.md)) |
| Role mail relayed (`postmaster@`, `abuse@` and the other RFC 2142 names) | 30 an hour per domain, 5 an hour per envelope sender | Pylota Identity | A temporary failure; the sender's server retries ([D14](../project/edge-cases.md)) |
| Hop count of a send (`X-Pylota-Mail-Hop`) | Below 10, for every `kind` | Pylota Identity | `409 loop_detected` ([N13](../project/edge-cases.md)) |
| Unknown transport outcomes in a row | 3 per transport (Cloudflare account, SES region, SMTP relay) | Pylota Identity | The transport's breaker opens for 5 minutes, doubling to at most 1 hour; queued messages wait unclaimed instead of becoming `uncertain` ([J28](../project/edge-cases.md)) |
| Tenant and domain complaint and bounce rates | Complaints 0.1%, bounces 5%, over 7 UTC days with at least 500 outcomes | Policy (`abuse.tenant_*`); the complaint default is Amazon SES's review rate ([SES sending review FAQ](https://docs.aws.amazon.com/ses/latest/dg/faqs-enforcement.html), read 2026-10-10) | Sending paused: `409 sending_paused`, no fallback; a platform key resumes ([G12](../project/edge-cases.md)). Readable before they apply with `GET /v1/metrics/rates` |

## Domains and addresses

| Limit | Value | Source |
|---|---|---|
| Mail domains per zone (routing + sending, including apex) | 30 | Cloudflare ([subdomains](https://developers.cloudflare.com/email-service/configuration/subdomains/), read 2026-10-10). The 31st returns `422 zone_domain_limit` before anything is created |
| Unverified domains per tenant | 5 | Pylota Identity. Domains never verified, including adds in progress; the 6th returns `422 unverified_domain_limit`. Platform keys are not limited |
| Time to verify a new domain | 14 days | Pylota Identity. A domain never verified 14 days after it was added (or after its created zone became active) is removed with `reason: "unverified_expired"`; a reminder is sent on day 12. Until it is verified, another tenant that publishes its claim TXT can evict it |
| Literal routing rules per domain (subdomain mail domains) | 200 | Cloudflare. So at most 200 addresses per subdomain mail domain. Apex domains use catch-all and have no limit |
| Literal routing rule matcher | 90 characters | Cloudflare (Email Routing rules API, read 2026-10-09). An address on a subdomain mail domain longer than 90 characters is refused with `400 address_invalid` |
| Catch-all | apex domains only | Cloudflare. This is why the platform domain must be a zone apex |
| Addresses per identity (all states) | 20 | Pylota Identity |
| Pending addresses per identity per domain | 1 | Pylota Identity |
| Address retirement grace | 0–365 days, default 90 | Pylota Identity |
| Forwarding test (`inbound: forward`) | The token must arrive within 10 minutes, otherwise `forwarding` is `failed` | Pylota Identity |

## Amazon SES

Applies to domains connected with `dns_records`, `send_only`, or `smtp_relay` with `inbound: ses`
([Domains on any DNS host](../project/design/domain-connections.md)). SES quotas are per AWS region.

| Limit | Value | Source | What happens |
|---|---|---|---|
| Inbound message size | 40 MB, including headers | [SES quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html), read 2026-10-09 | Larger messages are not stored in S3 and never reach the Worker |
| Verified identities per region | 10,000 (raised only through the AWS account manager) | [SES quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html), read 2026-10-09 | At 9,000 the operator alert `ses_identities_90pct` fires and `idvy doctor` warns. At 10,000, adding a domain that needs an SES identity gets `422 transport_unavailable` with `details.reason = "ses_identity_limit"`. The count is the `domains` rows with `ses_region` set and not `removed`, plus adds in progress that hold an SES identity, plus the platform identity, or the region's total from the daily `ListEmailIdentities` when that is larger |
| Rules per receipt rule set | 200, not adjustable | [SES quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html), read 2026-10-09 | Pylota Identity uses at most 150 `pm-retired-{n}` rules |
| Recipients per receipt rule | 500, not adjustable | [SES quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html), read 2026-10-10 | When a `pm-retired-{n}` rule is full, `SesControl`, the rules' only writer, opens the next one |
| Receiving deployments per AWS account and region | 1 | Pylota Identity (`pm-deliver` has no recipient condition) | `idvy setup ses` refuses a second deployment in the same account and region; use a separate AWS account for staging |
| Retired addresses bounced per deployment | 75,000 (150 rules × 500) | Pylota Identity | Beyond it the oldest retired addresses leave the rules, and their mail is dropped without a bounce, like mail to an unknown address |
| SES API requests other than sends | 1 per second per account and region; not adjustable | [SES quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html) (SES API sending quotas), read 2026-10-09 | One deployment-wide token bucket (the `SesControl` Durable Object) admits one control-plane call per second. Domain create, `PATCH` and removal wait up to 5 s, then get `429 upstream_rate_limited` with `Retry-After`; background checks wait up to 60 s, then retry later. Each SES domain's daily identity check runs at a fixed time of day derived from a hash of its ID, so checks spread across the day ([Domains on any DNS host §4.8](../project/design/domain-connections.md#48-ses-api-rate-one-request-per-second)) |
| Sending from the SES sandbox | 200 messages per 24 hours, 1 per second, to verified addresses only | [SES quotas](https://docs.aws.amazon.com/ses/latest/dg/quotas.html), read 2026-10-09 | `idvy setup ses` stops until production access is enabled |
| Raw message in S3, and notifications in the SQS backstop | 14 days | Pylota Identity (`idvy setup ses`) | An object deleted before ingestion is lost: its `queued` ledger row becomes `lost` and the `ses_object_lost` alert pages |

## SMTP relay

Applies to domains connected with `smtp_relay`.

| Limit | Value | Source |
|---|---|---|
| Ports | 465 (TLS from the start) and 587 (STARTTLS) only. Any other port, 25 included: `400 smtp_port_not_allowed`. A relay that offers no TLS: `422 smtp_tls_required`, and the credentials are not sent | Cloudflare: "Workers cannot create outbound connections on port `25`" ([TCP sockets](https://developers.cloudflare.com/workers/runtime-apis/tcp-sockets/), read 2026-10-09); Pylota Identity |
| SMTP sends in parallel | 4 per outbound consumer invocation | Cloudflare allows each invocation up to six connections waiting at once, and opening a socket counts ([Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-09) |
| Connections per message | 1, without pipelining | Pylota Identity |
| Timeouts | 10 s to connect, 30 s per command, 60 s for the reply after the final dot | Pylota Identity. No reply after the final dot makes the send `uncertain`; it is never resent |
| Alignment probe | Before the first send and every day. On demand at most once a minute per domain (`429 rate_limited`). No probe back within 15 minutes is `smtp_probe_timeout` | Pylota Identity |

## API

| Limit | Value |
|---|---|
| Request body | 7 MiB (a 5 MiB message after base64 decoding, plus JSON) |
| Requests per API key | 600 per minute |
| Requests per tenant, all its tenant and identity keys together (`RL_TENANT`) | 1,800 per minute (`429 rate_limited`, `details.bucket: "tenant"`) |
| Requests per partner, all its partner keys together (`RL_PARTNER_API`) | 1,800 per minute (`429 rate_limited`, `details.bucket: "partner"`) |
| Active API keys | 100 tenant and identity keys per tenant, 10 partner keys per partner; revoked and expired keys do not count (`422 key_limit_reached`). Platform keys are not capped |
| Search per key | 120 per minute |
| Agentic search per key | 20 per minute. Tenant daily cap 500 by default, counted once per validated request |
| Workers AI per tenant (`RL_AI`, `RL_EMBED`) | 60 text-generation calls (triage, agentic planning and answers), 60 more for screening's classifier (`RL_AI` under `screen:{tenant_id}`), and 600 embedding requests a minute; the account's limits, shared by every tenant, are 300 and 3,000 a minute ([Workers AI limits](https://developers.cloudflare.com/workers-ai/platform/limits/), read 2026-10-10; 300 confirmed 2026-10-18). Triage and embedding jobs over the share wait and retry; interactive search degrades; screening defers until its deadline ([F16](../project/edge-cases.md)) |
| Model-backed triage per tenant | 2,000 a day by default (`triage.daily_model_cap`); then rules-only until local midnight |
| Sends per identity | 120 per minute. Daily caps from policy |
| Signing per identity (`RL_SIGN`): agent assertions and signed HTTP requests together | 600 per minute. Not counted against any plan allowance |
| Tenant creation and invitations per partner (`RL_PARTNER`) | 10 per minute together, across all of the partner's keys |
| Tenants per partner | `max_tenants` tenants that are not erased: 25 by default, set by the operator (`403 partner_tenant_limit`) |
| Rate-limit headers | Every authenticated response carries `RateLimit-Limit` (the bucket's limit per period). A `429` also carries `Retry-After` and `RateLimit-Reset`, the seconds to the end of the bucket's current period (for `rate_limited` the two are equal; other `429` codes set `Retry-After` to their own wait). No `RateLimit-Remaining`: the rate-limiting binding answers only allow or deny |
| Page size | 25 by default, 100 maximum |
| Search `limit` | 10 by default, 50 maximum |
| Search response size | 256 KB. Above it, results are cut and `truncated: true` |
| Tenant search fan-out | 100 identities |
| `wait` timeout | 60 seconds |
| Concurrent long polls of the event feed | 5 per key, 20 per tenant |
| Idempotency key retention | 30 days |
| Cursor lifetime | 24 hours |
| Metadata on identities and messages | 16 keys, 512 bytes per value |
| Labels | 64 per message, 64 characters each |
| Batch reads and updates | 100 message IDs per request; `batch-get` responses at most 1,048,576 bytes, `extracted_text` at most 16,384 bytes per message (2,048 by default); `batch-update` at most 64 labels to add, 64 to remove and 10 in `require_absent`. One request against the rate limits |
| Allow and block list entries | 10,000 per scope (the tenant, and each identity), 50,000 per tenant (`422 list_limit_reached`) |
| Per-identity message retention | `retention.message_hours` 1–8,760; a due message is erased within 15 minutes while R2, Vectorize and the mailbox answer (target) |

## Agent signing keys

From [Agent signing keys and signed requests](../project/design/agent-keys.md). Every value outside its
range gets `400 invalid_request`.

| Limit | Value |
|---|---|
| Active signing keys per identity | 1, plus `retiring` keys during an overlap |
| Overlap after an identity key rotation | `PM_IDENTITY_KEY_OVERLAP_DAYS`, default 7 days |
| Identity JWKS cache | `Cache-Control: public, max-age=300`: verifiers should cache it for at most 5 minutes |
| Assertion `audience` | 1–256 printable ASCII characters, required |
| Assertion `expires_in` | 60–600 seconds, default 300 |
| Assertion `nonce` | 1–128 printable ASCII characters |
| Assertion `ext` | 2 KB as JSON; it cannot set a registered or Pylota claim |
| HTTP signature `url` | `https` only, 2,048 characters |
| HTTP signature `expires_in` | 30–300 seconds, default 60 |
| HTTP signature components | Always `@authority`, `signature-agent` and `from`; optionally `@method`, `@path` and `@query`. ASCII values only |
| Web Bot Auth key directory | At most 3 keys (one active, two retiring); a rotated deployment key stays listed for 7 days. `Cache-Control: max-age=86400` |

## Service sign-up ledger

From [Service sign-up ledger](../project/design/service-accounts.md).

| Limit | Value |
|---|---|
| Pending entries per identity | 10; the next request gets `422 account_limit_reached` |
| Entries per identity, any status | 200; the next request gets `422 account_limit_reached` |
| Pending entry lifetime | 7 days, then `rejected` with reason `expired` |
| Rejected and closed entries | Deleted 90 days after the decision or closure |
| `sender_domains` | The service domain plus at most 5 more organisational domains |
| `account_identifier` | 1–254 characters |
| `purpose` | 1–500 characters |
| `note` (approve, reject, close) | At most 500 characters |

## Drafts and scheduled send

From [Drafts, approvals and scheduled send](../project/design/drafts.md).

| Limit | Value | Source |
|---|---|---|
| Drafts per identity, not counting `sent` and `canceled` records | 1,000; then `422 draft_limit_reached` | Pylota Identity |
| `send_at` | At least 60 seconds and at most 90 days after the request | Pylota Identity ([Drafts §6.1](../project/design/drafts.md#61-the-window)) |
| Late limit of a scheduled send | 1 hour after `send_at`; then `failed` with `send_window_missed` | Pylota Identity ([Drafts §6.4](../project/design/drafts.md#64-transient-conditions-and-the-late-limit)) |
| Attempts after an interrupted send | 3; then `failed` with `send_interrupted` | Pylota Identity |
| Scheduled sends per alarm run of one mailbox | 10, then the alarm re-arms at once | Pylota Identity |
| Pending approval | 7 days after it was requested, or `send_at` + 1 hour when later | Pylota Identity |
| Draft retention | 30 days after the last change (or `retention.message_days` when shorter); `sent` and `canceled` records 30 days | Pylota Identity |
| `text`, `html` | 524,288 and 1,048,576 bytes | Pylota Identity ([Inbound › Storage caps](../project/design/inbound.md#storage-caps)) |
| Attachments | 32 per draft; inline bytes within the 7 MiB request body; 26,214,400 decoded bytes in total, as for a send | Pylota Identity |
| Recipients, subject, headers, labels, metadata | As for sends | [Mail](#mail) |
| `note`, `reason` | 500 characters | Pylota Identity |
| `on_behalf_of` | 1–128 printable ASCII characters, no `@` | Pylota Identity |
| `client_id` | 1–255 printable ASCII characters without `@`, unique per identity | Pylota Identity |
| Durable Object alarm | One per object; at-least-once; 6 retries from 2 seconds | [Alarms](https://developers.cloudflare.com/durable-objects/api/alarms/), read 2026-10-17 |
| Alarm handler wall time | 15 minutes | [Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-17 |

## Screening

From [Screening hold](../project/design/screening.md#19-billing-and-limits), which also lists the internal
bounds (attachment wait, deterministic scan input, model input, the `pm-screen` queue settings) that callers
never meet directly.

| Limit | Value |
|---|---|
| Deadline | 30–900 seconds, default 120 (`screening.deadline_seconds`) |
| Model calls per day | `triage.daily_model_cap` (default 2,000), counted separately from triage; one sender key (an authenticated `From` address, or all unauthenticated mail) at most a tenth of it, and senders that are not known correspondents together at most four fifths. A reached cap holds the message as `screening_unavailable` whatever `screening.on_unavailable` says |
| Model call | 10-second timeout; no call starts with under 12 seconds left before the deadline; one immediate retry for invalid output; `max_tokens` 2,048 |
| Text-generation rate | `RL_AI` under `screen:{tenant_id}`: 60 a minute per tenant, apart from triage and agentic search |
| Classifier input | 6,000 estimated tokens; the deterministic scan covers the whole stored text, so size never makes a verdict unavailable |
| Signals per record | 32 |
| Feedback per message | 20; the next gets `422 feedback_limit_reached` |
| Feedback notes | 1–500 characters |
| Summary window | Default 30 days, at most 92 |

## Calendar

From [Calendar invitations](../project/design/calendar-invitations.md).

| Limit | Value |
|---|---|
| Calendar parts read per message | 10 |
| Calendar part read | 1 MiB decoded; larger: `object_status: "too_large"` |
| Content lines; one unfolded line | 20,000; 64 KiB (an inbound inline `ATTACH` or `IMAGE` value is skipped and counted in `removed` instead) |
| Components directly inside the `VCALENDAR` | 1,100 |
| `CONFERENCE` entries kept; URI | 5; 2,048 bytes |
| Overrides (`RECURRENCE-ID`) read; kept in the object | 1,000; 100 |
| Attendees read per component; kept | 1,000; 100 |
| Distinct party addresses per object | 1,000 |
| `VTIMEZONE`s; observances; `RDATE`s per observance | 20; 50; 100 |
| `EXDATE` and `RDATE` values per component | 10,000 each |
| `UID` | 1,024 bytes |
| `summary`, `location`, `description` excerpt, `comment`, names | 1,000, 1,024, 2,000, 2,000 and 256 characters |
| `RRULE` text; `INTERVAL`; `COUNT`; values per `BY…` part | 512 bytes; 1–1,000; 1–100,000; 366 |
| Stored calendar object | 262,144 bytes per message |
| Outbound `calendar.ics` | 262,144 bytes (`413 calendar_too_large`) |
| Generated `VTIMEZONE`s | 10 per message, 8 KiB each |
| Calendar-reply `comment` | 1,000 characters |
| Calendar recipients on the `cloudflare` transport | As `max_recipients`, or 1 if spike S1 shows that a raw send takes one recipient (`400 too_many_recipients`, `calendar_single_recipient`) |

## Uploads

From [Uploads](../project/design/uploads.md).

| Limit | Value | Source | What happens |
|---|---|---|---|
| Upload size | 1–26,214,400 bytes (25 MiB) | Pylota Identity; below the Workers request-body limit of every account plan (100 MB Free and Pro, 200 MB Business; [Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-17) | `400 invalid_request` at create; a `PUT` of another length gets `400 upload_size_mismatch` |
| Live uploads per identity | 100, and 524,288,000 declared bytes | Pylota Identity | `422 upload_limit_reached` |
| Upload lifetime | 900–604,800 seconds, default 86,400; never past the identity's mail retention | Pylota Identity | Deleted at `expires_at`; `410 upload_expired` for 7 days, then `404` |
| Upload URL | 3,600 seconds, never past `expires_at` | Pylota Identity | `404 upload_not_found`; `GET` the upload for a new URL |
| Failed `PUT`s | 10 caused by the client | Pylota Identity | `expired` (`too_many_attempts`) |
| Attachments per message, decoded | 26,214,400 bytes in total | Pylota Identity | `413 message_too_large` (`attachments_total`) |

## Agent self-sign-up

From [Agent self-sign-up](../project/design/agent-signup.md).

| Limit | Value |
|---|---|
| Sign-ups per client network (IPv4 address or IPv6 /64) | 5 per UTC day; and `RL_SIGNIN`, 10 requests per 60 seconds, shared with the console's sign-in and sign-up forms |
| Sign-ups per ASN | 100 per UTC day |
| Sign-ups per deployment | `PM_AGENT_SIGNUP_DAILY_CAP` per UTC day (default 200, 1–10,000) |
| Sign-ups naming one person (base address) | 3 per UTC day, and one live provisional sign-up |
| Code emails to one person (base address) | 5 per UTC day; 3 per rolling hour per sign-up |
| `signup` class share of the system identity's day | 5% |
| Request body | 4,096 bytes |
| `username` | 2–32 characters of `[a-z0-9-]`, starting with a letter or digit; never released |
| `agent_name`, `source` | 1–78 characters; 1–64 characters of `[A-Za-z0-9._/+-]` |
| Provisional life | 7 days from the sign-up, not extended by replacements |
| Code and link | 6 digits; 24 hours; 10 attempts per code; 30 failed codes per sign-up |
| Replacements of the person | 2, before verification only |
| Sends while provisional | To the person only; 1 recipient per message; 10 messages per UTC day |
| Inbound while provisional | 20 per sender per hour, 20 unauthenticated per hour, 100 per hour for the workspace; the excess is stored `throttled` |

## Deliverability metrics

From [Deliverability metrics](../project/design/metrics.md).

| Limit | Value | What happens |
|---|---|---|
| Hourly event-count buckets | Kept 192 hours; at most 168 per request | `400 query_range_too_wide` |
| Daily event-count buckets | Kept 92 days; at most 92 per request | `400 query_range_too_wide` |
| Identities per tenant- or domain-scope count | 100 (narrow with `identity_ids`) | `422 scope_too_large` |
| Per-mailbox deadline in a fan-out | 2,000 ms | `partial: true`, `failed_identities` |
| Rate limit of `GET /v1/metrics/events` | `RL_SEARCH`, 120 per 60 seconds per key | `429 rate_limited` |

## Notifications

From [Notifications and usage alerts](../project/design/notifications.md).

| Limit | Value |
|---|---|
| Notification email per person | 50 a day (in the workspace's time zone), all kinds except `account` and `digest`; further items go into one `digest` email at the next 09:00 |
| Notification email per workspace | 200 a day, all kinds except `account` and `digest` |
| `new_mail`, `instant` | A 2-minute hold after the first message, then at most one email per person and inbox every 10 minutes |
| `new_mail`, `hourly` and `daily` | One email at the top of each hour that had messages; one at 09:00 local time |
| `needs_reply` filter | Waits up to 5 minutes for triage |
| "Needs a person" email | Daily at 09:00 in the workspace's time zone |
| Usage alerts | 80% and 100% of each allowance; once per threshold per period for `sends` and `triage`; a 24-hour cooldown per feature and threshold for counts. None with `PM_BILLING=off` |
| Unsubscribe link | 90 days, or until its `link` key leaves its 7-day window after a rotation |
| Retries while the platform domain is failing, or while the system identity's submit is refused | Hourly, for 24 hours |

## Storage

| Limit | Value | Source |
|---|---|---|
| Durable Object SQLite per identity | 10 GB | Cloudflare. Alert at 70%. Raw MIME and attachments live in R2, so this is mostly text and index |
| D1 database | 10 GB; it cannot be raised ([D1 limits](https://developers.cloudflare.com/d1/platform/limits/), read 2026-10-10) | Cloudflare. One database for the whole deployment: control plane only. Event and delivery logs are pruned after the tenant's `retention.events_days` (default 30). Alerts at 70% (ticket) and 80% (page); at 90% event and delivery logs are kept 7 days for every tenant. When full, D1 refuses writes ("Exceeded maximum DB size", [Debug D1](https://developers.cloudflare.com/d1/observability/debug-d1/), read 2026-10-10): API writes get `503 unavailable` (`storage_full`), inbound mail is still accepted and outbox events wait in their mailboxes ([J30](../project/edge-cases.md)). 70% is the trigger to write the sharding ADR |
| Vectorize vectors per index | 20,000,000 ([Vectorize limits](https://developers.cloudflare.com/vectorize/platform/limits/), last updated 2026-08-05, read 2026-10-10) | Cloudflare. About 4,000–10,000 vectors per 1,000 messages, so roughly 2–5 million messages per deployment. Alerts at 70% and 80% of the count the nightly reconciliation reads. When full, new messages stay keyword-searchable and semantic search reports `degraded` ([J31](../project/edge-cases.md)) |
| Vectorize namespaces per index | 50,000 | Cloudflare. One per tenant, so at most 50,000 tenants per index. Alerts at 70% and 80% of tenants that are not erased ([J31](../project/edge-cases.md)) |
| Queue message | 128 KB | Cloudflare. Queues carry pointers only |
| Queue delay per retry | 24 hours | Cloudflare |
| Queue retention | 14 days | Cloudflare. Dead-letter items are kept at most 14 days |

## Plans

On a deployment with billing on (Pylota Identity Cloud), the plan sets allowances for inboxes, sends, triage
analyses, custom domains, storage and seats. The table and the rules (holds, `402 billing_limit`, top-ups,
resets) are in [Plans and billing](../guides/plans.md). Read your workspace's live numbers with
`GET /v1/usage`. Self-hosted deployments have no plan limits; only the daily caps in tenant policy apply
(see [API](#api)). A workspace can hold at most 100 units of each top-up feature (`topup.max_quantity`).
A plan or top-up increase counts from the moment its invoice is paid.

| Shared limit (Pylota Identity Cloud, or any deployment with `PM_DAILY_SEND_QUOTA` set) | Value |
|---|---|
| Shared-domain breaker, stage 1 | At 60% of the account's daily Email Sending quota, sends from Free and ramped workspaces get `429 daily_cap_reached` (`details.cap: "shared_domain"`) until 00:00 UTC |
| Shared-domain breaker, stage 2 | At 90%, every tenant's sends get the same refusal until 00:00 UTC; sign-in mail still goes out |
| A disputed payment | The workspace's sends get `429 daily_cap_reached` (`details.cap: "billing_dispute"`) until the dispute closes |

## Console

| Limit | Value |
|---|---|
| Sign-in link or code requests | 3 per 10 minutes per address |
| Code verification attempts | 10 per code; the token is burned after 10 failures |
| Failed sign-in codes per address | 30 per UTC day; then sign-in by code is locked for that address until 00:00 UTC (links still work) |
| Sign-in requests per client network (`RL_SIGNIN`) | 10 per minute, keyed by `CF-Connecting-IP` (an IPv6 address by its /64 prefix), across sign-in, sign-up, waitlist, pending-step and OAuth-start requests |
| System mail per recipient | 10 a UTC day (sign-in, sign-up, waitlist and invitation mail together); agent sign-up code emails have their own 5 a day |
| Sign-in mail per client network | 20 a UTC day per IPv4 address or IPv6 /64, and 200 per ASN |
| Invitation emails per workspace | 50 a UTC day, new and re-sent; then `429 daily_cap_reached` (`details.cap: "invitations"`) |
| Self-serve Free workspaces | One per person; `+tag` variants of an address count as the same person |
| Pending sign-in step (second factor, or the code that confirms a Google or GitHub link) | 5 minutes (10 when it starts with the emailed link code), single use |
| Two-step enrolment | The secret shown must be confirmed within 10 minutes |
| Form ticket (sign-in, sign-up and waitlist forms) | Submitted at least 2 seconds and at most 30 minutes after the page was served, from the same network |
| Link and code lifetime | 10 minutes, single use |
| Two-step verification codes | 5 attempts a minute per person. 10 failures in a row lock two-step sign-in for 15 minutes |
| Recovery codes | 10 per person, each single use. Generating new ones invalidates the old |
| Google or GitHub sign-in | 10 minutes from start to callback, single use |
| Waitlist | An entry is written only when its confirmation link is used; an unused confirmation link expires after 10 minutes. An invite link (`/console/sign-up?invite=…`) is valid for 7 days, for the waitlisted address only. Entries are deleted 30 days after invitation |
| New workspace on Free (Pylota Identity Cloud) | At most 50 messages a day (the effective `tenant_daily_send_cap` is the policy value or 50, whichever is lower) for the first 7 days. A daily evaluation lifts the ramp from day 7 if bounce and complaint rates are under the auto-pause thresholds; otherwise it stays and is evaluated again each day. A paid plan lifts it once its first invoice is paid, and a disputed payment restores it. Above it: `429 daily_cap_reached` |
| Session lifetime | 7 days rolling, 30 days absolute |
| Re-authentication for sensitive actions | signed in within the last 10 minutes |
| Workspace policy saves | Owner and admin only; each save needs a sign-in within the last 10 minutes, and a change that deletes mail needs a second, confirmed `POST` ([Workspace policy §6](../project/design/workspace-policy.md#6-the-console-page)) |
| Invitation lifetime | 7 days |

## SDKs and agent tooling

| Limit | Value | Source |
|---|---|---|
| MCP tools per key (`mcp_tools`) | 64 names | Pylota Identity |
| TypeScript SDK attempt timeout | 30 s (1–120 s); `waitForMessage`: its `timeout` + 15 s | Pylota Identity |
| TypeScript SDK retries | 3 (0–10), backoff 0.5 s, 1 s, 2 s + up to 250 ms; `Retry-After` slept up to 60 s (1–300 s); `request_in_progress` 5 more attempts | Pylota Identity |
| TypeScript SDK `collect` | 10,000 items by default | Pylota Identity |
| TypeScript SDK deduplication lease | 60 s by default (15–3,600 s) | Pylota Identity |
| Webhook timestamp tolerance (SDK and `idvy webhooks verify`) | 300 s by default, at most 600 s | Pylota Identity |
| MCP result-size hint | `_meta["anthropic/maxResultSizeChars"]` = 100,000 on tools under the 96 KB cap | Claude Code persists larger text results above 50,000 characters ([MCP output limits](https://code.claude.com/docs/en/mcp), read 2026-10-17) |
| MCP tool descriptions and server instructions | 4,096 characters | Claude Code cuts longer ones (same page) |

## Webhooks

| Limit | Value |
|---|---|
| Endpoints per tenant | 20 |
| Endpoints per partner | 20 |
| Platform endpoints | 20 |
| Timeout per attempt | 15 seconds |
| Retry window | About 72 hours, 13 attempts |
| Replay window | 30 days from the event's `occurred_at` (never from when the delivery went dead), or the tenant's `retention.events_days` if that is shorter |
| Response body read | 4 KB |
| Custom delivery headers | 10 per endpoint; names 1–64 characters; values 1–1,024 bytes and 4,096 bytes together (`422 webhook_header_limit_reached`) |

## Event feed and stream

From [Event feed and live stream](../project/design/event-feed.md#12-billing-and-limits). Reads, consumers
and streams are never metered: no `402 billing_limit`, on any plan.

| Limit | Value | Source | What happens |
|---|---|---|---|
| Page size (`limit`) | 1–500, default 100 | Pylota Identity | `400 invalid_request` outside it |
| Response body | 4 MiB of envelopes | Pylota Identity | The page ends early with `has_more: true` |
| Scan window | 10,000 log positions per read | Pylota Identity | An empty page with `has_more: true`; ask again |
| Long poll (`wait`) | 0–60 seconds, default 0; checked every second | Pylota Identity | – |
| Concurrent long polls | 5 per key, 20 per tenant | Pylota Identity | `429 rate_limited`, `details.bucket = "events_wait"`, `Retry-After: 5` |
| Type patterns | At most 50, each at most 64 characters | Pylota Identity | `400 invalid_request` |
| Identity filter | At most 100 identity IDs | Pylota Identity | `400 invalid_request` |
| Retention | `retention.events_days` after the append (1–365, default 30) | Policy | Older cursors get `410 cursor_expired` |
| Log size per tenant | 6,000,000,000 bytes of `log_bytes` (each event's envelope plus 1,024 bytes); pruned, oldest first, to 5,000,000,000; the database size is watched at 8,000,000,000 bytes | Pylota Identity, inside Cloudflare's 10 GB per object ([Durable Objects limits](https://developers.cloudflare.com/durable-objects/platform/limits/), read 2026-10-18) | Older cursors get `410 cursor_expired` (`size_limit`); `event_log_size` alert |
| Durable consumers | 20 per tenant | Pylota Identity: the same as webhook endpoints per tenant; each is a polling client, and Pylota needs one per tenant plus a few local tools (AgentMail allows 10 per scope) | `422 event_consumer_limit_reached` |
| Consumer name | 1–64 characters of `[a-z0-9._-]`, starting with a letter or digit | Pylota Identity | `400 invalid_request` |
| Stream connections | 100 per tenant; 10 per key (the oldest is replaced) | Pylota Identity; Cloudflare allows 32,768 per object ([Durable Object State](https://developers.cloudflare.com/durable-objects/api/state/), read 2026-10-17) | `429 rate_limited` (`streams`) or close `4429`; `4409` for the replaced connection |
| Stream token | Single use, 60 seconds, at most 100 live (unredeemed and unexpired) per tenant | Pylota Identity | `4401` when spent or expired; `429 rate_limited` (`stream_tokens`) |
| Client message | At most 16 KiB; at most 100 per 10 seconds | Pylota Identity; Cloudflare's received-message limit is 32 MiB ([Workers WebSockets](https://developers.cloudflare.com/workers/runtime-apis/websockets/), read 2026-10-17) | Close `1009`, or `1008` for the rate |
| Server message | At most 100 events and 1 MiB | Pylota Identity | – |
| Acknowledgement window | 1,000 events or 8 MiB sent and not acknowledged | Pylota Identity | Sending pauses; after 60 seconds full, close `4408` |
| Subscribe deadline | 60 seconds after the upgrade | Pylota Identity | Close `4400` (`subscribe_timeout`) |
| Heartbeat | Server every 60 seconds; client ping every 30 seconds; dead after 90 seconds of silence | Pylota Identity | – |
| Revocation | Streams close within 60 seconds; nothing is sent after the last validation is 60 seconds old | Pylota Identity | Close `4401`, `4403` or `4404` |
| Close reason | At most 123 bytes | RFC 6455 § 5.5 (125-byte control frames, read 2026-10-17) | The full error is in the `error` message ([Errors › Stream close codes](errors.md#stream-close-codes)) |
| Connection attachment | At most 16,384 bytes | Cloudflare ([Durable Objects › WebSockets](https://developers.cloudflare.com/durable-objects/best-practices/websockets/), read 2026-10-17) | The largest valid state is under 9,000 bytes |
