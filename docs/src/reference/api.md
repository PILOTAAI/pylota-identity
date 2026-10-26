# REST API

The machine-readable contract is [`openapi.yaml`](openapi.yaml) (OpenAPI 3.1). A running deployment also
serves it at `/openapi.json`, generated from the Rust types. This page is the readable version. If the
two ever disagree, the OpenAPI file is the contract and this page has a bug.

## Basics

| | |
|---|---|
| Base URL | `https://<your-api-host>/v1`, for example `https://mail.example.com/v1` |
| Auth | `Authorization: Bearer pmk_live_…` (or `pmk_test_…`) |
| Format | JSON (`application/json; charset=utf-8`). Times are RFC 3339 UTC. Sizes are bytes |
| Request ID | Every response carries a `Request-Id` header (`req_…`), also echoed in errors |
| Versioning | Breaking changes get a new prefix (`/v2`). Additive changes (new fields, new event types, new enum values) can happen within `/v1`, so clients must ignore unknown fields and handle unknown enum values |

### Pagination

List endpoints take `limit` (default 25, max 100) and `cursor`. They return:

```json
{ "data": [ ... ], "next_cursor": "c_01J9..." }
```

`next_cursor` is `null` on the last page. Cursors are opaque to clients and expire after 24 hours. Every
list uses one format (search keeps its own, [Search › Cursors](../project/design/search.md#58-cursors-and-as_of-pinning)):

- **Order.** Each list returns rows newest first by its keyset: `created_at DESC`, then the row's ID
  `DESC` (`kid` for the identity-key list). The cursor holds the keyset values of the last row returned,
  and the next page takes rows strictly below them, so a row inserted or deleted between two pages never
  shifts the others.
- **Encoding.** `next_cursor = "c_" ‖ kid ‖ base64url_nopad(payload ‖ tag)`, where `kid` is the kid of
  the current `signing_keys` key of purpose `cursor` (the key search cursors use), `payload` is the
  compact JSON `{"v":1,"r":"<operationId>","f":"<filter hash>","k":["<created_at>","<id>"],"t":<issued_at ms>}`
  and `tag = HMAC-SHA256(cursor key {kid}, "pm-list-cursor-v1\0" ‖ payload)[..16]`. `f` is the first
  16 bytes, hex, of SHA-256 over the operation's path parameters, its filter query parameters in name
  order and the calling key's ID, so a cursor works only for the list, filters and key that produced it.
  The domain-separation prefix keeps list and search cursors apart although they share a key.
- **Errors.** Bad base64, an unknown kid (neither the current cursor key nor one inside its 24-hour
  verify window after a rotation), a bad tag, another operation or another filter hash:
  `400 invalid_request` with `details.errors[0].path = "cursor"`. Older than 24 hours:
  `410 cursor_expired`.

### Idempotency

- **Required** on `POST …/messages`, `…/reply`, `…/reply-all`, `…/forward`, `…/calendar-reply` and
  `POST …/drafts/{draft_id}/send`. A missing key returns
  `400 idempotency_key_required`. The one exception is a dry run (`?dry_run=true`), where the key is
  optional and never recorded ([Sending](#sending)).
- **Required** also on the phone routes that can cost money or move a number: `POST …/phone-numbers`
  (one-call purchase), `POST …/phone-requests` and its approve, `PUT …/phone-budget`, `POST …/sms` and
  `POST …/calls`; and on the vault's decisions and uses: `POST …/vault/uses`, and approving a vault grant or use
  ([Phone numbers](#phone-numbers), [Vault](#vault)).
- **Optional** on every other `POST`, except ten that ignore the header and never record it
  (`x-idempotency: none` in [`openapi.yaml`](openapi.yaml)): the two search endpoints
  (`POST /v1/identities/{identity_id}/search` and `POST /v1/tenants/{tenant_id}/search`), which return
  mail and change nothing, so their results are never stored;
  `POST /v1/identities/{identity_id}/messages/batch-get`, for the same reason; the two signing endpoints
  ([`…/assertions`](#post-v1identitiesidentity_idassertions--tenant-or-identity-key-identitiessign) and
  [`…/http-signatures`](#post-v1identitiesidentity_idhttp-signatures--tenant-or-identity-key-identitiessign)),
  because each call signs anew and a replay record would have to store what was signed; the two
  Amazon SNS endpoints, `POST /hooks/ses` and `POST /hooks/ses/inbound`, which SNS calls without the
  header; `POST /v1/events/stream-tokens`, because each call mints a single-use secret that a stored
  replay would have to keep or drop; `POST /v1/agent-signup` (no key), whose answer holds a key secret, for
  the same reason ([Agent self-sign-up](../project/design/agent-signup.md)); and the mailbox restore,
  `POST /v1/platform/identities/{identity_id}/restore`.
- The header is `Idempotency-Key: <1–255 printable ASCII characters>`. Keys are kept for 30 days. For
  mail they are scoped per identity. For everything else they are scoped per calling API key and per
  tenant (or, for a request that names no tenant, per partner for a partner key and per deployment for a
  platform key), so another key, even of the same tenant, never receives this key's replay.
- The same key with the same request returns the original response, with `"deduplicated": true` in
  mail responses and the header `Idempotent-Replayed: true`.
- A response that carried a one-time secret (`POST /v1/keys`, `POST /v1/keys/{key_id}/rotate`,
  `POST /v1/webhooks`, `POST /v1/tenants/{tenant_id}/webhooks`,
  `POST /v1/webhooks/{webhook_id}/rotate-secret`) is stored without it: a replay returns the same body
  without `secret` and with `"secret_replayed": false`. A secret is shown once, in the first response;
  if it was lost, rotate or revoke ([J19](../project/edge-cases.md)).
- No stored response holds mail content. A response that carries a message or a thread
  (`…/release`, `…/cancel`, `…/resolve`, `…/threads/{thread_id}/hold`), or a draft (the `…/drafts…`
  routes), is stored as a reference, and a
  replay reads the resource again with the calling key's visibility (a resource erased since replays its
  `404`). Records belong to the identity they name, so identity and tenant erasure delete them
  ([Data model](../project/design/data-model.md#1-d1-control-plane)).
- The same key with a different request returns `409 idempotency_conflict`.
- The same key while the first request is still running returns `409 request_in_progress` with
  `retryable: true`.

See [Sending and safe retries](../guides/sending.md#safe-retries).

### Rate limits

| Bucket | Default | Scope |
|---|---|---|
| All requests | 600 per minute | per API key |
| All requests of a tenant's keys together (binding `RL_TENANT`) | 1,800 per minute | per tenant, across all its tenant and identity keys |
| All requests of a partner's partner keys together (binding `RL_PARTNER_API`) | 1,800 per minute | per partner, across all its partner keys |
| Search (`keyword`, `semantic`, `hybrid`, related messages, contacts) and event counts (`GET /v1/metrics/events`), together (binding `RL_SEARCH`) | 120 per minute | per API key |
| Agentic search | 20 per minute | per API key, plus a daily tenant cap |
| Send (accepted into queue) | 120 per minute | per identity, plus daily caps from policy |
| Signing (agent assertions and HTTP signatures together, binding `RL_SIGN`) | 600 per minute | per identity |
| Tenant creation and invitations by partner keys (binding `RL_PARTNER`) | 10 per minute, together | per partner, across all its keys |
| Agent sign-up (binding `RL_SIGNIN`, shared with the console's sign-in forms) | 10 per minute | per client network |

Every authenticated response includes `RateLimit-Limit`: on a `429`, the limit of the bucket that refused
(`details.bucket` names it: `key`, `tenant`, `partner`, `search`, `agentic`, `send`, `sign`,
`partner_create` or `signin`, and on the [event feed](#event-feed) `events_wait`, `stream_tokens` or
`streams`); otherwise the limit of the route's per-key bucket.
A `429 rate_limited` also includes `Retry-After` and `RateLimit-Reset`, both the seconds to the end of the
bucket's current period (other `429` codes, such as `daily_cap_reached`, set `Retry-After` to their own
wait). There is no `RateLimit-Remaining`: Cloudflare's rate-limiting
binding answers only allow or deny, so the service cannot tell how many requests are left.

### Permissions

A key holds a list of permissions. Every endpoint below names the one it needs.

| Permission | Allows |
|---|---|
| `tenants:manage` | Create, update and suspend tenants, and their billing accounts. Platform keys, for every tenant; partner keys, for the tenants their partner's keys created, without changing billing ([Partners](#partners)) |
| `identities:read`, `identities:write` | Read, and create, update, pause or delete identities and addresses, and test forwarding; read, and create, rotate or revoke [identity signing keys](#identity-keys-and-signatures). Deleting an identity also needs `erasure:manage`, because it starts an identity-scope erasure |
| `identities:sign` | Mint agent assertions and Web Bot Auth HTTP signatures as an identity. Tenant and identity keys (an identity key only for its own identity); platform and partner keys cannot hold it, but can grant it to the tenant and identity keys they mint |
| `domains:read`, `domains:write` | Read, and add, update, verify, probe or remove domains |
| `messages:read` | Threads, messages (one at a time or up to 100 with `batch-get`), raw MIME, deliveries |
| `messages:send` | Send, reply, reply-all, forward, cancel, calendar replies, and send drafts (with `drafts:read`); create, read and delete uploads (or with `drafts:write`) |
| `messages:write` | Labels, read state and archiving, of a message, a thread or up to 100 messages (`batch-update`, with its atomic claim); re-run triage; resolve uncertain sends; screening feedback |
| `attachments:read` | Attachment bytes and extracted text |
| `search:read` | Keyword, semantic, hybrid search, contacts, related, wait |
| `search:agentic` | Agentic search |
| `quarantine:review` | See and release quarantined mail, and see mail still being screened |
| `webhooks:read` | Read webhook endpoints and their deliveries |
| `webhooks:manage` | Create, change, test, rotate and delete webhook endpoints, and replay. Includes `webhooks:read` |
| `events:read` | Read the tenant's event log: `GET /v1/events`, durable event consumers, stream tokens and the WebSocket stream ([Event feed](#event-feed)). Each event is shown only when the key could read its subject: mail events need `messages:read`, quarantine and screening events `quarantine:review` too, and identity keys see only their own identity's events. Every key level |
| `keys:manage` | API keys within the caller's scope. A partner key manages only tenant and identity keys of its own tenants |
| `erasure:manage` | Erasure requests, legal holds, exports. A `tenant`-scope erasure request also needs `tenants:erase` |
| `tenants:erase` | Delete the workspace: a `tenant`-scope erasure request, together with `erasure:manage`. Platform keys, partner keys for their own tenants, and tenant keys that the workspace owner minted in the console (or that descend from one); never admins or their keys ([Security › Who minted a key](../project/design/security.md#who-minted-a-key)) |
| `suppressions:manage` | Suppressions, and allow or block lists of the tenant or of one identity |
| `usage:read` | Plan, allowances and usage figures. Every tenant and identity key holds it implicitly for its own workspace, without listing it. Platform and partner keys must hold it explicitly and pass `tenant_id` |
| `audit:read` | Audit log |
| `policy:write` | Read the per-field limits of, and change, the tenant's policy through `GET` and `PATCH /v1/tenants/{tenant_id}/policy`, within the key level's rules: a tenant key may set free fields and lower lower-only fields under its ceilings ([Configuration › Who may change a field](configuration.md#who-may-change-a-field)). Platform, partner and tenant keys; never identity keys |
| `accounts:request` | Request, list, read and close entries of the [service sign-up ledger](#service-accounts). Every key level |
| `accounts:approve` | Approve, reject and delete service-ledger entries. Includes `accounts:request`. Platform, partner and tenant keys; never identity keys, so an agent cannot approve its own request. Approval by an API key is also a decision reserved for people ([Approve](#post-v1identitiesidentity_idaccountsaccount_idapprove--accountsapprove)) |
| `phone:read` | The catalogue, setup status, budget, numbers, requests, SMS, calls, usage, registrations, consent, charges. Every key level; an identity key only for its own identity ([Phone numbers](#phone-numbers)) |
| `phone:request` | Buy within the budget; search; create and cancel own purchase, port-in and release requests; onboarding drafts; shorten `expires_at`. Every key level; an identity key only for its own identity ([Phone numbers](#phone-numbers)) |
| `phone:send` | Send SMS; resolve uncertain SMS; record an opt-out. Every key level; an identity key only for its own identity ([Phone numbers](#phone-numbers)) |
| `phone:call` | Place calls or create `outbound_call` requests. Every key level; an identity key only for its own identity ([Phone numbers](#phone-numbers)) |
| `phone:approve` | Approve and reject phone requests; decide port-outs (decisions reserved for people). Platform, partner and tenant keys; never identity keys ([Phone numbers](#phone-numbers)) |
| `phone:manage` | Number roles and moves; connectors; routes and emergency requests; opt-ins; tighten the budget; extend `expires_at`; tenant reconcile. Platform, partner and tenant keys; never identity keys ([Phone numbers](#phone-numbers)) |
| `phone:compliance` | The compliance profile and documents; compliance, 10DLC and toll-free requests; the sole-proprietor code. Platform, partner and tenant keys; never identity keys ([Phone numbers](#phone-numbers)) |
| `vault:read` | Vault item metadata (never values), grants, uses, the tenant's vault status (tenant level and above) ([Vault](#vault)). Every key level; an identity key only for its own identity |
| `vault:write` | Create, update, archive, restore and rotate vault items; values are write-only; import a session. Every key level; an identity key only for its own identity's items |
| `vault:request` | Ask for vault grants; give up its own identity's grants; P1 `sign` uses. Every key level |
| `vault:use` | Create, cancel and mint tickets for vault uses: the integrator's orchestrator. Platform, partner and tenant keys; **never identity keys** |
| `vault:approve` | Approve and deny vault grants and uses, and loosen an item's approval level; a decision reserved for people. Includes `vault:read`. Platform, partner and tenant keys; never identity keys |
| `vault:admin` | Delete items, manage consumers, rotate the tenant's vault key, change custody, `revoke-all`. Includes `vault:read`. Platform, partner and tenant keys; never identity keys |
| `drafts:read` | List and read drafts and their attachments, and the tenant's queue of drafts awaiting approval, scheduled or failed ([Drafts](#drafts)). Every key level; the tenant queue needs a platform, partner or tenant key |
| `drafts:write` | Create, update, request approval for and delete (cancel) drafts; create, read and delete uploads (or with `messages:send`). Includes `drafts:read`. Every key level |
| `drafts:approve` | Approve and reject drafts. Includes `drafts:read`. Platform, partner and tenant keys; never identity keys, so an agent cannot approve its own mail. Approving by an API key is also a decision reserved for people: a key approves only while `PM_QUARANTINE_KEY_RELEASE` is `on` or `PM_CONSOLE` is `off`, or where the tenant's `drafts.key_approval` is `true`, which admits only a platform key or the tenant's own partner key; approving with `send: true` also needs `messages:send` |
| `members:read` | List console members and pending invitations (tenant, partner and platform keys; every console role holds it) |
| `members:manage` | Invite, revoke, change roles and remove console members (tenant, partner and platform keys). Includes `members:read` |
| `partners:manage` | Create, list, read, update and delete partners, the integrators whose partner keys create tenants ([Partners](#partners)). Platform keys only |
| `platform:ops` | Platform operations: signing-key rotation, the dead-letter queue, maintenance jobs, waitlist invitations, switches, mailbox restores and re-applied erasures (platform keys only) |
| `metrics:read` | Pause-window rates and event counts ([Deliverability metrics](#deliverability-metrics)); the screening summary ([Screening](#screening)). Every key level; never implicit. An identity key reads rates for its own identity, its tenant and the domains its addresses use, and event counts for its own identity only |

The upload routes take either `messages:send` or `drafts:write`: a key needs one of them, not both.

Key levels limit which resources a key can reach, whatever its permissions. From widest to narrowest:

- A **platform** key reaches every tenant.
- A **partner** key reaches the tenants created with its partner's keys, and its partner's webhook
  endpoints ([Partners](#partners)). It uses `tenant_id` and resource IDs exactly as a platform key does.
  A tenant created by another partner, or by no partner, answers it as a missing one does.
- A **tenant** key reaches its own tenant.
- An **identity** key reaches its own identity. It also reaches the tenant's domains read-only with
  `domains:read`, and the tenant's webhook endpoints and deliveries read-only with `webhooks:read`.

**Provisional agent sign-ups.** While a workspace is a provisional agent sign-up, every key of it acts
with at most `identities:read`, `messages:read`, `messages:send`, `messages:write`, `attachments:read`,
`search:read`, `events:read` and the implicit `usage:read`, whatever it lists; a route outside that set
answers `403 permission_denied` with `details.reason = "signup_unverified"` ([Agent sign-up](#agent-sign-up)).

A route or field that needs a higher key level than the caller's returns `403 scope_denied`: for example
an identity key on tenant search, or a partner key on `PATCH /v1/tenants/{tenant_id}/billing` of one of
its own tenants. Wherever this page allows "tenant or platform keys" or says what a platform key passes
(`tenant_id`, filters), a partner key is allowed and passes the same, for its own tenants only.

Some permissions can be held only at some levels. [`POST /v1/keys`](#post-v1keys) refuses a key that
lists one its level cannot hold with `400 invalid_request` and
`details.reason = "permission_not_allowed_for_level"`:

| Permissions | Key levels that can hold them |
|---|---|
| `platform:ops`, `partners:manage` | platform |
| `tenants:manage` | platform, partner |
| `members:read`, `members:manage`, `suppressions:manage`, `audit:read`, `usage:read`, `policy:write`, `accounts:approve`, `drafts:approve`, `phone:approve`, `phone:manage`, `phone:compliance`, `vault:use`, `vault:approve`, `vault:admin` | platform, partner, tenant (an identity key holds `usage:read` implicitly for its own workspace, but cannot list it)  |
| `tenants:erase` | platform, partner, and a tenant key minted by the workspace owner's console session or by a key descended from one (otherwise `details.reason = "permission_owner_only"`) |
| `identities:sign` | tenant, identity (platform and partner keys may grant it without holding it) |
| Every other permission | platform, partner, tenant, identity |

There are no wildcard permissions and no implicit full set: every key, platform and partner keys included,
holds the permissions listed when it was created, plus the implicit `usage:read` of tenant and identity keys. A
`POST /v1/keys` without `permissions`, or with an empty list, returns `400 invalid_request`. A key can never
create, read, rotate or revoke a key wider than itself ([API keys](#api-keys--keysmanage)).

### The console and billing routes

These routes are served by the same Worker but are not part of the developer API. None takes an API key:
they use session cookies, OAuth state, unsubscribe tokens, or Stripe, SNS and link signatures instead.

| Route | What it is | In `openapi.yaml` | Design |
|---|---|---|---|
| `/console/*`: the server-rendered console, including `/console/sign-in…` (link and code), `/console/sign-up`, `/console/waitlist`, `/console/workspaces/new`, `/console/oauth/{provider}/start`, `/console/oauth/{provider}/callback`, `/console/settings/security`, `/console/settings/notifications`, `/console/plan/return`, `/console/connect`, `/console/agent-signup/verify`, `/console/agent-signup/claim` and `/console/agent-signup/refuse` | Console pages, sign-up and sign-in (session cookies) | No | [Console design](../project/design/console.md), [Cloud sign-up and sign-in](../project/design/cloud-signup.md), [Agent self-sign-up](../project/design/agent-signup.md) |
| `GET /console/notifications/unsubscribe?t={token}`, `POST /console/notifications/unsubscribe?t={token}` | Unsubscribe from a kind of notification email. `GET` shows a confirmation page with a one-click form; `POST` is the RFC 8058 one-click unsubscribe and turns that kind off for that person and workspace. The token `t` is the only authority: no session, no CSRF token or `Origin` check, served even with `PM_CONSOLE=off`. An expired or foreign token changes nothing | No | [Notifications](../project/design/notifications.md#5-the-emails) |
| `/billing/stripe/webhook` | Stripe events (Stripe signature) | No | [Billing design](../project/design/billing.md) |
| `POST /hooks/ses`, `POST /hooks/ses/inbound` | Amazon SES delivery events and inbound mail, through SNS (SNS signature) | Yes | [Signed links and provider hooks](#signed-links-and-provider-hooks) |
| `GET /v1/links/{token}` | Signed downloads (link signature) | Yes | [Signed links and provider hooks](#get-v1linkstoken) |
| `PUT /v1/links/{token}` | Send an upload's bytes (link signature) | Yes | [Uploads](#put-v1linkstoken) |

**Two hosts.** `PM_CONSOLE_HOST` names the console's host and defaults to `PM_API_HOST`, so a deployment
can keep one hostname. When the two differ, console paths (`/console/*`, the unsubscribe pair included)
answer only on `PM_CONSOLE_HOST`, and the API host `PM_API_HOST` serves exactly:

- the REST API, `/v1/*`;
- MCP, `/mcp`;
- `/openapi.json` and `/health`;
- `/.well-known/*` (the security contact, identity JWK Sets and the Web Bot Auth key directory);
- signed links, `/v1/links/*`;
- the provider hooks, `/hooks/*`, and `/billing/stripe/webhook`.

Anything else returns `404 route_not_found`, as does a path that is no route at all; a route called with a
method it does not have returns `405 method_not_allowed` with an `Allow` header. No cookie is set or read on the API host
([Cloud sign-up › Hostnames](../project/design/cloud-signup.md#2-hostnames)).

### Errors

Every error looks like this:

```json
{
  "error": {
    "code": "idempotency_conflict",
    "message": "This Idempotency-Key was used with a different request body.",
    "retryable": false,
    "fix": "Use a new Idempotency-Key for a different message, or resend the original body.",
    "docs": "https://pylotamail.com/docs/reference/errors#code-idempotency_conflict",
    "request_id": "req_01J9Z4…",
    "details": { "original_message_id": "msg_01J9Z3…" }
  }
}
```

`docs` links to the code's entry on the deployment's published errors page (`PM_DOCS_URL`,
[Configuration](configuration.md#variables)).

The code catalogue is in [Errors](errors.md).

---

## Meta

### `GET /health`

No auth. Returns `200` with `{ "status": "ok", "version": "1.0.0", "commit": "abc1234", "env": "production" }`.
`env` is `PM_ENV`; `version` and `commit` are compiled into the release bundle. When SES is configured,
the body also has `ses_region` (the value of `PM_SES_REGION`). While the deployment is frozen for a restore
it has `"frozen": true` ([Observability § 7.1](../project/design/observability.md#71-get-health)).

- **Degraded.** When the Worker serves with a feature turned off by its configuration, `status` is
  `degraded`, still with `200`, and one field per feature names why: `ses` (`sns_topic_missing`: SES
  credentials without `PM_SES_SNS_TOPIC_ARN`, so the SES transport is off), `billing`
  (`stripe_secrets_missing`: `PM_BILLING=stripe` without its secrets, so billing has not started) and
  `cf_api_token` (`missing`: a Cloudflare REST fallback is in use without `PM_CF_API_TOKEN`). Absent
  fields are healthy.
- **Invalid configuration.** A missing required variable or secret, or a malformed optional one, returns
  `503 unavailable` with `details.config_invalid` naming the variable, as every other route does
  ([Rust workspace › Startup rules](../project/design/rust-workspace.md#61-errors-and-configuration)).

### `GET /openapi.json`

No auth. The OpenAPI 3.1 document for this deployment.

### `GET /v1/me`

Any key. Describes the calling key. For a partner key, `level` is `partner`, `partner_id` names its
partner, and `tenant_id` and `identity_id` are `null`.

```json
{
  "key_id": "key_01J9…", "name": "pylota-api", "level": "tenant", "mode": "live",
  "partner_id": null, "tenant_id": "ten_01J9…", "identity_id": null,
  "permissions": ["identities:read", "messages:send", "search:read"],
  "mcp_tools": null,
  "expires_at": null
}
```

`mcp_tools` is the key's MCP tool list, or `null` for every allowed tool.

---

## Tenants

Keys with `tenants:manage`: a platform key reaches every tenant, and a partner key the tenants its
partner's keys created ([Partners](#partners)). A tenant key can `GET /v1/tenants/{tenant_id}` for its
own tenant; it cannot list tenants or change them, except its policy through
[`PATCH /v1/tenants/{tenant_id}/policy`](#get-v1tenantstenant_idpolicy--patch-v1tenantstenant_idpolicy--policywrite)
with `policy:write`.

### `POST /v1/tenants`

```json
{
  "slug": "acme",
  "name": "Acme Car Hire",
  "mode": "live",
  "timezone": "Europe/London",
  "address_suffix": ".acme",
  "policy": { "identity_daily_send_cap": 500 },
  "owner": { "email": "sam@acmecarhire.example", "name": "Sam Patel" },
  "billing": { "mode": "exempt" }
}
```

- `address_suffix` defaults to `"." + slug`. Only one tenant (the default tenant made by `idvy setup`)
  can have an empty suffix. Every tenant shares the platform domain, so a suffix must not pass for
  someone else ([D12](../project/edge-cases.md)). Its fold (the confusable fold of
  [Identities › Username validation](../project/design/identity-domains.md#username-validation), without the
  dot) is refused with `400 address_reserved` (`details.field = "address_suffix"`) when it equals the fold
  of a reserved username, or of a name in the compiled list `core::address::RESERVED_SUFFIXES`: `pylota`,
  `pylotamail`, `stripe`, `paypal`, `google`, `gmail`, `microsoft`, `outlook`, `apple`, `icloud`, `amazon`,
  `aws`, `cloudflare`, `github`, `hmrc`, `gov`, `govuk`, `dvla`, `police`, `bank`, `visa`, `mastercard`,
  `amex`, `billing`, `payments` and `admin`. A suffix whose fold equals another tenant's (`.acrne` beside
  `.acme`) gets `409 suffix_taken`, enforced by the unique `tenants.suffix_fold`.
- `policy` is merged over the defaults. See [Configuration › Tenant policy](configuration.md#tenant-policy).
- `owner` (optional) creates the workspace's console owner and emails them a sign-in link. Without it, a
  platform or partner key can add an owner later with an invitation and an ownership transfer in the
  console. An address this deployment hosts (on the platform domain, or an identity's address) cannot
  be an owner: `400 invalid_request` with `details.errors[0].path = "owner.email"`, because any key that
  reads that mailbox could read the sign-in codes. The owner email is system mail of class `invitation`
  ([Cloud sign-up § 10.2](../project/design/cloud-signup.md#102-system-mail-budgets)), keyed by the creating
  partner (or `platform` for a platform key) and by the owner's address. The budget is checked before
  anything is written: when it is spent, the answer is `429 daily_cap_reached` with
  `details.cap: "invitations"` and `details.resets_at` (the next 00:00 UTC), and no tenant is created.
- `billing.mode` defaults to `metered` on a deployment with billing on (plan `free`) and to `disabled`
  otherwise.
- **With a partner key**, the new tenant's `partner_id` is the key's partner, for good, and its billing
  mode is the partner's `default_billing_mode`. `billing` is platform-only: a partner key that sends it
  gets `403 scope_denied`. The audit row `tenant.create` records the `partner_id`.
  - `policy` is checked field by field, for the fields sent: a platform-only field, or a lower-only
    field above its ceiling, gets `403 scope_denied` with `details.field`
    ([Configuration › Who may change a field](configuration.md#who-may-change-a-field)). A partner key may
    set `quarantine.key_release` and `drafts.key_approval` here, at creation.
  - A partner has at most `max_tenants` tenants that are not `erased` (default 25): the next creation
    gets `403 partner_tenant_limit` with `details.max_tenants`. Creations count in `RL_PARTNER` (10 a
    minute per partner, shared with invitations; [Rate limits](#rate-limits)).

Returns `201` with a [Tenant](#tenant-object).

### `GET /v1/tenants` · `GET /v1/tenants/{tenant_id}`

List (filters: `status`, `mode`, `partner_id`; platform and partner keys) and get. A partner key lists
only its own tenants, and keeps reading one while it is `erasing` and after it is `erased`. An unknown
`partner_id`, or for a partner key any partner but its own, returns `404 partner_not_found`.

### `PATCH /v1/tenants/{tenant_id}`

Updatable: `name`, `timezone`, `policy` (deep merge; `null` resets a field to its default), and `status`
(`active` | `suspended`). Suspension behaviour: FR-TEN-3. `partner_id` and `mode` never change. A tenant
key cannot call this route (`403 permission_denied`: it can never hold `tenants:manage`).

- **A partner key** updates only its own tenants, `policy.quarantine.key_release` and
  `policy.drafts.key_approval` included. Each policy
  field sent is checked by its class: platform-only fields get `403 scope_denied`, and a lower-only field
  may be set at most to min(deployment default, platform ceiling), otherwise `403 scope_denied` with
  `details.field` ([Configuration › Who may change a field](configuration.md#who-may-change-a-field)), so
  one partner cannot spend the shared sending reputation or AI budget.
- **Operator enforcement stays.** `suspended_by` records who suspended the tenant; a partner key that sets
  `status: "active"` on a tenant a platform key suspended gets `403 scope_denied`
  (`details.field: "status"`). `status: "suspended"` on a tenant that is already suspended changes
  nothing (`suspended_by` and `suspended_at` stay), so a partner cannot take over a platform suspension and
  then lift it; a platform key's suspension replaces a partner's. A value a platform key sets on a
  lower-only field becomes that field's ceiling for partner keys ([J17](../project/edge-cases.md)).
- **Erasing and erased tenants.** Once a tenant is `erasing` or `erased`, only the erasure job changes its
  status: a platform key gets `409 tenant_erased`, and any other key gets `404 tenant_not_found` here and
  on every other write to the tenant ([I8](../project/edge-cases.md)).
- **Listed zones.** Dropping a zone from `policy.domains.cloudflare_zones` while the tenant still has
  domains in it returns `409 domain_in_use` with `details.reason: "zone_has_domains"` and
  `details.domain_ids`; remove those domains first ([H13](../project/edge-cases.md)).
- **Every policy write** goes through the same checks as `PATCH …/policy` below: it is a compare-and-set
  (`503 unavailable` after three lost attempts), writes the audit row `tenant.policy_update` and emits
  `tenant.policy_updated`. A lower-only value a partner key sets also becomes that field's partner ceiling
  for the tenant's own keys and people ([J26](../project/edge-cases.md)).

### `GET /v1/tenants/{tenant_id}/policy` · `PATCH /v1/tenants/{tenant_id}/policy` — `policy:write`

The workspace's own policy route ([Workspace policy](../project/design/workspace-policy.md)). Platform,
partner (its own tenants) and tenant (its own tenant) keys holding `policy:write`; identity keys can never
hold it. `PATCH` takes a partial policy (the `policy` object of `PATCH /v1/tenants/{tenant_id}`: deep
merge, `null` resets a field, arrays replace) and checks each field sent by the caller's level
([Configuration › Who may change a field](configuration.md#who-may-change-a-field)). For a tenant key:

- free fields take any valid value;
- a lower-only field may be set at most to its workspace ceiling, the strictest of the deployment default,
  the platform ceiling and the partner ceiling; above it, `403 scope_denied` with `details.field`,
  `details.reason = "above_ceiling"`, `details.ceiling` and `details.ceiling_source`
  ([J23](../project/edge-cases.md), [J26](../project/edge-cases.md));
- a guard field (`send_allowlist_only`, `quarantine.on_auth_fail`, `quarantine.spam_threshold`,
  `quarantine.unsolicited_otp`, `drafts.require_approval`, `screening.mode`, `screening.on_unavailable`,
  `screening.deadline_seconds`) may only be tightened, unless API keys may take decisions reserved for
  people on this tenant (`PM_QUARANTINE_KEY_RELEASE=on`; or the tenant's `quarantine.key_release: true`, and
  then only for a platform key or the tenant's partner key, never a tenant or identity key);
  otherwise `403 permission_denied` with `details.field` and `details.reason = "person_required"`
  ([J25](../project/edge-cases.md));
- platform-only fields, `quarantine.key_release` and `drafts.key_approval` get `403 scope_denied` with `details.field` and
  `details.reason = "not_writable"` ([J24](../project/edge-cases.md), [J14](../project/edge-cases.md)).

One refused field refuses the whole write. Concurrent writes never lose one another: a write that keeps
losing the compare-and-set gets `503 unavailable` (retryable) after three attempts
([J27](../project/edge-cases.md)). Both methods return the policy view; `GET` changes nothing:

```json
{ "tenant_id": "ten_01J9…", "policy_version": 7,
  "policy": { "...": "full effective policy" },
  "fields": [
    { "field": "tenant_daily_send_cap", "class": "lower_only", "writable": true, "ceiling": 1000,
      "ceiling_source": "partner", "person_required_to_loosen": false },
    { "field": "quarantine.on_auth_fail", "class": "guard", "writable": true, "ceiling": null,
      "ceiling_source": null, "person_required_to_loosen": true },
    { "field": "quarantine.key_release", "class": "partner_or_platform", "writable": false, "ceiling": null,
      "ceiling_source": null, "person_required_to_loosen": false } ] }
```

`fields` describes what **the caller** may do: for a platform key every field is `writable` with no
ceiling. Each write writes the audit row `tenant.policy_update` and emits `tenant.policy_updated`.
Errors: `400 invalid_request`, `403 permission_denied`, `403 scope_denied`, `404 tenant_not_found`
(out of scope, or a write to an `erasing` or `erased` tenant), `503 unavailable`.
- **Automatic sending pause.** `sending_paused_at` is set when the tenant's complaint or bounce rate
  reaches the provider's review level over 7 days ([G12](../project/edge-cases.md)), or when one of its
  domains has a burst far above it within an hour or a day ([J33](../project/edge-cases.md)); the
  `409 sending_paused` a send gets names which in `details.reason`. `"sending_paused": false` lifts
  it, audit-logged; only a platform key may send it (`403 scope_denied`, `details.field:
  "sending_paused"`), and no key can set a pause by hand.

#### Tenant object

```json
{
  "id": "ten_01J9…", "slug": "acme", "name": "Acme Car Hire", "mode": "live", "status": "active",
  "suspended_by": null, "partner_id": null, "address_suffix": ".acme", "timezone": "Europe/London",
  "policy": { "...": "full effective policy" }, "sending_paused_at": null,
  "created_at": "2026-10-09T10:00:00Z", "updated_at": "2026-10-09T10:00:00Z"
}
```

`partner_id` is the partner whose key created the tenant, or `null`; it never changes, also after the
tenant is erased and the partner deleted. `suspended_by` is `platform` or `partner` while the tenant is
suspended, otherwise `null`. Tenants are deleted through an erasure request with `scope: "tenant"`.

---

## Partners

A **partner** is an integrator that creates tenants for its own customers on a shared deployment and
manages them with **partner keys**. On Pylota Identity Cloud, Pylota is a partner: each car-rental operator
is a tenant created with Pylota's partner key, billed `exempt`, and no Pylota key reaches another Cloud
customer ([FR-KEY-4](../project/prd.md#61-tenancy-and-access)). The routes in this section need a
platform key with `partners:manage`, which a partner key can never hold.

### `POST /v1/partners`

```json
{ "name": "Pylota", "default_billing_mode": "exempt", "max_tenants": 25, "ramp_exempt": false }
```

`default_billing_mode` is `exempt` or `metered` (the default). `max_tenants` (default 25) is the most
tenants that are not erased the partner may have. `ramp_exempt` (default `false`) lets its new tenants
skip the new-workspace send ramp, which they otherwise follow whatever their billing mode
([Cloud sign-up › New-workspace send ramp](../project/design/cloud-signup.md#101-new-workspace-send-ramp)).
Returns `201` with a [Partner](#partner-object). Audit-logged (`partner.create`).

### `GET /v1/partners` · `GET /v1/partners/{partner_id}`

List (filter: `status`) and get. An unknown ID returns `404 partner_not_found`; a deleted partner is
returned with `status: "deleted"` and an empty `name`.

### `PATCH /v1/partners/{partner_id}`

Updatable: `name`, `status` (`active` | `suspended`), `default_billing_mode`, `max_tenants` and
`ramp_exempt`. A deleted partner returns `404 partner_not_found`. Audit-logged (`partner.update`).

- Suspending a partner contains it at once: every one of its keys, and every tenant and identity key of
  its tenants, gets `403 partner_suspended` on every route, so nothing can send for those tenants. Their
  status does not change and their inbound mail is still stored. Deliveries to the partner's endpoints
  and to its tenants' endpoints are held, and resume when the partner is `active` again
  ([J13](../project/edge-cases.md)).
- Lowering `max_tenants` below the current count refuses new tenants and changes no existing one.
- A new `default_billing_mode` applies to tenants created afterwards. Existing tenants keep their mode,
  which only a platform key changes ([`PATCH /v1/tenants/{tenant_id}/billing`](#get-v1tenantstenant_idbilling--patch-v1tenantstenant_idbilling--tenantsmanage-platform-key-to-change)).

### `DELETE /v1/partners/{partner_id}`

Returns `204`. While any tenant with this `partner_id` is not `erased` (it is `active`, `suspended` or
`erasing`), it returns `409 partner_has_tenants` with `details.tenants`, how many, and changes nothing:
erase those tenants first (`POST /v1/erasure-requests` with `scope: "tenant"`). Deletion is soft: the
partner stays with `status: "deleted"` and an empty `name`; its partner keys are revoked and deleted, and
its partner webhook endpoints deleted with their deliveries. Its erased tenants keep their `partner_id`.
Audit-logged (`partner.delete`).

#### Partner object

```json
{ "id": "ptn_01JA…", "name": "Pylota", "status": "active", "default_billing_mode": "exempt",
  "max_tenants": 25, "ramp_exempt": false,
  "created_at": "2026-10-10T09:00:00Z", "updated_at": "2026-10-10T09:00:00Z" }
```

A partner holds nothing but its name and these settings. `status` is `active`, `suspended` or `deleted`.

### Partner keys

Only a platform key mints a partner key, with [`POST /v1/keys`](#post-v1keys), `level: "partner"` and
the `partner_id`; only a platform key rotates or revokes one:

```json
{ "name": "pylota-backend", "level": "partner", "partner_id": "ptn_01JA…",
  "permissions": ["tenants:manage", "tenants:erase", "keys:manage", "webhooks:manage", "quarantine:review",
                  "usage:read", "audit:read", "identities:read", "identities:write", "domains:read",
                  "domains:write", "messages:read", "messages:send", "messages:write", "attachments:read",
                  "search:read", "search:agentic", "erasure:manage", "suppressions:manage",
                  "members:manage"] }
```

A partner key acts only on the tenants its partner's keys created, with `tenant_id` or a resource ID,
exactly as a platform key does:

| Permission | What a partner key can do with it |
|---|---|
| `tenants:manage` | Create tenants (each gets the partner's `partner_id` and `default_billing_mode`; at most `max_tenants`), list, read, update and suspend its own, and read their billing accounts. It never changes a billing account or sends `billing` (`403 scope_denied`), raises a lower-only policy field above its ceiling, sets a platform-only one, or lifts a platform suspension |
| `keys:manage` | Mint, list, rotate and revoke tenant and identity keys of its own tenants, granting them `identities:sign` when their agents sign (it cannot hold it itself). Never a partner or platform key (`404 key_not_found` for an existing one, `403 key_scope_exceeded` to mint one), and never a key wider than itself |
| `tenants:erase`, `erasure:manage` | Delete one of its tenants (`POST /v1/erasure-requests` with `scope: "tenant"`), which [`DELETE /v1/partners/{partner_id}`](#delete-v1partnerspartner_id) needs first, and the other erasure scopes |
| `webhooks:manage`, `webhooks:read` | Partner endpoints (`POST /v1/webhooks` makes one, with `scope: "partner"`), which receive only its own tenants' events, and its tenants' endpoints ([Webhooks](#webhooks)) |
| `events:read` | Its tenants' event logs, one tenant per request with `tenant_id` ([Event feed](#event-feed)) |
| `quarantine:review` | See and release its tenants' quarantined mail; release by key follows the tenant's `quarantine.key_release` ([Release](#post-v1identitiesidentity_idmessagesmessage_idrelease--quarantinereview)) |
| `usage:read` | Read one of its tenants' usage, with `tenant_id` |
| Every other tenant-level permission | The same as a platform key, on its own tenants: identities and their addresses and signing keys (not `identities:sign`), domains, mail, search, suppressions and lists, audit, members |

The example lists what Pylota's backend uses: tenant creation and deletion, keys for its operators'
agents, webhooks, quarantine release, usage and the audit log of its tenants (`audit:read`, which
[`GET /v1/audit-events`](#get-v1audit-events--auditread) needs), identities and domains, mail and search
(agentic included), erasure requests for its operators' data subjects, suppressions and lists, and members.

A partner key can never hold `platform:ops`, `partners:manage` or `identities:sign`, and never reaches
`/v1/platform/*`, `/v1/partners/*`, the platform's webhook endpoints, or any partner or platform key, its
own included (`GET /v1/me` describes it). A tenant created by another partner, or by no partner, and
everything in it, answers `404 …_not_found` exactly as a missing ID does. A suspended partner's keys,
and its tenants' keys, get `403 partner_suspended`. Partner keys are `live`, act on both the `live` and
`test` tenants of their partner, and count against the same [rate-limit buckets](#rate-limits) as
platform keys, keyed by their own key ID, and against `RL_PARTNER`, keyed by the partner, for tenant
creation and invitations.

Whatever the number of its keys, one partner is bounded by `max_tenants` (25 by default) times each
tenant's caps: with the default `tenant_daily_send_cap` of 5,000, at most 125,000 messages a day, and
1,250 while its new tenants are on the send ramp. Only a platform key raises `max_tenants`, a ceiling or
`ramp_exempt` ([Security › Partner keys](../project/design/security.md#partner-keys)).

---

## Identities

### `POST /v1/tenants/{tenant_id}/identities` — `identities:write`

```json
{
  "username": "bookings",
  "display_name": "Acme Car Hire",
  "purpose": "bookings",
  "owner": { "name": "Sam Patel", "email": "sam@acmecarhire.example" },
  "signature": { "text": "Acme Car Hire · 0113 496 0000" },
  "domain_id": "dom_01J9…",
  "client_id": "acme:bookings",
  "send_policy": { "daily_cap": 300 },
  "retention": { "message_hours": null },
  "metadata": { "operator_id": "op_123" }
}
```

- `send_policy` (optional) sets the identity's own send policy at creation, as `PATCH` does later. For any
  key but a platform key, a `daily_cap` above the tenant's effective `identity_daily_send_cap` gets
  `403 scope_denied` with `details.field: "send_policy.daily_cap"`, and nothing is created.
- `retention.message_hours` (optional, `null` or 1–8,760): each message of the identity, with its
  attachments, index rows and drafts, is erased that many hours after it is stored, except under a legal
  hold ([Parity additions § 7](../project/design/parity-additions.md#7-per-identity-message-retention-fr-prv-7)).
  A number needs `erasure:manage` as well (`403 permission_denied`, `details.required: ["erasure:manage"]`).

- `username`: stored lower case as `^[a-z0-9][a-z0-9._-]{0,23}$`. The request value is checked by the
  username rules, not by a schema pattern, so each failure has its own code: a reserved or confusable name
  gets `address_reserved`, any other non-ASCII character `address_unsupported`, and anything else that
  does not lower-case to the stored form `address_invalid`.
  `postmaster`, `abuse`, `noreply` and similar are reserved everywhere; the other RFC 2142 role names
  (`support`, `sales`, `info`, `marketing` and the rest) only where they would stand alone on the shared
  platform domain, that is, for the default tenant, whose suffix is empty
  ([Identities and domains](../project/design/identity-domains.md#username-validation)).
- In the default tenant, whose addresses stand alone on the platform domain, only a platform key may create
  identities; any other key gets `403 scope_denied` with `details.reason = "default_tenant"`
  ([A18](../project/edge-cases.md)).
- The primary address is `{username}{tenant.address_suffix}@{platform domain}`, or
  `{username}@{domain}` when `domain_id` names a tenant domain that is `healthy` or `degraded`. The full local part must be at
  most 64 characters with room for a thread token: the combined username and suffix can be at most 40.
- `client_id` makes the create idempotent: the same `client_id` with the same body returns `200` and
  the existing identity, and with a different body returns `409 client_id_conflict`.
- `owner` is required before the identity can send (`identity_owner_required`).
- When the plan's `inboxes` allowance is spent, the request fails with `402 billing_limit`
  (`details.feature: "inboxes"`). A primary address that needs a literal routing rule while
  `PM_CF_API_TOKEN` is not set fails with `422 cf_token_required`.

Returns `201` with an [Identity](#identity-object).

### `GET /v1/tenants/{tenant_id}/identities` — `identities:read`

Filters: `status`, `purpose`, `client_id`.

### `GET /v1/identities` — `identities:read`

Identities the key can reach. Filters: `status` (`active`, `paused`, `deleting` or `deleted`), `purpose`,
and for platform and partner keys `tenant_id`; `status` and `purpose` work as on the tenant's list above. The system
identity that sends `PM_SYSTEM_FROM` mail is never listed.

### `GET /v1/identities/lookup?address=bookings@acme.example.com` — `identities:read`

Resolves any active or retiring address to its identity. Returns `404 identity_not_found` for unknown,
retired or out-of-scope addresses.

### `GET /v1/identities/{identity_id}` — `identities:read`

Returns the identity in any status, `deleting` and `deleted` included; once erased, a `deleted` identity
has its personal fields scrubbed (`display_name` empty, `owner` and `signature` `null`, `metadata` `{}`,
[Privacy § 6.5](../project/design/privacy.md#65-identity-scope-fr-idn-4)).

### `PATCH /v1/identities/{identity_id}` — `identities:write`

On a `deleting` or `deleted` identity this returns `404 identity_not_found` and changes nothing, like every
other write by identity ID except the legal-hold routes, which stay usable on a `deleting` identity
([Privacy § 6.5](../project/design/privacy.md#65-identity-scope-fr-idn-4)).

Updatable: `display_name`, `purpose`, `owner`, `signature`, `metadata`, `send_policy`, `retention`, and
`status` (`active` | `paused`). Setting `status: "active"` on an identity paused for `abuse_threshold` needs a
platform, partner or tenant key and is audit-logged; on a tenant a partner's key created it needs a
platform key (`403 scope_denied`, [J17](../project/edge-cases.md)). A `send_policy.daily_cap` above the
tenant's effective `identity_daily_send_cap` needs a platform key (`403 scope_denied` with
`details.field: "send_policy.daily_cap"`); the same applies at creation.
`send_policy.require_approval` (`never`, `new_recipients`, `external`, `always` or `null`) can only make
the tenant's `drafts.require_approval` stricter; a looser value than the identity's current one gets
`403 scope_denied` from an identity key and `403 permission_denied` (`person_required`) from a tenant
key where people decide ([Drafts](#drafts)). Setting `retention.message_hours` where it was `null`, or
lowering it, needs `erasure:manage` as well; raising it or setting `null` needs `identities:write` only and
brings nothing back. Each change writes the audit row `identity.retention_update`.

### `DELETE /v1/identities/{identity_id}` — `identities:write` and `erasure:manage`

Returns `202` with an [Erasure request](#erasure-request-object) of scope `identity`. A second `DELETE` of
a `deleting` or `deleted` identity returns `200` with the identity's latest identity-scope erasure request
(the same `era_` ID) and starts nothing, unless that request is `failed`: then it returns `202` with a new
request that resumes it at its failed step. The system identity cannot be deleted
(`409 system_protected`, [A17](../project/edge-cases.md)). The identity's
addresses are tombstoned and can never be assigned to another identity. Its
[signing keys](#identity-keys-and-signatures) are deleted and their key IDs tombstoned, so a deleted key ID
is never published again ([O7](../project/edge-cases.md)). While the identity is `deleting` or `deleted`,
signing and its JWK Set return `404 identity_not_found`.

#### Identity object

```json
{
  "id": "idn_01J9Z3K8V4…", "tenant_id": "ten_01J9…",
  "username": "bookings", "display_name": "Acme Car Hire", "purpose": "bookings",
  "status": "active", "pause_reason": null,
  "primary_address": "bookings.acme@agents.example",
  "addresses": [ { "...": "Address objects" } ],
  "owner": { "name": "Sam Patel", "email": "sam@acmecarhire.example" },
  "signature": { "text": "…", "html": null },
  "send_policy": { "daily_cap": 500, "auto_reply": "allowed", "require_known_recipient": true,
                   "require_approval": null },
  "retention": { "message_hours": null },
  "metadata": { "operator_id": "op_123" },
  "client_id": "acme:bookings",
  "created_at": "…", "updated_at": "…"
}
```

---

## Addresses

### `GET /v1/identities/{identity_id}/addresses` — `identities:read`

### `POST /v1/identities/{identity_id}/addresses` — `identities:write`

```json
{ "local_part": "bookings", "domain_id": "dom_01JA…" }
```

Creates an `alias`. `local_part` follows the username rules for a tenant domain, with a maximum of 40
characters instead of 24 (stored as `^[a-z0-9][a-z0-9._-]{0,39}$`, with the same error codes): role names
such as `support@` are allowed, `postmaster` and `abuse` are not. The status is `pending` until the domain is
`healthy` or `degraded`, then `active`. Only one pending
address per identity and domain is allowed; a newer request replaces an older pending one
([A11](../project/edge-cases.md)).

### `POST /v1/identities/{identity_id}/addresses/{address_id}/promote` — `identities:write`

```json
{ "retire_previous_after_days": 90 }
```

Makes the address `primary`. The previous primary becomes an `alias` with status `retiring`, and its
`retire_at` is set (default 90 days, range 0–365), with one exception: when the previous primary is the
identity's **platform address**, it becomes an `active` alias instead. The platform address is the
fallback address for domain failures (FR-DOM-6), so it is never retired. Promoting a `retiring` address
(or the platform address) back cancels the change: this is how you roll back. Fails with
`409 domain_not_ready` unless the domain is `healthy` or `degraded`. Emits `identity.address_promoted`.

### `POST /v1/identities/{identity_id}/addresses/{address_id}/retire` — `identities:write`

```json
{ "after_days": 0 }
```

Moves an alias to `retiring` (or straight to `retired` when `after_days` is 0). The primary cannot be
retired (`409 address_is_primary`), and neither can the identity's platform address
(`409 address_in_use`). Emits `identity.address_retired` when the address becomes `retired`.

### `DELETE /v1/identities/{identity_id}/addresses/{address_id}` — `identities:write`

Only for `pending` addresses that never received mail. Otherwise `409 address_in_use` (retire it
instead). The platform address can never be deleted.

### `POST /v1/identities/{identity_id}/addresses/{address_id}/test-forwarding` — `identities:write`

No body. For an address on a domain with `inbound: forward` (method `send_only`, or `smtp_relay` with
`inbound: forward`), where the customer's own mailbox forwards mail to the identity's platform address.
Any other address returns `422 transport_unavailable` with `details.reason: "method_not_supported"`.

It sends a short message to the address, from `mailer-daemon@{platform domain}` with the subject
"Pylota Identity forwarding check" and a one-time token. If the token reaches the identity's platform address
within 10 minutes, the address's `forwarding` becomes `ok`; otherwise `failed`
([N12](../project/edge-cases.md)). The check is never stored as a message and does not count as a plan
send. Returns `202` with the [Address](#address-object); read the address again for the result. No
webhook event is sent.

#### Address object

```json
{
  "id": "adr_01J9…", "identity_id": "idn_01J9…", "address": "bookings@brightwell.example",
  "local_part": "bookings", "domain_id": "dom_01JA…",
  "role": "primary", "status": "active",
  "retire_at": null, "retired_at": null,
  "forwarding": "ok", "forwarding_checked_at": "2026-10-09T10:20:00Z",
  "created_at": "…"
}
```

- `forwarding` is `null` when the address's domain does not use `inbound: forward`. Otherwise it is
  `unverified` (no forwarding test and no forwarded message has arrived yet), `ok` (the last test passed,
  or mail arrived through forwarding) or `failed` (the last test timed out).
- `forwarding_checked_at` is when `forwarding` last changed, or `null`.

---

## Identity keys and signatures

An identity can prove who it is outside email: with an **agent assertion**, a short-lived JWT signed by
the identity's own Ed25519 key that any service can check against the identity's JWK Set, and with a
**signed HTTP request** (Web Bot Auth), whose headers let a website tell which agent made the request. The
design is in [Agent signing keys](../project/design/agent-keys.md); the integrator's view is in
[Agents › Agent assertions](../guides/agents.md#agent-assertions).

- Each identity has at most one `active` key, which signs and is published, plus `retiring` keys during
  an overlap after a rotation. A key is created on the identity's first signing request, or with
  `POST …/keys`. Private keys are generated, sealed and used inside the Worker; no endpoint returns them.
- Key management (`…/keys`, rotate, revoke) stays available while the identity is paused, so a suspected
  leak can be handled before it resumes. Signing does not: suspended tenant → `403 tenant_suspended`
  (checked first, as on sends); paused identity → `409 identity_paused`. The JWK Set of either answers
  `404 identity_not_found` until the identity resumes ([O1](../project/edge-cases.md)). A `deleting` or `deleted` identity gets
  `404 identity_not_found` on every route here.
- Creating, rotating and revoking keys is audit-logged (`identity_key.create`, `identity_key.rotate`,
  `identity_key.revoke`) and emits `identity.key_created`, `identity.key_rotated` or
  `identity.key_revoked` ([Webhook events](events.md#identities-and-addresses)).
- Signing needs `identities:sign`, which platform and partner keys cannot hold (they can grant it to the
  tenant and identity keys they mint). Both signing endpoints count against
  the signing rate limit (600 a minute per identity, `429 rate_limited` over it), ignore
  `Idempotency-Key`, and store nothing but a daily count (`assertions` and `http_signatures` in
  [`GET /v1/usage/daily`](#get-v1usagedaily--usageread-platform-partner-or-tenant-key)). Signing is not metered
  against any plan allowance.

### `GET /v1/identities/{identity_id}/keys` — `identities:read`

Every key the identity has, `retired` ones included, newest first. Filter: `status` (`active`,
`retiring` or `retired`). Paginated.

```json
{
  "data": [
    { "kid": "zMkUmAQOlq9JtFPzTK1XINZdWd7gmhXxgA8Ph7cNKHo", "identity_id": "idn_01J9Z3K8V4…",
      "status": "active", "alg": "EdDSA",
      "public_jwk": { "kty": "OKP", "crv": "Ed25519", "x": "NjwMjIq2mTA1VpuDzRvkMIfQ0sCSHWavo0KT_4FcKO0",
                      "kid": "zMkUmAQOlq9JtFPzTK1XINZdWd7gmhXxgA8Ph7cNKHo", "alg": "EdDSA", "use": "sig" },
      "created_at": "2026-10-09T09:00:00Z", "verify_until": null, "retired_at": null },
    { "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k", "identity_id": "idn_01J9Z3K8V4…",
      "status": "retiring", "alg": "EdDSA",
      "public_jwk": { "kty": "OKP", "crv": "Ed25519", "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
                      "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k", "alg": "EdDSA", "use": "sig" },
      "created_at": "2026-10-02T09:00:00Z", "verify_until": "2026-10-16T09:00:00Z", "retired_at": null }
  ],
  "next_cursor": null
}
```

### `POST /v1/identities/{identity_id}/keys` — `identities:write`

No body (an empty `{}` is accepted). Creates the identity's first key and returns it with `201` when it
has no `active` key; otherwise returns the existing active key with `200` and changes nothing. A created
key emits `identity.key_created`, as does a key created lazily by a signing request; the `200` case emits
nothing. A thumbprint found among the key tombstones is never reused: a new seed is drawn instead.
`Idempotency-Key` is optional.

### `POST /v1/identities/{identity_id}/keys/rotate` — `identities:write`

No body. Makes a new key `active` at once and moves the previous active key to `retiring`, with
`verify_until` set to now plus `PM_IDENTITY_KEY_OVERLAP_DAYS` (default 7 days). The retiring key stays
in the JWK Set and no longer signs, so an assertion signed just before the rotation still verifies until
then ([O2](../project/edge-cases.md)). With no active key (none yet, or the active key was revoked), it
creates one, `previous` is `null` and it emits `identity.key_created`; otherwise it emits
`identity.key_rotated`. Two concurrent creations end with one active key
([O28](../project/edge-cases.md)). Returns `200`:

```json
{
  "key": { "kid": "zMkUmAQOlq9JtFPzTK1XINZdWd7gmhXxgA8Ph7cNKHo", "status": "active",
           "created_at": "2026-10-09T09:00:00Z", "verify_until": null, "retired_at": null,
           "...": "the rest of the Identity key object" },
  "previous": { "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k", "status": "retiring",
                "created_at": "2026-10-02T09:00:00Z", "verify_until": "2026-10-16T09:00:00Z",
                "retired_at": null, "...": "the rest of the Identity key object" }
}
```

### `POST /v1/identities/{identity_id}/keys/{kid}/revoke` — `identities:write`

No body. Moves the key straight to `retired`, whatever its state, for a suspected compromise. It is gone
from the next JWK Set response, and verifiers cache the set for at most 5 minutes
([O3](../project/edge-cases.md)). Returns `200` with the key (`status: "retired"`, `retired_at` set) and
emits `identity.key_revoked`. A key that is already `retired` is returned unchanged with `200`, and no
event is emitted. An unknown `kid` returns `404 key_not_found`. The row is kept until the identity is
deleted, so its thumbprint is never reused.

#### Identity key object

```json
{
  "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k", "identity_id": "idn_01J9Z3K8V4…",
  "status": "retiring", "alg": "EdDSA",
  "public_jwk": { "kty": "OKP", "crv": "Ed25519", "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
                  "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k", "alg": "EdDSA", "use": "sig" },
  "created_at": "2026-10-02T09:00:00Z", "verify_until": "2026-10-16T09:00:00Z", "retired_at": null
}
```

| Field | Meaning |
|---|---|
| `kid` | The key ID: the base64url RFC 7638 thumbprint of the public JWK (43 characters). It is also the JWS `kid` of every assertion the key signs |
| `status` | `active` (signs and is published; at most one), `retiring` (published, does not sign, until `verify_until`) or `retired` (not published) |
| `alg` | Always `EdDSA` (Ed25519) |
| `public_jwk` | The public key exactly as published in the identity's JWK Set |
| `verify_until` | Set when the key becomes `retiring`: the rotation time plus `PM_IDENTITY_KEY_OVERLAP_DAYS`. Until then the key stays in the JWK Set, unless it is revoked. `null` while `active` |
| `retired_at` | When the key became `retired`, or `null` |

### `POST /v1/identities/{identity_id}/assertions` — tenant or identity key, `identities:sign`

Mints an agent assertion: a JWT signed with the identity's active key. Each call mints a new token, so
`Idempotency-Key` is ignored and never recorded.

```json
{ "audience": "https://portal.supplier.example",
  "expires_in": 300,
  "nonce": "b3f1c2d47a9e",
  "ext": { "booking_ref": "BK-2291" } }
```

| Field | Rules |
|---|---|
| `audience` | Required. 1–256 characters of printable ASCII: a URL or an identifier the verifier expects. Becomes `aud` ([O4](../project/edge-cases.md)) |
| `expires_in` | 60–600 seconds, default 300 ([O5](../project/edge-cases.md)) |
| `nonce` | Optional, 1–128 characters of printable ASCII, copied into the token for the verifier's own challenge |
| `ext` | Optional object, at most 2 KB as JSON, placed under the `ext` claim. Its members cannot use a registered or Pylota claim name (`iss`, `sub`, `aud`, `iat`, `nbf`, `exp`, `jti`, `email`, `email_verified`, `unverified`, `ai_agent`, `nonce`, `ext`) ([O6](../project/edge-cases.md)) |

Returns `201`:

```json
{ "assertion": "eyJhbGciOiJFZERTQSIsInR5cCI6ImFnZW50LWFzc2VydGlvbitqd3QiLCJraWQiOiJ6TWtVbUFRT2xx…",
  "kid": "zMkUmAQOlq9JtFPzTK1XINZdWd7gmhXxgA8Ph7cNKHo",
  "expires_at": "2026-10-09T12:05:00Z",
  "jwks_uri": "https://mail.example.com/.well-known/jwks/idn_01J9Z3K8V4QW7X2M5N6P8R0T1Y.json" }
```

The token's header is `{"alg":"EdDSA","typ":"agent-assertion+jwt","kid":"<thumbprint>"}`. Its claims:

```json
{ "iss": "https://mail.example.com", "sub": "idn_01J9Z3K8V4QW7X2M5N6P8R0T1Y",
  "aud": "https://portal.supplier.example", "iat": 1791547200, "nbf": 1791547200, "exp": 1791547500,
  "jti": "01M4G8HMG0Z6G25EVAN36PQG0H", "email": "bookings.acme@agents.example", "email_verified": true,
  "unverified": { "name": "Acme Car Hire", "org": "Acme Car Hire", "accountable_human": true },
  "ai_agent": true, "nonce": "b3f1c2d47a9e", "ext": { "booking_ref": "BK-2291" } }
```

- `iss` is `https://{PM_API_HOST}`, `sub` the identity ID, `jti` a new ULID and `email` the identity's
  primary address while the deployment still receives mail for it (its domain `healthy` or `degraded`,
  with inbound by routing, SES or a passing forwarding check), else its platform address
  ([O29](../project/edge-cases.md)). These are what the deployment vouches for.
- `unverified` holds what the workspace says about itself, which the deployment does not check: `name`
  (the identity's display name), `org` (the workspace name) and `accountable_human` (`true` when the
  identity has owner fields set). A verifier must not grant access on them. The owner's name and address
  are never in the token.
- The token is never stored or logged. A verifier checks it as in
  [Agents › Verifying an assertion](../guides/agents.md#verifying-an-assertion): `alg` and `typ`, an
  issuer it trusts, `sub` matching `^idn_[0-9A-HJKMNP-TV-Z]{26}$` before any fetch, the key from
  `{iss}/.well-known/jwks/{sub}.json` (no redirects, `application/jwk-set+json` only, cached for at most
  5 minutes), the
  signature, `aud`, `nbf` and `exp` with 60 seconds of skew, and `jti` against replays
  ([Agent signing keys § 4.3](../project/design/agent-keys.md#43-how-a-verifier-checks-it)).

Errors: `403 tenant_suspended` (checked first, before `409 identity_paused`), `400 invalid_request`
([O4–O6](../project/edge-cases.md)), `403 permission_denied`, `403 scope_denied`, `404 identity_not_found`,
`409 identity_paused` and `429 rate_limited`.

### `POST /v1/identities/{identity_id}/http-signatures` — tenant or identity key, `identities:sign`

Returns the headers that make an HTTP request a Web Bot Auth signed request (RFC 9421), signed with the
deployment's `web_bot_auth` key, with the identity's address in a signed `From` header. The Worker never
makes the request itself, and nothing is created or stored. `Idempotency-Key` is ignored and never
recorded.

```json
{ "url": "https://www.brightwell.example/fleet/availability?from=2026-10-12",
  "method": "GET",
  "expires_in": 60,
  "components": ["@authority", "signature-agent", "from"] }
```

| Field | Rules |
|---|---|
| `url` | Required, `https` only, at most 2,048 characters. An internationalised host is converted to its A-label for `@authority` ([O10](../project/edge-cases.md)) |
| `method` | Optional, an upper-case token. Signed only if `@method` is in `components`, and then required (`400 invalid_request` without it) |
| `expires_in` | 30–300 seconds, default 60. Too short an expiry fails in transit ([O11](../project/edge-cases.md)) |
| `components` | Optional. Always includes `@authority`, `signature-agent` and `from`; may add `@method`, `@path` and `@query`. Any other component, or one whose value is not ASCII, returns `400 invalid_request` |

Returns `200`:

```json
{ "headers": {
    "Signature-Agent": "\"https://mail.example.com\"",
    "From": "bookings.acme@agents.example",
    "Signature-Input": "sig1=(\"@authority\" \"signature-agent\" \"from\");created=1791547200;expires=1791547260;keyid=\"poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U\";alg=\"ed25519\";nonce=\"e8N7S2MF…\";tag=\"web-bot-auth\"",
    "Signature": "sig1=:jdq0SqOwHdyHr9+r5jw3iYZH6aNGKijYp/EstF4RQTQdi5N5YYKrD+mCT1HA1nZDsi6nJKuHxUi/5Syp3rLWBA==:" },
  "expires_at": "2026-10-09T12:01:00Z" }
```

- `Signature-Agent` names the deployment's origin; its key directory is at
  [`/.well-known/http-message-signatures-directory`](#well-known).
- `From` is the same address as the assertion's `email` (RFC 9110: whoever is responsible for the request).
- `keyid` is the deployment key's JWK thumbprint, `nonce` 64 random bytes (base64), and `tag` is
  `web-bot-auth`.

Signed HTTP requests are off unless the operator sets `PM_WEB_BOT_AUTH=on` (allowed once spike S13 has
passed) and the tenant opts in. While `PM_WEB_BOT_AUTH=off`, this returns `422 web_bot_auth_disabled`
([O9](../project/edge-cases.md)); while tenant policy `web_bot_auth.allowed` is `false`, the default,
`403 policy_denied` ([O13](../project/edge-cases.md);
[Configuration › Tenant policy](configuration.md#tenant-policy)). Other errors as for assertions,
`403 tenant_suspended` first among them. The
operator side is in [Self-hosting › Signed HTTP requests](../self-hosting.md#signed-http-requests-web-bot-auth).

---

## Service accounts

The service sign-up ledger: an agent records each third-party account it wants to create with its
identity's address, and an operator approves or rejects it. Where the tenant's `accounts.require_approval`
is `true` (the default on Pylota Identity Cloud), verification mail from a service reaches the agent only when
an approved entry matches it; other such mail is quarantined with `account_unapproved`
([Service sign-up ledger](../project/design/service-accounts.md)). Every change emits an `account.*`
[event](events.md#service-accounts) and writes an audit row (`account.request`, `account.approve`,
`account.reject`, `account.close`, `account.delete`).

### `POST /v1/identities/{identity_id}/accounts` — `accounts:request`

```json
{ "service_domain": "github.com", "account_identifier": "brightwell-bookings",
  "purpose": "Open issues on the booking widget's repository.",
  "sender_domains": ["github.com"], "address": "bookings.brightwell@pylotamail.com" }
```

- `service_domain` is reduced to its organisational domain (A-label, lower case). `sender_domains`
  (optional, at most 5) adds other organisational domains the service's mail comes from; the service domain
  is always included. A public suffix, an IP literal or a non-DNS name is `400 invalid_request`; this
  deployment's own platform, API or console domain is `400 invalid_request` with
  `details.reason = "own_deployment"` ([E14](../project/edge-cases.md)).
- `account_identifier` (1–254 characters) and `purpose` (1–500) are shown to the operator. `address`
  (optional) must be an `active` or `retiring` address of the identity; it defaults to its primary address.
- Returns `201` with the [entry](#service-account-object) in `pending_approval`, which expires after 7 days.
- `409 account_exists` (`details.account_id`, `details.status`) when a pending or approved entry exists for
  the same service and account identifier; `422 account_limit_reached` beyond 10 pending or 200 entries
  for the identity ([E12](../project/edge-cases.md)); `403 tenant_suspended`; `409 identity_paused`.

### `GET /v1/identities/{identity_id}/accounts` · `GET /v1/identities/{identity_id}/accounts/{account_id}` — `accounts:request`

The identity's entries, newest first (filters `status`, `service_domain`), and one entry. An entry outside
the key's scope is `404 account_not_found`.

### `GET /v1/tenants/{tenant_id}/accounts` — `accounts:request`, tenant, partner or platform key

The tenant's entries (filters `status`, `identity_id`, `service_domain`). An identity key gets
`403 scope_denied`.

### `POST /v1/identities/{identity_id}/accounts/{account_id}/approve` — `accounts:approve`

`{ "note": "Approved for the widget repository only." }` (optional, at most 500 characters). Platform,
partner and tenant keys. Approving lets codes reach an agent, so it is a decision reserved for people: an
API key may approve only when `PM_QUARANTINE_KEY_RELEASE` is `on`, or when the tenant's policy has
`quarantine.key_release: true` and the key is a platform key or the tenant's partner key; otherwise
`403 permission_denied` with `details.reason = "person_required"`, and a person approves in the console.
The key that requested the entry never approves it (`403 permission_denied`,
`details.reason = "self_approval"`) ([E11](../project/edge-cases.md)). Returns `200` with the entry, now `approved`. Approving an approved
entry again returns it unchanged; an entry that is rejected, closed or past its 7 days gets
`409 account_not_pending` with `details.status`. Mail already held as `account_unapproved` stays
quarantined until someone releases it.

### `POST /v1/identities/{identity_id}/accounts/{account_id}/reject` — `accounts:approve`

As approve, with an optional `note`, but always allowed for a key that holds `accounts:approve`. The entry
becomes `rejected` with `rejected_reason: "operator"`.

### `POST /v1/identities/{identity_id}/accounts/{account_id}/close` — `accounts:request`

An optional `note`. A pending or approved entry becomes `closed`; no mail matches it afterwards
([E13](../project/edge-cases.md)). Closing a closed or rejected entry returns it unchanged. It does not
close the account at the service.

### `DELETE /v1/identities/{identity_id}/accounts/{account_id}` — `accounts:approve`

Removes the entry, closing it first (with `account.closed`) when it was pending or approved. Returns `204`.

#### Service account object

```json
{ "id": "sac_01JA…", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…",
  "service_domain": "github.com", "sender_domains": ["github.com"],
  "account_identifier": "brightwell-bookings", "address": "bookings.brightwell@pylotamail.com",
  "purpose": "Open issues on the booking widget's repository.", "status": "approved",
  "rejected_reason": null, "note": "Approved for the widget repository only.",
  "requested_by_key_id": "key_01J9…", "decided_by_key_id": null, "decided_by_user_id": "usr_01JA…",
  "decided_at": "…", "closed_by_key_id": null, "closed_by_user_id": null, "closed_at": null,
  "expires_at": null, "created_at": "…", "updated_at": "…" }
```

`status` is `pending_approval`, `approved`, `rejected` or `closed`. `purpose` was written by an agent: show
it as untrusted text.

---

## Agent sign-up

Self-sign-up for agents on Pylota Identity Cloud, behind `PM_AGENT_SIGNUP` (FR-ASU,
[Agent self-sign-up](../project/design/agent-signup.md)). One call without a key creates a provisional
workspace, one inbox `agent.{username}@{platform domain}` and one identity key; the named human verifies
with a six-digit code or a link; an unverified workspace is erased after 7 days. Until then every key of
the workspace acts with the provisional permission set ([Permissions](#permissions)), and the inbox can
email only that human, one recipient per message and 10 messages a UTC day.

The five routes after `POST` act on **the calling key's own sign-up**: the sign-up of the key's tenant.
They need a tenant or identity key of that tenant and no listed permission; any other tenant or identity
key gets `404 signup_not_found`, and a platform or partner key `403 scope_denied`. `RL_API` and
`RL_TENANT` apply as to every key.

### `POST /v1/agent-signup` — no key

The body may be at most 4,096 bytes (`413 payload_too_large`, `details.limit: 4096`).

```json
{
  "username": "travel-planner",
  "human_email": "sam@brightwell.example",
  "agent_name": "Travel planner",
  "source": "pylota-identity-python/1.2.0",
  "accept_terms": true
}
```

- `username` (required): lower-cased, then 2–32 characters matching `^[a-z0-9][a-z0-9-]{1,31}$`. It
  becomes the workspace's slug and address suffix, so the inbox is `agent.{username}@{platform domain}`. It
  is checked with the address-suffix rules of [`POST /v1/tenants`](#post-v1tenants) and is never released.
- `human_email` (required): the address of the person responsible for the agent. Example and reserved
  domains, disposable domains and addresses this deployment receives are refused, and its domain must be
  able to receive mail.
- `agent_name` (optional): 1–78 characters, no CR or LF; the identity's display name and the workspace's
  name. Never written into system mail.
- `source` (optional): 1–64 characters of `[A-Za-z0-9._/+-]`, for example the SDK and its version.
- `accept_terms` (required, `true`): the caller accepts the Terms of Service, the Privacy Policy and the
  Data Processing Addendum (`PM_TERMS_URL`, `PM_PRIVACY_URL`, `PM_DPA_URL`).

`Idempotency-Key` is ignored and nothing is recorded: the answer holds a secret that a replay could not
return. Returns `201`; the key's secret is shown in this answer only:

```json
{
  "signup_id": "asu_01JA2…",
  "state": "provisional",
  "tenant_id": "ten_01JA2…",
  "identity_id": "idn_01JA2…",
  "address": "agent.travel-planner@pylotamail.com",
  "key": { "id": "key_01JA2…", "secret": "pmk_live_7k2m9q4xa0bc_…" },
  "human_email_hint": "s***@brightwell.example",
  "expires_at": "2026-10-24T09:00:00Z",
  "verification": { "email": "queued", "code_expires_at": "2026-10-18T09:00:00Z" },
  "limits": { "recipients": "human_only", "daily_sends": 10 },
  "next": "Ask s***@brightwell.example for the six-digit code from our email, then POST /v1/agent-signup/verify with this key."
}
```

`verification.email` is `queued`, or `not_sent` when the system identity's submit refused the email. The
first failing check decides the answer:

1. Agent sign-up is closed (`PM_AGENT_SIGNUP` is `off`, `PM_SIGNUP` is not `open`, billing or the console
   is off: `details.reason = "disabled"`), or paused by the operator's switch (`paused`):
   `403 signup_closed`.
2. `RL_SIGNIN`, keyed by the client network (`CF-Connecting-IP`; an IPv6 address by its /64 prefix):
   `429 rate_limited` with `details.bucket = "signin"`.
3. The body: `413 payload_too_large`; `400 invalid_request` for an unknown field, a wrong type, a value out
   of bounds or `accept_terms` other than `true`.
4. `username`: `400 address_reserved` (a reserved or confusable name, `details.field = "username"`),
   `400 address_unsupported` (other non-ASCII) or `400 address_invalid` (the pattern).
5. `human_email`: `400 address_invalid`, `400 address_unsupported` (a non-ASCII local part), or
   `400 invalid_request` with `details.reason = "human_email_not_allowed"` (an example, disposable or hosted
   address).
6. The username, or one that folds to the same, is taken: `409 username_taken`.
7. The human's mail domain cannot receive mail: `400 invalid_request` with `details.reason =
   "no_mail_domain"`; `503 unavailable` with `details.reason = "dns_unavailable"` when DNS cannot be read.
8. The person refused agent sign-ups, or an admission budget is spent: `429 signup_limit_reached` with
   `details.scope` (`network`, `asn`, `human_email` or `deployment`), `details.resets_at` and `Retry-After`.

### `GET /v1/agent-signup` — the sign-up's own tenant or identity key

```json
{
  "signup_id": "asu_01JA2…", "state": "provisional", "tenant_id": "ten_01JA2…",
  "identity_id": "idn_01JA2…", "address": "agent.travel-planner@pylotamail.com",
  "human_email_hint": "s***@brightwell.example", "human_status": "delivered",
  "replacements_left": 2, "expires_at": "2026-10-24T09:00:00Z",
  "code": { "expires_at": "2026-10-18T09:00:00Z", "attempts_left": 10, "failures_left": 30,
            "emails_left_this_hour": 2, "email": "queued" },
  "permissions": ["identities:read", "messages:read", "messages:send", "messages:write",
                  "attachments:read", "search:read", "events:read", "usage:read"],
  "verified_at": null, "verified_via": null, "source": "pylota-identity-python/1.2.0",
  "created_at": "2026-10-17T09:00:00Z"
}
```

`state` is `provisional`, `verified`, `expired`, `abandoned` or `refused`. `permissions` is the key's
effective set. `human_status` is `pending`, `delivered` or `bounced`. A verified sign-up has `verified_at`,
`verified_via` (`code`, `link` or `claim`) and `code: null`. A sign-up that ended answers with its state
until the erasure revokes the key, then `401 key_revoked`.

### `POST /v1/agent-signup/verify` — the sign-up's own tenant or identity key

`{ "code": "482913" }`. `Idempotency-Key` optional. Returns `200` with the sign-up, now `verified`; a
sign-up that is already verified answers `200` with `"already_verified": true`. Errors:
`409 signup_not_pending` (not provisional, or past its 7 days; `details.state`), `403 tenant_suspended`,
`400 invalid_request` (not six digits), `422 signup_code_invalid` (`details.reason` is `wrong` with
`attempts_left` and `failures_left`, `burned`, `locked`, `expired`, `superseded` or `none`) and
`409 signup_cannot_verify` (`details.reason` is `workspace_limit`, `human_email_hosted` or
`person_disabled`). A code has 10 attempts and lives 24 hours; a sign-up allows 30 wrong codes in all.

### `POST /v1/agent-signup/resend` — the sign-up's own tenant or identity key

No body. `Idempotency-Key` optional (a replay sends nothing). Issues and emails a new code and link, which
makes the previous ones invalid, and returns `202` with the sign-up. Errors: `409 signup_not_pending`,
`403 tenant_suspended`, `403 signup_closed` (`paused`), `409 signup_human_unreachable` (the human's address
hard-bounced: replace the human), and `429 signup_limit_reached` (`details.scope` is `resend` after 3 code
emails in an hour, or `human_email` or `deployment`).

### `PUT /v1/agent-signup/human` — the sign-up's own tenant or identity key

`{ "human_email": "sam.patel@brightwell.example" }`. `Idempotency-Key` optional. Replaces the named person,
at most twice and only before verification, and emails the new person a new code and link; the old code
and link stop working at once, and `expires_at` does not move. The same address as now is a resend. Returns
`200` with the sign-up. Errors: those of resend, the `human_email` errors of `POST /v1/agent-signup`, and
`422 signup_replace_limit` once both replacements are used.

### `DELETE /v1/agent-signup` — the sign-up's own tenant or identity key

Abandons a provisional sign-up, for example after its key leaked. Returns `202` with
`{ "state": "abandoned", "erasure_request_id": "era_…" }`: the tenant-scope erasure has started, and its
first step revokes the key. A sign-up that is not provisional gets `409 signup_not_pending` (a verified
workspace is deleted by its owner). Budgets are not refunded and the username stays taken.

## Phone numbers

Behind `PM_PHONE` and the tenant's `phone.allowed` (with the flag off every route below answers
`404 route_not_found`). A number is an address of one identity, registered in the tenant's own legal name. An
agent buys inside the tenant's standing budget, or asks; a person decides everything else
([Phone numbers](../project/design/phone-numbers.md), [guide](../guides/phone.md)). Every route that can cost
money requires `Idempotency-Key` (`x-idempotency: required` in [`openapi.yaml`](openapi.yaml)).

### `GET /v1/phone/countries` — `phone:read`

The catalogue rows the deployment offers, each with `country`, `number_type`, `status`, `capabilities`,
`sms_use`, `local_presence`, `default_for_country`, `destinations`, `registration` (fees and
`typical_wait_days`), `review_hours`, `emergency` and the tenant's own standing: `eligible`, or `reason`
(`local_presence_required`, `compliance_missing`, `not_allowed_by_policy`, `platform_prerequisite`,
`entity_type`).

### `GET /v1/tenants/{tenant_id}/phone-setup` · `GET /v1/tenants/{tenant_id}/phone-setup/{country}` — `phone:read`

Phone onboarding: one entry per country the tenant's policy allows ([Phone numbers § 4.7](../project/design/phone-numbers.md#47-phone-onboarding)).

```json
{ "country": "US", "status": "action_needed",
  "steps": [ { "kind": "platform_prerequisite", "status": "done" },
             { "kind": "profile", "status": "done" },
             { "kind": "register_us_a2p", "status": "not_started", "who": "person",
               "fee": { "setup_pence": 2839, "monthly_pence": 823 }, "typical_wait_days": [7, 15] } ],
  "after_setup": { "outbound_sms": "enabled", "inbound_sms": "enabled", "inbound_calls": "enabled",
                   "outbound_calls": "registration_required" },
  "next_step": { "who": "person", "kind": "register_us_a2p", "console_url": "https://console.example.com/phone/setup/US" } }
```

`status` is `not_started`, `in_review`, `action_needed`, `ready` or `unavailable` (with `reason`). Identity keys
may read it: it holds no personal data.

### `PUT /v1/tenants/{tenant_id}/phone-setup/{country}/draft` · `GET` · `DELETE` — `phone:request`

An agent's draft of the text a registration needs, for a person to review and submit. `Idempotency-Key`
optional. Body fields, all optional strings unless stated: `business_description`, `use_case`,
`sample_messages` (2–5, each 20–1,024 characters and containing the brand name), `consent_flow`, `website`,
`privacy_policy_url`, `terms_url`, `opt_in_image_url`. Any other field, a tax identifier or personal data is
`400 invalid_request`. Returns `200` with the draft (`pdr_`, `drafted_by_key_id`, `updated_at`). One draft per
tenant and country; deleted when its request is created or after 30 days untouched.

### `GET /v1/tenants/{tenant_id}/phone-budget` — `phone:read` · `PUT` — `phone:manage`

The standing purchase budget ([Phone numbers § 5.7](../project/design/phone-numbers.md#57-the-standing-purchase-budget)).

```json
{ "enabled": true, "countries": ["GB", "US"], "number_types": ["mobile", "local"],
  "max_numbers_per_identity": 2, "max_new_numbers_per_month": 10,
  "max_setup_pence_per_number": 300, "max_monthly_pence_per_number": 300, "expiring_only": false,
  "version": 4, "set_by": { "user_id": "usr_…", "key_id": null, "on_behalf_of": null }, "set_at": "…",
  "this_month": { "covered": 3, "resets_at": "2026-11-01T00:00:00Z" } }
```

`PUT` replaces the object (`Idempotency-Key` required). Tightening any field, or `enabled: false`, is allowed to
`phone:manage`. Setting it or loosening any field is a decision reserved for people: from an API key only a
platform key or the tenant's partner key where `phone.key_approval` is `true`, with `on_behalf_of`; otherwise
`403 permission_denied` (`person_required`) and a person saves it in the console. Identity keys get
`403 scope_denied`. Emits `phone_budget.changed`; audit `phone_budget.set`.

### `POST /v1/identities/{identity_id}/phone-numbers` — `phone:request`

Buy a number in one call ([Phone numbers § 6.7](../project/design/phone-numbers.md#67-one-call-purchase)).
`Idempotency-Key` required.

```json
{ "country": "US", "area_code": "512", "expires_at": null, "primary_for": ["sms"],
  "client_reference": "branch-austin" }
```

| Field | Rule |
|---|---|
| `country` | Required |
| `type` | Default the country's row that can text (`mobile` in GB, `local` in the US) |
| `area_code`, `contains`, `locality` | Optional; `400 invalid_request` (`location_not_supported`) on a type with no location, such as GB mobile |
| `number`, `quote_token` | An exact number from a search; otherwise the service picks |
| `expires_at` | Optional, 1 hour to 365 days ahead; the number is released then |
| `acknowledge_not_available` | `true` to buy even though a `follow_on` entry says a capability cannot be had for this tenant |

- **`201`**: the [number](#phone-number-object) once the provider accepted the order inside the tenant's
  standing budget. Configuration continues; follow `phone_number.status_changed` and
  `phone_number.capability_changed`.
- **`202`**: the [request](#phone-request-object) when a person must act (outside the budget, `details.outside_budget`),
  when the purchase waits on onboarding (`blocked_by`), or when the run continues in the queue. `next_step`
  names who must act.
- `409 phone_number_taken` with `details.alternatives[]` (exact numbers only); `402 spend_cap_reached`;
  `422 country_not_offered`, `local_presence_required`, `compliance_incomplete` (`entity_type`),
  `phone_number_limit_reached`, `phone_request_limit_reached`; `403 policy_denied` (`phone_not_allowed`,
  `plan_required`).

### `GET /v1/identities/{identity_id}/phone-numbers/available` — `phone:request`

Search: `country` and `type` (required), `contains`, `starts_with`, `ends_with`, `locality`, `region`, `limit`
(1–20). Each candidate has `phone_number`, `locality`, `region`, `features`, a GBP `quote` (with `follow_on`)
and a 30-minute `quote_token`. Results are not reserved. 300 searches a day per tenant (`429 daily_cap_reached`,
`details.cap = "phone_search"`).

### `POST /v1/identities/{identity_id}/phone-numbers/portability` — `phone:request`

`{ "numbers": ["+447700900456"] }`: the provider's portability check per number (`portable`, `fast_portable`,
`messaging_capable`, `reason`). Free, read-only, no approval.

### <a id="phone-number-object"></a>`GET /v1/identities/{identity_id}/phone-numbers` · `GET …/phone-numbers/{number_id}` — `phone:read`

```json
{ "id": "pnm_01JB…", "identity_id": "idn_01J9…", "phone_number": "+15125550123", "country": "US",
  "number_type": "local", "status": "active", "setup_state": "registration_pending",
  "capabilities": {
    "outbound_sms":   { "state": "registration_required",
                        "next_step": { "who": "person", "kind": "register_us_a2p",
                                       "console_url": "https://console.example.com/phone/setup/US" } },
    "inbound_sms":    { "state": "enabled" },
    "inbound_calls":  { "state": "enabled" },
    "outbound_calls": { "state": "registration_required",
                        "next_step": { "who": "person", "kind": "enable_emergency" } } },
  "primary_for": ["sms"], "provisioning": { "status": "ready", "code": null, "retryable": false },
  "requirements_status": null, "deadline_at": null, "action_needed": null, "expires_at": null,
  "port_pin": "set", "compliance_status": "own", "legacy_deadline": null,
  "active_from": "…", "active_until": null, "request_id": "phr_01JB…", "client_reference": "branch-austin" }
```

Each capability `state` is `enabled`, `activating`, `registration_required` or `not_available` (with `reason`).
`provider` (provider IDs) appears only for platform and partner keys. The list filters `status` (default the
live statuses).

### `PATCH /v1/identities/{identity_id}/phone-numbers/{number_id}` — `phone:manage` · `phone:request`

`{ "primary_for": ["sms","voice"], "expires_at": "…" }`. `primary_for` needs `phone:manage`. An earlier
`expires_at` is allowed to `phone:request`; a later one or `null` needs `phone:manage`.

### `POST /v1/identities/{identity_id}/phone-numbers/{number_id}/move` — `phone:manage`

`{ "to_identity_id": "idn_…" }` within the tenant. History stays with the old identity.

### `GET /v1/identities/{identity_id}/phone-numbers/{number_id}/voice-route` — `phone:read`

The number's route (`kind`, configuration without secrets, `status`, `vapi_phone_number_id`).

### `GET /v1/tenants/{tenant_id}/phone-numbers` · `GET /v1/phone-numbers/lookup?number=` — `phone:read`, tenant level and above

The tenant's numbers; and the identity and number IDs of a live number in the key's scope, or `404
phone_number_not_found` (also for another tenant's number).

### <a id="phone-requests"></a><a id="phone-request-object"></a>Phone requests — `phone:request`, `phone:read`, `phone:approve`

`POST /v1/identities/{identity_id}/phone-requests` (`Idempotency-Key` required) with `kind` and `payload`;
`GET` lists (filters `status`, `kind`); `GET …/phone-requests/{request_id}`; `GET
/v1/tenants/{tenant_id}/phone-requests` (the tenant's queue, oldest pending first).

```json
{ "id": "phr_01JB…", "kind": "purchase_number", "status": "pending_approval",
  "payload": { "selection": { "mode": "any", "country": "US", "type": "local", "area_code": "512" } },
  "quote": { "currency": "GBP", "max_setup_pence": 127, "max_monthly_pence": 127,
             "follow_on": [ { "capability": "outbound_sms", "kind": "register_us_a2p", "setup_pence": 2839,
                              "monthly_pence": 823, "typical_wait_days": [7, 15], "who": "person", "once_per": "tenant" } ],
             "rate_card_version": 3, "valid_until": "…" },
  "digest": "9f2c41ab…", "blocked_by": [], "approved_by": null, "budget_version": null,
  "details": { "outside_budget": "country" }, "expires_at": "…", "result": null }
```

`kind` is `purchase_number`, `port_in`, `release_number`, `submit_compliance`, `register_us_a2p`,
`verify_toll_free`, `enable_emergency`, `set_voice_route` or `outbound_call`. An identity key creates only
`purchase_number`, `port_in` (without secrets), `release_number` and `outbound_call` for its own identity.

- `POST …/phone-requests/{request_id}/approve` (`phone:approve`, `Idempotency-Key` required) with
  `{ "digest", "note"?, "on_behalf_of"? }`: a decision reserved for people (`403 permission_denied`,
  `person_required`, unless `phone.key_approval` and a platform or the tenant's partner key); never the
  requesting key (`self_approval`); `409 phone_request_not_pending`; `409 phone_request_digest_mismatch`
  with `details.current_digest`.
- `POST …/reject` (`phone:approve`) with `{ "reason" }`; `POST …/cancel` (the requester or `phone:approve`).
- `POST /v1/platform/phone-requests/{request_id}/resolve` (`platform:ops`) with `outcome` (`ordered` with the
  provider order ID, or `not_ordered`) ends an attempt left `uncertain` for 24 hours.

### Compliance — `phone:compliance`

- `GET`, `PATCH /v1/tenants/{tenant_id}/phone-compliance`: the profile (`legal_name`, `trading_name`,
  `entity_type`, `registration_country`, `company_number`, `company_registry` `{country, region, name}`,
  `tax_ids` (write-only, shown masked), `website`, `use_case`, `representative` (contact and birth data
  write-only), `addresses[]`, and `status` per country, type and action). Identity keys get `403 scope_denied`.
- `POST /v1/tenants/{tenant_id}/phone-compliance/documents` with `kind`, `filename`, `content_type`, `size`,
  `sha256`, `valid_until`: `201` with the document and a one-hour `upload_url`; `PUT` the bytes there. `GET`
  lists metadata; `GET`, `DELETE …/documents/{document_id}`. No route returns a document's bytes.
- `POST /v1/platform/phone-compliance/{profile_id}/export` (`platform:ops`) with `reason`: a single-use link to
  an encrypted export, audited.

### US registrations — `phone:read`, `phone:compliance`

`GET /v1/tenants/{tenant_id}/phone-registrations` returns `{configured, brand_status, campaign_status,
numbers_registered[], failure_reasons[], brand_entity, otp_status}`. For a US sole-proprietor brand,
`POST /v1/tenants/{tenant_id}/phone-registrations/{registration_id}/otp` with `{ "pin" }` and `POST …/otp/resend`
(at most 5 a day) are decisions reserved for people; `422 phone_registration_otp_invalid`.

### Voice connectors and routes — `phone:manage`

`POST`, `GET /v1/tenants/{tenant_id}/phone-connectors` (`{ "kind": "vapi", "region": "eu", "api_key" }`; the key
is never shown again), `DELETE …/{connector_id}`, `POST …/{connector_id}/rotate`. Routes change only through
`set_voice_route` requests.

### SMS — `phone:send`, `phone:read`, `quarantine:review`, `erasure:manage`

- `POST /v1/identities/{identity_id}/sms` (`Idempotency-Key` required except with `?dry_run=true`) with `from`,
  `to`, `text`, `kind`, `conversation_id`, `client_reference`: `202` with the SMS (`status: "queued"`). Refusals
  in order: `403 tenant_suspended`, `409 identity_paused`, `503 unavailable` (`phone_paused`), `404
  phone_number_not_found`, `409 phone_number_not_ready`, `403 policy_denied` (`destination_country`,
  `destination_untested`, `destination_not_supported`), `403 destination_blocked`, `403 recipient_opted_out`,
  `403 consent_required`, `403 a2p_not_registered`, `409 quiet_hours`, `429 daily_cap_reached`,
  `402 spend_cap_reached`. A dry run answers `200 { "would_send", "segments", "price_pence" }`.
- `POST …/sms/{sms_id}/resolve` (`phone:send`) with `outcome` `sent` or `not_sent` for an `uncertain` SMS.
- `GET /v1/identities/{identity_id}/sms`, `GET …/sms/{sms_id}`, `GET …/sms/{sms_id}/media/{n}`,
  `GET …/sms-conversations` (`phone:read`); codes in text appear as `[code]`. The SMS list filters by
  `conversation_id`, `number_id`, `counterparty`, `direction`, `status` (held SMS only with `quarantine:review`),
  `after` and `before`; the call list (`GET …/calls`) by `number_id`, `counterparty`, `direction`, `status`, `after`
  and `before`.
- `GET /v1/identities/{identity_id}/sms/wait` (`phone:read`) with `number_id`, `from`, `account_id`, `kind`,
  `timeout` (1–60), `since`: `200 { "sms", "verification", "timed_out" }`.
- `POST …/sms/{sms_id}/release` (`quarantine:review`); `DELETE …/sms/{sms_id}` (`erasure:manage`).

### Calls — `phone:call`, `phone:read`

`POST /v1/identities/{identity_id}/calls` (`Idempotency-Key` required) with `from`, `to`, `purpose`, `assistant`:
`202` with the call, or with the `outbound_call` request under `phone.calls.outbound = approval`. `GET
/v1/identities/{identity_id}/calls`, `GET …/calls/{call_id}`, `GET …/phone-usage`; `DELETE …/calls/{call_id}`
(`erasure:manage`).

### Consent — `phone:read`, `phone:send`, `phone:manage`

`GET /v1/tenants/{tenant_id}/phone-consent?number=` returns the ledger entry (masked hint). `POST` with
`{ "number", "state", "purpose", "evidence" }`: `opted_out` needs `phone:send` (any key); `opted_in` needs
`phone:manage` and evidence.

### Port-outs — `phone:read`, `phone:approve`

`GET …/phone-numbers/{number_id}/port-outs`; `POST …/port-outs/{port_out_id}/authorize` (`{ "note"?,
"on_behalf_of"? }`) and `…/reject` (a reason code from the provider's list, `1001` needing text, and
`on_behalf_of`) are decisions reserved for people; a platform or partner key deciding under `phone.key_approval`
names the person in `on_behalf_of`; `409 port_out_closed`.

### Money — `phone:read`

`GET /v1/tenants/{tenant_id}/phone-charges?month=` (charges, caps including the registration cap, holds) and
`GET /v1/partners/{partner_id}/phone-statement?month=` (partner keys and platform keys).

### Operations

`POST /v1/tenants/{tenant_id}/phone/reconcile` (`phone:manage`, once every 10 minutes); `POST
/v1/platform/phone/reconcile`, `POST /v1/platform/phone/probe` and `POST /v1/platform/phone-numbers/adopt`
(`platform:ops`); `POST /hooks/telnyx` (no key; Ed25519 signature over `telnyx-timestamp|raw body`).

---

## Vault

Logins, API keys and browser sessions that agents use without ever seeing them
([Vault guide](../guides/vault.md), [design](../project/design/vault.md)). Off unless the deployment sets
`PM_VAULT = "on"` and the platform sets the tenant's `vault.allowed: true` (`422 vault_disabled`); on a Stripe
deployment a Free-plan workspace gets `403 policy_denied` (`plan_required`). No route returns a value, a
ticket other than at minting, a packet, an unlock key other than at enrolment, or a TOTP seed. Every change
writes a row to the tenant's hash-chained vault audit and emits a `vault.*` [event](events.md#vault).

### Items

#### `POST /v1/identities/{identity_id}/vault/items` — `vault:write` · `POST /v1/tenants/{tenant_id}/vault/items` — `vault:write`, tenant, partner or platform key

An identity-scoped item (usable only by that identity; `account_id` required) or a tenant-scoped item (usable by
identities it is granted to; each grantee needs its own approved ledger entry). `Idempotency-Key` optional; a
replay returns the item.

```json
{ "kind": "login", "title": "TfL road user charging", "account_id": "sac_01JA…",
  "username": "compliance.brightwell@pylotamail.com", "origins": ["https://tfl.gov.uk"],
  "password": "…", "totp": "otpauth://totp/TfL:compliance?secret=…&period=30",
  "approval": "per_run", "otp_channel": { "kind": "email", "address": "compliance.brightwell@pylotamail.com",
  "sender_domains": ["tfl.gov.uk"] }, "rotation": { "remind_after_days": 90 } }
```

- `kind`: `login` (`password` as a string or `{ "generate": { "length", "classes" } }`, optional `totp`,
  `recovery_codes`, `otp_channel`), `api_key` (`secret`, `inject: { "scheme": "bearer" | "basic" | "header",
  "header_name"?, "username"? }`, P1 `sign`, optional `rotate_after_use`) or `session` (`state` in Playwright's
  `storageState` shape, `session: { "ttl_seconds" }`, optional `import: { "source": "pylota_sealed_session",
  "source_ref" }`).
- `origins`: 1–10 exact `https` origins, canonicalised; each must belong to the ledger entry's service domain or
  `login_domains` (`422 invalid_request`, `ledger_entry_not_approved` or `origin_not_in_ledger`).
- `approval`: `grant`, `per_run` or `per_use` (default).
- Returns `201` with the [item](#vault-item-object). Refusals: `422 vault_kind_refused` (cards, bank details,
  passkeys, refused domains, confusable origins), `422 vault_item_limit_reached`, `422 vault_session_too_large`,
  `409 identity_paused`, `403 tenant_suspended`. A session's response adds `cookies_dropped`,
  `clearance_cookies_dropped` and `origins_dropped`.

#### `GET /v1/identities/{identity_id}/vault/items` · `GET /v1/tenants/{tenant_id}/vault/items` — `vault:read`

Newest first; filters `kind`, `state`, `health`, `account_id`. The identity list includes tenant-scoped items
with an active grant for the identity.

#### `GET /v1/vault/items/{item_id}` · `PATCH /v1/vault/items/{item_id}` — `vault:read` · `vault:write`

`PATCH` changes `title`, `origins` (a removed origin leaves grants with the rest; an added one widens no grant),
`approval` (looser only by a person or a key that may decide, otherwise `403 permission_denied`,
`person_required`), `state` (`archived` or `active`), `otp_channel` and `rotation`. Values are changed only by a
new version.

#### `POST /v1/vault/items/{item_id}/versions` — `vault:write`

Rotates: the kind's values, and `overlap_hours` (0–168, default 24) for logins and API keys. Fields not given are
carried over (in partner custody that needs `X-Pylota-Identity-Vault-Unlock`). Open uses close (`item_rotated`).
Returns `201` with the item.

#### `DELETE /v1/vault/items/{item_id}` — `vault:admin`, never an identity key

Crypto-shreds every version, deletes the item's grants and closes its uses. Returns `200` with the shred receipt
(`versions_shredded`, `r2_objects_deleted`, `uses_closed`, `uses_deleted`, `grants_deleted`, `restorable_until`,
`custody`). The item's uses are deleted with it; the audit keeps their IDs.

#### Vault item object

```json
{ "id": "vit_01JAD4…", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…", "kind": "login",
  "title": "TfL road user charging", "account_id": "sac_01JA…", "username": "compliance.brightwell@pylotamail.com",
  "origins": ["https://tfl.gov.uk"], "approval": "per_run", "state": "active", "health": "ok",
  "health_reason": null, "current_version": 3,
  "has": { "password": true, "totp": true, "recovery_codes": false, "email_otp": true, "sms_otp": false },
  "otp_channel": { "kind": "email", "address": "compliance.brightwell@pylotamail.com", "sender_domains": ["tfl.gov.uk"] },
  "rotation": { "remind_after_days": 90, "last_rotated_at": "…" }, "session": null, "managed_by": null,
  "created_by": { "key_id": "key_01J9…", "user_id": null }, "first_used_at": "…", "last_used_at": "…",
  "created_at": "…", "updated_at": "…" }
```

`title` was written by a person or an agent: show it as untrusted text.

### Grants

#### `POST /v1/identities/{identity_id}/vault/grants` — `vault:request`

```json
{ "item_id": "vit_01JAD4…", "operations": ["fill"], "origins": ["https://tfl.gov.uk"],
  "purpose": "Check PCN status each morning.", "max_uses": null }
```

Creates a grant `pending_approval` for 7 days. With `"approve": true`, `expires_in` and (from an API key)
`on_behalf_of`, a caller that holds `vault:approve` and may decide creates it `active`. `409 vault_grant_exists`,
`422 vault_grant_limit_reached`, `403 vault_not_granted` (`ledger_not_approved`), `404 vault_item_not_found`.

#### `GET /v1/identities/{identity_id}/vault/grants` · `GET /v1/tenants/{tenant_id}/vault/grants` · `GET /v1/vault/grants/{grant_id}` — `vault:read`

The tenant list is the approval queue (oldest pending first). Filters `status`, `item_id`.

#### `POST /v1/vault/grants/{grant_id}/approve` — `vault:approve`

`{ "expires_in": 604800, "note": "…", "on_behalf_of": "usr_8812" }`. `Idempotency-Key` required. A decision
reserved for people: an API key may approve only under the rule of
[Vault § 5](../project/design/vault.md#5-grants) (`403 permission_denied`, `person_required`); the key that
requested it never approves (`self_approval`); `409 vault_not_pending`.

#### `POST /v1/vault/grants/{grant_id}/deny` — `vault:approve` · `POST /v1/vault/grants/{grant_id}/revoke` — `vault:approve`, or `vault:request` from the grantee's own identity key

`{ "note" }` or `{ "reason" }`. Always allowed: they only keep a credential away. Revoking closes the grant's open
uses (`grant_revoked`).

#### Vault grant object

```json
{ "id": "vgr_01JAD5…", "tenant_id": "ten_01J9…", "item_id": "vit_01JAD4…", "identity_id": "idn_01J9…",
  "operations": ["fill"], "origins": ["https://tfl.gov.uk"], "status": "active",
  "purpose": "Check PCN status each morning.", "max_uses": null, "use_count": 12,
  "requested_by_key_id": "key_01J9…", "decided_by_key_id": "key_01J8…", "decided_by_user_id": null,
  "on_behalf_of": "usr_8812", "note": null, "decided_at": "…", "expires_at": "…",
  "revoked_at": null, "revoke_reason": null, "created_at": "…", "updated_at": "…" }
```

### Uses and tickets

#### `POST /v1/identities/{identity_id}/vault/uses` — `vault:use`, never an identity key

The orchestrator asks to use an item now. `Idempotency-Key` required.

```json
{ "item_id": "vit_01JAD4…", "operation": "fill", "phases": ["credentials", "email_otp"],
  "consumer_id": "vcn_01JAC2…", "run_id": "run_8f2c41", "origin": "https://tfl.gov.uk",
  "trigger": "login_form", "purpose": "Check PCN status for WM12ABC", "capture_into": null }
```

`version` is optional: `"current"` (the default) or `"previous"`, for `fill` and `inject` while the item's
previous version is inside its overlap (otherwise `403 vault_refused`, `previous_version_unavailable`). `phases`
is non-empty, except for `session_capture`, which takes `phases: []`; its use starts when the ticket is minted and
must write back within `run_window_seconds`.

Returns `201` with `use` and, when no person must decide, `ticket` and `ticket_expires_at` (redeem within 60
seconds). A replay returns `ticket: null` and `ticket_replayed: false`. `operation: "sign"` (P1) takes
`vault:request`, may come from an identity key, and answers with the signature instead of a ticket. Errors:
`403 vault_not_granted`, `403 vault_refused`, `404 vault_consumer_not_found`, `403 consumer_not_active`,
`429 rate_limited` (`RL_VAULT`, or `details.bucket = "vault_pending"`).

#### `GET /v1/identities/{identity_id}/vault/uses` · `GET /v1/tenants/{tenant_id}/vault/uses` · `GET /v1/vault/uses/{use_id}` — `vault:read`

Never shows a ticket, its hash or a sealed key. The tenant list defaults to `status=pending_approval`, oldest
first. `404 vault_use_not_found`.

#### `POST /v1/vault/uses/{use_id}/approve` — `vault:approve` · `…/deny` — `vault:approve` · `…/cancel` — `vault:use`

`approve` takes `{ "digest", "note"?, "on_behalf_of"? }` (`Idempotency-Key` required); a digest that is not the
use's is `400 invalid_request` (path `digest`). It does not return a ticket.

#### `POST /v1/vault/uses/{use_id}/ticket` — `vault:use`

Mints a ticket for an approved use whose phases are unredeemed, replacing any earlier one. In partner custody,
also `X-Pylota-Identity-Vault-Unlock`. Returns `{ "ticket", "ticket_expires_at", "phases", "use_id" }`. Not
idempotent: each call mints a new ticket.

#### `POST /v1/vault/uses/{use_id}/signature` — `vault:request` (P1)

Once, for an approved `sign` use; the use then closes.

#### Vault use object

```json
{ "id": "vus_01JAD6…", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…", "item_id": "vit_01JAD4…",
  "grant_id": "vgr_01JAD5…", "operation": "fill", "phases": ["credentials", "email_otp"],
  "phases_redeemed": ["credentials"], "consumer_id": "vcn_01JAC2…", "run_id": "run_8f2c41",
  "origin": "https://tfl.gov.uk", "trigger": "login_form", "purpose": "Check PCN status for WM12ABC",
  "status": "in_use", "approval_reason": "per_run", "approved_by": "run", "digest": "c41a…",
  "deadline_at": null, "redeem_by": "…", "closes_at": "…", "close_reason": null, "outcome": null,
  "created_by_key_id": "key_01J9…", "decided_by_key_id": null, "decided_by_user_id": null,
  "on_behalf_of": null, "created_at": "…", "updated_at": "…" }
```

### Consumers

#### `POST /v1/vault/consumers` — `vault:admin`

```json
{ "kind": "browser", "name": "Browser fleet", "level": "tenant", "tenant_id": "ten_01J9…",
  "hosting": "self_hosted", "provider": null, "contract": "vault-consumer/1",
  "signing_jwk": { "kty": "OKP", "crv": "Ed25519", "x": "…" },
  "encryption_jwk": { "kty": "OKP", "crv": "X25519", "x": "…" },
  "declarations": { "session_recording": "off", "session_logging": "off", "captcha_solving": "off",
                    "extensions": "none", "honest_identity": "web_bot_auth",
                    "observation_paused_during_fill": true, "post_fill_check": true, "modes": ["cdp"] } }
```

Created `pending_activation`. A JWK with a private member is `400 invalid_request` (`private_key_supplied`);
`422 vault_consumer_limit_reached`.

#### `POST /v1/vault/consumers/{consumer_id}/activate` · `…/suspend` · `…/resume` · `…/rotate-keys` · `DELETE /v1/vault/consumers/{consumer_id}` · `GET /v1/vault/consumers` · `GET /v1/vault/consumers/{consumer_id}` — `vault:admin`

Activation and resume are decisions reserved for people (a platform key, or a signed-in owner or admin for a
tenant-level self-hosted consumer); `activate` takes `{ "note" }`. `rotate-keys` takes the new JWKs and
`overlap_seconds` (0–86,400, default 3,600). Suspension and revocation close the consumer's open uses.

### The tenant's vault

#### `GET /v1/tenants/{tenant_id}/vault` — `vault:read`, tenant, partner or platform key

`{ "enabled", "custody", "kid", "items", "active_grants", "pending_grants", "pending_uses", "retiring_versions" }`.

#### `POST /v1/tenants/{tenant_id}/vault/rotate-key` — `vault:admin`

A new tenant key; versions are re-wrapped by the every-minute sweep. In partner custody, needs the unlock key.

#### `POST /v1/tenants/{tenant_id}/vault/custody` · `DELETE /v1/tenants/{tenant_id}/vault/custody` — `vault:admin`, the tenant's own partner key

Enrol `{ "slots": 1 | 2 }`: answers once with the unlock keys (`pvu_…`) and `operator_copy_restorable_until`; a
replay answers without them (`secret_replayed: false`). Release needs a valid unlock key in the header.

#### `POST /v1/tenants/{tenant_id}/vault/revoke-all` — `vault:admin`

`{ "shred_sessions": true, "archive_items": false }`. Revokes every grant and closes every use; optionally deletes
every session version and archives every item.

#### `GET /v1/tenants/{tenant_id}/vault/audit` — `audit:read`, tenant, partner or platform key

Rows in `seq` order with `prev_hash` and `row_hash`; filters `item_id`, `identity_id`, `consumer_id`, `action`,
`since_seq`. Verify the chain from any row you stored.

### Consumer routes

No API key. Every request carries an RFC 9421 HTTP message signature made with the consumer's Ed25519 key over
`@method`, `@authority`, `@path` and `content-digest` (not for `GET`), with `created`, `expires` (at most 60
seconds later), a `nonce`, `keyid`, `alg="ed25519"` and `tag="pylota-identity-vault-consumer"`. Any failure is
`401 consumer_unauthenticated` with the same body
([Vault § 8.4](../project/design/vault.md#84-authenticating-a-consumer)).

| Route | Does |
|---|---|
| `GET /v1/vault/consumer/self` | The consumer's own record |
| `POST /v1/vault/consumer/redeem` | `{ ticket, phase, wait_seconds, context }`: redeems one phase; answers the HPKE-sealed packet ([§ 8.6](../project/design/vault.md#86-redemption-checks)) |
| `POST /v1/vault/consumer/report` | `{ ticket, phase?, outcome, final? }`: reports an outcome; answers with `closed`. Accepted until the use's `ticket_valid_until`, 10 minutes after it closed or its window ended |
| `POST /v1/vault/consumer/write-back` | `{ ticket, kind, … }`: a refreshed session, a rotated secret or a password confirmation; answers with `closed`. Accepted before `closes_at`, after a `final` report too; a use closed for any other reason refuses it (`410 vault_ticket_expired`) |
| `GET /v1/vault/consumer/uses/{use_id}` | The use's `status`, `closed` and `close_reason`, for a use bound to this consumer; at most 6 a minute per use |

The header `X-Pylota-Identity-Vault-Unlock` is accepted only from the tenant's own partner key on
`POST …/vault/uses` (when it mints), `POST /v1/vault/uses/{use_id}/ticket`, `…/signature`, `…/vault/rotate-key`,
`POST …/versions` and `DELETE …/vault/custody`; anywhere else it is `400 invalid_request`
(`unlock_not_accepted`). It is never logged or stored.

---

---

## Domains

### `POST /v1/tenants/{tenant_id}/domains` — `domains:write`

```json
{ "name": "agents.brightwell.example", "method": "dns_records", "receiving": true, "sending": true, "replace_mx": false }
```

The **connection method** says what the customer changes at their DNS host. It fixes the domain's `kind`,
`inbound` (how mail reaches identities) and `transport` (how mail is sent) (FR-DOM-7). The full model is in
[Domains on any DNS host](../project/design/domain-connections.md#2-inbound-source-and-outbound-transport-are-separate-choices).

| `method` | The customer changes | `kind` | `inbound` | `transport` |
|---|---|---|---|---|
| `cloudflare_zone` | Nothing: the zone is in this Cloudflare account and the Worker writes the records | `zone` | `routing` | `cloudflare` |
| `nameservers` | Two NS records at the registrar, for a domain used only for mail | `zone` | `routing` | `cloudflare` |
| `dns_records` | One MX, three DKIM CNAMEs, a MAIL FROM MX and TXT, and the ownership TXT, at any DNS host | `external` | `ses` | `ses` |
| `send_only` | Three DKIM CNAMEs, a MAIL FROM MX and TXT, and the ownership TXT; their own mailbox forwards to the agent | `external` | `forward` | `ses` |
| `smtp_relay` | The ownership TXT, plus what their own mail provider already needs | `external` | `forward` or `ses` | `smtp` |
| `delegated_subdomain` | NS records for one subdomain, for example `agents.brightwell.example` | `delegated` | `routing` | `cloudflare` |

| Field | Applies to | Meaning |
|---|---|---|
| `name` | all | The domain, for example `agents.brightwell.example` |
| `method` | all | One of the six methods. Required for new clients. When it is absent, the old `kind` is mapped: `zone` → `cloudflare_zone`, `external` → `send_only`. `"kind": "zone"` with `"create_zone": true` is the old spelling of `nameservers` |
| `receiving`, `sending` | all | Default `true` |
| `replace_mx` | `cloudflare_zone` (apex), `dns_records` | Default `false`. A name that already has MX records, none of them the expected host, is refused with `409 existing_mx` unless this is `true` ([H5](../project/edge-cases.md)). On a zone apex, enabling routing replaces the existing mail provider. On `dns_records` it means "I will replace these": health reports `mx_unexpected` until the old records are gone |
| `confirm_dedicated` | `nameservers` | Default `false`. Confirms that a website or mail on the name may stop (below) |
| `claim` | all | Default `false`. Claims a name that another tenant holds but never verified, after you published the TXT record from `details.claim` of an earlier `409 domain_exists` (below) |
| `inbound` | `smtp_relay` (required) | `forward` (the customer's mailbox forwards) or `ses` (they also publish the SES MX and DKIM records) |
| `smtp` | `smtp_relay` (required) | `host` (a DNS name, not an IP literal), `port` (`465` or `587`), `username`, `password` and `probe_from` (an address the relay accepts as sender; default `postmaster@{name}`). The credentials are sealed under `PM_MASTER_KEY` and never returned, logged or exported |

What each method checks before the domain is created:

- **`cloudflare_zone`, `nameservers` and `delegated_subdomain`** need `PM_CF_API_TOKEN` on the Worker;
  without it the request fails with `422 cf_token_required`. For an apex `cloudflare_zone`,
  `idvy domains add --local-token` with your own Cloudflare token works instead (catch-all, no literal
  rules).
- **Zone permission** (tenant and partner keys): `cloudflare_zone`, and `replace_mx` with it, work only on
  a zone this deployment created for the tenant (with `nameservers` or `delegated_subdomain`) or one
  listed in the tenant's platform-only policy `domains.cloudflare_zones` (names strictly under a listed
  zone, never its apex; or exactly one name for an entry written `=name`). `replace_mx` is platform-only
  except on a zone created for the tenant, and an existing MX set at any name, apex or subdomain, needs it
  ([H18](../project/edge-cases.md)). A zone created for another
  tenant, and any name under the zone of the platform domain, the API host or the console host, is
  refused, for `nameservers` and `delegated_subdomain` too: `403 scope_denied` with
  `details.reason: "zone_not_allowed"`, before anything is changed ([H8](../project/edge-cases.md)).
  Platform keys may use any zone.
- **`nameservers`** creates the zone in this account, and never uses a zone that already exists there
  (`409 domain_exists`, [H10](../project/edge-cases.md)). Platform keys may use it when the Worker's
  Cloudflare token can create zones; tenant and partner keys only when the tenant's policy also has
  `domains.allow_create_zone: true` (otherwise `422 transport_unavailable`,
  `details.reason: "zone_creation_not_allowed"`). Pylota Identity Cloud does not offer it. Moving the
  nameservers hands the whole domain to this
  deployment, so when the name has A, AAAA or MX records, or `www` has a CNAME, A or AAAA record, the request
  needs `"confirm_dedicated": true`; otherwise it fails with `409 domain_not_dedicated` and
  `details.records` lists what was found ([N21](../project/edge-cases.md)). The response's `records` are
  the zone's nameservers, as `NS` records to set at the registrar. Cloudflare deletes a zone that is not
  activated within 28 days; the domain then becomes `removed` with `state_reason: "zone_expired"`
  ([N23](../project/edge-cases.md)).
- **`delegated_subdomain`** is off unless `PM_CF_SUBDOMAIN_SETUP=on` (otherwise
  `422 transport_unavailable`, `details.reason: "subdomain_setup_disabled"`), and needs a Cloudflare
  Enterprise account. The response's `records` are `NS` records for the subdomain, to add at the parent's
  DNS host.
- For both zone-creating methods, a Cloudflare zone hold returns `409 zone_hold`
  ([N24](../project/edge-cases.md)), and Cloudflare error 1105 (too many attempts to add a domain) returns
  `429 upstream_rate_limited` with `Retry-After: 10800` and `details.retry_after: 10800`
  ([N22](../project/edge-cases.md)).
- **`dns_records` and `send_only`** need the SES transport (`PM_SES_*`); without it they fail with
  `422 transport_unavailable`, `details.reason: "ses_not_configured"`. `dns_records`, and `smtp_relay` with
  `inbound: ses`, also need SES receiving (`PM_SES_INBOUND_TOPIC_ARN`, bucket and queue), otherwise
  `details.reason: "ses_receiving_not_configured"`. Every method that needs an SES identity (`dns_records`,
  `send_only`, `smtp_relay` with `inbound: ses`) fails with `details.reason: "ses_identity_limit"` once the
  SES region holds 10,000 identities.
- **`smtp_relay`**: a `port` other than `465` or `587` (port `25` included) returns
  `400 smtp_port_not_allowed`. Before it stores anything, the Worker connects to the relay once (EHLO,
  STARTTLS, AUTH, QUIT). No STARTTLS on 587 (or no TLS on 465) returns `422 smtp_tls_required`, and the
  credentials are not sent; a `535` answer to AUTH returns `422 smtp_auth_failed`; a connection that
  cannot be made returns `502 upstream_error`. The domain sends only after an alignment
  [probe](#post-v1domainsdomain_idprobe--domainswrite) passes.

Also:

- A name your own tenant already has returns `409 domain_exists` with `details.domain_id`. A name held by
  another tenant (in any state), or one for which the deployment's Cloudflare or AWS account already holds a
  zone, sending domain, routing setup or SES identity it did not create for you, returns `409 domain_exists`
  with one body for all of these cases, so the answer reveals nothing about others. Its `details.claim` is a
  TXT record, `_pylota-identity.{name}` = `pm-claim=…`. If you control the name's DNS, publish it and repeat
  the request with `"claim": true`: a holder that never verified the domain is evicted, the answer is
  `409 domain_claim_pending` with `Retry-After` until its removal finishes, and then the same request
  succeeds. A verified holder is never evicted ([H14](../project/edge-cases.md)).
- A tenant may hold 5 domains that were never verified: the 6th returns `422 unverified_domain_limit`
  (platform keys are not limited). A domain still unverified 14 days after it was added is removed, with
  `domain.removed` and `reason: "unverified_expired"`.
- A Cloudflare zone holds at most 30 mail domains, routing and sending together: the 31st returns
  `422 zone_domain_limit` ([H15](../project/edge-cases.md)).
- When the plan's `custom_domains` allowance is spent, the request fails with `402 billing_limit`
  (`details.feature: "custom_domains"`).
- An apex whose merged SPF record would need more than 10 DNS lookups (or more than 2 void lookups) is
  refused with `400 spf_lookup_limit`; `details.lookups` gives the count and `fix` names the includes to
  flatten ([H2](../project/edge-cases.md)).

Returns `201` with a [Domain](#domain-object) in `pending` state. Its `records` are read from the
provider APIs at that moment.

### `GET /v1/tenants/{tenant_id}/domains` · `GET /v1/domains/{domain_id}` — `domains:read`

The platform domain is visible to every key, with `tenant_id: null`.

### `GET /v1/domains/{domain_id}/records` — `domains:read`

Re-reads the expected records from the provider APIs and checks each against DNS:

```json
{
  "data": [
    { "type": "TXT", "name": "_pylota-identity.mail.acmecarhire.example", "host": "_pylota-identity.mail",
      "value": "pm-verify=8f2k…", "purpose": "ownership", "required": true, "status": "ok",
      "observed": ["pm-verify=8f2k…"] },
    { "type": "TXT", "name": "cf-bounce._domainkey.mail.acmecarhire.example", "host": "cf-bounce._domainkey.mail",
      "value": "v=DKIM1; …", "purpose": "dkim", "required": true, "status": "missing", "observed": [] }
  ],
  "checked_at": "2026-10-09T10:05:00Z"
}
```

- `name` is fully qualified. `host` is the same name relative to the registrable domain (from the Public
  Suffix List), because DNS hosts differ in which of the two they ask for ([N17](../project/edge-cases.md)).
- `purpose` is `ownership`, `mx`, `dkim`, `return_path`, `spf`, `dmarc` or `ns`.
- `status` is one of `ok`, `missing`, `mismatch` or `unexpected`, where `unexpected` means an extra record
  that conflicts (for example a second SPF record).

With `?format=bind` the same records come back as an RFC 1035 zone fragment (`text/plain`), with
suggested records, records to remove and registrar nameservers as comments
([Parity additions § 9](../project/design/parity-additions.md#9-zone-file-export-fr-dom-15)).

### `PATCH /v1/domains/{domain_id}` — `domains:write`

The body has `transport`, `smtp`, `sending_paused`, or several. Returns `200` with the domain.
Audit-logged.

**`sending_paused`**, platform keys only (`403 scope_denied` with `details.field: "sending_paused"` for
others): `false` lifts an automatic sending pause of a tenant domain (`sending_paused_at`,
[G12](../project/edge-cases.md)). A paused domain's sends get `409 sending_paused` and never fall back to
the platform domain.

**`transport`**, platform keys only (`403 scope_denied` for others):

```json
{ "transport": "ses" }
```

Switches the transport that sends as a domain on Cloudflare: `cloudflare` or `ses`. This is the Email
Sending failover of [J5](../project/edge-cases.md). `ses` needs the SES transport configured
(`422 transport_unavailable`, `details.reason: "ses_not_configured"`) and an SES identity for the domain
(`ses_region` set). A `cloudflare_zone`, `nameservers` or `delegated_subdomain` domain gets one, with its
three DKIM records, during onboarding when the SES transport is configured; without one the switch gets
`422 transport_unavailable`. The switch also needs SES to report the identity as verified for sending with
DKIM `SUCCESS` at that moment; otherwise `422 transport_unavailable` with
`details.reason: "ses_identity_not_verified"` and `details.dkim_status`. A transport the domain's method
cannot use returns
`422 transport_unavailable` with `details.reason: "method_not_supported"`: `dns_records` and `send_only`
domains send only through `ses`, `smtp_relay` domains only through `smtp`, and the platform domain only
through `cloudflare`. The change applies to sends that reach the transport after it and starts a health
check at once (alignment differs per transport). A switch that must call SES waits up to 5 seconds for
the deployment's SES control-plane budget (one call per second), then fails with
`429 upstream_rate_limited` and `Retry-After`.

**`smtp`**, tenant, partner or platform keys, `smtp_relay` domains only (otherwise `method_not_supported`):

```json
{ "smtp": { "host": "smtp.provider.example", "port": 587, "username": "agents@brightwell.example",
            "password": "…", "probe_from": "agents@brightwell.example" } }
```

Rotates the relay credentials or changes the relay. It takes the fields of `smtp` on domain create, with
the same port rule and connection test (`400 smtp_port_not_allowed`, `422 smtp_tls_required`,
`422 smtp_auth_failed`, `502 upstream_error`). The new values are kept pending until an alignment probe
with them passes; until then sends keep using the current values, which the domain's `smtp` still shows.
The probe result arrives as a domain health change.

### `POST /v1/domains/{domain_id}/probe` — `domains:write`

No body. Runs the alignment probe now, for a domain whose transport is `smtp` (otherwise
`422 transport_unavailable`, `details.reason: "method_not_supported"`). At most once a minute per domain
(`429 rate_limited`). Returns `202`:

```json
{ "probe_id": "prb_01JA…" }
```

The probe sends a message `From: {probe_from}` through the relay to an address on the platform domain. It
passes when the `From` header arrives unchanged and DMARC for the domain passes on Pylota Identity's own
check. The result arrives as a domain health change within 15 minutes: in the domain's `probe`, and on
failure as the issue `smtp_unaligned`, `smtp_from_rewritten` or `smtp_probe_timeout`
([Domains on any DNS host › The probe](../project/design/domain-connections.md#53-proving-alignment-the-probe)).
A probe also runs before the domain's first send and every day after.

### `POST /v1/domains/{domain_id}/verify` — `domains:write`

Runs a check now (rate-limited to one a minute per domain) and returns the domain.

### `GET /v1/domains/{domain_id}/health` — `domains:read`

```json
{
  "state": "failing", "reason": "dkim_missing", "since": "…",
  "issues": [ { "code": "dkim_missing", "record": "cf-bounce._domainkey…", "fix": "Add TXT … with value …" } ],
  "checks": [ { "at": "…", "resolver": "cloudflare-doh", "outcome": "fail" } ],
  "fallback_active": true
}
```

The issue codes and their levels are listed in
[Identities and domains › What each check verifies](../project/design/identity-domains.md#what-each-check-verifies)
and, for each connection method, in
[Domains on any DNS host › Health checks per method](../project/design/domain-connections.md#6-health-checks-per-method).

### `POST /v1/domains/{domain_id}/reprove` — `domains:write`

Issues a new ownership TXT value for a `suspended` domain. Returns the domain with the new record.

### `DELETE /v1/domains/{domain_id}` — `domains:write`

Fails with `409 domain_in_use` while any address on it is `active` or `retiring`, and with
`403 scope_denied` on the platform domain. Otherwise it starts removal of what onboarding created for
this domain, and nothing else: its routing rules, its own routing records (Email Routing stays on for the
zone's other mail domains), sending onboarding and the event subscription, and for a domain with an SES
identity, the SES identity and the domain's addresses in the retired-address receipt rules
(`pm-retired-{n}`). Returns `202`. `domain.removed` follows with `reason: "requested"`. If the removal
fails after 10 attempts on one step, the domain stays `removing` and the removal restarts once a day;
calling `DELETE` again restarts it at once ([H12](../project/edge-cases.md)). Every SES call of a
removal runs in the `domain_remove` job, which waits for the deployment's SES control-plane budget
in the background, so `DELETE` never waits for it.

#### Domain object

```json
{
  "id": "dom_01JA…", "tenant_id": "ten_01J9…", "name": "agents.brightwell.example",
  "method": "dns_records", "kind": "external", "inbound": "ses", "transport": "ses",
  "is_apex": false, "routing_mode": "catch_all", "reply_token": "subaddress",
  "receiving": true, "sending": true,
  "ses_region": "eu-west-2", "mail_from_domain": "pm-bounce.agents.brightwell.example",
  "smtp": null, "probe": null,
  "state": "healthy", "state_reason": null, "state_changed_at": "…",
  "delivery_events": "active", "details": null,
  "records": [ "...as in /records..." ], "created_at": "…"
}
```

| Field | Values |
|---|---|
| `method` | One of the six methods, or `platform` for the platform domain |
| `kind` | `platform`, `zone`, `delegated` or `external` |
| `inbound` | `routing` (Cloudflare Email Routing), `ses`, `forward` (the customer's mailbox forwards) or `none` |
| `transport` | `cloudflare`, `ses` or `smtp` |
| `routing_mode` | `catch_all`, `literal` (one routing rule per address, on a zone subdomain) or `forward` |
| `ses_region` | The region of the domain's SES identity: set when `inbound` or `transport` is `ses`, and on a `cloudflare_zone`, `nameservers` or `delegated_subdomain` domain that got an SES identity for the Email Sending failover ([J5](../project/edge-cases.md)) during onboarding; otherwise `null` |
| `mail_from_domain` | `pm-bounce.{name}` on a `dns_records` or `send_only` domain, whose mail SES sends; the local part `pm-bounce` is reserved on such domains. Otherwise `null`, including a Cloudflare-method domain sending through its J5 failover identity after a `PATCH` to `ses`: that identity has no custom MAIL FROM |
| `smtp` | `smtp_relay` only, otherwise `null`: `{ "host", "port", "username", "probe_from" }`. Never the password |
| `probe` | `smtp` transport only, otherwise `null`: `{ "last_at", "result" }`. `result` is `pass` or the issue code of the failure (`smtp_unaligned`, `smtp_from_rewritten`, `smtp_probe_timeout`, `smtp_auth_failed`, `smtp_tls_required`); both are `null` before the first probe |
| `state_reason` | The first issue code; `zone_expired` on a `nameservers` domain whose zone Cloudflare deleted; `evicted` or `unverified_expired` on a never-verified domain being or already removed |
| `delivery_events` | `active` (provider delivery events reach the service), `manual` (a Cloudflare-transport domain created without an event subscription: run `idvy domains subscribe <domain>`; until then its sends go out from each identity's platform address, marked `sent_via_fallback`), or `none` (`sending: false`). See [Identities and domains › Kind `zone`](../project/design/identity-domains.md#kind-zone) |
| `details` | `null`, or `{ "action": "run idvy domains subscribe <domain>" }` while `delivery_events` is `manual`: the operator step that remains |

---

## Threads and messages

### `GET /v1/identities/{identity_id}/threads` — `messages:read`

Filters: `label`, `category`, `needs_reply_gte` (0–1; the search operator `is:needs_reply` uses 0.5),
`is_unread`, `direction` (of the last message), `after`, `before`, `archived` (default `false`). Sorted
by `last_at` descending.

Threads are built from visible mail only: quarantined, screening, hidden and throttled messages are never
listed or counted here, whatever the key's permissions. A key with `quarantine:review` reaches them through the
message list with an explicit `status` filter (below), or the [quarantine list](#quarantine)
(quarantined messages only).

```json
{
  "data": [{
    "id": "thr_01J9…", "subject": "Booking BK-2291 — change of dates",
    "participants": [ { "address": "jo@example.net", "name": "Jo Rivera" } ],
    "message_count": 4, "unread_count": 1,
    "first_at": "…", "last_at": "…", "last_inbound_at": "…", "last_direction": "inbound",
    "snippet": "Could we move the pick-up to Friday…",
    "labels": ["booking"], "category": "customer_request", "needs_reply": 0.92, "urgency": 2,
    "hold": null
  }],
  "next_cursor": null
}
```

### `GET /v1/identities/{identity_id}/threads/{thread_id}` — `messages:read`

Query: `messages_limit` (default 20, max 100), `cursor`, and `include` (comma list: `quoted`, `html`, `headers`).

Returns the thread summary plus `messages` (oldest first within the page). By default each message
carries `extracted_text` (quotes stripped) rather than the full `text`.

### `PATCH /v1/identities/{identity_id}/threads/{thread_id}` — `messages:write`

```json
{ "labels_add": ["claims"], "labels_remove": [], "read": true, "archived": false }
```

When the tenant's `events.labels_changed` is `true`, a change of labels emits `message.labels_changed`
([Webhook events](events.md#messages)).

### `POST /v1/identities/{identity_id}/threads/{thread_id}/hold` — `erasure:manage`

```json
{ "reason": "PCN dispute WM12345678", "until": "2027-10-09T00:00:00Z" }
```

`DELETE /v1/identities/{identity_id}/threads/{thread_id}/hold` (`erasure:manage`) removes it. Both are
audit-logged. On a tenant that is `erasing` with held threads, these two routes are the one write that
a partner key (and a platform key) can still make, so a hold can be extended or released
([Privacy design § 8](../project/design/privacy.md#8-legal-holds)).

### `GET /v1/identities/{identity_id}/messages` — `messages:read`

Filters: `thread_id`, `direction`, `status`, `label`, `after`, `before`. Sorted newest first.

Quarantined, hidden and throttled messages are left out by default, whatever the key's permissions. They
are listed only when the request filters on that status explicitly (`status=quarantined`, `hidden` or
`throttled`) and the key holds `quarantine:review`. A key without it that sends such a filter gets
`200` with none of those messages, never `403`, as search treats `include_quarantined`
([Security design § 5.3](../project/design/security.md#53-cross-level-read-access)).

`screening_status` (`pending`, `clear`, `suspect`, `unavailable`) filters on the message's screening record,
inside the visibility rule. Messages still being screened (`status=screening`) are left out by default and
listed only for that explicit filter from a key holding `quarantine:review`, as for quarantined mail.

### `GET /v1/identities/{identity_id}/messages/{message_id}` — `messages:read`

`include` takes `html`, `headers` and `quoted`. A `quarantined`, `screening`, `hidden` or `throttled` message
is returned only to a key that holds `quarantine:review`; any other key gets `404 message_not_found`, as for
a message that does not exist ([Security](../project/design/security.md)). The same rule applies to its
attachments, their extracted text, its raw MIME and re-running its triage. Reply, reply-all and forward
need `quarantine:review` for a quarantined or screening message and never accept a hidden or throttled one.

#### Message object

```json
{
  "id": "msg_01J9…", "thread_id": "thr_01J9…", "identity_id": "idn_01J9…",
  "direction": "inbound", "status": "received",
  "from": { "address": "accounts@brightwell.example", "name": "Brightwell Leeds" },
  "to": [ { "address": "maintenance.acme@agents.example", "name": "" } ],
  "cc": [], "bcc": [], "reply_to": [],
  "delivered_to": "maintenance.acme@agents.example", "is_primary_recipient": true,
  "subject": "Invoice 88213 – AB12 CDE",
  "sent_at": "2026-09-14T08:12:00Z", "received_at": "2026-09-14T08:12:03Z",
  "extracted_text": "Please find attached invoice 88213 for brake pads and discs…",
  "text": null,
  "html": null,
  "attachments": [
    { "id": "att_01J9…", "filename": "INV-88213.pdf", "content_type": "application/pdf",
      "size": 48213, "disposition": "attachment", "text_status": "ready", "pages": 2, "risk": null,
      "upload_id": null }
  ],
  "labels": ["invoice"],
  "kind": "normal",
  "calendar": null,
  "trust": {
    "verdict": "pass", "spf": "pass", "dkim": "pass", "dmarc": "pass", "arc": "pass",
    "spf_domain": "mail.brightwell.example", "spf_aligned": true,
    "dkim_domains": [
      { "domain": "brightwell.example", "selector": "s1", "result": "pass", "aligned": true },
      { "domain": "mailer.example", "selector": "k2", "result": "pass", "aligned": false }
    ],
    "dmarc_domain": "brightwell.example", "dmarc_policy": "reject", "dmarc_aligned_by": "dkim",
    "arc_sealer": "google.com",
    "known_sender": true, "quarantined": false, "spam_score": 0.02, "automated": false, "flags": []
  },
  "triage": {
    "status": "done", "category": "billing", "needs_reply": 0.15, "urgency": 1,
    "summary": "Brightwell invoice 88213 for AB12 CDE brake work, £412.80 inc VAT.",
    "language": "en", "risk_flags": [], "model": "@cf/openai/gpt-oss-20b", "version": 3, "run": 1
  },
  "screening": null,
  "refs": [ { "kind": "uk_plate", "value": "AB12CDE" }, { "kind": "invoice", "value": "88213" } ],
  "rfc_message_id": "CAF8a…@mail.brightwell.example",
  "in_reply_to": null,
  "draft_id": null,
  "deliveries": null,
  "flags": [],
  "metadata": {}
}
```

- `text` is the full plain text. It is included with `include=quoted`.
- `html` is sanitised HTML. It is included with `include=html` and is never rendered by the service.
- `trust.flags` can hold `hidden_text`, `display_name_spoof`, `lookalike_domain`, `reply_to_mismatch`,
  `thread_join_unverified` (a thread join without a valid token: a failed token, or `In-Reply-To` or
  `References` from a sender who is not a participant, [C9](../project/edge-cases.md)) and
  `shared_domain_sender` (the sender is another workspace's address on the shared platform domain,
  [D12](../project/edge-cases.md)), `steering_suspected` (screening judged that the message tries to
  steer an agent) and `attachments_unscreened` (screening could not read the text of at least one
  attachment, for example an image while `inbound.extract_image_text` is off or a file whose conversion
  failed: treat those attachments' bytes as unscreened content).
- `trust.verdict` is `pass`, `arc_rescued`, `fail`, `softfail`, `none`, `unaligned` or `unverified`.
  `arc_rescued`: the message failed DMARC, and a trusted forwarder's ARC seal vouches that it passed when
  the forwarder received it ([Parity additions § 6](../project/design/parity-additions.md#6-trusted-arc-sealers-fr-in-15)).
  It is never quarantined for authentication and never releases a verification code.
- `trust.spf_domain`, `spf_aligned`, `dkim_domains` (at most 10), `dmarc_domain`, `dmarc_policy`,
  `dmarc_aligned_by` and `arc_sealer` say which domain each method checked and which aligned
  ([Parity additions § 5](../project/design/parity-additions.md#5-authentication-detail-on-messages-fr-in-14)).
- `screening` is the message's [screening record](../project/design/screening.md#3-the-model-and-the-state-machine)
  (`status` `pending`, `clear`, `suspect` or `unavailable`; `mode`; `reason`; `signals[]`; `model`;
  `started_at`; `deadline_at`; `completed_at`; `late`; `released_by`), or `null` when the message was not
  screened. `status: "screening"` means the message is held until its verdict, at the latest at
  `screening.deadline_at`.
- `calendar` is the message's calendar object, or `null`: method, verification (`verified`,
  `verified_by`, `reason`), `uid`, `sequence`, `dtstamp`, `recurrence_id`, `status`, `summary`,
  `description` (an excerpt), `location`, `start` and `end` (UTC instants with the original `TZID` and
  how it was resolved), `all_day`, `floating`, `ends_at`, `rrule` (validated, never expanded),
  `exdates_count`, `organizer`, `attendees` (with `partstat`, `role`, `rsvp`, `is_self`), overrides,
  `conference` (meeting links), `organized_here` and the `attachment_id` of the raw `.ics`. All its strings
  are untrusted content. Alarms, attachments, images and extension properties are never included
  ([Calendar invitations §2.9](../project/design/calendar-invitations.md#29-the-calendar-object)).
- `draft_id` is the draft an outbound message was sent from, or `null`.
- `attachments[].upload_id` is the upload the attachment was copied from; `null` for inline, inbound and
  forwarded attachments. It stays after the upload is gone.
- `triage.status` is `pending`, `done`, `skipped` or `failed`. `triage.version` is the triage logic
  version of the run that produced the record: a re-run keeps it on the same release and records the
  current one after an upgrade; `triage.run` counts completed runs
  (`0` for a record written at ingest). `triage.reason` is present only for
  `skipped` (`allowance`, `policy_disabled`, `not_eligible`, `ai_unavailable`) and `failed` (`invalid_output`,
  `model_unavailable`, `input_unavailable`) ([Triage design](../project/design/triage.md)). For example,
  mail that arrives after the workspace's `triage` allowance is spent is still stored, and its triage is
  skipped with reason `allowance`; the built-in rules' risk flags are kept and the model does not run
  ([W7](../project/edge-cases.md)):
  `{ "status": "skipped", "reason": "allowance", "category": null, "needs_reply": null, "urgency": null, "summary": null, "language": null, "risk_flags": ["unknown_sender"], "model": null, "version": 3, "run": 0 }`.
- `deliveries` is set on outbound messages:
  `[{ "address", "field", "status", "smtp_code", "enhanced_code", "bounce_type", "updated_at" }]`
  (`enhanced_code` is the RFC 3463 code, for example `5.1.1`, when the provider or relay gave one).
- Message-level `flags` include `sent_via_fallback`, `parse_degraded`, `encrypted`,
  `message_id_conflict`, `reprocessed`, `reconciled`, `bcc`, `loopback` (delivered inside the deployment
  for a test tenant, [L3](../project/edge-cases.md)), `body_truncated` (a stored body was cut at its
  storage cap; the full message is in the raw MIME), `sender_suppressed` (the sender is on the tenant's
  suppression list; the message is stored as usual, [D7](../project/edge-cases.md)), `body_redacted`
  (links and codes replaced in a system-identity message, [A15](../project/edge-cases.md)) and
  `dsn_untrusted` (a delivery report that changed no delivery, [D11](../project/edge-cases.md)).
- `headers` (with `include=headers`) is `null` once the raw MIME it is read from is gone (past
  `retention.raw_days`).
- `is_primary_recipient` is `true` on exactly one copy when one message reached several identities of
  the tenant ([A9](../project/edge-cases.md)).

All text fields (subject, display names, filenames, bodies) are **untrusted content**. Show them to a
model inside a clearly delimited block, never as instructions.

### `GET /v1/identities/{identity_id}/messages/{message_id}/raw` — `messages:read`

`message/rfc822` bytes, available for `raw_days` (default 90). Then `410 raw_expired`.

### `PATCH /v1/identities/{identity_id}/messages/{message_id}` — `messages:write`

`labels_add`, `labels_remove`, `read`.

When the tenant's `events.labels_changed` is `true`, a change of labels emits `message.labels_changed`
([Webhook events](events.md#messages)).

### `POST /v1/identities/{identity_id}/messages/batch-get` — `messages:read`

```json
{ "message_ids": ["msg_01JA4B1G7PW7X2M5N6P8R0T1YC", "msg_01JA4B2K9QW7X2M5N6P8R0T1YD", "msg_01JA4B3M2RW7X2M5N6P8R0T1YE"],
  "extracted_text_bytes": 2048 }
```

```json
{
  "data": [
    { "id": "msg_01JA4B1G7PW7X2M5N6P8R0T1YC", "thread_id": "thr_01JA…", "direction": "inbound", "status": "received",
      "from": { "address": "jo@example.net", "name": "Jo Rivera" }, "subject": "Change of dates for BK-2291",
      "extracted_text": "Could we move the pick-up to Friday…", "extracted_text_truncated": false,
      "text": null, "html": null, "labels": ["booking"], "trust": { "verdict": "pass", "…": "…" }, "…": "…" }
  ],
  "not_found": ["msg_01JA4B3M2RW7X2M5N6P8R0T1YE"],
  "remaining": [],
  "truncated": false
}
```

- `data` holds [Message objects](#message-object) in request order, built as
  `GET …/messages/{message_id}` builds them, without `text`, `html` and `headers` (there is no `include`),
  and with `extracted_text` cut at a UTF-8 character boundary to `extracted_text_bytes` (0–16,384,
  default 2,048), with `extracted_text_truncated: true` when it was cut.
- `not_found` lists, in request order, every ID that is not a message of this identity that the key may
  see: missing, erased, of another identity, or `quarantined`, `hidden` or `throttled` without
  `quarantine:review`. The answer is the same for each of them.
- `remaining` and `truncated`: the serialised response body never exceeds 1,048,576 bytes. When the next
  message would pass that budget, it and every later found message are listed in `remaining`, in request
  order, and `truncated` is `true`; the client asks again for them. The first found message is always in
  `data` (one message without `text` and `html` is far below the budget).

### `POST /v1/identities/{identity_id}/messages/batch-update` — `messages:write`

```json
{ "message_ids": ["msg_01JA4B1G7PW7X2M5N6P8R0T1YC", "msg_01JA4B2K9QW7X2M5N6P8R0T1YD"],
  "labels_add": ["claimed", "claimed-by-bookings"], "labels_remove": ["new"],
  "read": true, "archived": false, "require_absent": ["claimed"] }
```

```json
{ "updated": ["msg_01JA4B1G7PW7X2M5N6P8R0T1YC"], "unchanged": [],
  "conflicts": ["msg_01JA4B2K9QW7X2M5N6P8R0T1YD"], "not_found": [],
  "threads_archived": 0, "threads_unarchived": 0 }
```

- `labels_add`, `labels_remove`: labels as in `PATCH …/messages/{message_id}` (pattern
  `^[a-z0-9][a-z0-9_:-]{0,63}$`), at most 64 each. `read` sets the read state. `archived` sets the
  archived flag of each **thread** that holds a listed message, as the single-thread `PATCH` does.
- `require_absent` (at most 10 labels) makes the change conditional per message: a message that carries
  any of these labels when the transaction runs is not changed and is listed in `conflicts`. It is the
  atomic claim that lets several agents share one inbox: `require_absent: ["claimed"]` with
  `labels_add: ["claimed"]` lets exactly one of two racing callers take a message.
- `updated`: messages this request changed (a label, the read state, or its thread's archived flag).
  `unchanged`: messages found and allowed whose state already matched. `conflicts`: messages skipped by
  `require_absent`. `not_found`: as for `batch-get`. Every listed ID appears in exactly one of the four.
- `threads_archived` and `threads_unarchived` count the threads whose flag this request changed.
- `Idempotency-Key` is optional, as on every other `POST`. The record is kept in the identity's mailbox and
  written in the same transaction as the change, scoped per identity as for sends, so a retry after a lost
  response replays the committed result (`Idempotent-Replayed: true`) and a claim the caller already made
  reads as `updated`, not as a conflict. A second request with the same key waits for the first and
  replays it ([Parity additions § 2.4](../project/design/parity-additions.md#24-concurrency-idempotency-and-failures)).

A batch is checked once for scope and permission. IDs the key cannot see are in `not_found`, exactly as
missing ones. `batch-update` is one transaction: with a message that would pass 64 labels nothing changes
(`400 invalid_request`, `details.reason = "label_limit"`, `details.message_ids`).

### `GET /v1/identities/{identity_id}/messages/{message_id}/attachments/{attachment_id}` — `attachments:read`

Returns the bytes with `Content-Disposition: attachment`, `X-Content-Type-Options: nosniff` and
`Content-Security-Policy: sandbox`. The message's visibility is checked first: an attachment of a
`quarantined`, `screening`, `hidden` or `throttled` message is `404 message_not_found` without `quarantine:review`.
Then attachments with a `risk` need `quarantine:review` too (`403 permission_denied`).

### `GET /v1/identities/{identity_id}/messages/{message_id}/attachments/{attachment_id}/text` — `attachments:read`

Query: `pages=1-3` (default: all, capped at 200 KB of text).

```json
{ "status": "ready", "pages": [ { "page": 1, "text": "INVOICE 88213 …" } ], "total_pages": 2, "truncated": false }
```

`status` is one of `pending`, `ready`, `unavailable` (extraction failed or unsupported type) or
`skipped` (by policy or risk).

### `POST /v1/identities/{identity_id}/messages/{message_id}/triage` — `messages:write`

Re-runs triage (FR-TRI-5). Returns `202` with no body. A `message.triaged` event follows, and the record
then has a `run` one higher and the current release's `version` (the same one unless triage was
upgraded since the last run). A message that ingest never triages (quarantined,
hidden, throttled, a delivery report or read receipt, or a system-identity message), or a message still
being screened (`details.reason = "screening"`), gets `409 triage_not_eligible` with `details.reason`
(`quarantined`, `screening` or `not_eligible`): release a quarantined message instead, which triages it.
Outbound messages get `400 invalid_request`.

### `POST /v1/identities/{identity_id}/messages/{message_id}/feedback` — `messages:write`

```json
{ "verdict": "false_positive", "notes": "Our booking system's own confirmation format." }
```

Feedback on a screening verdict ([Screening hold §11](../project/design/screening.md#11-feedback)). `verdict` is
`correct`, `false_positive`, `missed_threat`, `legitimate_instruction` or `spam`, and must fit the record's status;
`notes` is optional, at most 500 characters. Returns `201` with the feedback object. At most 20 per message.
Feedback never changes the message. Errors: `400 invalid_request` (`details.reason = "feedback_not_applicable"`
for a verdict that does not fit), `404 message_not_found` (a quarantined or screening message without
`quarantine:review`), `409 not_screened`, `409 screening_pending`, `422 feedback_limit_reached`.

### `GET /v1/identities/{identity_id}/messages/{message_id}/feedback` — `messages:read`

The message's feedback, oldest first: `{ "data": [ … ] }`.

### `POST /v1/identities/{identity_id}/messages/{message_id}/release` — `quarantine:review`

```json
{ "reason": "Known supplier, DKIM key rotated" }
```

Moves a quarantined message to `received`, emits `message.released` and, when the message was never evented as
received, `message.received` with `released: true` (in that order, in one transaction), and runs triage. A
message still being screened gets `409 not_quarantined` with `details.status = "screening"` and
`details.deadline_at`. Audit-logged
(`quarantine.release`, with the key). When `PM_QUARANTINE_KEY_RELEASE` is `off` (Pylota Identity Cloud),
every API key gets `403 permission_denied` and the release has to be done by a person in the console
(FR-CON-6), unless the message's tenant has `policy.quarantine.key_release: true`: then a platform key, or
the partner key of the tenant's own partner, with `quarantine:review` may release it; tenant and identity
keys still get `403 permission_denied` ([J16](../project/edge-cases.md)). Only a platform
key, or the partner key of the tenant's own partner, can set that policy
([Configuration › Tenant policy](configuration.md#tenant-policy)).

### `DELETE /v1/identities/{identity_id}/messages/{message_id}` — `erasure:manage`

Returns `202` with an erasure request of scope `message`. If the message's thread is under a legal hold,
it returns `423 legal_hold` and creates nothing (an erasure request of a wider scope skips held threads
instead).

---

## Sending

Send, reply, reply-all, forward and [calendar-reply](#post-v1identitiesidentity_idmessagesmessage_idcalendar-reply--messagessend)
need `messages:send` and an `Idempotency-Key`. They return `202 Accepted` with the
[Message object](#message-object) (`direction: "outbound"`, `status: "queued"`) plus `"deduplicated": false`.

When the plan's `sends` allowance is spent, send, reply, reply-all and forward fail with
`402 billing_limit` (`details.feature: "sends"`). Nothing is stored; after an upgrade or a top-up, retry
with the **same** `Idempotency-Key`.

**Dry run.** Add `?dry_run=true` to send, reply, reply-all or forward to run every check (permissions,
policy, recipients, suppressions and lists, size) without sending or storing anything, taking quota or
locking the thread. The `Idempotency-Key` header is optional on a dry run and is never recorded. It
returns `200`:

```json
{ "would_send": true,
  "recipients": [ { "address": "jo@example.net", "field": "to", "status": "queued" },
                  { "address": "old@example.org", "field": "cc", "status": "suppressed", "reason": "hard_bounce" } ] }
```

or the error a real send would get, plus `422 all_recipients_suppressed` and `422 recipient_blocked`,
which only a dry run returns. A `200` always has `would_send: true`; each recipient's `status` is
`queued` or `suppressed` (with `reason`: the suppression reason, or `send_block`, `not_on_allowlist` or
`unknown_recipient`).

### `POST /v1/identities/{identity_id}/messages`

```json
{
  "to": [ { "address": "jo@example.net", "name": "Jo Rivera" } ],
  "cc": [], "bcc": [],
  "subject": "Your booking BK-2291 is confirmed",
  "text": "Hi Jo, your Golf is booked for Friday 10:00…",
  "html": "<p>Hi Jo, your Golf is booked for <b>Friday 10:00</b>…</p>",
  "attachments": [
    { "filename": "BK-2291.pdf", "content_type": "application/pdf",
      "content_base64": "JVBERi0xLjcK…", "disposition": "attachment" }
  ],
  "kind": "transactional",
  "thread_id": null,
  "from_address": null,
  "labels": ["booking"],
  "headers": { "X-Booking-Ref": "BK-2291" },
  "metadata": { "booking_id": "bk_2291" }
}
```

- Recipients can be strings (`"jo@example.net"`) or objects. At most `policy.max_recipients` (default 10,
  hard maximum 49) across `to`, `cc` and `bcc`. Duplicates are removed.
- At least one of `text` and `html` is required. Text is derived from HTML when it is missing. The
  identity's signature and the tenant's AI-disclosure footer are appended according to policy.
- `kind`:
  - `transactional` (the default);
  - `marketing`, which needs an `unsubscribe` object (`{ "url": "https://…", "mailto": "…" }`) and the
    tenant's consent attestation (`"consent": { "basis": "opt_in", "recorded_at": "…" }`), else
    `400 marketing_requirements_missing`. It also needs a sending domain whose transport is `ses` or
    `smtp` (FR-OUT-15): Cloudflare Email Service is for transactional mail only, so a marketing send from
    the platform domain or from a `cloudflare`-transport domain gets `422 transport_unavailable` with
    `details.reason: "marketing_needs_ses"`. The check runs again before transport; a message whose domain
    has changed transport or fallen back to the platform domain by then ends `rejected` with reason
    `marketing_needs_ses`;
  - `auto_reply`, which sets `Auto-Submitted: auto-replied`. It is only allowed in reply to a
    non-automated message.
- **Known recipients** (FR-OUT-13, [E2](../project/edge-cases.md)). By default
  (`send_policy.require_known_recipient: true`) a recipient the identity has never sent to, that no
  send-allow entry names, and that is not the authenticated sender being answered, is not an error: its
  delivery is `suppressed` with `policy: unknown_recipient`. Mail received from an address does not make
  it known. Set `require_known_recipient: false` on an identity that sends to new people by design.
- **Loops and pauses.** Any send whose hop count would reach 10 gets `409 loop_detected`
  (`details.hop`), whatever its `kind` ([N13](../project/edge-cases.md)). So does a reply, forward or
  threaded send after 25 sends in that thread in 24 hours, or a reply to an address that has sent the
  identity 25 messages in 24 hours, which catches a loop with a correspondent that drops the hop header
  (`details.reason`: `thread_rate` or `sender_rate`, with `details.retry_after`; [N35](../project/edge-cases.md)). A tenant or sending domain whose
  sending was paused automatically gets `409 sending_paused` (`details.scope`: `tenant` or `domain`,
  [G12](../project/edge-cases.md)).
- **Approval required** ([Drafts](#drafts), FR-DRF-4). When the tenant's `drafts.require_approval`, or
  the identity's stricter `send_policy.require_approval`, needs approval for these recipients, the send
  is refused with `403 approval_required` (`details.reason`, `details.recipients`) and nothing is
  stored: create the same message as a draft and send it; it waits for an approver.
- **Expired messages.** A retry with the same `Idempotency-Key` and body after the message expired under
  the identity's `retention.message_hours` gets `410 message_expired` (`details.message_id`,
  `details.expired_at`) and nothing is sent; with another body, `409 idempotency_conflict`.
- `thread_id` continues an existing thread without quoting. References are set from the thread.
- `from_address` must be an `active` address of the identity, or a `retiring` one on a thread that
  already uses it (G7; with `thread_id`). Otherwise `400 invalid_request` with
  `details.errors[0].path = "from_address"`. The default is the primary.
- `headers` accepts only `X-` names matching `^X-[A-Za-z0-9_-]+$` (at most 100 bytes), plus the
  allow-listed `Importance`, `Priority`, `Sensitivity`, `Keywords`, `Comments` and `Organization`. Names
  are matched case-insensitively, as Cloudflare matches them: `importance` is accepted and sent as
  `Importance`, `x-booking-ref` as given, and the reserved `X-Pylota-*` and `X-AI-Generated` are refused in
  any case. Any other name gets `400 header_not_allowed`; two names that differ only in case get
  `400 invalid_request`. `Importance` takes `high`, `normal` or `low`,
  `Priority` `normal`, `non-urgent` or `urgent`, and `Sensitivity` `personal`, `private` or
  `company-confidential`; another value gets `400 invalid_request`. These checks run when the request
  arrives, so a bad header never becomes a later `rejected`. Everything else is set by the service.
- `calendar`: `{ "ics": "<iCalendar>", "attach_copy": true }` sends iMIP. `ics` is at most 262,144
  bytes (`413 calendar_too_large`) and needs exactly one `METHOD`, one `UID`, a `DTSTAMP` in each
  `VEVENT` and the structure RFC 5546 gives its method (`400 calendar_invalid` with
  `details.reason`). For `PUBLISH`, `REQUEST`, `ADD`, `CANCEL` and `DECLINECOUNTER` the `ORGANIZER`
  must be the address the message is sent from (`400 calendar_organizer_mismatch`, which names it),
  and every recipient an `ATTENDEE` except for `PUBLISH`, which has none and may go to anyone; for `REPLY`, `REFRESH` and `COUNTER` the replying `ATTENDEE`
  must be the `From` (`400 calendar_attendee_mismatch`) and the one recipient the `ORGANIZER`
  (`400 calendar_recipient_mismatch` otherwise). `kind` must be `transactional`. Missing
  `VTIMEZONE`s are added and extension properties other than `X-WR-TIMEZONE` removed (both listed in
  `calendar.outbound`). A lower `SEQUENCE` than one this identity already sent for the `UID` gets
  `409 calendar_sequence_stale`. A calendar message never falls back to the platform domain; on a
  `cloudflare`-transport domain it gets `422 transport_unavailable` (`calendar_needs_raw_mime`) until
  the raw send is available ([Calendar invitations §5](../project/design/calendar-invitations.md#5-outbound-sending-icalendar)).
- Attachments: each item is either inline (`filename`, `content_type`, `content_base64`) or an upload
  (`upload_id`, optional `filename`), with `disposition` (`attachment` or `inline`) and `content_id`
  (for inline). An upload must be `ready` and belong to the sending identity (`404 upload_not_found`,
  `409 upload_not_ready`, `410 upload_expired`). Every attachment is classified like inbound mail, and
  one with a quarantining risk (also after a rename) gets `422 attachment_risky`. Attachments total at
  most 26,214,400 decoded bytes; the encoded message must fit the transport limit (5 MiB with
  Cloudflare) or the request fails with `413 message_too_large` (`details.reason` `attachments_total` or
  `transport_limit`). When the tenant enables `large_attachments: "link"`, oversized attachments become
  expiring signed links instead.

### `POST /v1/identities/{identity_id}/messages/{message_id}/reply`

```json
{ "text": "Friday works. See you at 10.", "html": null, "attachments": [], "kind": "transactional" }
```

Replies to the sender of `message_id` (or its `Reply-To`, under the rules in
[Sending](../guides/sending.md#who-a-reply-goes-to)). The subject gets one `Re:` prefix. The `From` is
the address the counterparty wrote to. `In-Reply-To` and `References` are set. It also accepts
`calendar`, as `send` does.

### `POST /v1/identities/{identity_id}/messages/{message_id}/reply-all`

As `reply`, to the sender plus every `To`/`Cc` recipient except this identity's own addresses. BCC
recipients of the original are never included ([A10](../project/edge-cases.md)). `calendar` is refused
(`400 invalid_request`).

### `POST /v1/identities/{identity_id}/messages/{message_id}/forward`

```json
{ "to": ["claims@insurer.example"], "text": "Forwarding the photos for claim 7781.", "include_attachments": true,
  "attachments": [ { "upload_id": "upl_01JAC3Q7W9X2M5N6P8R0T1Y4ZB" } ] }
```

`attachments` adds files, inline or from uploads, as for send; `include_attachments` keeps the
original's. `calendar` is refused (`400 invalid_request`).

### `POST /v1/identities/{identity_id}/messages/{message_id}/calendar-reply` — `messages:send`

```json
{ "partstat": "accepted", "comment": "Our driver will bring the car at 08:45.", "recurrence_id": null }
```

Answers a verified invitation (`method: "request"`) that names this identity as an attendee: an iTIP
`REPLY` goes to the invitation's `ORGANIZER` only, from the attendee's address, in the invitation's
thread. `partstat` is `accepted`, `declined` or `tentative`; `comment` at most 1,000 characters;
`recurrence_id` (an RFC 3339 instant, a local time without offset for a floating invitation, or a date
for an all-day one; `instance_zone_unknown` when the invitation's zone is unknown) answers one instance.
`Idempotency-Key` is required, and `?dry_run=true` works as on sends. Returns `202` with the Message
object. Refusals: `409 calendar_not_invitation`, `409 calendar_not_verified` (also for a quarantined
invitation, release it first, or one in screening), `409 calendar_not_attendee`, `409 calendar_cancelled`,
`409 calendar_superseded`, `409 calendar_event_past`, `409 calendar_attendee_address_unavailable`,
and every send error ([Calendar invitations §6](../project/design/calendar-invitations.md#6-the-calendar-reply-route)).

### `POST /v1/identities/{identity_id}/messages/{message_id}/cancel` — `messages:send`

Only while the message is `queued`, no transport attempt is in progress, and no recipient has been sent
to yet. Returns the message with `status: "canceled"`. Otherwise
`409 not_cancelable`.

### `POST /v1/identities/{identity_id}/messages/{message_id}/resolve` — `messages:write`

For `uncertain` messages only (otherwise `409 not_uncertain`). The body is `{ "outcome": "sent" }` or
`{ "outcome": "not_sent" }`. `sent` moves the message and its uncertain deliveries to `submitted` and
emits `message.sent` (with `provider_message_id: null`); later delivery events still apply. `not_sent`
marks the message `failed` with reason `resolved_not_sent`, after which you may send again with a
**new** Idempotency-Key. Audit-logged.

### Outbound status

| Status | Meaning | Terminal |
|---|---|---|
| `queued` | Accepted, waiting for the transport | no |
| `submitted` | The transport accepted it. `provider_message_id` is set | no |
| `delivered` | Every recipient is delivered | yes |
| `deferred` | At least one recipient has a temporary failure and the provider is still retrying | no |
| `bounced` | At least one recipient bounced and none remains in flight | yes |
| `complained` | A recipient reported spam (can follow `delivered`) | yes |
| `rejected` | The transport refused it, at submission or, for some recipients, when the recipient's server rejected it after submission (validation, policy, a definitive recipient-server rejection) | yes |
| `failed` | It could not be sent (quota exhausted after retries, or resolved as not sent) | yes |
| `uncertain` | The outcome is unknown. It is **never resent automatically** | until resolved |
| `suppressed` | Every recipient is suppressed. Nothing was sent | yes |
| `canceled` | Cancelled while queued | yes |

The message status is a roll-up. Per-recipient status is in `deliveries`.

---

## Drafts

An agent writes an email as a draft, a person or the operator's system approves it when needed, and it
is sent at once or at `send_at` through the same pipeline as every send. The design is
[Drafts, approvals and scheduled send](../project/design/drafts.md).

### `POST /v1/identities/{identity_id}/drafts` — `drafts:write`

The body takes the fields of `POST …/messages` (`to`, `cc`, `bcc`, `subject`, `text`, `html`,
`attachments`, `kind`, `thread_id`, `from_address`, `headers`, `unsubscribe`, `consent`, `labels`,
`metadata`), or `in_reply_to` (with `reply_all`) or `forward_of` with the fields of reply or forward, plus
`send_at`, `client_id` and `request_approval`. An attachment is inline (`content_base64`) or
`{ "upload_id": "…" }`; a draft copies an upload's bytes when its revision is saved, so an upload reference
gets `404 upload_not_found`, `409 upload_not_ready`, `410 upload_expired` or `422 attachment_risky` here and
on update, and attachments are classified and limited to 26,214,400 bytes in total as on a send. A draft
with `in_reply_to` or `forward_of` also needs `messages:read`. The kind is fixed at creation. `client_id`
makes the create idempotent (`200` with the existing draft, `409 client_id_conflict` for another body).
Returns `201` with the [Draft object](#draft-object) and an `ETag`; an `Idempotency-Key` replay returns
`201` again with `Idempotent-Replayed: true`. `Idempotency-Key` optional.

### `GET /v1/identities/{identity_id}/drafts` · `GET …/drafts/{draft_id}` · `GET …/drafts/{draft_id}/attachments/{attachment_id}` — `drafts:read`

List filters: `status` (one or a comma list), `draft_kind`, `label`, `thread_id`, `client_id`,
`scheduled_after`, `scheduled_before`; newest first; items without `text` and `html`, with `preview`.
A draft whose source is quarantined or in screening needs `quarantine:review`, on every route and in the
tenant's queue. `GET` of one reply draft first stores a change in its resolved recipients as a new
revision ([Drafts §2.4](../project/design/drafts.md#24-revision-etag-and-digest)).

### `PATCH /v1/identities/{identity_id}/drafts/{draft_id}` — `drafts:write`

Changes fields; `If-Match` makes a stale write fail with `409 draft_revision_mismatch`. Attachments
change through `attachments_add` and `attachments_remove`, labels through `labels`, `labels_add`,
`labels_remove`. Changing the content of a draft that was approved, pending or scheduled drops the
approval and the schedule and asks for approval again (`draft.approval_requested`); changing only
`send_at`, `labels` or `metadata` keeps them. `send_at` reschedules a scheduled draft, `null`
unschedules it, and a time on a failed draft schedules it again. `409 draft_not_editable` while it is
being sent, and once it is `sent` or `canceled`. `423 legal_hold` for a change of the content, `labels` or
`metadata` of a draft under a legal hold.

### `DELETE /v1/identities/{identity_id}/drafts/{draft_id}` — `drafts:write`

Cancels the draft (`status: "canceled"`, content deleted); a scheduled draft is never sent afterwards.
Returns `200`, also again on a `canceled` draft. A draft under a legal hold is canceled with its content
kept until the hold ends. `409 draft_not_editable` while it is being sent or once sent.

### `POST /v1/identities/{identity_id}/drafts/{draft_id}/send` — `messages:send` and `drafts:read`

`Idempotency-Key` required. Body (optional): `send_at` (a time schedules; `null` sends now; left out
uses the draft's future `send_at` or sends now), `revision` (when given, the draft must still be at it),
`labels_add`, `labels_remove`. Returns `202` with the draft: `sent` with `message_id`, `scheduled`, or
`pending_approval` when policy needs an approval it does not have. A refusal of the send pipeline
answers its own error and leaves the draft as it was. `?dry_run=true` returns
`{ would_send, outcome, approval_reason, send_at, recipients }` and changes nothing.

### `POST /v1/identities/{identity_id}/drafts/{draft_id}/approve` — `drafts:approve`

Body: `revision` (required), `digest` (optional), `send` (required), `note`, `on_behalf_of` (keys
only: your own user's ID, 1–128 printable ASCII, no `@`). `send: true` also needs `messages:send` and
sends now, or at a future `send_at`. A key may approve only where people decide is not required
(`PM_QUARANTINE_KEY_RELEASE=on` or `PM_CONSOLE=off`), or where the tenant's `drafts.key_approval` is
`true` and the key is a platform key or the tenant's own partner key; otherwise
`403 permission_denied` (`person_required`); a key cannot approve content it wrote
(`self_approval`). An `editing`, `pending_approval`, `approved`, `scheduled` or `failed` draft can be
approved; a `scheduled` one stays scheduled. Returns `200` with the draft; a refused send shows
`status: "failed"` and `failure`. Audit-logged.

### `POST /v1/identities/{identity_id}/drafts/{draft_id}/reject` — `drafts:approve`

Body: `reason` (required, at most 500 characters), `revision`, `on_behalf_of`. An `editing`,
`pending_approval`, `approved`, `scheduled` or `failed` draft can be rejected. Returns `200` with the
draft, `status: "rejected"` (also again on a `rejected` one). `409 draft_not_editable` while it is being
sent; `409 draft_not_pending` once `sent` or `canceled`. Audit-logged.

### `GET /v1/tenants/{tenant_id}/drafts` — `drafts:read`, tenant, partner or platform key

The tenant's queue, oldest first: `status` (`pending_approval` by default, `scheduled`, `failed`),
`identity_id`, `scheduled_after`, `scheduled_before`. Items hold IDs, status, kind, counts and times,
never content.

#### Draft object

```json
{ "id": "drf_01JA2B3C4D5E6F7G8H9J0K1M2N", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…",
  "client_id": "pm:v1:draft:apv_8812", "draft_kind": "new", "kind": "transactional",
  "status": "pending_approval", "revision": 3, "etag": "\"3-9f2c41ab\"", "digest": "9f2c41ab…",
  "in_reply_to": null, "forward_of": null, "thread_id": null, "from_address": null,
  "to": [ { "address": "jo@example.net", "name": "Jo Rivera" } ], "cc": [], "bcc": [],
  "subject": "Your quote for the Golf, 14–16 November", "text": "Hi Jo, …", "html": null,
  "attachments": [ { "id": "att_01JA…", "filename": "quote.pdf", "content_type": "application/pdf",
    "disposition": "attachment", "content_id": null, "size": 48213, "sha256": "…", "source": "upload",
    "upload_id": "upl_01JA…" } ],
  "include_attachments": null, "calendar": null, "headers": {}, "unsubscribe": null, "consent": null,
  "labels": ["quote"], "metadata": { "quote_id": "q_118" }, "send_at": null,
  "approval": { "state": "requested", "reason": "new_recipients", "requested_at": "…",
    "requested_by_key_id": "key_01J9…", "approved_revision": null, "approved_digest": null,
    "approved_at": null, "approved_by_key_id": null, "approved_by_user_id": null, "on_behalf_of": null,
    "note": null },
  "send": { "origin": "api", "requested_at": "…", "requested_by_key_id": "key_01J9…",
    "requested_by_user_id": null, "due_at": null, "labels_add": [], "labels_remove": [] },
  "rejection": null, "failure": null, "message_id": null, "sent_at": null,
  "created_by_key_id": "key_01J9…", "updated_by_key_id": "key_01J9…", "content_by_key_id": "key_01J9…",
  "created_at": "…", "updated_at": "…",
  "deduplicated": false }
```

- `id`: `drf_` + ULID.
- `identity_id`, `tenant_id`: from the path and the key's scope, never from the body.
- `client_id`: optional caller key for an idempotent create: 1–255 printable ASCII characters without `@`
  (it appears in events, which hold no addresses), unique per identity.
- `draft_kind`: `new`, `reply`, `reply_all` or `forward`, fixed at creation from `in_reply_to`,
  `reply_all` and `forward_of` ([Drafts §2.2](../project/design/drafts.md#22-kinds-and-the-fields-each-takes)).
- `kind`: the send kind, as on `POST …/messages`: `transactional` (default), `marketing` (new drafts only)
  or `auto_reply` (reply drafts only).
- `status`: `editing`, `pending_approval`, `approved`, `scheduled`, `sending`, `sent`, `failed`, `rejected`
  or `canceled` ([Drafts §2.3](../project/design/drafts.md#23-states-and-who-moves-them)).
- `revision`: starts at 1 and increases by one on every successful `PATCH`, and when a reply draft's
  resolved recipients change ([Drafts §2.4](../project/design/drafts.md#24-revision-etag-and-digest)).
- `etag`: the HTTP entity tag of the current revision
  ([Drafts §2.4](../project/design/drafts.md#24-revision-etag-and-digest)).
- `digest`: the content digest of the current revision, 64 hex characters; `null` once the content is
  cleared.
- `in_reply_to`, `forward_of`: the source message (`msg_`) of a reply or forward draft.
- `thread_id`: the thread the message will join: the source's thread for replies and forwards, or the
  `thread_id` given to a new draft; `null` for a new thread.
- `from_address`, `to`, `cc`, `bcc`, `subject`, `text`, `html`, `headers`, `unsubscribe`, `consent`,
  `include_attachments`, `calendar`: as on the send routes (`calendar` as in
  [Calendar invitations §5](../project/design/calendar-invitations.md#5-outbound-sending-icalendar)), limited
  per kind. `html` is kept exactly as written; it is sanitised only when shown in the console and when
  the message row is stored.
- `attachments`: `[{ id, filename, content_type, disposition, content_id, size, sha256, source, upload_id }]`:
  `source` is `inline` (bytes given as base64) or `upload` (copied from an upload when the revision was
  saved; `upload_id` records which); either way the draft holds its own copy.
- `labels`: labels the sent message will carry (`^[a-z0-9][a-z0-9_:-]{0,63}$`, at most 64).
- `metadata`: as on messages (16 keys, 512 bytes per value); copied to the message.
- `send_at`: the time the draft should be sent, or `null` for "when sent". Setting it alone schedules
  nothing ([Drafts §6](../project/design/drafts.md#6-scheduled-send)).
- `approval`: `{ state, reason, requested_at, requested_by_key_id, approved_revision, approved_digest,
  approved_at, approved_by_key_id, approved_by_user_id, on_behalf_of, note }`: `approved_digest` is the
  digest the approval covers; `state` is `none`, `requested` or
  `approved`; `reason` says why approval was requested (`requested` by the writer, or the policy level
  `new_recipients`, `external`, `always`).
- `send`: the pending or last send request: `{ origin, requested_at, requested_by_key_id,
  requested_by_user_id, due_at, labels_add, labels_remove }`, `origin` = `api`, `approval` or `console`;
  `null` when none.
- `rejection`: `{ reason, note, rejected_at, rejected_by_key_id, rejected_by_user_id, on_behalf_of }`,
  `reason` = `operator` or `expired`; `null` unless rejected.
- `failure`: `{ reason, detail, failed_at, attempts }` while `failed` ([Errors › Draft failures](errors.md#draft-failures)).
- `message_id`, `sent_at`: set when the draft became a message.
- `created_by_key_id`, `updated_by_key_id`: the keys that created the draft and wrote its current revision
  (`null` for a console write, which never edits content).
- `content_by_key_id`: the key that last changed the content (the create, or a `PATCH` that changed the
  digest); it may not approve the draft (`self_approval`).
- `created_at`, `updated_at`: bookkeeping times.

Every text field of a draft was written by an agent that mail may have steered: show it as untrusted
content.

---

## Uploads

An upload stages one file for one identity: create it with the file's name, type, size and SHA-256,
`PUT` the bytes to its `upload_url`, then attach it by `upload_id` to sends, replies, reply-alls,
forwards and drafts of the same identity until `expires_at`. Each message copies the bytes, so an
upload can be reused and its expiry never changes a sent message. There is no download route. Pylota
Mail never fetches attachments from URLs.

The four routes below need **either** `messages:send` **or** `drafts:write`, at every key level within its
scope (an identity key for its own identity). The design is [Uploads](../project/design/uploads.md).

### `POST /v1/identities/{identity_id}/uploads` — `messages:send` or `drafts:write`

```json
{
  "filename": "FNOL-7781.pdf",
  "content_type": "application/pdf",
  "size": 9437184,
  "sha256": "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae",
  "expires_in": 86400
}
```

Returns `201` with the [upload](#upload-object), `pending`, with `upload_url`. `Idempotency-Key` is
optional; a replay re-reads the upload and mints a new `upload_url`. In order:

1. A missing or foreign identity is `404 identity_not_found`.
2. Body: `filename` 1–255 bytes after sanitising, `content_type` a valid `type/subtype` of at most 127
   bytes, `size` an integer 1–26,214,400, `sha256` 64 hexadecimal characters, `expires_in` 900–604,800
   seconds (default 86,400). Each failure is `400 invalid_request` with `details.errors[]`.
3. A suspended tenant is `403 tenant_suspended`; a paused identity `409 identity_paused` with
   `details.reason`.
4. While the workspace's `storage_gb` is at or over its allowance: `402 billing_limit`
   (`details.feature = "storage_gb"`).
5. The identity already has 100 live uploads (`pending`, `uploading`, `ready`), or their declared sizes
   plus this one would pass 524,288,000 bytes: `422 upload_limit_reached` with `details.limit` (`count`
   or `bytes`), `details.used` and `details.max`.
6. `expires_at` is `created_at + expires_in`, capped by the tenant's `retention.message_days` and the
   identity's `retention.message_hours` when they are set. The response shows the effective value.

Then send the bytes with [`PUT /v1/links/{token}`](#put-v1linkstoken) to `upload_url`.

### `GET /v1/identities/{identity_id}/uploads` · `GET …/uploads/{upload_id}` — `messages:send` or `drafts:write`

The list returns the identity's uploads, newest first, with `limit`, `cursor` and an optional `status`
filter. Tombstones (`rejected`, `expired`) are listed while they exist. Lists never carry `upload_url`.
`GET` of one returns the upload; while it is `pending` or `uploading` it carries a freshly minted
`upload_url`. A missing, foreign or deleted upload is `404 upload_not_found`. No route returns an upload's
bytes; a sent message's attachments are read with
[`GET …/attachments/{attachment_id}`](#get-v1identitiesidentity_idmessagesmessage_idattachmentsattachment_id--attachmentsread).

### `DELETE /v1/identities/{identity_id}/uploads/{upload_id}` — `messages:send` or `drafts:write`

Returns `204`. Deletes the upload and its bytes, in any state. A send that is attaching it at that moment
makes the delete answer `409 upload_in_use` (retryable, `Retry-After: 5`); a `PUT` in progress loses its
claim. A missing upload is `404 upload_not_found`. Messages and drafts that copied the upload keep their
copies.

#### Upload object

```json
{
  "id": "upl_01JAC3Q7W9X2M5N6P8R0T1Y4ZB",
  "identity_id": "idn_01J9Z4K8V4QW7X2M5N6P8R0T1Y",
  "filename": "FNOL-7781.pdf",
  "content_type": "application/pdf",
  "size": 9437184,
  "sha256": "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae",
  "status": "pending",
  "status_reason": null,
  "sniffed_type": null,
  "risk": null,
  "scan_status": "skipped",
  "use_count": 0,
  "last_used_at": null,
  "created_at": "2026-10-17T09:00:00Z",
  "expires_at": "2026-10-18T09:00:00Z",
  "upload_url": "https://mail.example.com/v1/links/bDE6YTp1cGxvYWQ6dGVuXzAxSjk…",
  "upload_url_expires_at": "2026-10-17T10:00:00Z"
}
```

| Field | Meaning |
|---|---|
| `id` | `upl_` + ULID. This is the value a send passes as `attachments[].upload_id` |
| `filename` | Sanitised as attachment filenames are (no path components, no control characters, at most 255 bytes). Untrusted content |
| `content_type` | As declared: a valid `type/subtype`, at most 127 bytes |
| `size` | Declared bytes, 1–26,214,400. The bytes must match it exactly |
| `sha256` | Declared SHA-256 of the bytes, 64 hexadecimal characters; stored and returned lower case |
| `status` | `pending` (no verified bytes yet), `uploading` (one `PUT` holds the claim), `ready` (the bytes have the declared size and SHA-256, were classified and carry no quarantining risk; only `ready` uploads can be attached), `rejected` (a quarantining risk, or the scanner said `infected`; the bytes are deleted) or `expired` |
| `status_reason` | For `rejected`: `risk` or `infected`. For `expired`: `expired`, `too_many_attempts` or `object_missing`. Otherwise `null` |
| `sniffed_type`, `risk` | Set when the bytes are classified; `risk` is one of the attachment risk values or `null` |
| `scan_status` | `skipped` (no scanner configured), `clean`, `infected` or `error` |
| `use_count`, `last_used_at` | Accepted messages that copied this upload, and when the last one did |
| `expires_at` | When the upload and its bytes are deleted, whether or not it was used. A `rejected` or `expired` upload stays as a tombstone for 7 days, so a late attach gets `422 attachment_risky` or `410 upload_expired`, then `404 upload_not_found` |
| `upload_url`, `upload_url_expires_at` | Present only in the create response and in `GET` of one upload while it is `pending` or `uploading`; `null` otherwise and never in lists. A new URL is minted on every such response, valid for at most one hour |

**Attaching an upload.** On send, reply, reply-all and forward (and drafts), an attachment item has
exactly one of `content_base64` and `upload_id`; neither or both is `400 invalid_request`
(`details.errors[0].path = "attachments[{i}]"`). With `upload_id`, `filename` is optional (the upload's by
default), `content_type` is not accepted, and `disposition` and `content_id` are as for inline items. An
upload of another identity, of another tenant, or one that does not exist is `404 upload_not_found` with
`details.index`, the same answer in every case.

---

## Search

### `POST /v1/identities/{identity_id}/search` — `search:read` (`search:agentic` for `mode: "agentic"`)

```json
{
  "q": "from:@brightwell.example ref:AB12CDE has:attachment newer_than:45d",
  "mode": "hybrid",
  "filters": { "direction": "inbound", "labels": [], "after": null, "before": null },
  "group_by": "message",
  "limit": 10,
  "snippet_chars": 240,
  "facets": true,
  "include_quarantined": false,
  "cursor": null
}
```

The operators, modes and ranking are explained in [Search](../guides/search.md).

```json
{
  "query": { "parsed": "from:@brightwell.example ref:AB12CDE has:attachment newer_than:45d", "mode": "hybrid" },
  "hits": [{
    "message_id": "msg_01J…", "thread_id": "thr_01J…", "identity_id": "idn_01J…",
    "date": "2026-09-14T08:12:00Z", "direction": "inbound",
    "from": { "name": "Brightwell Leeds", "address": "accounts@brightwell.example" },
    "subject": "Invoice 88213 – AB12 CDE",
    "snippet": "…brake pads and discs, total £412.80 inc VAT…",
    "score": 0.913,
    "why": ["ref:AB12CDE (attachment p.1)", "from:brightwell.example", "type:pdf"],
    "attachment_hits": [ { "attachment_id": "att_…", "filename": "INV-88213.pdf", "page": 1 } ],
    "trust": { "verdict": "pass", "known_sender": true, "quarantined": false }
  }],
  "facets": {
    "sender": { "accounts@brightwell.example": 3 }, "sender_domain": { "brightwell.example": 3 },
    "month": { "2026-09": 2, "2026-08": 1 },
    "label": { "invoice": 3 }, "attachment_type": { "pdf": 3 }, "category": { "billing": 3 }
  },
  "next_cursor": null, "truncated": false, "semantic_coverage": 0.998, "degraded": false,
  "as_of": "2026-10-09T10:12:00Z"
}
```

With `group_by: "thread"`, `hits` has one row per thread. Each row has `thread_id`, `subject`,
`participants`, `message_count`, `last_at`, the best `snippet` and `why`, and `top_message_id`.

`facets` has six keys: `sender` (the from address), `sender_domain`, `month` (in the tenant's time zone),
`label`, `attachment_type` and `category`. Each lists the top 10 values by count (`month`: the 24 most
recent months). Facets are computed on the first page only: they are `null` on later pages and when the
request sets `facets: false`.

#### Agentic mode

```json
{ "q": "Did the insurer accept the Golf claim after we sent the photos?", "mode": "agentic",
  "budget": { "max_steps": 6, "max_seconds": 8 }, "stream": false }
```

```json
{
  "status": "answered",
  "answer": {
    "text": "Yes. Admiral accepted claim 7781 on 2 October, after the photos sent on 28 September [msg_01JA…][msg_01JB…].",
    "sentences": [ { "text": "Yes. Admiral accepted claim 7781 on 2 October…", "citations": ["msg_01JA…", "msg_01JB…"],
                     "citation_trust": "authenticated" } ],
    "confidence": 0.86,
    "untrusted": true
  },
  "evidence": [ { "...": "search hits, as above, with quotes and steering_suspected",
                  "quotes": [ "we are pleased to confirm claim 7781 has been accepted" ], "steering_suspected": false } ],
  "trace": [
    { "step": 1, "action": "search", "q": "claim Golf photos", "mode": "hybrid", "hits": 7, "ms": 412 },
    { "step": 2, "action": "read_thread", "thread_id": "thr_01JA…", "ms": 38 },
    { "step": 3, "action": "answer", "removed_sentences": 0 }
  ],
  "degraded": false,
  "usage": { "steps": 3, "ms": 2810, "model": "@cf/qwen/qwen3.8-27b" }
}
```

- `status` is one of `answered`, `insufficient_evidence`, `budget_exhausted` (evidence returned, no
  answer or a partial one) or `degraded` (hybrid results only, no answer).
- The answer is model-written text derived from mail (FR-SRCH-13, [F17](../project/edge-cases.md)):
  `answer.untrusted` is always `true`. Each sentence's `citation_trust` is `authenticated` (every cited
  message is outbound or passed authentication) or `partly_authenticated`; a sentence that cites a
  `steering_suspected` message, or no authenticated message, is removed before the response. Treat the
  answer as data to check against `evidence`, never as an instruction.
- Search requests ignore `Idempotency-Key` and are never stored ([Idempotency](#idempotency)). The tenant's
  daily agentic budget counts a request once, after it is validated.
- When tenant policy turns agentic search off, `mode: "agentic"` fails with `422 agentic_disabled`, on
  this endpoint and on tenant search.
- With `stream: true` and `Accept: text/event-stream`, the response is a server-sent event stream:
  `event: step` (each trace entry), `event: evidence` (hits as they are found), `event: answer` and
  `event: done`. A keep-alive comment is sent after every 10 seconds of silence.

### `POST /v1/tenants/{tenant_id}/search` — tenant, partner or platform key, `search:read`

The same body, plus an optional `identity_ids` filter. Runs across every identity of the tenant (up to
100; more returns `422 scope_too_large`), never the deployment's system identity. A partner key reaches
only tenants whose `partner_id` is its partner. Hits carry `identity_id`, and facet counts are summed across
identities. `mode: "agentic"` with agentic search off returns `422 agentic_disabled`.

The response adds two fields, always present: `partial` and `failed_identities` ([F15](../project/edge-cases.md)).
Each identity's mailbox has 900 ms from the start of the fan-out to answer. One that errors or misses the
deadline is listed in `failed_identities`, `partial` is `true`, and its late result is discarded. When
every identity answered, they are `false` and `[]`.

```json
{ "query": { "...": "as above" }, "hits": [ "..." ], "facets": { "...": "summed" },
  "next_cursor": null, "truncated": false, "semantic_coverage": 0.994, "degraded": false,
  "as_of": "2026-10-09T10:12:00Z", "partial": true, "failed_identities": ["idn_01JA…"] }
```

### `GET /v1/identities/{identity_id}/messages/{message_id}/related` — `search:read`

Query: `limit` (default 10, max 50). Returns semantically similar messages from other threads, as search hits.

### `GET /v1/identities/{identity_id}/contacts` — `search:read`

Query: `q` (name, address or domain prefix), `limit`, `cursor`.

```json
{ "data": [ { "address": "claims@admiral.example", "name": "Admiral Claims", "domain": "admiral.example",
  "first_seen_at": "…", "last_seen_at": "…", "inbound_count": 6, "outbound_count": 4,
  "last_thread_id": "thr_01JA…" } ], "next_cursor": null }
```

### `GET /v1/identities/{identity_id}/wait` — `search:read`

Long-polls until a matching message arrives after the request started (or after `since`).

Query parameters:

- `from`: an address or `@domain`;
- `subject_contains`;
- `thread_id`;
- `kind`: `any`, `reply` or `verification`;
- `since`;
- `timeout`: seconds, default 30, max 60.

```json
{ "message": { "...": "Message object or null on timeout" },
  "verification": { "code": "481 207", "link": "https://service.example/verify?t=…", "sender_domain": "service.example" },
  "timed_out": false }
```

A verification code or link is released only when `from` names the expected sender domain and the
message passed authentication (`verdict: pass`). See [E4](../project/edge-cases.md). When the tenant's
`accounts.require_approval` is `true`, `kind=verification` also needs an approved
[service-ledger entry](#service-accounts) for the sender domain: without one the request gets
`403 policy_denied` with `details.reason = "account_not_approved"` at once ([E9](../project/edge-cases.md)),
and a code is released only while its entry is still approved. The handler polls
the mailbox every second and keeps the sender domain registered for unsolicited-OTP detection while it
waits; the full behaviour is in [Inbound › The `wait` handler](../project/design/inbound.md#the-wait-handler-e4).
A message still being screened is never returned; it is returned once it is `received`.

---

## Screening

### `GET /v1/identities/{identity_id}/screening/summary` · `GET /v1/tenants/{tenant_id}/screening/summary` — `metrics:read`

Verdict counts, reasons, outcomes, feedback, and precision and recall from feedback, for records started in
`from`–`to` (default the last 30 days, at most 92). The tenant route takes `identity_ids` and needs a tenant,
partner or platform key (`403 scope_denied` for an identity key); it reports `partial` and `failed_identities`
like tenant search, and refuses more than 100 identities without `identity_ids` (`422 scope_too_large`).
See [Screening hold §12](../project/design/screening.md#12-summary-precision-and-recall-per-tenant).

---

## Quarantine

### `GET /v1/identities/{identity_id}/quarantine` — `quarantine:review`

Quarantined messages, newest first, with `quarantine_reason`.

Releasing a message is `POST …/messages/{message_id}/release` (above).

---

## Webhooks

The event types and payloads are in [Webhook events](events.md).

Reads (`GET`) need `webhooks:read`; every other webhook route needs `webhooks:manage`, which includes
`webhooks:read`.

### `POST /v1/webhooks` (platform or partner key) · `POST /v1/tenants/{tenant_id}/webhooks` — `webhooks:manage`

```json
{ "url": "https://api.example.com/webhooks/mail", "events": ["message.received", "message.bounced"],
  "identity_ids": null, "description": "Production API",
  "headers": { "X-Ingress-Token": "t0k_8f2k3m9q…" } }
```

Returns `201` with the endpoint and `"secret": "whsec_…"`. **The secret is shown only once**: an
idempotent replay returns the body with `"secret_replayed": false` instead ([Idempotency](#idempotency)).
`events: ["*"]` subscribes to every event type that is not opt-in, including types added later; the opt-in
type `message.labels_changed` must be named.

`headers` (optional) adds up to 10 static request headers to every delivery, values together at most
4,096 bytes. Values are write-only: the endpoint shows `header_names` only. A refused name gets
`400 header_not_allowed` (`details.reason = "webhook_header"`); too many or too long,
`422 webhook_header_limit_reached` ([Parity additions § 8](../project/design/parity-additions.md#8-webhook-delivery-headers-fr-wh-7)).

An endpoint's `scope` says whose events it receives:

| `scope` | Created by | Receives |
|---|---|---|
| `platform` | `POST /v1/webhooks` with a platform key | Every tenant's events |
| `partner` | `POST /v1/webhooks` with a partner key (`partner_id` is set) | Only the events of tenants whose `partner_id` is its partner's |
| `tenant` | `POST /v1/tenants/{tenant_id}/webhooks` | Its tenant's events |

A tenant, a partner and the platform can each have at most 20 endpoints; on both routes, the 21st returns
`422 webhook_limit_reached`. `webhook.disabled` about an endpoint of a partner (a partner endpoint, or a
tenant endpoint of one of its tenants) goes to that partner's other endpoints and to platform endpoints,
never to tenant endpoints ([Webhook events](events.md#privacy-platform-and-webhooks)). While a partner
is suspended, deliveries to its endpoints and its tenants' endpoints are held, and they are delivered
within a minute of its reactivation. Events of the deployment's system identity go to platform endpoints
only.

An endpoint is a way to read mail, so `webhooks:manage` alone is not enough for mail events
([J29](../project/edge-cases.md)): creating an endpoint, changing its `url` or `events`, or replaying to it
needs `messages:read` as well when its `events` include `"*"`, any `message.*` type,
`verification.received` or `calendar.received`, and `quarantine:review` as well when they include `"*"`,
`message.quarantined` or `message.screened`. Otherwise `403 permission_denied` with `details.required`.

### `GET /v1/webhooks` · `GET /v1/tenants/{tenant_id}/webhooks` · `GET|PATCH|DELETE /v1/webhooks/{webhook_id}`

`GET` needs `webhooks:read`; `PATCH` and `DELETE` need `webhooks:manage`. `GET /v1/webhooks` lists the
platform endpoints for a platform key, the partner's endpoints for a partner key, and the tenant's
endpoints for a tenant or identity key. A partner key reaches its partner's endpoints and its tenants'
endpoints by ID; any other endpoint is `404 webhook_not_found` to it.

`PATCH` accepts `url`, `events`, `identity_ids`, `description` and `enabled`. A changed `url` disables the
endpoint with `disabled_reason: "url_changed"` until a test delivery to the new URL succeeds
(`POST …/test`), which re-enables it; `enabled: true` on such an endpoint gets `400 invalid_request`. A
`url` on another host removes every custom header in the same update.

### `PATCH /v1/webhooks/{webhook_id}/headers` — `webhooks:manage`

```json
{ "set": { "X-Ingress-Token": "t0k_new…" }, "remove": ["Authorization"] }
```

Returns `200` with the endpoint, whose `header_names` lists the names in their stored casing. Values are
never returned, logged or exported.

- **Names**: 1–64 characters matching `^[A-Za-z0-9][A-Za-z0-9-]*$`, compared case-insensitively. A name that
  fails the pattern, or is one of the following, is refused with `400 header_not_allowed`
  (`details.reason = "webhook_header"`, `details.name`): the forbidden request-header names of the Fetch
  standard (`Accept-Charset`, `Accept-Encoding`, `Access-Control-Request-Headers`,
  `Access-Control-Request-Method`, `Connection`, `Content-Length`, `Cookie`, `Cookie2`, `Date`, `DNT`,
  `Expect`, `Host`, `Keep-Alive`, `Origin`, `Referer`, `Set-Cookie`, `TE`, `Trailer`, `Transfer-Encoding`,
  `Upgrade`, `Via`) and every name starting `Proxy-` or `Sec-`, and `X-HTTP-Method`,
  `X-HTTP-Method-Override` and `X-Method-Override`; the headers the service sets (`Content-Type`,
  `Content-Encoding`, `User-Agent` and every name starting `webhook-`); names a receiver's proxy may trust
  as the client's address (`Forwarded`, `X-Real-IP`, `True-Client-IP` and every name starting
  `X-Forwarded-` or `CF-`, except `CF-Access-Client-Id` and `CF-Access-Client-Secret`); and every name
  starting `X-Pylota-`. `Authorization` is allowed: a static bearer token is the main use. So are
  `CF-Access-Client-Id` and `CF-Access-Client-Secret`, the two headers of a Cloudflare Access service token.
- **Values**: 1–1,024 bytes of `0x20`–`0x7E`, not starting or ending with a space, otherwise
  `400 invalid_request` (path `headers.{name}` or `set.{name}`).
- **Counts**: at most 10 headers and 4,096 bytes of values per endpoint, after the change, otherwise
  `422 webhook_header_limit_reached` (`details.limit`, `details.reason`: `count` or `bytes`).
- The request needs at least one entry in `set` or `remove` (`400 invalid_request`); a name in both, in any
  casing, or twice in `set` in different casings, is `400 invalid_request`; removing a name that is not set
  changes nothing. `set` replaces a value atomically. After three concurrent changes won, the request gets
  `409 request_in_progress`; retry. Every change writes the audit row `webhook.headers_update`, with names
  only.

### `POST /v1/webhooks/{webhook_id}/rotate-secret`

`{ "overlap_hours": 24 }` (0–168). Returns the new secret once. During the overlap, deliveries carry
both signatures.

### `POST /v1/webhooks/{webhook_id}/test`

Sends a `webhook.test` event straight away and returns the delivery attempt. A `2xx` re-enables an
endpoint disabled with `url_changed`.

### `GET /v1/webhooks/{webhook_id}/deliveries` — `webhooks:read`

Filters: `status` (`succeeded`, `failed`, `dead`), `event_type`, `after`.

### `POST /v1/webhooks/{webhook_id}/replay`

```json
{ "event_ids": ["evt_01J…"] }
```

or

```json
{ "since": "2026-10-08T00:00:00Z", "until": "2026-10-09T00:00:00Z", "status": "dead" }
```

An event can be replayed for 30 days from its `occurred_at` (or `retention.events_days`, if shorter,
because its payload is gone after that). The window never starts from when a delivery went `dead`, and
older events are not queued. Returns `202` with `{ "queued": 42 }`.

---

## Event feed

Every event of a tenant, the same envelopes that webhooks deliver, in one ordered log per tenant
([Event feed and live stream](../project/design/event-feed.md)). Every route needs `events:read`. Tenant and
identity keys read their own tenant; platform and partner keys pass `tenant_id` (`400 invalid_request`
without it, `404 tenant_not_found` for a tenant outside their scope). A tenant that is being erased has no
feed (`404 tenant_not_found`). A key sees an event only when it could read its subject: mail events need
`messages:read`, `message.quarantined` and `message.screened` also `quarantine:review`, and identity keys
see only their own identity's events; types a key may not see are listed in `withheld_types`, never
refused. Webhook endpoint filters do not apply. Responses carry `Cache-Control: no-store`.

Positions follow the order in which events were appended, not `occurred_at`. Use each envelope's
`sequence` to order events of one owner, and deduplicate on `event.id` across webhooks, the feed and the
stream.

### `GET /v1/events` — `events:read`

Query: `tenant_id`; `after` (a cursor, `earliest` or `latest`; default `earliest`) or `since` (RFC 3339);
`types` (comma-separated, at most 50: `*`, `message.*`, `message.received`); `identity_id`
(comma-separated, at most 100); `limit` (1–500, default 100); `wait` (0–60 seconds, default 0).

```json
{ "data": [ { "cursor": "ec_…", "event": { "id": "evt_01JA…", "type": "message.received", "…": "…" } } ],
  "next_cursor": "ec_…", "has_more": false, "withheld_types": [], "since_truncated": false }
```

`event` is the webhook envelope, byte for byte. `next_cursor` is never null; pass it as `after` to
continue. A page can be empty with `has_more: true`: ask again at once. With `wait`, the request waits
until a matching event is appended or the time is up. Cursors do not expire with age: a cursor behind the
retained log (`retention.events_days`, or the log's size budget) gets `410 cursor_expired` with
`details.oldest_cursor`. A `since` never expires, but when retention or the size budget already removed
events appended after it, the page has `since_truncated: true`. At most 5 long polls per key and 20 per
tenant at once (`429 rate_limited`, `details.bucket: "events_wait"`).

### `POST /v1/event-consumers` · `GET /v1/event-consumers` · `GET|DELETE /v1/event-consumers/{consumer_id}` — `events:read`

A durable consumer keeps a named position in the log for you.

```json
{ "name": "pylota-reconcile", "types": ["message.*"], "identity_ids": null, "start": "latest" }
```

Returns `201` with the consumer, or `200` with the existing one when the same key repeats the same name and
filters; another key or other filters get `409 event_consumer_name_taken`. At most 20 per tenant
(`422 event_consumer_limit_reached`). `types` that name events the key can never see get
`403 permission_denied` with `details.required`. `start` is `latest` (default), `earliest` or a cursor.
A consumer is bound to the key that created it. Identity keys list and get their own consumers; tenant,
partner and platform keys list, get and delete every consumer of the tenant. Only the owner key reads its
events, commits and resets it (below).

```json
{ "id": "evc_01JA…", "name": "pylota-reconcile", "tenant_id": "ten_01J9…", "owner_key_id": "key_01J9…",
  "types": ["message.*"], "identity_ids": null, "committed_cursor": "ec_…", "committed_at": null,
  "last_read_at": null, "positions_behind": 0, "state": "active", "created_at": "…", "updated_at": "…" }
```

### `GET /v1/event-consumers/{consumer_id}/events` · `POST …/commit` · `POST …/reset` — `events:read`, owner key

`GET …/events` (`limit`, `wait`) returns the events after the committed position, as `GET /v1/events`, plus
`committed_cursor`; until you commit, the same events come back. To work through a backlog, commit each
page's `next_cursor` (also an empty page's) before reading the next, while `has_more` is true; a read takes
no `after`. `POST …/commit { "cursor": "ec_…" }` acknowledges up to that cursor and only moves forward. When the position falls behind the retained log,
these routes get `410 cursor_expired` (with `details.consumer_id`) until
`POST …/reset { "to": "earliest" | "latest" | "ec_…" }` or `{ "since": "…" }`. Other keys get
`403 permission_denied` (`details.reason: "consumer_owner_only"`), or `404 event_consumer_not_found` for an
identity key.

### `POST /v1/events/stream-tokens` — `events:read`

Returns `201` with `{ "token", "expires_at", "url", "protocols" }`: a single-use token valid for 60 seconds,
for a browser that cannot send `Authorization`. Mint it on your server and hand it to your page. At most 100
live (unredeemed and unexpired) per tenant (`429 rate_limited`, `details.bucket: "stream_tokens"`).
`Idempotency-Key` is ignored.

### `GET /v1/events/stream` — `events:read`, WebSocket

Upgrade with `Authorization: Bearer …`, or with the subprotocols `pylota-identity.events.v1` and
`pylota-identity.token.{token}`. Never put a key or token in the URL (`400 invalid_request`,
`details.reason: "credential_in_url"`). Send `{"type":"subscribe","after":"ec_…"}` (or `since`, or
`consumer_id`) within 60 seconds; receive `subscribed`, `events`, `heartbeat` and `error` messages; send
`ack` with a cursor (optional below 1,000 events per connection) and `{"type":"ping"}`. At most 100
connections per tenant and 10 per key. Reconnect only after `1001`, `1006`, `1011`, `1012`, `1013`, `4408`
or a code you do not know, retry `4429` after 30 seconds, reconcile after `4410`, and stop on every other
close: after `4409` another connection holds your place, and reconnecting would replace it in turn. The
protocol, the close codes and resuming without loss are in
[Event feed › The WebSocket stream](../project/design/event-feed.md#7-the-websocket-stream); the close codes
are also in [Errors › Stream close codes](errors.md#stream-close-codes). A `GET` without
`Upgrade: websocket` gets `426 upgrade_required`.

---

## Suppressions and lists — `suppressions:manage`

### `GET /v1/tenants/{tenant_id}/suppressions`

Query: `address` (exact lookup), `reason`. Items show `address_hint` (masked), `reason`, `created_at`
and `expires_at`.

### `POST /v1/tenants/{tenant_id}/suppressions`

`{ "address": "jo@example.net", "reason": "manual", "note": "Asked not to be contacted" }`

### `DELETE /v1/tenants/{tenant_id}/suppressions/{address}`

Removes a `manual`, `unsubscribe`, `hard_bounce` or `provider` suppression. Removing a `complaint`
suppression needs `"confirm_complaint_removal": true` in the body and is audit-logged (`suppression.remove`).

### `GET|PUT|DELETE /v1/tenants/{tenant_id}/lists/{direction}/{kind}/{entry}`

`direction` is `receive` or `send`, `kind` is `allow` or `block`, and `entry` is `user@example.com` or
`@example.com`. `GET /v1/tenants/{tenant_id}/lists/{direction}/{kind}` lists the entries.

- **Receive-block**: mail is stored hidden and never shown to agents.
- **Receive-allow**: mail skips spam quarantine. It does not skip authentication quarantine.
- **Send-block**: a listed recipient is not sent to. The send is accepted and that recipient's delivery
  is `suppressed` with `policy: send_block`; a dry run reports `422 recipient_blocked`.
- **Send-allow**: with `policy.send_allowlist_only`, only listed recipients are sent to; the others are
  `suppressed` with `policy: not_on_allowlist`.

Entries can also belong to one identity; for that identity's mail its own entries override the tenant's of
the same or a coarser granularity (an identity `@domain` entry never overrides a tenant entry for the exact
address), and among the entries left block wins over allow
([Parity additions § 3.1](../project/design/parity-additions.md#31-scopes-and-precedence)). List entries carry `scope` (`tenant` or `identity`) and `identity_id`. Each scope holds at most
10,000 entries and a tenant 50,000 (`422 list_limit_reached`). A dry run's suppressed recipient carries
`scope` beside `reason: "send_block"`.

### `GET|PUT|DELETE /v1/identities/{identity_id}/lists/{direction}/{kind}/{entry}`

`GET /v1/identities/{identity_id}/lists/{direction}/{kind}` lists the identity's entries, and
`GET|PUT|DELETE /v1/identities/{identity_id}/lists/{direction}/{kind}/{entry}` reads, adds or updates, and
removes one, all with `suppressions:manage`. `direction`, `kind`, `entry` and the bodies are those of the
tenant routes. Platform, partner and tenant keys only: identity keys cannot hold `suppressions:manage`, so an
agent can never change its own lists, and an identity key gets `403 permission_denied`
(`details.required: ["suppressions:manage"]`).

- `PUT` adds the entry, or updates its `note` when it exists, and answers `200` with it. `DELETE` answers
  `204`, or `404 list_entry_not_found` for an entry that does not exist.
- The list entry gains `scope` (`tenant` or `identity`) and `identity_id` (`null` for tenant scope). The
  tenant routes are unchanged and read and write tenant-scope entries only.
- The system identity: `404 identity_not_found` for every key but a platform key, which gets
  `400 invalid_request` with `details.reason = "system_identity"` (its sign-in mail must never be filtered
  by a list).

```json
{ "scope": "identity", "identity_id": "idn_01JA9K…", "direction": "send", "kind": "allow",
  "entry": "seller.4471@marketplace.example", "note": "operator_approved_seller acq_01JA…",
  "created_at": "2026-10-17T10:05:00Z" }
```

---

## API keys — `keys:manage`

### `POST /v1/keys`

```json
{ "name": "bookings-agent", "level": "identity", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…",
  "permissions": ["messages:read", "messages:send", "search:read", "attachments:read", "identities:sign"],
  "mcp_tools": null, "expires_at": "2027-10-09T00:00:00Z" }
```

The new key's level, tenant, identity and permissions must all lie within the caller's own, otherwise
`403 key_scope_exceeded`. The one exception: a platform or partner key may grant `identities:sign`, which
it cannot hold, to a tenant or identity key it mints; the `key.create` audit row records it in
`details.granted_without_holding`. A tenant key's `mode` follows its tenant; platform and partner keys are
`live`. Returns `201` with `"secret": "pmk_live_…"`, shown only once: an idempotent replay returns the
body with `"secret_replayed": false` instead ([Idempotency](#idempotency)).

- **Who minted it.** `created_by_user_id` in the key object is the console user behind the key: the person
  whose console session minted it, copied to every key minted with that key. It is `null` for keys minted
  by a platform or partner key with no person behind it. Removing that person from the workspace, or
  lowering their role, revokes their keys ([Members](#members)).
- **Limits.** A tenant has at most 100 active (neither revoked nor expired) tenant and identity keys, and a
  partner at most 10 partner keys: the next mint gets `422 key_limit_reached` with `details.limit`.
- **MCP tools.** `mcp_tools` (optional) lists the MCP tools the key may see, at most 64 distinct names.
  It narrows what the key's permissions and level allow and never widens them. Absent or `null` means
  every allowed tool; `[]` means none. An unknown name is `400 invalid_request` with
  `details.reason = "unknown_mcp_tool"`; a tool the new key could not see with its permissions and level
  is `details.reason = "mcp_tool_not_permitted"` (both with `details.tool`). Rotation keeps the list;
  keys minted by this key do not inherit it. It limits MCP only: REST access follows the permissions
  ([SDKs, skills and agent tooling §5.3](../project/design/sdk-and-skills.md#53-per-key-tool-allow-list)).

- **Partner keys.** `level: "partner"` needs `partner_id` and no `tenant_id` or `identity_id`, and only
  a platform key may ask for it ([Partner keys](#partner-keys)); an unknown partner is
  `404 partner_not_found`. Other levels refuse `partner_id` (`400 invalid_request`). A partner key mints
  only `tenant` and `identity` keys of its own tenants: a `partner` or `platform` key, or another tenant,
  is `403 key_scope_exceeded`.

- `permissions` is required at every level, `platform` included. There is no implicit full set: a
  missing or empty list returns `400 invalid_request`.
- Each permission must be one the new key's level can hold ([Permissions](#permissions)), whoever the
  caller is, otherwise `400 invalid_request` with `details.reason = "permission_not_allowed_for_level"`: `platform:ops` and
  `partners:manage` only on platform keys; `tenants:manage` only on platform and partner keys;
  `members:read`, `members:manage`, `suppressions:manage`, `audit:read` and `usage:read` never on
  identity keys; `identities:sign` never on platform or partner keys; `tenants:erase` never on identity
  keys.
- `tenants:erase` on a tenant key needs a caller whose `created_by_role` is `owner` (the workspace owner's
  console session, or a key it minted): otherwise `400 invalid_request` with
  `details.reason = "permission_owner_only"`. Admins and the keys they mint can never delete the workspace.
- These checks come before the scope check, so a refused permission is `400`, not `403`.

### `GET /v1/keys` · `GET /v1/keys/{key_id}` · `DELETE /v1/keys/{key_id}`

`DELETE` revokes the key immediately. Minting and revoking are audit-logged (`key.create`, `key.revoke`).

Reading, revoking and rotating a key follow two rules ([J20](../project/edge-cases.md)):

- **Reach.** A platform key reaches every key; a partner key, the tenant and identity keys of its own
  tenants (a partner or platform key ID, its own included, is `404 key_not_found` to it); a tenant key, the
  keys of its tenant; an identity key, only the identity keys of its own identity. A key out of reach is
  `404 key_not_found`, like a missing one.
- **Not wider.** The target's level must be at most the caller's, and each of its permissions one the
  caller holds (or, for a platform or partner caller, `identities:sign`). Otherwise
  `403 key_scope_exceeded`, and nothing is changed or returned. A key always passes for itself.
- **Provider-managed** (rotate and revoke). A key a platform or partner key minted (`created_by_level`
  `platform` or `partner`) can be rotated or revoked only by a platform key or a partner key that reaches
  it; other callers get `403 key_scope_exceeded` with `details.reason = "provider_managed"`
  ([W48](../project/edge-cases.md)). A key always passes for itself.

`GET /v1/keys` lists only the keys that pass both rules.

### `POST /v1/keys/{key_id}/rotate`

`{ "overlap_hours": 24 }` (0–168). Returns a new secret. The old one keeps working until the overlap ends.
The caller must reach the key and not be narrower than it, as above, so no key can obtain a wider key's
secret. The key's `created_by_user_id` and `created_by_role` become the caller's (its own values, or the
console person; `null` for a platform or partner caller), so the new secret is traced to whoever received
it ([W48](../project/edge-cases.md)).

---

## Privacy — `erasure:manage`

### `POST /v1/erasure-requests`

```json
{ "tenant_id": "ten_01J9…", "scope": "counterparty", "counterparty_address": "jo@example.net",
  "reason": "Data subject request DSR-1182" }
```

| `scope` | Also needs | Deletes |
|---|---|---|
| `message` | `identity_id`, `message_id` | One message, its attachments, text, index rows, vectors, raw copies |
| `thread` | `identity_id`, `thread_id` | Every message in the thread |
| `counterparty` | `counterparty_address` | Every message to or from that address, in every identity of the tenant |
| `identity` | `identity_id` | The whole mailbox and the identity's signing keys. Its addresses and key IDs are tombstoned |
| `tenant` | none | Everything in the tenant, every identity's signing keys included (their key IDs are tombstoned). Then the tenant is marked `erased`, once no held thread remains. Needs `tenants:erase` as well as `erasure:manage` (`403 permission_denied` with `details.required: ["tenants:erase"]` otherwise) |

Held threads are skipped and listed in the receipt (FR-PRV-4): an erasure request is never refused
because of a hold (it never returns `423 legal_hold`). The request's `status` is `queued`, `running`,
`completed`, `completed_with_holds` (finished, but at least one held thread was skipped), `failed`, or
`canceled` (a tenant erasure superseded it). Returns `202` with the object below. An erasure that keeps
failing is retried automatically until 20 hours after the request (`deadline_at` − 4 hours), then ends
`failed`; a new request for the same scope and target then resumes it at its failed step, for every scope
(`202`, `resumes_id` names it, `deadline_at` is copied; a counterparty request carries the address again).
At most one request resumes a given request; a second one racing it gets `200` with the first. A `tenant` request for a tenant already `erasing` depends on the
tenant's latest tenant-scope request ([I8](../project/edge-cases.md)): `queued` or `running` returns it
with `200` (same `era_` ID); `failed` returns `202` with a new request that resumes the failed one at its
failed step (`resumes_id` names it; `deadline_at` is copied); `completed_with_holds` returns `202` with a
continuation request (`resumes_id` names it) that finishes the erasure once no hold remains. For an
`erased` tenant it returns `409 tenant_erased`. A tenant erasure never deletes a held thread: while one
remains the request ends `completed_with_holds` and the tenant stays `erasing`
([Privacy design § 6.6](../project/design/privacy.md#66-tenant-scope)):

#### Erasure request object

```json
{
  "id": "era_01J9…", "tenant_id": "ten_01J9…", "scope": "counterparty", "status": "completed",
  "identity_id": null, "target_id": null, "resumes_id": null,
  "created_at": "…", "deadline_at": "…", "completed_at": "…", "created_by_key_id": "key_01J9…",
  "receipt": {
    "messages_deleted": 14, "attachments_deleted": 9, "r2_objects_deleted": 38,
    "fts_rows_deleted": 14, "refs_deleted": 51, "vectors_deleted": 63,
    "events_deleted": 31, "log_events_deleted": 31, "drafts_deleted": 2, "uploads_deleted": 3,
    "screenings_deleted": 3, "screening_feedback_deleted": 1, "list_entries_deleted": 0,
    "identities_affected": ["idn_01J9…", "idn_01JA…"],
    "held": [ { "thread_id": "thr_01JA…", "reason": "PCN dispute WM12345678" } ],
    "probe": { "keyword_hits": 0, "semantic_hits": 0, "log_hits": 0 }
  }
}
```

In the receipt, `log_events_deleted` counts rows deleted from the tenant's event log, `drafts_deleted`
draft rows, `uploads_deleted` upload rows, `screenings_deleted` and `screening_feedback_deleted` screening
records and feedback deleted with their messages, `list_entries_deleted` the identity-scope allow and block
entries an identity-scope erasure removes ([Parity additions § 3](../project/design/parity-additions.md#3-identity-scope-allow-and-block-lists-fr-out-17)),
and `probe.log_hits` what the event-log probe still found (always 0 on success).

`identity_id` is set for message, thread and identity scope, and `target_id` (the `msg_` or `thr_` ID) for
message and thread scope; the counterparty address is never stored or returned. `deadline_at` is when
the 24 hours of NFR-PRV-1 end, counted from the first request.

`GET /v1/erasure-requests/{erasure_id}` and `GET /v1/erasure-requests` (filters: `tenant_id`, `status`). An
`erasure.completed` event is emitted. The partner key of an erased tenant's partner can still read the
tenant's erasure requests and their receipts.

### `POST /v1/exports` · `GET|DELETE /v1/exports/{export_id}`

```json
{ "tenant_id": "ten_01J9…", "scope": "counterparty", "counterparty_address": "jo@example.net" }
```

`scope` is `counterparty` (with `counterparty_address`: every message to or from it across the tenant's
identities) or `identity` (with `identity_id`: the whole mailbox). Needs `erasure:manage`,
`messages:read` and `attachments:read`, from a platform, partner or tenant key (an identity key gets
`403 permission_denied`). Quarantined, hidden and throttled messages are included as `.eml` only when
the key also holds `quarantine:review`; otherwise `messages.json` lists them with
`content_withheld: "quarantine_review_required"` ([I14](../project/edge-cases.md)). Returns `202` with
the export (`status: "queued"`).

```json
{ "id": "exp_01JA4…", "tenant_id": "ten_01J9…", "scope": "counterparty", "identity_id": null,
  "status": "completed", "size": 1843321, "created_at": "…", "expires_at": "…",
  "download_url": "https://mail.example.com/v1/links/bDE6Mz…" }
```

`status` is `queued`, `running`, `completed`, `failed`, `canceled` (a tenant erasure superseded it) or
`expired` (7 days after creation, or earlier when an erasure of the tenant expired it, because the ZIP
could hold erased mail). The finished export has
`download_url`: a [signed link](#get-v1linkstoken) to a ZIP holding one `.eml` per message plus
`messages.json`, minted again on each `GET` and valid for one download within one hour from it (or until
`expires_at`, 7 days after completion, if that is sooner); a second request with the same link gets
`404 export_not_found`. An `export.completed` event is emitted.

`DELETE /v1/exports/{export_id}` (`erasure:manage`) revokes every link at once: a `completed` export's
ZIP is deleted and it becomes `expired`, a `queued` or `running` one is stopped and becomes `canceled`,
and any other is left as it is. It returns `204`, and a link of a deleted export answers
`404 export_not_found`. Audit-logged (`export.delete`).

---

## Usage and audit

### `GET /v1/usage` — `usage:read` (implicit for tenant and identity keys on their own workspace)

The workspace's plan and the state of every allowance in the current period. Agents read it to know their
limits before they hit `402 billing_limit`. Every tenant and identity key holds `usage:read` implicitly
for its own workspace, so it can always call this. A platform or partner key must hold `usage:read`
explicitly and must pass `tenant_id` (a partner key, one of its own tenants); without `tenant_id` it gets
`400 invalid_request`. The MCP tool `mail_get_usage` is hidden from platform and partner keys.

```json
{
  "billing": "metered",
  "plan": { "plan_id": "developer", "status": "active", "current_period_end": "2026-11-01T00:00:00Z",
            "cancel_at_period_end": false },
  "features": [
    { "feature": "inboxes",        "granted": 10,    "used": 4,    "remaining": 6,    "unlimited": false, "resets_at": null },
    { "feature": "sends",          "granted": 12000, "used": 8312, "remaining": 3688, "unlimited": false, "resets_at": "2026-11-01T00:00:00Z" },
    { "feature": "triage",         "granted": 10000, "used": 2210, "remaining": 7790, "unlimited": false, "resets_at": "2026-11-01T00:00:00Z" },
    { "feature": "custom_domains", "granted": 5,     "used": 1,    "remaining": 4,    "unlimited": false, "resets_at": null },
    { "feature": "storage_gb",     "granted": 10,    "used": 2,    "remaining": 8,    "unlimited": false, "resets_at": null },
    { "feature": "seats",          "granted": 2,     "used": 2,    "remaining": 0,    "unlimited": false, "resets_at": null }
  ],
  "topups": { "inboxes": 0, "sends": 2, "triage": 0 },
  "plans": [ { "plan_id": "free", "name": "Free", "price": 0, "currency": "gbp", "interval": "month",
               "included": { "inboxes": 5, "sends": 1000, "triage": 500, "custom_domains": 0, "storage_gb": 1, "seats": 1 },
               "topups": false, "support": "github_issues" } ]
}
```

- `billing` is `metered`, `exempt` (no limits) or `disabled` (self-hosted without billing; `features` show
  the real `used` with `granted: null`, `remaining: null` and `unlimited: true`).
- `used` for `storage_gb` is measured, rounded up, and refreshed at least hourly.
- `granted` includes top-ups. `plans` is the whole catalog from `PM_PLAN_CATALOG`.

### `GET /v1/usage/daily` — `usage:read`, platform, partner or tenant key

Query: `tenant_id` (platform and partner keys), `from`, `to` (dates, at most 92 days apart). A tenant key
holds `usage:read` implicitly for its own tenant; platform and partner keys need it explicitly.

```json
{ "data": [ { "day": "2026-10-08", "inbound": 312, "outbound": 128, "sends": 141, "triage": 298,
  "search": 940, "agentic": 41, "assertions": 57, "http_signatures": 0, "ai_neurons": 18233,
  "storage_bytes": 2147483648 } ] }
```

`assertions` and `http_signatures` count the agent assertions and HTTP signatures made that day. They
are counts only: signing is not metered against any plan allowance. Event counts per hour, identity or
domain are on `GET /v1/metrics/events`; rates on `GET /v1/metrics/rates`.

### `GET /v1/plans` — no auth

The plan catalog, as in `plans` above. Returns `{ "billing_enabled": false, "data": [] }` on a deployment
without billing.

### `GET /v1/tenants/{tenant_id}/billing` · `PATCH /v1/tenants/{tenant_id}/billing` — `tenants:manage`, platform key to change

Read or change a workspace's billing account. A partner key with `tenants:manage` may read the billing
account of its own tenants; `PATCH` is platform-only, so a partner key gets `403 scope_denied` on its own
tenant (only a platform key changes a partnered tenant's billing mode). `PATCH` accepts `mode` (`metered`, `exempt`, `disabled`) and,
for workspaces without a Stripe subscription, `plan_id` (a complimentary plan). Plans paid through Stripe
change only through Stripe (`409 plan_managed_by_stripe`). Audit-logged. Both return:

```json
{ "tenant_id": "ten_01J9…", "mode": "metered",
  "plan": { "plan_id": "developer", "status": "active", "current_period_end": "2026-11-01T00:00:00Z",
            "cancel_at_period_end": false },
  "topups": { "inboxes": 0, "sends": 2, "triage": 0 } }
```

### `GET /v1/audit-events` — `audit:read`

Filters: `tenant_id`, `actor_key_id`, `action`, `target_id`, `after`, `before`. Newest first. A partner
key reads the rows of its own tenants only; rows about a partner itself (`partner.*`, and the
`key.create` and `key.revoke` rows of partner keys, which have no `tenant_id`) are for platform keys.

```json
{ "data": [ { "id": "aud_01JA…", "tenant_id": "ten_01J9…", "actor_key_id": "key_01J9…",
  "actor_user_id": null, "action": "quarantine.release", "target_type": "message", "target_id": "msg_01JA…",
  "details": {}, "request_id": "req_01JA…", "created_at": "…" } ], "next_cursor": null }
```

Audit rows cover administrative actions. These are every `action` value:

- keys: `key.create`, `key.rotate`, `key.revoke`; identity signing keys `identity_key.create`, `identity_key.rotate`, `identity_key.revoke`; deployment signing keys `signing_key.rotate`;
- partners: `partner.create`, `partner.update`, `partner.delete`;
- tenants: `tenant.create` (with the `partner_id` when a partner key created it), `tenant.policy_update` (every policy write), `tenant.ramp_held`, `tenant.ramp_lifted`;
- identities: `identity.create`, `identity.update` (a status change by a key or a person, with `details.from` and `details.to`), `identity.delete`, `identity.auto_pause`, `identity.retention_update`;
- domains: `domain.create` (with any MX records `replace_mx` deleted), `domain.transport`, `domain.evict`;
- service accounts: `account.request`, `account.approve`, `account.reject`, `account.close`, `account.delete`;
- mail: `quarantine.release`, `message.cancel`, `message.resolve`, `message.batch_update`, `suppression.remove` (only for a `complaint` suppression), `draft.approve`, `draft.reject`, `screening.feedback`;
- webhooks and events: `webhook.create`, `webhook.update`, `webhook.delete`, `webhook.rotate_secret`, `webhook.replay`, `webhook.disable` (a creator left, [W47](../project/edge-cases.md)), `webhook.headers_update`, `event_consumer.create`, `event_consumer.reset`, `event_consumer.delete`;
- privacy: `erasure.create`, `hold.set`, `hold.removed`, `hold.expired`, `export.delete`, `retention.expire` (one row per identity and UTC day of per-identity expiry);
- agent sign-up: `agent_signup.create`, `agent_signup.resend`, `agent_signup.human_replace`, `agent_signup.verify`, `agent_signup.abandon`, `agent_signup.refuse`, `agent_signup.expire`, `user.agent_signup_allow`;
- people: `member.invite`, `member.invite_resend`, `member.invite_revoke`, `member.join`, `member.role_change`, `member.leave`, `member.remove`, `member.ownership_transfer`, `user.delete`, `user.notifications_resume`, `user.oauth_unlink`, `waitlist.invite`;
- billing: `billing.checkout_started`, `billing.portal_opened`, `billing.downgrade_scheduled`, `billing.downgrade_cancelled`, `billing.mode_change`, `billing.plan_set`, `billing.plan_changed`, `billing.duplicate_cancelled`, `billing.dispute_opened`, `billing.dispute_closed`, `billing.refund_recorded`;
- platform operations: `ops.switch`, `mailbox.restore`, `master_key.activated`, `master_key.resealed`;
- automatic containment: `partner.auto_suspend`, `tenant.auto_pause`, `domain.auto_pause`;
- reads of mail content by platform and partner keys: `mail.read`.

Every request by a platform or partner key to a route that returns mail content (`GET …/threads/{thread_id}`,
`GET …/messages`, `GET …/messages/{message_id}`, `POST …/messages/batch-get`, its `raw`, `attachments/{attachment_id}`,
`attachments/{attachment_id}/text` and `related`, `GET …/quarantine`, `GET …/wait`,
`POST …/identities/{identity_id}/search`, `POST /v1/tenants/{tenant_id}/search`, and the MCP tools that
read the same) writes one `mail.read` row before the response, with `target_type` `message`, `thread` or `identity`,
`target_id` and `details.route`; if the row cannot be written the request fails with
`503 unavailable`. **Sends are not
audit rows**: each send is recorded by its message, its events (`message.sent` and the delivery events)
and its per-recipient delivery log. To review what a key sent, list the outbound messages of the
identities it reaches for the period; request logs also carry the key ID for 7 days.

---

## Deliverability metrics

Two read-only routes. `rates` shows how close an identity, a tenant or a domain is to an automatic
sending pause, rule by rule, from the same state that decides the pause. `events` counts what happened
to messages per hour or day. Plan allowances and metered units stay on `GET /v1/usage` and `GET /v1/usage/daily`
([Usage and audit](#usage-and-audit)). The design is [Deliverability metrics](../project/design/metrics.md).

### `GET /v1/metrics/rates` — `metrics:read`

Every rule that can pause sending, or alert the operator, is one **window**:

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

Every window is relative to the UTC time it is evaluated at; thresholds are compared in parts per million,
in integers, so the JSON a client reads and the decision cannot drift apart. Identity scope lists the two
identity windows; tenant scope the two tenant windows and the two burst windows once for each domain that
has outcomes in the last 24 UTC hours (for an identity key, `domain_id: null` on the burst windows of a
domain none of its addresses use); domain scope all six domain windows for a tenant domain, and only the
burst and alert windows for the platform domain
([Deliverability metrics § 3.1](../project/design/metrics.md#31-the-windows)).

Query: `scope` (`identity`, `tenant` or `domain`; default `identity` for an identity key, `tenant`
otherwise), `identity_id` (identity scope; an identity key may omit it), `domain_id` (domain scope),
`tenant_id` (required for platform and partner keys, as for `GET /v1/usage`; ignored otherwise). A missing
`identity_id` or `domain_id` for its scope, or a parameter of another scope, is `400 invalid_request`.

```json
{
  "scope": "identity",
  "tenant_id": "ten_01J9Z4K8V4QW7X2M5N6P8R0T1Y",
  "identity_id": "idn_01J9Z4M2P6R8T0V2X4Z6B8D0F2",
  "domain_id": null,
  "evaluated_at": "2026-10-17T09:12:44Z",
  "exempt": false,
  "sending": { "paused": false, "stops": [] },
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
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `evaluated_at` | The `now` of the evaluation; every window is relative to it |
| `exempt` | `true` only for the system identity, whose outcomes are recorded but never pause it, and which only platform keys can read. Its windows are returned with `tripped` computed and the summary "Exempt: never paused" |
| `sending.paused` | Whether this scope itself is paused now (identity `status = "paused"`; tenant or domain `sending_paused_at` set) |
| `sending.stops` | Every pause that stops sends from this scope now: `{ "scope", "id", "reason", "paused_at" }`. For an identity: its own pause, its tenant's, and that of each tenant domain one of its `active` or `retiring` addresses is on; for a domain: its own and its tenant's; for a tenant: its own |
| `window.kind` | `last_outcomes` (`size`), `utc_days` (`from`, `to`: the first and last day's start, RFC 3339) or `utc_hours` (`from`, `to`: the first and last hour's start) |
| `outcomes` | Outcomes in the window (for `last_outcomes`, rows present, at most `size`) |
| `events` | Outcomes of the window's kind (complaints or bounces) in the window |
| `denominator` | `size` for `last_outcomes` windows; `outcomes` for the others |
| `rate` | `events / denominator`, rounded to 6 decimal places for display; `0` when the denominator is 0. Decisions use `events` and `denominator` |
| `evaluated` | `false` while `outcomes` is below `min_outcomes`: the rule cannot trip yet |
| `tripped` | The rule's predicate is true at `evaluated_at`. A tripped window of a scope that is not yet paused means the next counted outcome is decided against it |
| `headroom` | The fewest further outcomes of the window's kind, recorded in a row with nothing else recorded, that would make the rule trip at `evaluated_at`; fewer further outcomes, of any kinds, cannot trip it at that instant; `0` when tripped; `null` when no number of them could trip it (a threshold of 1 with `gt`, or a threshold of 1 with `ge` once the window holds an outcome that is not of its kind) |
| `summary` | One plain-English sentence built from the fields above, for a model or a person |

**Headroom** is computed with the rule's own predicate over simulated outcomes (for a `last_outcomes`
window the oldest rows drop out as new ones arrive), never with a separate formula. It describes one
instant and moves both ways between reads: a delivered outcome never lowers it on a `last_outcomes` window
or an `evaluated` one, but below a window's minimum it counts toward the minimum and can lower it by one
(100 tenant outcomes with no complaint give headroom 400; after 399 deliveries, 1), and an hour or day
rolling out of a window can lower or raise it. Read the windows just before a batch. A batch of `N`
recipients cannot trip a `last_outcomes` or `evaluated` window when `N` is smaller than its headroom; a
window with `evaluated: false` is not bounded by its headroom (each recipient's delivery and soft bounces
count toward the minimum), and cannot trip while `(events + N) × 1,000,000` is below
`threshold_ppm × min_outcomes` ([Deliverability metrics § 3.2](../project/design/metrics.md#32-get-v1metricsrates)).

### `GET /v1/metrics/events` — `metrics:read`

The mailbox counts these message events, by the domain of each count (inbound types: the identity's address
the message was routed to; sent and delivery types: the domain of the `From` address, the platform domain
when the message was `sent_via_fallback`; `suppressed` and `canceled`: the domain of `from_address` at
submit):

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

Other events are not counted; new types may be added within `/v1`, so ignore unknown keys. **Counts and
rates differ, by design**: `bounced` and `complained` here count every event, including first replies to
strangers and outcomes of the system identity, which the rate windows leave out. Rates answer "how close is
a pause"; event counts answer "what happened".

Query: `scope` (as for rates), `identity_id`, `domain_id`, `tenant_id` (as for rates), `bucket` (`hour` or
`day`, default `day`), `from` and `to` (RFC 3339; `from` is rounded down and `to` up to the bucket; `to`
is exclusive), `types` (comma-separated, default all twelve), `identity_ids` (tenant and domain scope: at
most 100, comma-separated). Defaults: `bucket=hour` covers the 24 hours ending with the current hour;
`bucket=day` the 30 days ending with today.

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

`data` has one entry per bucket in the range, zeros included, oldest first. In order:

1. An identity key may use only identity scope (`403 scope_denied` otherwise).
2. Rate limits: `RL_API` and `RL_SEARCH` (120 per 60 seconds per key), because tenant and domain scope
   fan out like a tenant search.
3. Range: `from < to` after rounding, else `400 invalid_request`. Hourly: at most 168 buckets, and `from`
   no earlier than the current hour minus 191 hours. Daily: at most 92 buckets, and `from` no earlier than
   today minus 91 days. Otherwise `400 query_range_too_wide` with `details.max_buckets`,
   `details.requested_buckets` and `details.earliest`. A `to` in the future is allowed; its buckets are
   zero. An unknown type is `400 invalid_request`.
4. Identities: identity scope, the one identity; tenant scope, the tenant's active and paused identities;
   domain scope, the tenant's identities with an address on the domain (every identity, for the platform
   domain); narrowed by `identity_ids` (a foreign ID is `404 identity_not_found`). More than 100 is
   `422 scope_too_large`: pass `identity_ids`, at most 100.
5. Each mailbox is read with a deadline of 2,000 ms; an identity that errors or misses it is listed in
   `failed_identities` and `partial` becomes `true`, and the response is still `200`.
6. Counts are summed per bucket and type; `totals` covers the range.

---

## Members

Console users of a workspace. The console is the main way to manage them; these endpoints let an
integrator provision people (for example, the owner of each customer workspace).

### `GET /v1/tenants/{tenant_id}/members` — `members:read`

Not paginated: a workspace's members and pending invitations are bounded by its seats.

```json
{ "data": [ { "user_id": "usr_01JA…", "email": "sam@acmecarhire.example", "name": "Sam Patel",
  "role": "owner", "last_login_at": "…", "created_at": "…" } ], "invitations": [ { "id": "inv_01JA…",
  "email": "kim@acmecarhire.example", "role": "member", "invited_by": "usr_01JA…", "expires_at": "…" } ],
  "seats": { "granted": 2, "used": 2 } }
```

### `POST /v1/tenants/{tenant_id}/invitations` — `members:manage`

`{ "email": "kim@acmecarhire.example", "role": "member" }`. Sends an invitation email from the deployment's
system identity (`PM_SYSTEM_FROM`), also when the console is off (`PM_CONSOLE=off`). A pending invitation uses a seat; with no seat left the request fails with
`402 billing_limit` (`details.feature: "seats"`). Roles: `admin`, `member`, `viewer`. The owner is set at
workspace creation (`owner` in `POST /v1/tenants`) or by an ownership transfer in the console. Returns
`201` with the invitation (`id`, `email`, `role`, `expires_at`). A tenant sends at most 50 invitation
emails (new and re-sent) per UTC day; past that the request gets `429 daily_cap_reached` with
`details.cap: "invitations"` and `details.resets_at`, before anything is written. An address this
deployment hosts cannot be invited (`400 invalid_request`, path `email`). The person accepts in the
console with an explicit click; when they are enrolled in two-step verification, the membership is
created only after their second factor.

### `DELETE /v1/tenants/{tenant_id}/invitations/{invitation_id}` · `DELETE /v1/tenants/{tenant_id}/members/{user_id}` — `members:manage`

Revokes an invitation, or removes a member and ends their sessions. Returns `204`. The owner cannot be
removed (`409 owner_required`). Removing a member also revokes, in the same step, every key of the
workspace whose `created_by_user_id` is that person, each with a `key.revoke` audit row
(`details.reason: "creator_removed"`); a role change in the console revokes those holding a permission
the new role lacks ([W36](../project/edge-cases.md)).

---

## Platform operations

Platform keys with `platform:ops` (`GET /v1/platform/status`: `audit:read`). Every call that changes something is audit-logged.

### `POST /v1/platform/keys/{purpose}/rotate`

`purpose` is one of:

| `purpose` | Signs | Previous key verifies for |
|---|---|---|
| `thread` | Thread tokens | 90 days |
| `link` | Download links, console sign-in, invitation and session tokens, and OAuth state hashes | 7 days |
| `cursor` | Search cursors (the cursor lifetime) | 24 hours |
| `web_bot_auth` | Web Bot Auth HTTP signatures and the [key directory](#well-known). Its `kid` is the key's 43-character JWK thumbprint, not one character | 7 days, during which it stays in the key directory |

Generates a new key inside the Worker and makes it current. No body. Rotating `web_bot_auth` while
`PM_WEB_BOT_AUTH=off` returns `422 web_bot_auth_disabled` ([O9](../project/edge-cases.md)). Returns `200`:

```json
{ "purpose": "thread", "kid": "4", "created_at": "2026-10-09T10:00:00Z",
  "previous": { "kid": "3", "verify_until": "2027-01-07T10:00:00Z", "revoked": false } }
```

`previous` is `null` when the purpose had no key yet; the rotation then creates the first one.

**`?revoke_previous=true`** deletes the previous key in the same D1 batch, so what it signed stops
verifying at once: thread tokens fall back to header threading; open links, console sign-in tokens,
invitations, sessions and OAuth flows under it fail; open search cursors fail with `400 invalid_request`;
a previous `web_bot_auth` key leaves the key directory. The response then has `previous.verify_until`
equal to the rotation time and `previous.revoked: true`.
Without it, a leaked key keeps verifying for its window. After a suspected leak, rotate with
`revoke_previous=true`, then rotate `PM_MASTER_KEY`. The audit action is `signing_key.rotate` for every
purpose, `web_bot_auth` included, with `details.revoke_previous`.

Key material is never returned, by this or any other endpoint. See
[Configuration › Thread and link keys](configuration.md#thread-and-link-keys).

### `GET /v1/platform/dlq`

Dead-letter items, oldest first. Filters: `queue` (`pm-inbound`, `pm-outbound`, `pm-delivery-events`,
`pm-webhooks`, `pm-index`, `pm-screen`), `status` (`open`, the default, or `redriven`), `tenant_id`, `cursor`,
`limit`.

```json
{ "data": [ { "id": "dlq_01JA…", "queue": "pm-inbound", "kind": "message", "tenant_id": "ten_01J9…",
  "first_seen_at": "…", "redriven_at": null, "redrive_count": 0 } ], "next_cursor": null }
```

The stored body is not returned: it is a pointer, and inbound pointers carry envelope addresses. Items
are kept for 14 days.

### `POST /v1/platform/dlq/{dlq_id}/redrive`

Publishes the stored body back to its source queue and returns the item with `redriven_at` set and
`redrive_count` incremented. Every consumer is idempotent, so a redrive is safe to repeat.
`Idempotency-Key` is optional.

### `POST /v1/platform/jobs` · `GET /v1/platform/jobs/{job_id}`

Starts a maintenance job ([J3](../project/edge-cases.md)):

```json
{ "kind": "reparse", "tenant_id": "ten_01J9…", "identity_ids": null,
  "after": "2026-09-01T00:00:00Z", "before": null }
```

| `kind` | Does |
|---|---|
| `reparse` | Re-parses messages from raw MIME with the deployed parser and re-emits their events with `reprocessed: true`. Messages past `raw_days` are skipped and counted |
| `reembed` | Re-chunks and re-embeds messages into Vectorize, for example after a model change |
| `reindex` | Rebuilds the keyword index (FTS5 and references) of each mailbox |
| `restore_reconcile` | After a mailbox point-in-time restore: re-queues inbound mail received after `after` whose raw MIME is still in R2, and re-inserts sends made after it as `uncertain` with their idempotency records ([Restore from PITR](../project/design/observability.md#restore-from-pitr)). Needs `after` and `identity_ids`; runs even while the deployment is frozen |

`tenant_id` is required; `identity_ids` (default: every identity of the tenant), `after` and `before`
narrow it. Returns `202` with the job:

```json
{ "id": "job_01JA…", "kind": "reparse", "tenant_id": "ten_01J9…", "status": "queued",
  "created_at": "…", "completed_at": null, "result": null }
```

`status` is `queued`, `running`, `completed`, `failed` or `canceled`; `result` holds counts once it
ends, and for a failed job the failed step and error code. `Idempotency-Key` is optional.
`GET /v1/platform/jobs/{job_id}` returns any job except erasure and export jobs, which are read through
their own requests: the jobs started here, and the `retention`, `backup` and `domain_remove` jobs that a
`job_failed:{kind}` alert names.

### `GET /v1/platform/status`

Platform keys with `audit:read` (the external heartbeat's key holds only that). The deployment's
operational state, read by `idvy doctor`, `idvy ops status` and the heartbeat
([Observability § 5.5](../project/design/observability.md#55-alert-email-and-the-external-heartbeat)):

```json
{
  "frozen": false, "frozen_since": null,
  "switches": { "read_only": "off", "free_sending": "on", "emergency_prune": "off" },
  "alert_email": "configured",
  "firing": [ { "alert": "dlq:pm-inbound", "severity": "page", "fired_at": "2026-10-10T03:12:00Z" } ],
  "master_key": { "active_slot": "a", "remaining": 0, "activated_at": "2026-07-01T09:00:00Z",
                  "resealed_at": "2026-07-01T09:40:00Z",
                  "slots": [ { "slot": "a", "present": true, "kid": "3f9a0c1d2e4b5a67" },
                             { "slot": "b", "present": true, "kid": "88c1d0e2f3a4b596" } ] },
  "capacity": { "d1_bytes": 1840000000, "d1_pct": 18.4, "vectors": 2100000, "vectors_pct": 10.5,
                "tenants": 1204, "namespaces_pct": 2.4, "measured_at": "2026-10-10T03:00:00Z" }
}
```

`alert_email` is `configured` or `missing` (neither `PM_ALERT_EMAIL` nor a `mailto:` `PM_SECURITY_CONTACT`).
`firing` lists every state alert that is firing. `master_key.remaining` counts sealed values not yet
re-sealed with the active slot's key; `activated_at` is when the Worker first saw that key, and
`resealed_at` when `remaining` first reached 0 under it (`null` until then)
([Security § 6.2](../project/design/security.md#62-rotation-procedures)). Key IDs are not secret: every
ciphertext carries one. Key material is never returned.

### `PUT /v1/platform/switches`

`platform:ops`. `name` is `read_only`, `free_sending`, `emergency_prune` or `agent_signup`
([Observability § 5.6](../project/design/observability.md#56-automatic-containment)). `agent_signup off`
stops new agent sign-ups, their resends and replacements, and their mail
([Agent self-sign-up §13.2](../project/design/agent-signup.md#132-the-automatic-switch)):

```json
{ "name": "read_only", "state": "off", "reason": "rpc_owner_mismatch fixed in 1.0.3" }
```

Returns `200` with `{ "name", "state", "set_by": "platform", "reason", "set_at" }`. Readers cache a
switch for 60 seconds. The audit action is `ops.switch`. `Idempotency-Key` is optional.

### `POST /v1/platform/identities/{identity_id}/restore`

`platform:ops`. Restores one identity's mailbox to a point in time, or undoes such a restore
([Restore from PITR](../project/design/observability.md#restore-from-pitr)). Only while the deployment is
frozen (`PM_FREEZE = "on"`); otherwise `409 not_frozen`.

```json
{ "at": "2026-10-09T09:00:00Z" }
```

or `{ "bookmark": "<undo bookmark>" }`. `at` must be within the last 30 days. Returns `200` with
`{ "identity_id", "restored_to", "undo_bookmark" }`; the audit action `mailbox.restore` records the undo
bookmark too.

### `POST /v1/platform/erasure-requests/{erasure_id}/reapply`

`platform:ops` and `erasure:manage`. Creates a new erasure request that repeats a stored one after a
restore brought erased data back. No body. The new request has the same tenant, scope, `identity_id`
and `target_id`, reason `reapply_after_restore:{erasure_id}` and its own `deadline_at`; a counterparty
erasure matches by the stored keyed hash of the address, which is never needed in clear
([Privacy design § 11](../project/design/privacy.md#11-what-remains-after-deletion)). Returns `202` with
the new erasure request. It runs even while the deployment is frozen. `Idempotency-Key` is optional.
Only a request that ended `completed` or `completed_with_holds` can be re-applied; any other gets
`409 erasure_not_completed` (a `queued` or `running` request erases the restored data itself, and a
`failed` one is resumed with a new request). A tenant-scope request is re-applied although the tenant
is `erased`: the new job erases the mailboxes of the identities in the source receipt's
`identities_affected` and sweeps the tenant's objects and vectors, and the tenant stays `erased`.

### `GET /v1/platform/agent-signups`

`platform:ops`. The operator's view of agent sign-ups ([Agent self-sign-up §6.6](../project/design/agent-signup.md#66-get-v1platformagent-signups)).
Filters: `state`, `created_after`, `network` (the 16-hex network hash shown in a row), `limit`, `cursor`.
Newest first. Each row: `signup_id`, `tenant_id`, `state`, `address`, `human_email_hint`, `human_status`,
`source`, `network`, `created_at`, `expires_at`, `verified_at`, `verified_via`. No clear address of a person
is returned. Contain a sign-up with the existing tools: suspend its tenant, erase it, or turn the
`agent_signup` switch off.

### `POST /v1/platform/waitlist/invite`

```json
{ "count": 50, "plan": null }
```

Invites the oldest confirmed, not yet invited entries of the sign-up waitlist
([Cloud sign-up › The waitlist](../project/design/cloud-signup.md#61-before-launch-the-waitlist)). `count`
is 1–500. `plan` (optional) invites only entries whose plan of interest is that plan. Each invited person
gets a sign-up link valid for 7 days. The audit action is `waitlist.invite`. `Idempotency-Key` is
optional. The CLI equivalent is `idvy waitlist invite --count N [--plan P]`. Returns `200` with the
number invited and the number of confirmed entries still waiting:

```json
{ "invited": 50, "waiting": 262 }
```

---

## Signed links and provider hooks

These routes need no API key.

### `GET /v1/links/{token}`

Serves a signed link: a large attachment replaced by a link in an outbound message, or an export ZIP.
The token carries the signing key's kid and an expiry, and is checked in constant time
([Security design](../project/design/security.md#73-signed-links)). The response is the file with
`Content-Disposition: attachment`, `X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox`
and `Cache-Control: private, no-store`. `Content-Type` is `application/zip` for an export; for an
attachment it is the sniffed type when it is on the safe list of
[Security § 8.5](../project/design/security.md#85-serving-attachments-and-raw-mime), otherwise
`application/octet-stream`. A bad, expired or unknown link, or a deleted target, returns
`404 attachment_not_found` or `404 export_not_found`. An upload token gets `404 upload_not_found`.

### `PUT /v1/links/{token}`

Sends an upload's bytes to its `upload_url` ([Uploads](#uploads)). No API key: the token is a signed link
for exactly one upload, whose size and SHA-256 were fixed at create, valid for at most one hour (never past
the upload's `expires_at`).

```http
PUT /v1/links/bDE6YTp1cGxvYWQ6dGVuXzAxSjk… HTTP/1.1
Host: mail.example.com
Content-Type: application/octet-stream
Content-Length: 9437184

<9,437,184 bytes>
```

Returns `200` with the [upload](#upload-object), `ready`, `upload_url: null`. An `upload` token is accepted
only by `PUT`, and attachment and export tokens only by `GET`: a token used with the other method, or a bad
or expired one, answers `404` with the code of the token's own target, so a link never reveals why it
failed. The request's `Content-Type` is ignored. Requests count against `RL_TENANT` of the token's tenant.
In order:

- a bad, expired or other-purpose token, or a deleted upload, identity or tenant: `404 upload_not_found`;
- `429 rate_limited`; `403 tenant_suspended` or `403 partner_suspended`; `409 identity_paused`;
- `Content-Length` missing or not the declared size: `400 upload_size_mismatch` (`details.reason =
  "content_length"`, `details.expected`, `details.received`); a `Content-Encoding`: the same with
  `"encoding"`;
- another `PUT` holds the upload: `409 upload_in_progress` with `Retry-After`; an upload that is already
  `ready` answers `200` without reading the body, so a retry after a lost response is a success;
- a `rejected` upload, or bytes with a quarantining risk or that the scanner found infected:
  `422 attachment_risky` (`details.risk` or `details.reason = "infected"`);
- an expired upload: `410 upload_expired` (`details.reason`);
- bytes whose SHA-256 is not the declared one: `400 upload_checksum_mismatch` (`details.expected`); a body
  that ended early: `400 upload_size_mismatch` (`"incomplete"`). Nothing is stored. The tenth such failure
  sets the upload `expired` (`too_many_attempts`);
- a storage failure: `503 unavailable` (retryable, never counted against the client).

### `POST /hooks/ses`

The Amazon SES **delivery event** endpoint, subscribed to the SNS topic `PM_SES_SNS_TOPIC_ARN`. A
subscription confirmation for that topic is confirmed; a notification becomes delivery events for the
matching messages and returns `200`. An internal failure returns `500`, so SNS retries (it retries `5xx`
and `429`) ([Outbound design](../project/design/outbound.md#amazon-ses)).

### `POST /hooks/ses/inbound`

The Amazon SES **inbound mail** endpoint, for domains with `inbound: ses`. It is subscribed to the SNS
topic `PM_SES_INBOUND_TOPIC_ARN`. A notification names the S3 object SES stored and its recipients; each
recipient is queued once for the inbound pipeline, then the endpoint returns `200`. Duplicates are dropped
by the `ses_ingest` ledger, so each object and recipient is ingested exactly once (FR-DOM-9). The SQS
queue `PM_SES_INBOUND_QUEUE_URL` is a backstop subscribed to the same topic: the every-minute cron feeds
its messages to the same handler. Mail to an unknown address on the domain is dropped without a bounce
([Domains on any DNS host › Inbound through SES](../project/design/domain-connections.md#45-inbound-through-ses)).
An internal failure returns `500`, so SNS retries.

**Verification, on both endpoints.** Only SNS messages that pass every check are accepted:

- `SignatureVersion` is `2` (SHA256withRSA) and the signature verifies. Version `1` is refused; setup sets
  `SignatureVersion=2` on both topics.
- `SigningCertURL` is `https` on the host `sns.{PM_SES_REGION}.amazonaws.com`.
- `TopicArn` equals that endpoint's topic.
- `Timestamp` is within one hour (14 days for messages the backstop reads from SQS).

Anything else gets `403 invalid_signature`.

---

## Well-known

Served on the API host, with no API key.

| Path | Content |
|---|---|
| `/.well-known/security.txt` | Security contact (from `PM_SECURITY_CONTACT`) |
| `/.well-known/jwks/{identity_id}.json` | The identity's JWK Set: its `active` and `retiring` signing keys, which verify its [agent assertions](#post-v1identitiesidentity_idassertions--tenant-or-identity-key-identitiessign). `Content-Type: application/jwk-set+json`, `Cache-Control: public, max-age=300`. An unknown, `deleting`, `deleted`, paused or suspended identity gets `404 identity_not_found` ([O1](../project/edge-cases.md)) |
| `/.well-known/http-message-signatures-directory` | The Web Bot Auth key directory: the deployment's `active` and `retiring` keys (at most three) as a JWK Set. `Content-Type: application/http-message-signatures-directory+json`, `Cache-Control: max-age=86400`. The response is signed once per listed key (`Signature-Input` and `Signature`, tag `http-message-signatures-directory`, component `("@authority";req)`), so a copy served elsewhere does not verify ([O12](../project/edge-cases.md)). `404 key_not_found` while `PM_WEB_BOT_AUTH=off` |

An identity's JWK Set during the overlap after a rotation (the first key is `active`, the second
`retiring`):

```json
{ "keys": [
  { "kty": "OKP", "crv": "Ed25519", "x": "NjwMjIq2mTA1VpuDzRvkMIfQ0sCSHWavo0KT_4FcKO0",
    "kid": "zMkUmAQOlq9JtFPzTK1XINZdWd7gmhXxgA8Ph7cNKHo", "alg": "EdDSA", "use": "sig" },
  { "kty": "OKP", "crv": "Ed25519", "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
    "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k", "alg": "EdDSA", "use": "sig" } ] }
```

Identity IDs are ULIDs, never derived from addresses, so the JWK Set path cannot be used to test
whether an address exists. Registering the key directory with Cloudflare's verified-bot programme is an
optional operator step ([Self-hosting › Signed HTTP requests](../self-hosting.md#signed-http-requests-web-bot-auth));
signatures verify for any Web Bot Auth verifier without it.
