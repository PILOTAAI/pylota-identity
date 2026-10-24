# Agent self-sign-up

Binding for implementation. An AI agent with no key and no person signed in can create its own
**provisional workspace** on Pylota Identity Cloud with one unauthenticated call, and gets one inbox on the
platform domain and one identity key in the answer. The agent must name the person responsible for it.
Until that person proves they control their address, by giving the agent a six-digit code or by opening
the link in the email, the workspace can email that person only, at most 10 messages a day, holds the
policy's guards strict, and is erased 7 days after it was created. Verification makes the
person the accountable human of the identity (FR-IDN-2) and the owner of an ordinary Free workspace,
which then runs the new-workspace send ramp (FR-CON-13). The feature exists for parity with AgentMail
(gap GAP-7, [ADR 0017](../adr/0017-agentmail-parity.md)), which also allows sign-up with no person at
all; Pylota Identity deliberately does not. It is off unless the owner sets `PM_AGENT_SIGNUP = "on"` on a
deployment whose public sign-up is open.

| | |
|---|---|
| Requirements | FR-ASU-1 to FR-ASU-16 (this page, [§23](#23-functional-requirements)); FR-IDN-2 (amended), FR-CON-8, FR-CON-13, FR-CON-17, FR-CON-18, FR-DLV-3, FR-DLV-6, FR-KEY-1, FR-KEY-2, FR-PRV-3 |
| Edge cases | [X1](#24-edge-cases)–[X63](#24-edge-cases) (63 rows; X60 is integrator-owned) |
| Code | `crates/core/src/signup.rs` (request normalisation, the username and human-email rules, the base address for counting, the permission and policy overlays, code and token encodings, the state machine; pure); `crates/worker/src/signup/{mod.rs, create.rs, admit.rs, verify.rs, human.rs, end.rs, mail.rs, outcomes.rs}`; `handlers/agent_signup.rs` (the six `/v1/agent-signup` routes and `GET /v1/platform/agent-signups`); `console/pages/agent_signup.rs` (the verify, claim and refuse pages); `crons/agent_signup.rs` (expiry, on the `*/15` cron). Hooks in other milestones' files: `auth/mod.rs` (the permission overlay, [§4.1](#41-permissions)), `crates/core/src/policy.rs` (`effective` with the policy overlay, and send steps 4 and 7a), `consumers/delivery.rs` (sign-up outcomes, [§13.3](#133-outcomes-of-sign-up-mail)), `handlers/identities.rs` and `handlers/addresses.rs` (the hosted-address check, [§3.3](#33-the-human-email-rules)), `console/pending.rs` (the `two_factor` step for a link), `jobs/erasure.rs`, `crons/signup_ramp.rs`, `quota/mod.rs` |
| Tables | D1 `agent_signups` and `agent_signup_refusals` (new); new columns `tenants.provisional_until` and `pending_auth.agent_signup_id`; `ops_switches` name `agent_signup` (new value); the default tenant's `TenantQuota` counters `sysmail:signup:{scope}:{key}` and `asu:{outcomes,bounced,complained}:{hour}` ([§15](#15-storage)) |
| Contracts | `POST /v1/agent-signup` (no key), `GET`, `DELETE /v1/agent-signup`, `POST /v1/agent-signup/verify`, `POST /v1/agent-signup/resend`, `PUT /v1/agent-signup/human`, `GET /v1/platform/agent-signups`; the console pages `/console/agent-signup/verify`, `/console/agent-signup/claim` and `/console/agent-signup/refuse`; the event `signup.verified`; the system-mail class `signup`; the `AccountEvent` `agent_signup_verified`; errors `403 signup_closed`, `404 signup_not_found`, `409 signup_not_pending`, `409 signup_cannot_verify`, `409 signup_human_unreachable`, `422 signup_code_invalid`, `422 signup_replace_limit`, `429 signup_limit_reached`, and the new `details.reason` values `signup_unverified` (on `403 permission_denied` and `403 policy_denied`) and `signup_human_unreachable` (on `403 policy_denied`); the ID prefix `asu_`; the variables `PM_AGENT_SIGNUP` and `PM_AGENT_SIGNUP_DAILY_CAP` |
| Limits | [§17](#17-billing-and-limits), to be copied to [Limits › Agent self-sign-up](../../reference/limits.md) |
| Built in | [M34](#26-milestone) |

External facts on this page were read on 2026-10-17: RFC 2606 (reserved names), RFC 7505 (null MX),
RFC 5321 §5.1 (the implicit MX), RFC 8058 (one-click unsubscribe), Cloudflare's
[Rate Limit binding](https://developers.cloudflare.com/workers/runtime-apis/bindings/rate-limit/)
(`simple.period` is 10 or 60 seconds; counters are per Cloudflare location and eventually consistent),
the [`request.cf` properties](https://developers.cloudflare.com/workers/runtime-apis/request/) (`asn` is
available on all plans) and the [D1 `batch()`](https://developers.cloudflare.com/d1/worker-api/d1-database/)
semantics (a batch is one SQL transaction; a failing statement rolls back the whole sequence). AgentMail
facts come from the inventory read the same day (`inv-identity.md` §1–§3, `inv-core.md` "Permissions").

## 1. When agent sign-up is on

`POST /v1/agent-signup` creates anything only when **all** of these hold. Otherwise it answers
`403 signup_closed` and writes nothing:

| Condition | Why | `details.reason` when it fails |
|---|---|---|
| `PM_AGENT_SIGNUP = "on"` (default `off`) | The owner turns the feature on deliberately ([Cloud commissioning](../cloud-commissioning.md), gate D in [§26](#26-milestone)) | `disabled` |
| `PM_SIGNUP = "open"` | See below | `disabled` |
| `PM_BILLING = "stripe"`, with billing started (its secrets present) | A verified sign-up becomes a Free workspace, and the send ramp of FR-CON-13 applies only to a workspace `metered` on the catalog's default plan ([Cloud sign-up §10.1](cloud-signup.md#101-new-workspace-send-ramp)). Without billing a verified agent would get the full tenant cap at once | `disabled` |
| `PM_CONSOLE = "on"` | The person verifies through the console link or claims there ([§7.2](#72-the-link-path-in-the-console), [§8](#8-claiming-in-the-console)) | `disabled` |
| The `ops_switches` row `agent_signup` is not `off` | The automatic containment of [§13.2](#132-the-automatic-switch), or the operator by hand | `paused` |

**Only `open`, not `waitlist`.** With `PM_SIGNUP = "waitlist"` the operator admits people in batches
(FR-CON-8). An agent sign-up would let anyone create a workspace without an invitation, which makes the
waitlist pointless. Allowing it for a human address that holds a valid waitlist invite was considered and
refused: the answer would tell a caller whether an address has an invite. Cloud therefore reaches agent
sign-up only after phase C of [Cloud commissioning §2](../cloud-commissioning.md#2-release-order), and
only when the owner also sets `PM_AGENT_SIGNUP`.

**Turning it off keeps what exists.** Setting `PM_AGENT_SIGNUP = "off"`, changing `PM_SIGNUP` away from
`open`, or the switch going `off` stops **new** sign-ups only ([X2](#24-edge-cases)). A provisional
workspace that already exists keeps its lifecycle: its agent can still verify, read its inbox and abandon;
its person can still open the link, claim and refuse; and expiry still erases it on time. A person who was
already named must not lose the inbox because the operator closed the door behind them, and the
provisional restrictions already contain it. While the **switch** is `off`, the two routes that send
system mail (`POST /v1/agent-signup/resend` and `PUT /v1/agent-signup/human`) also answer
`403 signup_closed` (`paused`), and every send of a provisional workspace gets `403 policy_denied`
(`signup_paused`, [§4.3](#43-sending-only-to-the-human)), because the switch exists to stop mail to people
who never confirmed anything from harming the platform domain ([X3](#24-edge-cases)).

**Configuration.** `PM_AGENT_SIGNUP` must be `on` or `off`, and `PM_AGENT_SIGNUP_DAILY_CAP` an integer
from 1 to 10,000 (default 200); any other value is `config_invalid`, as for every optional variable
([Rust workspace › Startup rules](rust-workspace.md#61-errors-and-configuration)). `PM_AGENT_SIGNUP = "on"`
while another condition of the table fails is a valid configuration: the Worker starts, the route answers
`403 signup_closed`, `GET /v1/platform/status` reports `agent_signup: { "configured": true, "active": false,
"reason": "signup_not_open" | "billing_off" | "console_off" | "switch_off" }`, and `idvy doctor` warns.
Self-hosted deployments keep the default `off`; nothing in this page runs for them.

## 2. The model

### 2.1 What a sign-up creates

One sign-up creates, in one D1 batch ([§3.5](#35-the-create-batch)):

| Object | Value |
|---|---|
| Tenant | `status = 'active'`, `mode = 'live'`, `partner_id = NULL`, `slug` = the username, `address_suffix` = `.` + the username, `name` = `agent_name` (else the username), `timezone = 'UTC'`, `policy_json = '{}'`, and the new column `provisional_until` = `created_at` + 7 days |
| Billing account | `mode = 'metered'`, `plan_id` = the catalog's `default_plan` (Free), no Stripe customer |
| Identity | `username = 'agent'`, `display_name` = `agent_name` (else the username), `owner_name` and `owner_email` `NULL` (there is no accountable human yet, [§4.3](#43-sending-only-to-the-human)), default `send_policy` |
| Address | The platform address `agent.{username}@{PM_PLATFORM_DOMAIN}`, primary, `active` |
| Identity key | Level `identity`, `mode = 'live'`, the permissions of [§4.1](#41-permissions), no expiry, `created_by_key_id`, `created_by_user_id` and `created_by_role` `NULL`. Its secret is returned once |
| Send-allow entry | `sender_lists` (`send`, `allow`, the human's address), so the human is a known recipient ([Outbound › Recipient filters](outbound.md#recipient-filters)) |
| Sign-up row | `agent_signups` ([§15](#15-storage)): the human's address, the code and link hashes, counters and the state |

A provisional workspace has no members, no tenant key, no domain and no webhook endpoint, and nothing
can add one until it is verified: the key cannot, and no person belongs to it.

### 2.2 States

```text
                         POST /v1/agent-signup
                                  │
                                  ▼
       ┌──────────────────── provisional ─────────────────────────┐
       │        │                    │                │            │
 code (agent),  │ DELETE             │ "Not me", or a │ 7 days     │
 link (person), │ /v1/agent-signup   │ complaint      │ pass       │
 or claim       │ (agent)            │ (person)       │ (cron)     │
       ▼        ▼                    ▼                ▼            │
   verified  abandoned            refused          expired         │
   (a Free      └──────────────┬─────┴───────────────┘             │
   workspace)                  ▼                                   │
                  tenant-scope erasure (reason agent_signup_{abandoned│refused│expired})
                  → tenant erased; slug, suffix and address never reused
```

| State | Meaning | `tenants.provisional_until` |
|---|---|---|
| `provisional` | Waiting for the person. The restrictions of [§4](#4-while-provisional) apply | set |
| `verified` | Terminal. The person is the owner and the accountable human | `NULL` |
| `abandoned`, `refused`, `expired` | Terminal. A tenant-scope erasure was started in the same batch; the erasure deletes the row ([§10](#10-expiry-abandonment-and-refusal-erasure)) | unchanged (the tenant is erased) |

The provisional state is **not** a value of `tenants.status`. `status` is the axis on which operators
enforce (`active`, `suspended`, `erasing`, `erased`), and a provisional workspace must be able to be
suspended and resumed without losing its provisional state, and to expire while suspended. So every
existing reader of `status` keeps its meaning, inbound routing included (a provisional workspace receives
mail like any `active` tenant), and the one new column `provisional_until` carries the provisional state.
It is read on every request by key authentication ([Security § 4.2](security.md#42-verification), step 9,
which already loads the tenant row without a cache) and by every reader of the effective policy, so
verification takes effect on the next request, with no cache to wait for. AgentMail documents up to five
minutes of stale limits after verification; Pylota Identity has none ([X28](#24-edge-cases)).

### 2.3 The address and the username

The agent chooses a **username**. It names the workspace, not the inbox: it becomes the tenant's `slug`
and its address suffix `.{username}`, and the first inbox is always `agent`, so the agent's address is
`agent.{username}@pylotamail.com`. That keeps the platform domain's one-suffix-per-tenant rule
([Identities and domains › Format](identity-domains.md#format)); after verification the person can add
inboxes such as `bookings.{username}@pylotamail.com`, and the `agent.` prefix of the first one tells every
correspondent that an agent created it. Global uniqueness then comes from the existing unique indexes on
`tenants.slug`, `tenants.address_suffix` and `tenants.suffix_fold`, which also settle a race between two
sign-ups for the same name ([X4](#24-edge-cases)).

`username` is ASCII-lower-cased and must then match `^[a-z0-9][a-z0-9-]{1,31}$` (2 to 32 characters: the
`slug` pattern, which is also the suffix pattern without its dot). It is checked with the address-suffix
rules of `POST /v1/tenants` ([REST API › Tenants](../../reference/api.md#post-v1tenants)), in this order:

1. A non-ASCII input that is confusable with a reserved name, mixed-script or right-to-left:
   `400 address_reserved`; any other non-ASCII input: `400 address_unsupported` (username validation
   steps 1–3, [Identities and domains › Username validation](identity-domains.md#username-validation)).
2. Not matching the pattern: `400 address_invalid`.
3. Its fold equal to the fold of a reserved username, of a name in `core::address::RESERVED_SUFFIXES`
   (`pylota`, `pylotamail`, `stripe`, `paypal`, `google` and the rest), or starting with `pm-`:
   `400 address_reserved` with `details.field = "username"`.
4. A `tenants` row with that `slug`, that `address_suffix` or that `suffix_fold`, or a tombstone for the
   address `agent.{username}@{PM_PLATFORM_DOMAIN}`: `409 username_taken`. A tenant row is never deleted
   ([Privacy §6.6](privacy.md#66-tenant-scope)), so the names of erased workspaces stay taken.

**A username is never released** ([X6](#24-edge-cases)). Releasing it after a quarantine period was
considered and refused, for three reasons. Tenant erasure keeps the row so that a slug and a suffix are
never reused ([Privacy §6.6](privacy.md#66-tenant-scope)). FR-ADR-5 tombstones the erased address, so the
same address could never be reassigned anyway. And people who wrote to `agent.{username}@` during the
provisional week would otherwise reach a stranger who registered the name later. Squatting is bounded by
the admission budgets of [§3.4](#34-admission-budgets): each burned name costs the squatter one of five
sign-ups a day per network, and the namespace of 2 to 32 characters is large. AgentMail behaves the same
way: a username stays with the organisation that lost its key.

## 3. Creating a sign-up: `POST /v1/agent-signup`

### 3.1 Request and response

No key. `Content-Type: application/json`; the body may be at most 4,096 bytes (`413 payload_too_large`
with `details.limit = 4096`).

```json
{
  "username": "travel-planner",
  "human_email": "sam@brightwell.example",
  "agent_name": "Travel planner",
  "source": "pylota-identity-python/1.2.0",
  "accept_terms": true
}
```

| Field | Rule |
|---|---|
| `username` | Required. [§2.3](#23-the-address-and-the-username) |
| `human_email` | Required: the address of the person responsible for the agent. [§3.3](#33-the-human-email-rules). AgentMail makes it optional and gives a receive-only inbox without it; Pylota Identity requires it, because FR-IDN-2 requires an accountable human and an inbox nobody answers for is the thing a spammer wants ([§27](#27-compared-with-agentmail-read-2026-10-17)) |
| `agent_name` | Optional. 1–78 characters, Unicode allowed, no CR or LF (the identity display-name rule). The identity's display name and the workspace's name. Never written into system mail ([§5](#5-the-verification-email)) |
| `source` | Optional. 1–64 characters of `[A-Za-z0-9._/+-]`, for example the SDK and its version. Stored on the sign-up row and shown to the operator. It has no `@`, so no address can be stored in it, and it is never a metric label (its values are unbounded) |
| `accept_terms` | Required and `true`: the caller accepts the Terms of Service, the Privacy Policy and the Data Processing Addendum (`PM_TERMS_URL`, `PM_PRIVACY_URL`, `PM_DPA_URL`, required whenever `PM_SIGNUP` is not `closed`). `PM_TERMS_VERSION` is stored on the sign-up row. The person accepts for themselves when they verify through the console ([§7.2](#72-the-link-path-in-the-console)) |

An unknown field, a wrong type or a value out of bounds: `400 invalid_request` with
`details.errors[]`. The route ignores `Idempotency-Key` and records nothing (`x-idempotency: none`): its
answer holds a secret, a replay could not return it (FR-KEY-2), and a replay without the key would be of no
use to an agent that lost the first answer ([X20](#24-edge-cases) says how it recovers).

`201`:

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

`verification.email` is `queued` (the system identity's submit accepted the email) or `not_sent`
([§12](#12-failure-handling)). The secret is shown in this answer only and stored only as its keyed hash
([Security § 4.1](security.md#41-key-format)).

### 3.2 Rules, in order

The first rule that fails decides the answer; nothing before step 10 writes to D1.

| # | Check | Failure |
|---|---|---|
| 1 | Agent sign-up is on ([§1](#1-when-agent-sign-up-is-on)) | `403 signup_closed` |
| 2 | `RL_SIGNIN`, keyed by the client network (`CF-Connecting-IP`; an IPv6 address by its /64 prefix), the binding the console's sign-in and sign-up forms already share | `429 rate_limited`, `details.bucket = "signin"` |
| 3 | The body: at most 4,096 bytes, valid JSON, the schema of [§3.1](#31-request-and-response), `accept_terms: true` | `413 payload_too_large`; `400 invalid_request` |
| 4 | `username` rules 1–3 of [§2.3](#23-the-address-and-the-username) | `400 address_unsupported`, `address_invalid` or `address_reserved` |
| 5 | `human_email` rules of [§3.3](#33-the-human-email-rules), syntax and policy | `400 address_invalid`, `address_unsupported`, or `invalid_request` with `details.reason` |
| 6 | `username` rule 4: one D1 read of `tenants` by `slug`, `address_suffix` and `suffix_fold`, and of `address_tombstones` by the address's keyed hash | `409 username_taken` |
| 7 | The mail domain of `human_email` can receive mail ([§3.3](#33-the-human-email-rules), last rule) | `400 invalid_request` (`no_mail_domain`); `503 unavailable` (`dns_unavailable`) |
| 8 | Caller admission: the `network`, `asn` and `deployment` budgets of [§3.4](#34-admission-budgets), checked and counted in one `TenantQuota` transaction | `429 signup_limit_reached`, `details.scope` = `network`, `asn` or `deployment` |
| 9 | The person: the refusal list (`agent_signup_refusals` has the human's base hash, [§9](#9-not-me-complaints-and-refusals)), then the person's budgets of [§3.4](#34-admission-budgets) and the class share, checked and counted in a second `TenantQuota` transaction | `429 signup_limit_reached`, `details.scope = "human_email"` (or `deployment` for the class share) |
| 10 | The create batch ([§3.5](#35-the-create-batch)) | `409 username_taken` (a lost race on the name); `429 signup_limit_reached` with `human_email` (a lost race on the person) |
| 11 | After the commit: the intents, then the verification email ([§5](#5-the-verification-email)) | none: a failed email gives `verification.email: "not_sent"` |

Steps 4 to 7 come before any counting, so a malformed or refused request costs no budget. The budgets are
spent at steps 8 and 9, before the batch, as the system-mail budgets of
[Cloud sign-up §10.2](cloud-signup.md#102-system-mail-budgets) are: a request that then loses a race at
step 10 has still spent its slot. That is deliberate: probing names or people by racing costs the prober.
The caller's own budgets are counted at step 8, before anything about the person is looked at, so the five
person-scope answers of [§3.4](#34-admission-budgets) cost a caller the same and cannot be told apart by
what they spend.

### 3.3 The human email rules

`core::signup::normalise_human(input)` lower-cases the address and converts its domain to an IDNA A-label,
then refuses, in this order:

1. Not an RFC 5321 address, or longer than 254 octets: `400 address_invalid`. A non-ASCII local part:
   `400 address_unsupported` (the system identity cannot mail it, [A3](../edge-cases.md)).
2. A **placeholder** domain: `example.com`, `example.net` and `example.org` and every name under them, and
   every name under the reserved top-level domains `.test`, `.example`, `.invalid` and `.localhost`
   (RFC 2606 §2 and §3, read 2026-10-17), which covers the simulator's `simulator.invalid`.
3. A **disposable** domain: the built-in list that ships with each release, or a domain in
   `PM_SIGNUP_BLOCKED_DOMAINS` (the list sign-up already uses, [W29](../edge-cases.md)).
4. A **hosted** address, whose mail this deployment receives, so that an agent of this deployment could
   read the code ([W45](../edge-cases.md)): the domain is `PM_PLATFORM_DOMAIN` or a name under it (which
   includes the agent's own future address `agent.{username}@`), or the address with everything from the
   first `+` of its local part removed is an `addresses` row in state `pending`, `active` or `retiring`.
   Inbound routing splits the local part at the first `+` and delivers to the base address
   ([Inbound › Steps](inbound.md#steps), step 2), so `ops+x@corp.example` reaches the identity
   `ops@corp.example`, which has no address row of its own. In the other direction, creating an identity or
   adding an address whose base address (below) hashes to the `human_hash` of a **provisional** sign-up is
   refused with `409 address_taken`, as an address equal to a console user's sign-in address already is
   ([Identities and domains › Create](identity-domains.md#create), step 4; [X44](#24-edge-cases)).

Rules 2 to 4 answer `400 invalid_request` with `details.errors[0].path = "human_email"` and the one
reason `details.reason = "human_email_not_allowed"`, whose `fix` says "Use the work or personal address of
the person responsible for this agent. Example, disposable and Pylota Identity addresses are refused." One
reason for the three keeps the answer from telling a caller which addresses of a customer's own domain are
identities, while still telling the agent what to change.

5. The domain cannot receive mail: the first resolver of `PM_DOH_RESOLVERS` (the second when the first
   fails) is asked for the domain's MX records through the existing `platform::Dns` trait. The domain passes
   when it has at least one MX record that is not a null MX, or no MX record and at least one A or AAAA
   record: "If an empty list of MXs is returned, the address is treated as if it was associated with an
   implicit MX RR" (RFC 5321 §5.1). It fails with `400 invalid_request`, `details.reason = "no_mail_domain"`,
   when the name does not exist, when its only MX is a **null MX** (a single MX of preference 0 whose
   exchange is ".", which RFC 7505 §3 defines as "this domain accepts no mail"), or when it has neither MX
   nor address records. When both resolvers fail or time out (2 seconds each) the answer is
   `503 unavailable` with `details.reason = "dns_unavailable"` and `Retry-After: 30`, and nothing is
   counted. Answers are cached per isolate for 5 minutes. The check removes typing mistakes and invented
   domains before they become bounces on the shared platform domain ([§13](#13-abuse-containment)).

**What is not refused.** An address that belongs to an existing console user, an owner of workspaces, or a
person with a pending invitation is accepted exactly like an unknown address ([X19](#24-edge-cases)). The
answer never depends on whether a person has an account, so the route is no oracle for accounts. AgentMail
refuses to attach an address that already has a console account; Pylota Identity links it at verification by
the verified address instead ([§7.1](#71-the-transaction)).

**The base address.** Every count and every refusal is keyed by the keyed hash of the **base address**,
which `core::signup::base_address` computes from the normalised address:

1. the local part with everything from the first `+` removed, so `jo+agent1@example.org` and
   `jo@example.org` are one person, as the one-Free-workspace rule already treats them
   ([Cloud sign-up §6.2](cloud-signup.md#62-after-launch-open-sign-up), step 3);
2. for `gmail.com` and `googlemail.com`, every `.` of the local part removed and the domain written
   `gmail.com`, because Google delivers every dotted form of a Gmail address to one mailbox and treats the
   two domains as one ([Dots don't matter in Gmail addresses](https://support.google.com/mail/answer/7436150)
   and [Getting someone else's mail](https://support.google.com/mail/answer/10313), read 2026-10-18). The
   same pages say dots do matter for an organisation's own domain, so no other domain is changed.

`human_hash = hex(HMAC-SHA256(PM_HASH_KEY, base address))`. The email itself goes to the address as
written. The hosted check of rule 4 uses only step 1, because inbound routing removes the `+` part and
nothing else.

### 3.4 Admission budgets

Steps 8 and 9 each make one `QuotaRequest::SystemMail { class: Signup, keys, day }` to the default
tenant's `TenantQuota`, which checks the budgets named in `keys` and counts them all in one transaction, or
counts none and answers `Spent { scope }` ([Outbound › TenantQuota](outbound.md#tenantquota); the request
already exists for the system-mail budgets, and this page adds the class `Signup`). Step 8 names the
caller's budgets (`network`, `asn`, `deployment`); step 9 names the person's (`recipient`, `human`) and the
class share. A request refused at step 9 keeps its step-8 counts. Days are UTC days; keys are keyed
hashes, never a clear address or IP; the counters are deleted with the day like the other `sysmail` rows.

| Counter `sysmail:signup:{scope}:{key}` | Counted on | Budget per UTC day | `details.scope` when spent |
|---|---|---|---|
| `recipient:{human_hash}` | Every code email: the sign-up's own, a resend, a replacement's | 5 | `human_email` |
| `human:{human_hash}` | A sign-up naming that person, and a replacement that names them | 3 | `human_email` |
| `network:{hash of the IPv4 address or IPv6 /64}` | A sign-up | 5 | `network` |
| `asn:{hash of request.cf.asn}` | A sign-up | 100 | `asn` |
| `deployment` | A sign-up | `PM_AGENT_SIGNUP_DAILY_CAP` (default 200) | `deployment` |
| the class's share | Every code email | 5% of the system identity's day ([Cloud sign-up §10.2](cloud-signup.md#102-system-mail-budgets)) | `deployment` |

- **`429 signup_limit_reached`** carries `details.scope` and `Retry-After` and `details.resets_at` set to
  the next 00:00 UTC. The scope `human_email` is given, with a byte-identical body apart from
  `request_id`, for five different causes: the person's daily sign-up budget, their daily email budget,
  their refusal ([§9](#9-not-me-complaints-and-refusals)), a live provisional sign-up that already names
  them (the unique index of [§3.5](#35-the-create-batch)), and a lost race for them
  ([X17](#24-edge-cases)). All five come after the caller's own budgets are counted (step 8), so they
  also cost the caller the same. The one bit it leaks, "this address cannot take a sign-up today", cannot
  be avoided while a person may have only one live sign-up, and it says nothing about accounts. A refusal
  lasts until the person lifts it while the other causes end within 7 days, so a caller who probes one
  address every day for more than a week can infer a refusal: that is the price of a refusal that lasts.
- **The class share.** `invitation` (20%), `notification` (50%) and now `signup` (5%) together can use at
  most 75% of the system identity's day, so at least 25% stays for `signin` and `account` mail. With the
  default 50,000 a day, 5% is 2,500 code emails, more than the default deployment cap can produce.
- **Why these numbers** (Pylota Identity choices). Five sign-ups a day per network is more than a developer
  testing an agent needs; an integrator that creates many agents must become a partner (FR-KEY-4). A
  hundred per ASN leaves room for agents on a shared cloud provider while bounding one provider's worth of
  abuse. Three sign-ups and five code emails a day to one person bound harassment of a person who never
  asked. 200 a day for the deployment bounds the whole feature to a size one operator can watch; the
  owner raises it with the variable.

### 3.5 The create batch

The handler mints the IDs (`asu_`, `ten_`, `idn_`, `adr_`, `key_`), the tenant's `TenantQuota` and
`Notifier` object IDs and the mailbox ID, a 32-byte key secret, a six-digit code (uniform, by rejection
sampling from `platform::Rng`, as sign-in codes are) and a 32-byte link token, then writes one D1 batch,
which runs as one transaction:

1. `INSERT INTO tenants (…, provisional_until) VALUES (…)`: the unique indexes `tenants.slug`,
   `tenants_suffix` and `tenants_suffix_fold` refuse a duplicate name, which fails the whole batch and
   answers `409 username_taken`.
2. `INSERT INTO billing_accounts` (`metered`, the default plan, the current period).
3. `INSERT INTO identities`, `INSERT INTO addresses` (the `addresses_address` unique index also refuses a
   duplicate address).
4. `INSERT INTO api_keys` (hash of the secret under `PM_KEY_PEPPER`).
5. `INSERT INTO sender_lists (tenant_id, 'send', 'allow', human_email, 'agent sign-up', now)`.
6. `INSERT INTO agent_signups (…)`: the partial unique index `agent_signups_live_human` on `human_hash`
   where `state = 'provisional'` refuses a second live sign-up for the same person, which fails the batch
   and answers `429 signup_limit_reached` with `details.scope = "human_email"`.
7. The `rpc_intents` rows for `QuotaRequest::Init`, `NotifierRequest::Init` and the mailbox `init` with its
   `identity.created` event ([Design conventions § 9](index.md#9-durable-object-calls-after-a-d1-change)).
8. Audit rows: `tenant.create`, `identity.create` and `key.create` with `tenant_id` set and
   `details_json.via = "agent_signup"`, and `agent_signup.create` with `tenant_id = NULL` (a
   deployment-level row that outlives the tenant's erasure, read by the spike alert,
   [§21](#21-observability)), whose `details_json` holds the `asu_` and `ten_` IDs, the network hash and
   `source`, never an address.

No `inboxes` or `seats` hold is taken: a new tenant has no other identity or member, the Free plan grants
5 inboxes and 1 seat, and the `TenantQuota` counts them from D1 when `QuotaRequest::Init` and the
first `SetPlan` and `Reconcile` run, as for a workspace created through `POST /v1/tenants`
([Plans, metering and billing › Reconciliation against D1](billing.md#reconciliation-against-d1)). After
the commit the handler sends the three intents (a failed call is repaired by `crons/intents.rs`,
[J21](../edge-cases.md)), submits the verification email, and answers `201`.

## 4. While provisional

### 4.1 Permissions

The key is minted with the **verified agent set**, the permissions the agent keeps after verification:

| Set | Permissions |
|---|---|
| Provisional (effective until verified) | `identities:read`, `messages:read`, `messages:send`, `messages:write`, `attachments:read`, `search:read`, `events:read` ([Event feed](event-feed.md)), and the implicit `usage:read` of every identity key |
| Verified agent set (minted) | The provisional set plus `identities:sign`, `search:agentic`, `accounts:request`, `drafts:read` and `drafts:write` ([Drafts](drafts.md)) |
| Never | Everything else: `identities:write`, `keys:manage`, `webhooks:read` and `webhooks:manage`, `domains:write`, `quarantine:review`, `erasure:manage`, `drafts:approve` (identity keys never hold it), `metrics:read`, and every tenant-only permission ([Security § 4.6](security.md#46-creating-keys-fr-key-1)) |

**The overlay.** While the key's tenant has `provisional_until` set, authentication
([Security § 4.2](security.md#42-verification), step 9) resolves the key's permissions as the
intersection of its list with the provisional set (`core::signup::overlay_permissions`), for every key of
that tenant, on REST and MCP alike (MCP tools call the same route entries). A route that needs a
permission the overlay removed answers `403 permission_denied` with `details.required` and
`details.reason = "signup_unverified"`, whose `fix` points to `POST /v1/agent-signup/verify`. When the
tenant is verified the overlay ends on the next request: the key is not rotated or rewritten. The
platform key that mints the key at sign-up does not exist: the sign-up handler writes the `api_keys` row
itself, as `idvy setup` writes the bootstrap key, and the `key.create` audit row records
`details.granted_without_holding = ["identities:sign"]` and `via: "agent_signup"`, as a partner key's grant
does ([Security § 4.6](security.md#46-creating-keys-fr-key-1), step 3).

So, until verification, the agent **cannot**: mint or rotate keys (no `keys:manage`); create webhooks,
domains, identities, lists, partners or tenants (identity keys never could, and `identities:write` is
absent); request service accounts (`accounts:request` is removed: there is nobody to approve them, and
unverified agents opening accounts elsewhere is the abuse the ledger exists to stop); sign assertions or
HTTP requests (`identities:sign` is removed: an identity with no accountable human must not vouch for
itself to third parties); run agentic search or the model (`search:agentic` is removed); write drafts;
delete its identity (`identities:write` and `erasure:manage`; it abandons instead,
[§6.5](#65-delete-v1agent-signup)); or release quarantined mail.

### 4.2 Policy overlay

While `provisional_until` is set, `core::policy::effective` applies one more layer after the built-in
defaults, `PM_DEFAULT_POLICY` and `policy_json`: `PROVISIONAL_POLICY`, merged so that each field takes the
**stricter** of its effective value and the overlay's (lower for numbers; the stricter value of each
switch as [Workspace policy §2](workspace-policy.md#2-classes-and-ceilings) defines it; for enums the
order named below). The overlay is code, not stored policy, so verification needs no policy write and
leaves no ceiling behind. Every reader of the effective policy (the send path, the inbound consumer,
the screening job, search and triage) passes the tenant's `provisional_until` with `policy_json`, which they already read
from the same row.

| Field | Overlay value | Why |
|---|---|---|
| `identity_daily_send_cap`, `tenant_daily_send_cap` | 10 | The send limit of [§4.3](#43-sending-only-to-the-human) |
| `max_recipients` | 1 | One person may be written to |
| `send_allowlist_only` (guard) | `true` | Only the human's send-allow entry passes ([§2.1](#21-what-a-sign-up-creates)) |
| `quarantine.on_auth_fail` (guard) | `true` | |
| `quarantine.unsolicited_otp` (guard) | `true` | |
| `quarantine.spam_threshold` (guard) | 0.5 (default 0.8) | Stricter. Not 0, its strictest valid value: at 0 every message would be held, the person's own replies included, and with no person in the workspace nobody could release them before verification |
| `screening.mode`, `screening.on_unavailable` (guards, [Screening](screening.md)) | `hold`, `quarantine` | Mail that could steer an unaccountable agent is held, and held when the screen cannot decide |
| `drafts.require_approval` (guard, [Drafts](drafts.md)) | not in the overlay | Step 16a exempts a provisional tenant's sends, which step 7a has already confined to the human ([§4.3](#43-sending-only-to-the-human)). Any level that needs approval would otherwise refuse the human's mail with nobody able to approve it, since the key holds no draft permission, and the overlay can only make a field stricter, so a stricter deployment or platform value would still apply ([Drafts §3.1](drafts.md#31-draftsrequire_approval)) |
| `accounts.require_approval` | `true` | Verification codes from services reach no provisional agent ([Service sign-up ledger §5](service-accounts.md#5-the-verification-gate), [X24](#24-edge-cases)) |
| `auto_reply.allowed` | `false` | |
| `inbound.per_sender_per_hour`, `inbound.unauthenticated_per_hour`, `inbound.per_tenant_per_hour` | 20, 20, 100 | A provisional inbox is not a bulk receiver; the excess is stored `throttled` |
| `inbound.extract_image_text`, `triage.enabled`, `search.agentic_enabled` | `false` | No triage, image text or agentic search is paid for a workspace nobody answers for |
| `triage.daily_model_cap` | 50 | It also bounds screening's model calls ([Screening](screening.md), step 7 of its job): at most 50 a day; past it a held message's verdict is `unavailable` and, with `on_unavailable: quarantine`, it is quarantined (fail closed) |
| `large_attachments` | `refuse` | No signed links to files from a provisional workspace |
| `quarantine.key_release` | `false` (unchanged) | Only a person may release, and none exists until verification |

`GET /v1/tenants/{tenant_id}/policy` (only a platform key reaches a provisional tenant) returns the
effective policy with the overlay applied and `"provisional_overlay": true`. A platform key may still
write the tenant's policy; the value is stored and the overlay keeps winning until verification
([X56](#24-edge-cases)). The upload routes of [Uploads](uploads.md) answer `403 permission_denied`
(`signup_unverified`) while provisional, although `messages:send` would allow them: inline attachments
within the transport limit are enough for ten messages a day to one person, and uploads would make a
provisional workspace free file storage.

### 4.3 Sending only to the human

FR-IDN-2 says an identity must not send until it records an accountable human. A provisional identity has
none (its `owner_name` and `owner_email` stay `NULL` until verification), yet it must be able to tell its
person what it wants. The send policy pipeline ([Outbound › Policy pipeline](outbound.md#policy-pipeline))
therefore changes in two places, and FR-IDN-2 gains the matching exception ([§23](#23-functional-requirements)):

- **Step 4** (no accountable human, `409 identity_owner_required`) passes for an identity of a tenant whose
  `provisional_until` is set, and only because step 7a follows.
- **New step 7a**, after the recipient count of step 7, for a provisional tenant only: every recipient in
  `to`, `cc` and `bcc` (after step 6 removed duplicates) must equal the sign-up's human address, compared as
  normalised addresses (exactly, not as base addresses). Otherwise `403 policy_denied` with
  `details.reason = "signup_unverified"`. When the sign-up's `human_status` is `bounced`, every send is
  refused with `403 policy_denied`, `details.reason = "signup_human_unreachable"` ([X38](#24-edge-cases)).
  While the `ops_switches` row `agent_signup` is `off` ([§13.2](#132-the-automatic-switch)), every send of
  every provisional tenant is refused with `403 policy_denied`, `details.reason = "signup_paused"`
  ([X3](#24-edge-cases)): the switch fires when sign-up mail draws bounces or complaints, and mail from up
  to 1,400 provisional workspaces to people who never confirmed anything is the same risk to the shared
  domain. The handler reads the row with the `free_sending` switch, cached for 60 seconds per isolate
  ([Outbound › Handler](outbound.md#handler), step 6), and applies it to provisional tenants only.
  Reply, reply-all and forward run the same step: a reply to a stranger who wrote to the inbox is refused,
  a forward of that mail to the person is allowed. Nothing is stored for a refused send, and a dry run
  returns the same error.

The handler reads the human address with one D1 query (`SELECT human_email, human_status FROM
agent_signups WHERE tenant_id = ?1 AND state = 'provisional'`) only when the key's tenant is provisional,
so other tenants pay nothing. Then the rest of the pipeline runs unchanged: step 16 lets the human through
(the send-allow entry makes them allowed and known), step 18 counts the 10-a-day caps of the overlay
(`429 daily_cap_reached`), the `sends` hold of the Free plan, the shared-domain breaker and the
`free_sending` switch ([X51](#24-edge-cases)).

**Why 10 a day** (a Pylota Identity choice, the same number AgentMail uses). It is enough for an agent to ask
for the code, report progress several times a day and answer the person, and not enough for a person who
never asked to be flooded: with `max_recipients` 1 it is at most 10 emails a day to one address, on top
of at most 5 code emails from the system identity.

### 4.4 What else is refused or limited

| Feature | While provisional |
|---|---|
| `wait` with `kind=verification` | `403 policy_denied` (`account_not_approved`): no ledger entry can be approved ([Service sign-up ledger §5](service-accounts.md#5-the-verification-gate)) |
| Codes and links from other services | Held as `quarantined` (`account_unapproved` or `otp_unsolicited`), invisible to the key, released only by the owner after verification |
| Webhooks | None can exist: an identity key cannot create an endpoint. The agent reads its own identity's events from the event feed (`events:read`), where verification shows as `identity.updated` and `signup.verified` ([§18](#18-events-and-audit)), or polls `GET /v1/agent-signup` |
| Members, invitations | None: no person belongs to the workspace and no key holds `members:manage` |
| Semantic indexing | Runs as for any tenant, bounded by the inbound caps of the overlay and `RL_EMBED` |
| Test mode | Not available: a sign-up always creates a `live` tenant and a `pmk_live_` key ([X55](#24-edge-cases)) |

## 5. The verification email

Every code is sent in one email from the system identity (`PM_SYSTEM_FROM`, through the platform domain,
[Identities and domains › The system identity](identity-domains.md#the-system-identity)), through the
normal outbound pipeline, as sign-in mail is:

- **Class and budgets.** System-mail class `signup` ([§3.4](#34-admission-budgets)). The sign-up's own
  email was counted at admission; a resend or a replacement counts its email in the same request before
  it writes anything.
- **Submit.** `kind: "transactional"`, `Idempotency-Key: asu:{signup_id}:{code_seq}`, and
  `metadata.agent_signup_id = "asu_…"`, which the system identity's mailbox uses to recognise the email's
  delivery outcomes ([§13.3](#133-outcomes-of-sign-up-mail)), as it recognises notifications by
  `metadata.notify_user_id`. The returned `msg_` ID is appended to `agent_signups.mail_message_ids_json`.
  Before submitting, the handler reads the platform domain's state (cached 60 seconds): while it is
  `failing` or `suspended` nothing is submitted and the email is `not_sent`, because the platform domain
  has no fallback and a submit would be accepted and then fail ([Notifications § 7](notifications.md#7-when-system-mail-cannot-be-sent)).
- **Stored redacted.** The system identity's mailbox replaces every URL with `[link]` and every run of six
  or more digits with `[code]` before the row is written, and deletes the outbound `.eml` once it leaves
  `queued` ([Inbound › The system identity's mailbox](inbound.md#the-system-identitys-mailbox)), so no
  stored copy holds the code or the link.
- **Content.** A fixed template. The only value from the request is the agent's address, whose characters
  are `[a-z0-9.-]`; `agent_name` and `source`, which are free text, never appear, so the email cannot be
  turned into a phishing message.

  ```text
  Subject: [Pylota Identity] An AI agent named you as its accountable human

  An AI agent signed up to Pylota Identity and gave this address as the person responsible for it.
  Its address is agent.travel-planner@pylotamail.com.

  If you set up this agent, give it this code: 482913
  Or confirm in your browser: https://app.pylotamail.com/console/agent-signup/verify?t=…

  Giving the code, or confirming, makes you the owner of the agent's workspace and the person
  answerable for the mail it sends. Pylota Identity never asks you for this code.
  The code expires at 09:00 UTC on 18 October 2026.

  Not you? Ignore this email: the agent can only write to you, at most 10 emails a day, and its
  inbox is deleted on 24 October 2026. We will not email you about it again.
  To stop AI agents naming you on Pylota Identity: https://app.pylotamail.com/console/agent-signup/refuse?t=…

  Terms: https://pylotamail.com/terms · Privacy: https://pylotamail.com/privacy
  ```

- **One-click refusal.** The email carries `List-Unsubscribe: <https://{PM_CONSOLE_HOST}/console/agent-signup/refuse?t={token}>`
  and `List-Unsubscribe-Post: List-Unsubscribe=One-Click`. RFC 8058 §3.1 (read 2026-10-17) requires one
  HTTPS URI, a POST that needs no cookies, authorisation or other context, no redirect in answer to it, and
  a DKIM signature covering both headers (§4). Mail providers then offer "unsubscribe" beside "report spam",
  and a person who did not ask is more likely to refuse than to complain, which protects the platform
  domain's reputation. Whether Cloudflare Email Sending's DKIM signature lists both headers in `h=` is not
  documented on the pages read; `live::signup::x40_refuse_one_click_headers` checks it on a real message,
  as the notification emails already need ([Notifications §5](notifications.md#5-the-emails)).

## 6. The agent's routes

All six routes act on **the calling key's own sign-up**: the one row of `agent_signups` whose
`tenant_id` is the key's tenant. They are registered with an empty permission list and a new scope rule,
`Scope::AgentSignup` (the key is a tenant or identity key whose tenant has an `agent_signups` row; any
other tenant or identity key gets `404 signup_not_found`, and a platform or partner key
`403 scope_denied`). The empty list is a new exception to the rule of
[Security § 5.1](security.md#51-deny-by-default-router-table), because the routes must work before and
after the overlay and no permission names "my own sign-up". `RL_API` and `RL_TENANT` apply as to every
key.

### 6.1 `GET /v1/agent-signup`

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

`permissions` is the key's effective set, with the overlay. `human_status` is `pending`, `delivered` or
`bounced` ([§13.3](#133-outcomes-of-sign-up-mail)). A verified sign-up answers with `state: "verified"`,
`verified_at`, `verified_via` (`code`, `link` or `claim`), `human_email_hint`, and `code: null`. A sign-up
that ended (`expired`, `abandoned`, `refused`) answers with its state until the erasure revokes the key,
then `401 key_revoked`.

### 6.2 `POST /v1/agent-signup/verify`

`{ "code": "482913" }`. `Idempotency-Key` optional. In order:

1. `state` is `verified`: `200` with the sign-up and `"already_verified": true` (a retry after a lost
   answer is safe, [X28](#24-edge-cases)). Any other state but `provisional`: `409 signup_not_pending` with
   `details.state`.
2. The tenant is `suspended`: `403 tenant_suspended`. `expires_at` has passed: `409 signup_not_pending`
   with `details.state = "expiring"`; the request erases nothing itself.
3. `code` is not six ASCII digits: `400 invalid_request`.
4. **Reserve an attempt** in one statement, before comparing, so that concurrent guesses cannot exceed the
   limit ([X27](#24-edge-cases)):

   ```sql
   UPDATE agent_signups SET attempts = attempts + 1, updated_at = ?now
   WHERE id = ?1 AND state = 'provisional' AND code_hash IS NOT NULL
     AND code_expires_at > ?now AND attempts < 10 AND failed_total < 30
   RETURNING code_seq, code_hash, key_kid;
   ```

   No row: the answer is `422 signup_code_invalid` with `details.reason` from a re-read: `locked` (30
   failed codes over the sign-up's life; the link and the claim still work), `burned` (10 attempts at
   this code), `expired` (past 24 hours, or hashed under a `link` key that was revoked, [X53](#24-edge-cases)),
   or `none` (no code yet). Each `fix` names `POST /v1/agent-signup/resend` or the link.
5. **Compare** `HMAC-SHA256(link key {key_kid}, "asu:" || signup_id || ":" || code_seq || ":" || code)`
   with `code_hash` through `Mac::verify_slice`, which compares in constant time
   ([Security § 4.2](security.md#42-verification), step 4).
6. **Wrong**: `UPDATE agent_signups SET failed_total = failed_total + 1 WHERE id = ?1 AND code_seq = ?seq`;
   when the reserved attempt was the tenth, the same statement sets `code_hash = NULL`. Answer
   `422 signup_code_invalid` with `details.reason = "wrong"`, `details.attempts_left` and
   `details.failures_left`.
7. **Right**: the verification transaction of [§7.1](#71-the-transaction) with `via = "code"` and the
   `code_seq` that was compared, so a code made stale by a resend or replacement between steps 4 and 7
   verifies nothing ([X59](#24-edge-cases)). `200` with the verified sign-up.

**Why these numbers** (Pylota Identity choices; AgentMail uses 24 hours and 10 attempts). Six digits give
10^6 codes. At most 30 wrong codes over a sign-up's life bound an agent that names a stranger and guesses
to a chance of 30 in 10^6 per sign-up, and each sign-up costs one of five a day per network. 24 hours lets
a person who reads mail once a day answer.

### 6.3 `POST /v1/agent-signup/resend`

No body. `Idempotency-Key` optional (a replay sends nothing). Issues a new code and link and emails them,
which makes the previous code and link invalid. In order: `state` must be `provisional`
(`409 signup_not_pending`); the tenant not suspended (`403 tenant_suspended`); not past `expires_at`
(`409 signup_not_pending`); agent sign-up not paused by the switch (`403 signup_closed`, `paused`);
`human_status` not `bounced` (`409 signup_human_unreachable`, whose `fix` names
`PUT /v1/agent-signup/human`); at most 3 code emails in the last hour for this sign-up, counted from
`sends_json`, the sign-up's own email included (`429 signup_limit_reached`, `details.scope = "resend"`,
`Retry-After` until the oldest of the three is an hour old); then `QuotaRequest::SystemMail` with the keys
`recipient:{human_hash}` and the class share (`429 signup_limit_reached`, `human_email` or `deployment`).
Then one guarded statement:

```sql
UPDATE agent_signups SET code_seq = code_seq + 1, code_hash = ?c, link_hash = ?l, key_kid = ?kid,
       code_expires_at = ?now + 86400000, attempts = 0, sends_json = ?sends, updated_at = ?now
WHERE id = ?1 AND state = 'provisional' AND expires_at > ?now AND code_seq = ?seq;
```

No row changed: another resend, replacement or verification won; the answer comes from a re-read. Then
the email ([§5](#5-the-verification-email)) and `202` with the sign-up. `failed_total` is never reset.

### 6.4 `PUT /v1/agent-signup/human`

`{ "human_email": "sam.patel@brightwell.example" }`. `Idempotency-Key` optional. Replaces the person, for
example after a typing mistake; AgentMail allows the same, twice.

- The normalised address equals the current one: the request is a resend ([§6.3](#63-post-v1agent-signupresend)),
  and no replacement is counted.
- Otherwise, in order: `state` `provisional`, not suspended, not past `expires_at`, not paused, as for a
  resend; `replacements_used < 2`, otherwise `422 signup_replace_limit`; the human-email rules 1 to 5 of
  [§3.3](#33-the-human-email-rules) for the new address; the refusal list
  (`429 signup_limit_reached`, `human_email`); `QuotaRequest::SystemMail` with `recipient:{new hash}`,
  `human:{new hash}` and the class share. Then one D1 batch: the guarded `UPDATE` of the sign-up (new
  `human_email`, `human_hash`, `human_status = 'pending'`, `replacements_used + 1`, a new code and link,
  `attempts = 0`, `WHERE … AND state = 'provisional' AND human_hash = ?old AND code_seq = ?seq`, so two
  replacements racing cannot both win); the unique index `agent_signups_live_human` refuses a new person
  who already has a live sign-up (`429 signup_limit_reached`, `human_email`); the old send-allow entry is
  deleted and the new one inserted; the audit row `agent_signup.human_replace` (hashes only). Then the email
  to the new person, and `200`.
- The replaced person receives nothing more. Their code and link stop working at once; their
  refusal link still records their own refusal ([§9](#9-not-me-complaints-and-refusals), [X31](#24-edge-cases)).
- A replacement does **not** move `expires_at`: the 7 days count from the sign-up, or two replacements
  would stretch a provisional workspace to three weeks.

### 6.5 `DELETE /v1/agent-signup`

Abandons a provisional sign-up, for example when the agent's key leaked or the agent was set up by
mistake. Only in state `provisional` (otherwise `409 signup_not_pending`: a verified workspace is deleted
by its owner, [Privacy §6.6](privacy.md#66-tenant-scope)). One D1 batch sets `state = 'abandoned'` and starts
the erasure of [§10](#10-expiry-abandonment-and-refusal-erasure), with the calling key as
`created_by_key_id`. `202` with `{ "state": "abandoned", "erasure_request_id": "era_…" }`. The
erasure's first step revokes the key, so the next request is `401 key_revoked`. Budgets are not refunded
and the username stays taken ([X42](#24-edge-cases)).

### 6.6 `GET /v1/platform/agent-signups`

Platform keys with `platform:ops`. The operator's view for an alert or a runbook ([§21](#21-observability)).
Filters `state`, `created_after`, `network` (the 16-hex network hash shown in a row); standard cursor
pagination. Each row: `signup_id`, `tenant_id`, `state`, `address`, `human_email_hint`, `human_status`,
`source`, `network`, `created_at`, `expires_at`, `verified_at`, `verified_via`. No clear address of a
person is returned. The operator contains a sign-up with the existing tools: suspend its tenant
(`PATCH /v1/tenants/{tenant_id}`), erase it (a tenant-scope erasure request), or turn the switch off.

## 7. Verification

### 7.1 The transaction

`signup::verify::complete(signup_id, via, expected_code_seq, person)` is the one function behind the
code, the link, the pending second step of a link, and the claim. It first re-reads the sign-up and its
tenant and checks, in order:

1. `state = 'provisional'` and `expires_at > now`; otherwise the state's answer.
2. The tenant is not `suspended` (`403 tenant_suspended`), `erasing` or `erased` (the key is already
   revoked; the console pages show "This inbox no longer exists").
3. The human address is still not hosted ([§3.3](#33-the-human-email-rules), rule 4, which compares its
   `+`-stripped base): another tenant may have created an identity at that address since
   ([X44](#24-edge-cases)). Otherwise `409 signup_cannot_verify`, `details.reason = "human_email_hosted"`.
4. **One Free workspace per person** (FR-CON-18): the person, or another person whose address is the same
   base address, already owns a workspace on the catalog's default plan (the query of
   [Cloud sign-up §6.2](cloud-signup.md#62-after-launch-open-sign-up), step 3). Otherwise
   `409 signup_cannot_verify`, `details.reason = "workspace_limit"`. The code is not burned and the attempt
   is not a failure, so the agent can submit the same code again once the person has upgraded or deleted
   the other workspace ([X45](#24-edge-cases)). AgentMail also allows one organisation per person. The
   query matches `+tag` variants only: it compares `users.email`, which holds the address as each person
   signed up, so two dotted forms of one Gmail address count as two people here, as they do in Cloud
   sign-up's own check. Closing that needs a stored canonical form on `users`, which this page does not add
   (the sign-up's own budgets, refusal list and live index use the Gmail rule of
   [§3.3](#33-the-human-email-rules)).
5. A `users` row with the human address and `status = 'disabled'`: `409 signup_cannot_verify`,
   `details.reason = "person_disabled"` ([X46](#24-edge-cases)).

Then one D1 batch. The first statement claims the sign-up; every later statement is guarded by a random
`verify_token` that only the winner wrote, so a concurrent verification that lost changes nothing
([X28](#24-edge-cases), [X34](#24-edge-cases)):

```sql
UPDATE agent_signups
SET state = 'verified', verified_at = ?now, verified_via = ?via, verified_user_id = ?usr,
    verify_token = ?tok, human_email = NULL, code_hash = NULL, link_hash = NULL, updated_at = ?now
WHERE id = ?1 AND state = 'provisional' AND expires_at > ?now
  AND (?via <> 'code' OR code_seq = ?seq);
-- each statement below ends with:  … WHERE EXISTS (SELECT 1 FROM agent_signups WHERE id = ?1 AND verify_token = ?tok)
INSERT INTO users (id, email, status, created_at, terms_version, terms_accepted_at) …;   -- only when no row exists
INSERT INTO members (tenant_id, user_id, role, created_at) … 'owner' …;
UPDATE tenants SET provisional_until = NULL, updated_at = ?now WHERE id = ?ten;
UPDATE identities SET owner_name = ?name, owner_email = ?email, updated_at = ?now WHERE id = ?idn;
UPDATE users SET last_tenant_id = COALESCE(last_tenant_id, ?ten) WHERE id = ?usr;
INSERT INTO rpc_intents …;            -- mailbox emit_event identity.updated (changed: ["owner"])
INSERT INTO event_index …;            -- signup.verified, a platform event (§18)
INSERT INTO audit_log …;              -- agent_signup.verify (tenant_id set, actor_user_id when known)
INSERT INTO audit_log …;              -- member.join (the owner)
```

- **The person.** The address is the one the code or link was sent to, so using either proves control of
  it, the same proof as a sign-up code (FR-CON-8) and the code that FR-CON-17 asks for before linking. An
  existing `users` row with that exact address is used; otherwise one is created. `owner_name` is the
  person's `users.name`, else the local part of the address; `owner_email` is the address.
- **Changed nothing.** When the first statement changed no row, the handler re-reads: verified by this
  same person is success (`200`, or the console's success page); verified by someone else, or ended, is
  the state's answer; still provisional with a newer `code_seq` (a resend or replacement committed after the
  attempt was reserved) is `422 signup_code_invalid` with `details.reason = "superseded"`, which is not
  counted as a failure ([X59](#24-edge-cases)).
- **After the commit.** The handler sends the intent, queues the event's `Fanout`
  ([Webhooks › Platform events](webhooks.md#platform-events)), and calls
  `NotifierRequest::Account { user_id, event: agent_signup_verified }` on the `Notifier` that [Notifications § 3](notifications.md#3-how-notifications-are-produced) picks for account events:
  an `account` email, which cannot be turned off, telling the person that they now own the workspace, that
  an agent holds its key, and where to see the key, revoke it or delete the workspace. It is sent on every
  path, because a person who gave the code after being deceived must learn what it did.

### 7.2 The link path in the console

`https://{PM_CONSOLE_HOST}/console/agent-signup/verify?t={token}`. The token is the 32-byte link token
with the `link` key's kid as its first character, stored only as `link_hash`
([Console › Keyed hashes](console.md#keyed-hashes)). The flow is the invitation-accept flow
([Console › Invitations](console.md#invitations)), with the same rules for sessions and second factors
(FR-CON-16, [W40](../edge-cases.md)):

- `GET` changes nothing, so a mail scanner cannot verify ([X37](#24-edge-cases)). With a valid,
  unexpired token of a provisional sign-up it shows the agent's address, the person's address, the
  expiry, what confirming means, the terms checkbox (required) and one **Confirm** button, a form `POST`
  of the token. Any other token shows "This link has expired or was already used", whatever the reason.
- The `POST` is a pre-session form, checked by `Origin` ([Console › CSRF](console.md#csrf)), and counts
  against `RL_SIGNIN`. Without the checkbox it shows the page again, marked, and does nothing. Then:
  - a browser with a session of the person whose address this is confirms in that session and makes the
    new workspace active;
  - a session of another person shows whose session it is and offers sign-out; nothing is confirmed;
  - no session, and a person without two-step verification or no `users` row yet: the transaction of
    [§7.1](#71-the-transaction) with `via = "link"`, which also creates a session with the new workspace
    active, in the same batch, and records `terms_version` and `terms_accepted_at` on the person;
  - no session, and a person enrolled in two-step verification: nothing is confirmed yet. The pending step
    `two_factor` starts with the new column `pending_auth.agent_signup_id` set
    ([Cloud sign-up §5.1](cloud-signup.md#51-the-pending-step)); only when the second factor passes do the
    transaction and the session commit, in one batch. An abandoned step confirms nothing, and the link
    still works until the code expires.
- Success lands on the new workspace's Overview, with the banner of [§19](#19-console).

### 7.3 After verification

- The tenant is an ordinary self-serve Free workspace with one owner: the person can rename it, add
  inboxes, invite members, mint keys, add a domain or upgrade. The key's verified set applies on its next
  request; the policy overlay is gone on the next message or submit.
- **The send ramp counts from verification.** `ramp_lifted_at` is `NULL`, so the new-workspace ramp
  applies (FR-CON-13). For a tenant with a verified `agent_signups` row, the daily evaluation of
  `crons/signup_ramp.rs` uses `verified_at` instead of the tenant's `created_at`, both for the 7 days and
  for `OutcomeRates { since }`, because the workspace could not send freely before ([X58](#24-edge-cases)).
- Quarantined mail of the provisional week stays quarantined; the owner reviews and releases it in the
  console as any quarantined mail (FR-CON-6).
- The `agent_signups` row stays, with the clear address removed, for the ramp, the operator's listing and
  the provenance of the workspace; it is deleted with the tenant ([§16](#16-privacy)).

## 8. Claiming in the console

`/console/agent-signup/claim` lets a signed-in person who did not get, or lost, the email take the
workspace by pasting the agent's key. Unlike AgentMail, where any console user may claim an inbox that has
no person attached, a claim here works only for the person the agent named: the session's sign-in address
(proved at sign-in) must equal the sign-up's human address exactly. A key alone is not a proof of
consent, because the agent itself holds it.

- **Who.** Any signed-in person, with or without a workspace. The link appears in the user menu and on the
  "No workspace yet" page only while `PM_AGENT_SIGNUP = "on"`; the page itself answers at its path whatever
  the variable, so a person named before the feature was turned off can still claim
  ([§1](#1-when-agent-sign-up-is-on)). The `POST` is a sensitive action: it needs a sign-in within the
  last 10 minutes ([Console › Re-authentication](console.md#re-authentication)), the CSRF token, and
  counts against `RL_SIGNIN`.
- **The form.** One `<input type="password" name="key" autocomplete="off">` and **Continue**. The key is
  handled in memory only: it is never logged, stored, written to an audit row or echoed back in a page
  ([Security § 12](security.md#12-logging-rules)).
- **Steps.** The handler checks the format, then authenticates the key with `auth::authenticate` (the
  constant-time path of [Security § 4.2](security.md#42-verification)), then requires an identity key
  whose tenant has an `agent_signups` row. The first refusal decides the page:

| Condition | Page | Changes |
|---|---|---|
| Not in the key format | "This does not look like a Pylota Identity API key. Paste the whole key, starting with `pmk_live_`." | nothing |
| Unknown, wrong secret, revoked or expired (one page for all four, as authentication answers them alike) | "This key is not valid. It may have been revoked, or the inbox may have been deleted." | nothing |
| A valid key that is not an agent sign-up's: a key of an ordinary workspace, of a partner's tenant (every Pylota-managed tenant), a partner key or a platform key | "This key cannot be used to claim an inbox." | nothing |
| The sign-up is `verified` | "This inbox has already been confirmed by its owner." | nothing |
| The sign-up ended, or its tenant is `erasing` or `erased` | "This inbox no longer exists." | nothing |
| The tenant is `suspended` | "This inbox cannot be claimed." | nothing |
| The session's address differs from the human address | "This inbox names a different accountable human (s***@brightwell.example). Sign in with that address to claim it." The masked hint tells nothing that the key's holder cannot read from `GET /v1/agent-signup` | nothing |
| `signup_cannot_verify` (`workspace_limit`) | "You already own a Free workspace (Brightwell). Pylota Identity allows one per person: upgrade or delete it, or create an inbox and a key there for your agent." | nothing |
| `signup_cannot_verify` (`human_email_hosted`) | "This inbox cannot be claimed." | nothing |
| Valid | A confirmation page: the agent's address, what claiming means, and **Claim inbox** (a second `POST` with the CSRF token and the key) | nothing yet |

  **Claim inbox** runs the transaction of [§7.1](#71-the-transaction) with `via = "claim"` and the session's
  person. A race lost to the link or the code by the same person shows the success page; lost to an
  expiry, an abandonment or a refusal, "This inbox changed while you were claiming it. Nothing was
  claimed." ([X34](#24-edge-cases)). Success makes the workspace active in the session, lands on its
  Overview, and shows: "The agent keeps its key. If the key was sent to you by email or chat, revoke it on
  the keys page and give the agent a new one" ([X50](#24-edge-cases)).

## 9. "Not me", complaints and refusals

A person who never asked must be able to stop it in one click, and an agent must not be able to name them
again.

- **The refuse link.** `https://{PM_CONSOLE_HOST}/console/agent-signup/refuse?t={token}`, in the body and in
  `List-Unsubscribe` of every code email. The token has the construction of the notification unsubscribe
  token, with its own prefix so that neither can be used as the other
  ([Notifications §5](notifications.md#5-the-emails)):

  ```text
  token   = base64url(payload || HMAC-SHA256(link key {kid}, payload)[0..16])    -- no padding, ≤ 256 characters
  payload = "r1:{kid}:{signup_id}:{human_hash}:{expires_unix_s}"                -- expires = issued + 90 days
  ```

  The token outlives the sign-up: a person who finds the email after the 7 days, or after the sign-up was
  abandoned or erased, can still stop agents naming them, so "Unsubscribe" never fails silently and turns
  into a spam report. 90 days is the notification unsubscribe token's life
  ([Security § 4.9](security.md#49-console-authentication-and-hosts)); like it, the token also stops working
  7 days after its `link` key is rotated.

- **`GET`** changes nothing: it shows "Stop AI agents naming {masked address} as their accountable human on
  Pylota Identity" with one **Stop** button, a form `POST` to the same URL. **`POST`** verifies the token (the
  `r1` prefix, a known kid inside its verify window, the MAC in constant time, the expiry); any failure
  shows the same expired page. Like the unsubscribe pair, both need no session, the `POST` is exempt from
  the CSRF token and the `Origin` check, and the `POST` never redirects (RFC 8058 §3.1), because a mail
  provider sends it without any of them ([X40](#24-edge-cases)). Neither counts against `RL_SIGNIN`: the
  one-click `POST`s of a large provider all come from its own networks.
- **Effect**, in one D1 batch: `INSERT OR IGNORE INTO agent_signup_refusals (human_hash, reason, created_at)
  VALUES (?h, 'refused', ?now)`; and, when the sign-up is still `provisional` **and** its current
  `human_hash` equals the token's, `state = 'refused'` and the erasure of [§10](#10-expiry-abandonment-and-refusal-erasure).
  A person whom the agent has since replaced records only their own refusal; the sign-up continues with the
  new person ([X31](#24-edge-cases)). When the sign-up has ended or its row is gone, only the refusal is
  recorded, from the token's `human_hash` ([X40](#24-edge-cases)). On a sign-up that is already
  `verified`, only the refusal is recorded and the page says "This inbox was already confirmed. If you own
  it, delete it in the console." A verified workspace is never deleted through an email link.
- **A complaint is a refusal.** A `complained` outcome on a code email, or on any message of a provisional
  tenant, records the refusal with `reason = 'complaint'` and, when it names the current person, refuses and
  erases the sign-up ([§13.3](#133-outcomes-of-sign-up-mail), [X39](#24-edge-cases)). The system identity's
  own suppression of that address follows as for any complaint (FR-DLV-2).
- **What a refusal does.** Every later sign-up or replacement naming the same base address gets
  `429 signup_limit_reached` with `details.scope = "human_email"`, indistinguishable from the other
  person-scope refusals. It has no expiry: it records an objection to contact, kept like a suppression
  ([Privacy §7.2](privacy.md#72-suppressions-after-erasure-i7)).
- **Lifting it** ([X41](#24-edge-cases)). `/console/settings` shows a signed-in person whose base address
  hash is on the list: "You stopped AI agents naming this address as their accountable human on 17 October
  2026" and **Allow again**, a `POST` with the CSRF token that deletes the row and writes the audit row
  `user.agent_signup_allow` (`tenant_id = NULL`). Signing in proves the address; a person whose address the
  system identity has suppressed after a complaint signs in with Google or GitHub, as the notification
  design already requires ([Notifications §5](notifications.md#5-the-emails)).

## 10. Expiry, abandonment and refusal: erasure

**Expiry after 7 days** (a Pylota Identity choice). It gives a person a full week, a weekend included, and it
matches the other waits for a person in this design: an invitation, a waitlist invite and a pending
service-ledger entry all last 7 days. A shorter life would lose sign-ups of people on holiday; a longer one
would keep unaccountable inboxes on the shared domain.

`crons/agent_signup.rs`, on the `*/15` cron, selects at most 50 sign-ups per run:

```sql
SELECT id, tenant_id FROM agent_signups
WHERE state = 'provisional' AND expires_at <= ?now ORDER BY expires_at LIMIT 50;
```

and ends each one with the **end batch**, which abandonment ([§6.5](#65-delete-v1agent-signup)) and
refusal ([§9](#9-not-me-complaints-and-refusals)) run too, with their own state and reason:

1. `UPDATE agent_signups SET state = ?end, ended_at = ?now, human_email = NULL, code_hash = NULL,
   link_hash = NULL WHERE id = ?1 AND state = 'provisional'` (for expiry also `AND expires_at <= ?now`);
   every statement below is guarded by `EXISTS (… WHERE id = ?1 AND state = ?end AND ended_at = ?now)`.
2. A tenant-scope erasure request and its job for the sign-up's tenant ([Privacy §6.6](privacy.md#66-tenant-scope)),
   reason `agent_signup_expired`, `agent_signup_abandoned` or `agent_signup_refused`, `created_by_key_id`
   `NULL` (or the agent's key for an abandonment). This system action needs no `tenants:erase`: no key
   asks for it; the sign-up's own state decides.
3. One message-scope erasure request on the default tenant for each `msg_` ID in
   `mail_message_ids_json`: the code emails, kept redacted in the system identity's mailbox, are deleted now
   rather than at its 30-day cutoff, so nothing about a person who never confirmed stays behind.
4. Audit rows `agent_signup.expire`, `agent_signup.abandon` or `agent_signup.refuse`, `erasure.create` for
   each request, all with IDs and hashes only.

After the commit the handler (or the cron) starts the jobs. The tenant-scope erasure runs its ten steps as
for any workspace: `stop_routing` revokes the key and makes mail to `agent.{username}@` answer `550 5.1.1`;
`erase_identities` tombstones the address; `delete_d1_rows` deletes the `agent_signups` row with the other
rows; the tenant row stays `erased` with its slug and suffix, so the username is never reused. Each
erasure has its receipt, kept with the deployment's erasure records ([Privacy §10](privacy.md#10-receipt)).

- **No mail to the person.** No reminder is sent before expiry and nothing at or after it. The code email
  already told them the date ([§5](#5-the-verification-email)).
- **The verify guard does not need the cron.** Every verification checks `expires_at > now` itself
  ([§7.1](#71-the-transaction)), so a late or failing cron can delay the erasure but never lets a sign-up
  be verified after its 7 days ([X57](#24-edge-cases)).
- **Holds.** Only a platform key can place a legal hold on a provisional tenant's thread. The erasure then
  ends `completed_with_holds` and the tenant stays `erasing` until the hold ends, as for any tenant
  ([Privacy §6.6](privacy.md#66-tenant-scope), [X49](#24-edge-cases)).

## 11. Concurrency and idempotency

Every state change is one D1 batch whose first statement is a compare-and-set on `agent_signups.state`
(and, where a code is involved, on `code_seq`); every later statement is guarded by a value only the
winner wrote. So exactly one of any two racing changes takes effect, and the loser re-reads and answers
from the state it finds.

| Race | Outcome |
|---|---|
| Two sign-ups for one username, or for two usernames that fold the same | The unique indexes on `tenants` let one batch commit; the other answers `409 username_taken`. Both spent their admission slot ([X4](#24-edge-cases)) |
| Two sign-ups naming one person | The partial unique index `agent_signups_live_human` lets one commit; the other answers `429 signup_limit_reached` (`human_email`) ([X18](#24-edge-cases)) |
| Two correct codes, or the code and the link, or the link and a claim | One verification; the other finds `verified` by the same person and answers success ([X28](#24-edge-cases), [X34](#24-edge-cases)) |
| Concurrent wrong codes | The reserving `UPDATE` admits at most 10 comparisons per code and 30 failures per sign-up ([X27](#24-edge-cases)) |
| Verification and expiry | The verification requires `expires_at > now`, the expiry `expires_at <= now`, both on `state = 'provisional'`: one wins. If expiry won, the agent gets `409 signup_not_pending` (`expiring`), then `401 key_revoked` once the erasure's first step has run ([X33](#24-edge-cases)) |
| Verification and abandonment, or refusal | One wins. A refusal that loses still records the refusal row ([X40](#24-edge-cases)) |
| A resend or replacement and a verification by code | The resend's guard on `code_seq` and the verification's `code_seq = ?seq` make the older code verify nothing once the newer exists ([X59](#24-edge-cases)) |
| Two replacements | The guard on the old `human_hash` lets one win; the other re-reads ([X32](#24-edge-cases)) |
| An identity created at the human's address, and verification | The identity insert checks the live sign-ups; the verification re-checks hosted addresses: whichever commits first decides ([X44](#24-edge-cases)) |

**Idempotency.** `POST /v1/agent-signup` records nothing ([§3.1](#31-request-and-response)). The four other
`POST` and `PUT` routes take an optional `Idempotency-Key`, scoped per calling key as for every other `POST`
([REST API › Idempotency](../../reference/api.md#idempotency)): a replayed resend sends nothing more. A
verification is idempotent by state: a second correct code after success answers `200` with
`already_verified`. `DELETE` on an abandoned sign-up answers `409 signup_not_pending` until the key is
revoked.

## 12. Failure handling

| Failure | What happens |
|---|---|
| The create batch fails (D1 unavailable, full) | Nothing is written; `503 unavailable` (or `storage_full`); the admission slot stays spent. A retry may succeed: the name is still free |
| An intent call fails (`TenantQuota`, `Notifier` or mailbox `Init`) | The answer is still `201`; `crons/intents.rs` re-sends it ([J21](../edge-cases.md)). Mail that arrives first waits in the inbound queue's retries until the mailbox accepts it |
| The verification email cannot be submitted: the platform domain `failing` or `suspended`, the system identity paused, its own daily cap spent, or a submit error | The sign-up stands with `verification.email: "not_sent"` (and `code.email` in `GET`). The agent resends later. Nothing is refunded. A refusal of the system identity raises `system_mail_blocked` as for any system mail ([Notifications § 7](notifications.md#7-when-system-mail-cannot-be-sent)) |
| The email is accepted and then fails in transport | Its outcome reaches the delivery hook ([§13.3](#133-outcomes-of-sign-up-mail)); a hard bounce marks the person unreachable |
| Both DoH resolvers fail | `503 unavailable` (`dns_unavailable`), nothing counted ([X11](#24-edge-cases)) |
| `TenantQuota` of the default tenant unavailable at admission | `503 unavailable`, nothing written. Sign-up fails closed: without the budgets it does not run |
| The verification batch fails | Nothing changed; the code attempt reserved at step 4 stays counted; `503 unavailable`. A retry with the same code works while attempts remain |
| The account email after verification fails | The `Notifier` keeps and retries it as any `account` item ([Notifications § 7](notifications.md#7-when-system-mail-cannot-be-sent)) |
| The expiry cron fails or is late | The next run picks the sign-up up; verification is already refused by its own guard ([X57](#24-edge-cases)) |
| An erasure step fails | The job's retries and alerts apply ([Privacy § 4](privacy.md#4-jobrunner)); the tenant stays `erasing` and its key is already revoked |
| The delivery hook fails | The delivery queue message is retried; every update it makes is idempotent (`outcome_events` by event ID, conditional `UPDATE`s) |
| `POST /v1/platform/keys/link/rotate?revoke_previous=true` | Codes, links and refuse tokens hashed or signed under the old kid stop working; the agent resends ([X53](#24-edge-cases)) |

## 13. Abuse containment

The platform domain's reputation and the account's Email Sending quota are shared by every workspace and
by Pylota itself, and one person operates Cloud ([ADR 0015](../adr/0015-solo-operator.md)). So every
control below acts on its own; none waits for a reviewer.

### 13.1 Risks and controls

| Risk | Controls |
|---|---|
| Floods of sign-ups | `RL_SIGNIN` (10 a minute per network); 5 a day per network and 100 per ASN; `PM_AGENT_SIGNUP_DAILY_CAP` for the deployment; the spike and 80% alerts ([§21](#21-observability)) |
| A person used as a target: someone names their address to flood or alarm them | One live sign-up per person; 3 sign-ups and 5 code emails a day per base address (`+tag` and Gmail dot variants included); 3 code emails an hour per sign-up; 10 emails a day from the agent, to that person only; a fixed email with no agent-written text; one-click "Not me" that also blocks every later sign-up naming them ([§9](#9-not-me-complaints-and-refusals)) |
| An agent verifying itself without its person, by guessing the code | 6 digits, 10 attempts per code, 30 failures per sign-up, at most 5 codes a day: at most 30 guesses in 10^6 per sign-up ([§6.2](#62-post-v1agent-signupverify)) |
| Throwaway inboxes on a reputable domain, used to open accounts at other services | No `accounts:request`; `accounts.require_approval: true` in the overlay, so codes from services are held and `wait` refuses them; nobody can release held mail before verification ([§4](#4-while-provisional)) |
| Squatting names, or look-alikes of brands | Reserved names, `RESERVED_SUFFIXES` and the confusable fold; the `agent.` prefix of every address; each name costs a sign-up slot ([§2.3](#23-the-address-and-the-username)) |
| Bounces from invented or mistyped addresses on the shared domain | The mail-domain check before any mail ([§3.3](#33-the-human-email-rules)); a hard bounce marks the person unreachable and stops every further email to them; the sign-up-mail breaker ([§13.2](#132-the-automatic-switch)), which also stops every provisional workspace's sends |
| Complaints on the shared domain | One-click refusal in the headers; a complaint refuses and erases the sign-up; the breaker |
| Free compute and storage | No triage, image text or agentic search; at most 50 screening model calls a day; low inbound caps; no uploads or signed links; Free allowances; 7-day life |
| An agent of this deployment reading the code meant for its "person" | Hosted human addresses refused at sign-up, at replacement and at verification, compared without their `+tag`, which inbound routing ignores; identities refused at a live sign-up's human base address ([§3.3](#33-the-human-email-rules)) |
| Abuse after verification | The verified workspace is an ordinary Free workspace: the new-workspace ramp from `verified_at`, identity and tenant auto-pause (FR-DLV-3, FR-DLV-6), the shared-domain breaker and the `free_sending` switch, one Free workspace per person |

### 13.2 The automatic switch

A new containment rule joins [Observability § 5.6](observability.md#56-automatic-containment):

| Rule | Trigger | Action | Undone by |
|---|---|---|---|
| `agent_signup_off` | Over the last 24 hours of sign-up outcomes ([§13.3](#133-outcomes-of-sign-up-mail)): bounced > 5% with ≥ 50 outcomes; or complained > 0.5% with ≥ 200 outcomes; or 5 or more complaints, whatever the count | The `ops_switches` row `agent_signup` becomes `off` (`set_by = 'auto'`): new sign-ups, resends and replacements get `403 signup_closed` (`paused`), and every send of a provisional tenant gets `403 policy_denied` (`signup_paused`). Verification by code, link and claim, refusals, abandonment and expiry continue. Audit `ops.switch`; page `containment:agent_signup_off` | `PUT /v1/platform/switches` `{"name": "agent_signup", "state": "on"}` (platform key, `platform:ops`) |

The thresholds are Pylota Identity choices. They are evaluated in the delivery consumer, where the outcomes
are ([Observability § 5.6](observability.md#56-automatic-containment)). The bounce threshold is the
tenant auto-pause's 5%, SES's review level ([Outbound › Tenant and domain auto-pause](outbound.md#tenant-and-domain-auto-pause-g12)).
Code emails go to addresses that nobody has confirmed, so the complaint rule has an absolute floor as
well: five people in a day who reported a sign-up email as spam are enough to stop the feature until the
owner looks. The operator can also set the switch by hand (`idvy ops switch agent_signup off`).

### 13.3 Outcomes of sign-up mail

The delivery-event consumer ([Outbound › The delivery consumer](outbound.md#the-delivery-consumer)) already
records every outcome with `QuotaRequest::RecordOutcome`. For two kinds of message it now also calls the
default tenant's `TenantQuota` with a new request, `QuotaRequest::CountSignupOutcome { outcome, at,
event_id }`, idempotent by `event_id` through `outcome_events`, which increments
`asu:outcomes:{UTC hour}` and `asu:bounced:{hour}` or `asu:complained:{hour}`, deletes windows older than
48 hours, and answers the sums of the last 24 hourly windows; the consumer evaluates the rule of
[§13.2](#132-the-automatic-switch) on that answer. The two kinds:

1. **Code emails**: messages of the system identity with `metadata.agent_signup_id`. On top of counting:
   - `delivered` for the current person sets `human_status = 'delivered'` (only from `pending`);
   - a hard `bounced` whose recipient's base hash equals the current `human_hash` sets
     `human_status = 'bounced'`: sends to the person, resends and the code path stop; a replacement is
     allowed ([X38](#24-edge-cases)). The system identity's suppression follows as for any hard bounce;
   - `complained` records the refusal for the recipient's base hash and, when it is the current person,
     ends the sign-up as `refused` ([§9](#9-not-me-complaints-and-refusals)).
2. **Mail of a provisional tenant** (every recipient is its person): the tenant's own `RecordOutcome`,
   suppression and pause rules apply as for any tenant. A hard bounce also sets `human_status =
   'bounced'`; a complaint also refuses and erases the sign-up: a person who reports the agent's first
   messages as spam did not want it.

The consumer finds the sign-up with one D1 read by `agent_signup_id`, or by `tenant_id` for a tenant whose
`provisional_until` it already read. Every update is a conditional `UPDATE`, so a redelivered event changes
nothing twice.

## 14. Security and visibility

- **Unauthenticated route.** `POST /v1/agent-signup` joins the explicit list of routes that skip
  authentication ([Security § 4.7](security.md#47-unauthenticated-routes)), as `Scope::Public`. It reads
  no key; its controls are `RL_SIGNIN`, the budgets, the human-email rules and the overlay.
- **Key level and reach.** The key is an identity key of a tenant with no partner: partner keys get the
  `404` of any foreign tenant (a provisional tenant is never a partner's, [X35](#24-edge-cases)); platform
  keys reach it as any tenant. The six routes of [§6](#6-the-agents-routes) act only on the caller's own
  sign-up: a key of another tenant gets `404 signup_not_found`, the same answer as a tenant with no sign-up,
  and the routes join `it::security::cross_tenant_matrix` ([Testing § 7](testing.md#7-cross-tenant-attack-suite)).
- **Secrets and hashes.** The code, the link token and the refuse token are never stored: the code and link
  as keyed hashes under the `link` keyring with their kid, the refuse token as a MAC. The key secret is
  stored as its keyed hash under `PM_KEY_PEPPER` and returned once. No idempotency record, audit row or
  log line holds any of them (FR-KEY-2).
- **Constant time.** Codes are compared with `Mac::verify_slice`; the claim authenticates keys through the
  same path as every request, so a miss and a mismatch cost the same.
- **What the routes reveal.**
  - Whether a username is taken: public anyway, because mail to the address is accepted or refused.
  - About a person: one bit, "this address cannot take a sign-up today", merged from five causes that
    give the same body and cost the caller the same budget ([§3.4](#34-admission-budgets)). Never whether
    the address has an account, an invitation or a workspace. A refusal is permanent while the other
    causes end within 7 days, so a caller who probes one address daily for more than a week can infer a
    refusal; that is the price of a refusal that lasts.
  - The claim page: only to someone who already holds the key, and only what `GET /v1/agent-signup`
    already shows that holder.
  - The verify and refuse pages: only for a valid token, which only the person's mailbox received.
- **Logging** ([Security § 12](security.md#12-logging-rules)). Never logged: the human address, the code,
  the link and refuse tokens, a pasted key, `agent_name` and `source`. Logged instead: `asu_`, `ten_`,
  `idn_` and `key_` IDs, `human_hash` truncated to 16 hex characters as a pseudonym, the network hash,
  error codes and counts.
- **Untrusted text.** `agent_name` and `source` were written by an unknown caller: the console escapes
  them, and neither reaches an email the deployment sends.
- **Keys sent by email** ([X50](#24-edge-cases)). The service never emails a key. An agent that mails its own
  key to its person, so that they can claim, exposes it to every mail system on the way, as AgentMail also
  warns. Before verification such a key can read the inbox, send at most 10 emails a day to the person,
  and abandon the sign-up; it cannot verify without the person's code, and cannot reach anything else.
  After verification it is an ordinary leaked identity key: the owner revokes it in the console, which the
  claim's success page and the account email both suggest.

## 15. Storage

```sql
-- migrations/d1/0001_init.sql (before v1.0, like the rest of the schema; Data model § 1)

-- One row per agent sign-up (Agent self-sign-up). The clear human address is kept only while the sign-up
-- is provisional; the row is deleted by the tenant-scope erasure of its tenant.
CREATE TABLE agent_signups (
  id                TEXT PRIMARY KEY,                        -- asu_
  tenant_id         TEXT NOT NULL UNIQUE REFERENCES tenants(id),
  identity_id       TEXT NOT NULL REFERENCES identities(id),  -- the 'agent' identity
  key_id            TEXT NOT NULL REFERENCES api_keys(id),    -- the key returned at sign-up
  state             TEXT NOT NULL
                    CHECK (state IN ('provisional','verified','expired','abandoned','refused')),
  human_email       TEXT,                                    -- normalised; NULL once the state leaves provisional
  human_hash        TEXT NOT NULL,                           -- hex HMAC-SHA256(PM_HASH_KEY, base address)
  human_status      TEXT NOT NULL DEFAULT 'pending'
                    CHECK (human_status IN ('pending','delivered','bounced')),  -- delivery hook (§ 13.3)
  replacements_used INTEGER NOT NULL DEFAULT 0 CHECK (replacements_used BETWEEN 0 AND 2),
  code_seq          INTEGER NOT NULL DEFAULT 1,              -- +1 with every new code (resend, replacement)
  code_hash         TEXT,                                    -- HMAC(link key {key_kid}, "asu:{id}:{code_seq}:{code}");
                                                             -- NULL when burned, used or ended
  link_hash         TEXT UNIQUE,                             -- HMAC(link key {key_kid}, link token)
  key_kid           TEXT,                                    -- signing_keys kid (purpose 'link') of both hashes
  code_expires_at   INTEGER,                                 -- code and link: issued + 24 hours
  attempts          INTEGER NOT NULL DEFAULT 0,              -- reserved tries of the current code (≤ 10)
  failed_total      INTEGER NOT NULL DEFAULT 0,              -- wrong codes over the sign-up's life (≤ 30)
  sends_json        TEXT NOT NULL DEFAULT '[]',              -- times of the code emails of the last hour (≤ 3)
  mail_message_ids_json TEXT NOT NULL DEFAULT '[]',          -- msg_ IDs of the code emails (system identity)
  mail_status       TEXT NOT NULL DEFAULT 'queued'
                    CHECK (mail_status IN ('queued','not_sent')),  -- the latest code email
  source            TEXT,                                    -- [A-Za-z0-9._/+-]{1,64}
  network_hash      TEXT NOT NULL,                           -- 16 hex: HMAC(PM_HASH_KEY, IPv4 or IPv6 /64)
  terms_version     TEXT NOT NULL,                           -- PM_TERMS_VERSION accepted by the caller
  expires_at        INTEGER NOT NULL,                        -- created_at + 7 days; equals tenants.provisional_until
  verified_at       INTEGER,
  verified_via      TEXT CHECK (verified_via IN ('code','link','claim')),
  verified_user_id  TEXT REFERENCES users(id),
  verify_token      TEXT,                                    -- the winner's nonce guarding the verification batch
  ended_at          INTEGER,                                 -- expired, abandoned or refused
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL
);
CREATE UNIQUE INDEX agent_signups_live_human ON agent_signups(human_hash) WHERE state = 'provisional';
CREATE INDEX agent_signups_expiry ON agent_signups(expires_at) WHERE state = 'provisional';
CREATE INDEX agent_signups_created ON agent_signups(created_at);         -- the platform listing

-- People who stopped agents naming them (§ 9). Keyed hash only; kept as an objection to contact.
CREATE TABLE agent_signup_refusals (
  human_hash TEXT PRIMARY KEY,                               -- hex HMAC-SHA256(PM_HASH_KEY, base address)
  reason     TEXT NOT NULL CHECK (reason IN ('refused','complaint')),
  created_at INTEGER NOT NULL
);

-- tenants: new column
provisional_until INTEGER,           -- set while the tenant is a provisional agent sign-up: when it expires.
                                     -- Written by the create batch, cleared by verification; read by key
                                     -- authentication (the permission overlay) and core::policy::effective
-- pending_auth: new column
agent_signup_id TEXT REFERENCES agent_signups(id) ON DELETE CASCADE,  -- two_factor step of a link (§ 7.2)
-- ops_switches.name: CHECK (name IN ('read_only','free_sending','emergency_prune','agent_signup'))
```

- **Writers and readers.** Every column has its writer and reader on this page: the create batch, the
  verify, resend, replacement and end batches, the delivery hook ([§13.3](#133-outcomes-of-sign-up-mail)),
  the platform listing and `GET /v1/agent-signup`.
- **Foreign keys.** `agent_signups` refers to `identities` and `api_keys`, so tenant erasure deletes it
  before those tables in `delete_d1_rows` ([§16](#16-privacy)); `pending_auth` rows referring to it go
  with it (`ON DELETE CASCADE`), because a second step for an erased sign-up means nothing.
- **Durable Objects.** No new table. The default tenant's `TenantQuota.counters` gains the metrics
  `sysmail:signup:{recipient|human|network|asn|deployment}:{key}` (per UTC day, deleted with the day) and
  `asu:{outcomes|bounced|complained}` (per UTC hour, deleted after 48 hours); the system identity's
  mailbox stores the code emails redacted ([§5](#5-the-verification-email)).
- **`TenantQuota` interface.** `SystemMailClass` gains `Signup`, and `QuotaRequest` gains
  `CountSignupOutcome { outcome: Outcome, at: i64, event_id: String } → SignupOutcomeAnswer { outcomes: u64,
  bounced: u64, complained: u64 }` (default tenant only). M5's stub declares both, as it declares every
  variant ([Build plan › Shared files](../build-plan.md#dependency-graph)).

## 16. Privacy

| Store | Personal data | Retention | Deletion |
|---|---|---|---|
| D1 `agent_signups` | While provisional: the person's address in clear. Always: the keyed hash of their base address, `source`, the network hash, the accepted terms version, times | Provisional: at most 7 days. Verified: life of the tenant (the clear address is removed at verification) | Tenant-scope erasure (`delete_d1_rows`, before `api_keys` and `identities`), which every end of a provisional sign-up starts; person deletion replaces `human_hash` of the rows they verified with the row's own ID ([Privacy §6.9](privacy.md#69-people-console-accounts)) |
| D1 `agent_signup_refusals` | Keyed hash of a base address | Until the person lifts it | `/console/settings` (**Allow again**). Not deleted by person deletion or by any erasure: it records an objection to contact, as a suppression does ([Privacy §7.2](privacy.md#72-suppressions-after-erasure-i7)); it holds no clear data |
| D1 `tenants.provisional_until`, `pending_auth.agent_signup_id` | None | As their rows | As their rows |
| Default tenant `TenantQuota` counters | Keyed hashes and counts | One UTC day (`sysmail:signup:*`); 48 hours (`asu:*`) | The object's daily cleanup |
| The system identity's mailbox | The code emails to the person, redacted (no code, no link) | 30 days (its fixed retention) when the sign-up was verified | Message-scope erasure of each code email when the sign-up ends unverified ([§10](#10-expiry-abandonment-and-refusal-erasure)); person deletion's restricted counterparty erasure covers a verified person ([Privacy §6.9](privacy.md#69-people-console-accounts)) |
| The provisional tenant's mailbox, R2 objects, vectors and D1 rows | The person's mail with the agent, and any mail others sent to the inbox | Until the tenant is verified or ends | Tenant-scope erasure ([Privacy §6.6](privacy.md#66-tenant-scope)), with its receipt and probes |

- **Every new store is covered by an existing erasure scope.** The tenant scope deletes `agent_signups`
  in `delete_d1_rows`, and its step-8 probe adds `agent_signups` to the D1 tables that must count zero rows
  for the tenant. The message scope deletes the code emails, and each of those requests has its own receipt
  (`messages_deleted`, `r2_objects_deleted`). Receipt fields do not change: D1 control-plane rows are not
  counted in receipts today, as for `service_accounts`.
- **Legal holds** apply unchanged: a held thread of a provisional tenant keeps the tenant `erasing`
  ([X49](#24-edge-cases)).
- **Counterparty erasure** of the person's address in a verified workspace erases their mail as for any
  correspondent; it does not touch the sign-up row, which holds no mail.
- **Lawful basis and notice.** The person's address is processed to ask them to confirm, which they can
  refuse in one click, and is deleted within 7 days if they do nothing. The Privacy Policy linked from every
  code email must say so, as the [Privacy guide](../../guides/privacy.md) does.

## 17. Billing and limits

- A provisional workspace is `metered` on the catalog's default plan (Free: 5 inboxes, 1,000 sends a month,
  1 GB, 1 seat) from creation, with no Stripe customer and no Stripe call. Its sends take `sends` holds,
  its mail counts towards `storage_gb`, and it uses at most one inbox and no seat.
- It does not count as "a self-serve Free workspace of a person" (FR-CON-18) until it has an owner; the
  rule is checked at verification ([§7.1](#71-the-transaction)).
- `402 billing_limit` cannot occur before verification in practice (10 sends a day are far below 1,000 a
  month), but the pipeline is unchanged and would return it.

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

## 18. Events and audit

`signup.verified` is a platform event ([Webhooks › Platform events](webhooks.md#platform-events)),
written to `event_index` in the verification batch with `owner_kind = 'platform'`, `tenant_id` and
`identity_id` set and `sequence` `null`, then fanned out and appended to the event feed with the same
`evt_` ID ([Event feed](event-feed.md)):

```json
{
  "id": "evt_01JA3…", "type": "signup.verified", "api_version": "2026-10-01",
  "occurred_at": "2026-10-17T11:42:10.512Z", "tenant_id": "ten_01JA2…", "identity_id": "idn_01JA2…",
  "sequence": null,
  "data": { "signup_id": "asu_01JA2…", "address": "agent.travel-planner@pylotamail.com",
            "verified_via": "code", "user_id": "usr_01JA3…" }
}
```

It reaches platform endpoints and the tenant's endpoints (none can exist at that moment); the tenant has no
partner. In the event feed, `signup.verified` is visible to the tenant and identity keys of that tenant, an
identity key only when the event's `identity_id` is its own: the event concerns the key's own sign-up and
carries no mail ([Event feed § 4](event-feed.md#4-who-sees-which-events)). The agent also learns of
verification from the `identity.updated` event (`changed: ["owner"]`) that the verification
emits through its mailbox, which an identity key with `identities:read` and `events:read` sees, or by polling
`GET /v1/agent-signup`. No other `signup.*` event is defined: the other changes are audit rows and metrics.

| Audit action | `tenant_id` | When |
|---|---|---|
| `agent_signup.create` | `NULL` (deployment-level, outlives the tenant) | A sign-up was created; `details_json`: `signup_id`, `tenant_id`, `network`, `source` |
| `tenant.create`, `identity.create`, `key.create` | the tenant | The same batch; `details_json.via = "agent_signup"`; `key.create` lists `granted_without_holding` |
| `agent_signup.resend`, `agent_signup.human_replace` | the tenant | A new code; a new person (both `human_hash` prefixes, never an address) |
| `agent_signup.verify`, `member.join` | the tenant | Verified; `details_json.via` = `code`, `link` or `claim`; `actor_user_id` for the link and the claim |
| `agent_signup.abandon`, `agent_signup.refuse`, `agent_signup.expire` | the tenant | The end batch; deleted by the erasure with the tenant's other audit rows, except the `erasure.*` rows |
| `user.agent_signup_allow` | `NULL` | A person lifted their refusal |
| `ops.switch` | `NULL` | The switch changed, by the rule or by hand |

## 19. Console

| Path | Screen | Who |
|---|---|---|
| `/console/agent-signup/verify` | Confirm being an agent's accountable human: the agent's address, what it means, the terms checkbox, **Confirm** ([§7.2](#72-the-link-path-in-the-console)) | Anyone with a link; no session needed |
| `/console/agent-signup/claim` | Claim an agent's inbox with its key ([§8](#8-claiming-in-the-console)) | Signed in |
| `/console/agent-signup/refuse` | Stop agents naming this address ([§9](#9-not-me-complaints-and-refusals)) | Anyone with a link; no session, no CSRF token, no `Origin` check |
| `/console/settings` | Gains the refusal notice and **Allow again** for a person on the refusal list | Signed in |

- **Landing.** A link confirmation or a claim lands on the new workspace's Overview: a row joins the table
  of [Cloud sign-up §7](cloud-signup.md#7-where-people-land) after the invitation row: "The sign-in
  confirmed or claimed an agent sign-up → that workspace's Overview".
- **Overview.** A verified agent workspace shows, until dismissed with the checklist, the banner "An AI
  agent created this workspace on 17 October 2026 and holds the key 'agent sign-up'. Review its access
  on the keys page." The first-run checklist's "Create your first inbox" and "Create an API key" steps are
  already done, from real data.
- **The keys page** shows the sign-up's key with the name `agent sign-up`; the owner can revoke or rotate
  it like any key. A rotation's new secret goes to the owner, who passes it to the agent.
- The pages need no JavaScript and join `browser::console::no_js` and `browser::console::axe_scan`.

## 20. MCP and CLI

MCP needs a key ([MCP § 3](mcp.md#3-authentication-and-tool-filtering)), so an agent cannot sign up
through MCP; it signs up over REST (or with the CLI or an SDK) and then uses the key on `/mcp`.

| Interface | What | Listed for |
|---|---|---|
| MCP `mail_signup_status` | `GET /v1/agent-signup` | Keys whose tenant has an agent sign-up |
| MCP `mail_signup_verify` | `POST /v1/agent-signup/verify` (`code`) | Keys of a provisional sign-up |
| MCP `mail_signup_resend` | `POST /v1/agent-signup/resend` | Keys of a provisional sign-up |
| MCP `mail_signup_set_human` | `PUT /v1/agent-signup/human` (`human_email`) | Keys of a provisional sign-up |
| CLI `idvy agent-signup create --username U --human-email E [--agent-name N] [--source S] --accept-terms [--profile P] [--print-key]` | `POST /v1/agent-signup` without a key. Writes the returned key into the profile (the config file's `0600` rules), printing it only with `--print-key` | – |
| CLI `idvy agent-signup status\|verify <code>\|resend\|set-human <email>\|abandon` | The other five routes (`abandon` asks for the typed address) | – |
| CLI `idvy agent-signups list [--state S] [--since T] [--network N]` | `GET /v1/platform/agent-signups` (platform key; `--since` is `created_after`, an RFC 3339 time or a date; `--network` the 16-hex network hash of a row) | – |
| CLI `idvy ops switch agent_signup on\|off` | `PUT /v1/platform/switches` | – |

Annotations: `mail_signup_status` is a read tool (`readOnlyHint: true`, `idempotentHint: true`,
`openWorldHint: false`); `mail_signup_verify` is `readOnlyHint: false`, `destructiveHint: false`,
`idempotentHint: true`, `openWorldHint: false`; `mail_signup_resend` and `mail_signup_set_human` are
`readOnlyHint: false`, `destructiveHint: false`, `idempotentHint: false`, `openWorldHint: true` (they
email a person). There is deliberately no MCP tool to abandon: it erases the workspace, and a steered
agent must not be able to do that from a tool call.

## 21. Observability

**Metrics** (Analytics Engine; bounded labels, no `source` label):

| Metric | Labels |
|---|---|
| `agent_signups_total` | `result`: `created`, `closed`, `rate_limited`, `limit_network`, `limit_asn`, `limit_human`, `limit_deployment`, `username_taken`, `invalid`, `no_mail_domain`, `dns_unavailable` |
| `agent_signup_codes_total` | `result`: `verified`, `wrong`, `burned`, `locked`, `expired`, `superseded` |
| `agent_signup_verified_total` | `via`: `code`, `link`, `claim` |
| `agent_signup_ended_total` | `reason`: `expired`, `abandoned`, `refused` |
| `agent_signup_mail_total` | `result`: `queued`, `not_sent`, `delivered`, `bounced`, `complained` |
| `agent_signup_refusals_total` | `reason`: `refused`, `complaint`, `lifted` |

**Alerts** ([Observability § 5.3](observability.md#53-alert-list)):

| Alert | Kind | Condition | Runbook step |
|---|---|---|---|
| `agent_signup_spike` | ticket | The state-alert evaluator counts `agent_signup.create` audit rows: the last hour has at least 20 and at least three times the hourly average of the previous 7 days | List with `idvy agent-signups list --since <the time an hour ago, RFC 3339>`; look for one network or source, then list that network's sign-ups with `--network <hash>`; if it is abuse, `idvy ops switch agent_signup off` |
| `agent_signup_cap_80` | ticket | Today's (UTC) `sysmail:signup:deployment` counter reaches 80% of `PM_AGENT_SIGNUP_DAILY_CAP` | Decide whether to raise the variable |
| `containment:agent_signup_off` | page | The rule of [§13.2](#132-the-automatic-switch) fired | Read the bounce and complaint counts, look at the sign-ups of the last day, fix the cause, then switch it back on |

`GET /v1/platform/status` gains `agent_signup` (configured, active, the reason when inactive, the switch,
today's sign-ups against the cap, and the last 24 hours' outcome rates). `idvy doctor` gains the check
`agent_signup`: `warn` when `PM_AGENT_SIGNUP = "on"` but inactive, or when the switch is `off`; `skip` when
the variable is `off`. The dashboard "Identity and notifications" gains sign-ups by result, verifications
by path, ends by reason and the outcome rates.

## 22. Pylota agentic OS

- **Pylota never uses agent self-sign-up.** Each car-rental operator's workspace is created by Pylota's
  partner key (FR-KEY-4; integration spec §4), billed `exempt`, with Pylota's own identities and keys
  (spec §6). No Pylota agent, coordinator or specialist calls `POST /v1/agent-signup`, and none needs to:
  Pylota's agents never hold a Pylota Identity key in their context.
- **Pylota-managed tenants can never be claimed.** A claim needs an identity key whose tenant has an
  agent sign-up row; a partner's tenant never has one, so a pasted key of an operator's workspace, Pylota's
  partner key, or any of K2–K7 gets "This key cannot be used to claim an inbox" and nothing changes
  ([X35](#24-edge-cases)). Pylota's operators never hold keys anyway.
- **Isolation.** A provisional or verified agent workspace has no partner, so Pylota's partner key gets the
  `404` of a foreign tenant for it, and `signup.verified` never reaches Pylota's partner endpoint.
- **A stranger's agent naming a Pylota operator.** An operator whose address an outside agent names gets the
  code email and can refuse in one click; nothing in Pylota changes. An operator's Pylota Identity inbox
  address (on `pylotamail.com` or a custom domain) cannot be named at all: it is hosted
  ([§3.3](#33-the-human-email-rules)).
- **What Pylota stores and how failures surface.** Nothing: the feature has no Pylota-side state.

For the owner of Pylota's integration spec (`docs/plans/pylota-mail-integration.md`):

1. §2 or §6: state that Pylota does not use `POST /v1/agent-signup`, and that every operator workspace comes
   from the partner key, so none can be claimed through `/console/agent-signup/claim`.
2. The browser agent's and any HTTP tool's denylist (spec F30 notes there is no egress allowlist today):
   refuse `https://api.pylotamail.com/v1/agent-signup*` and `https://app.pylotamail.com/console/agent-signup/*`,
   so that a page or an email that steers an agent, including through the public-webchat coordinator's
   untrusted input, cannot make it open an unmanaged inbox in an operator's name. Owner `I` row
   [X60](#24-edge-cases).
3. The service sign-up ledger already refuses `pylotamail.com` as a `service_domain` (`own_deployment`,
   [E14](../edge-cases.md)); no change, but the spec may cite it next to item 2.
4. Operator support copy: if an operator reports a "Pylota Identity" email asking them to confirm an AI agent,
   it did not come from Pylota; the email's own "Not you?" link stops it.

## 23. Functional requirements

To add to [PRD § 6](../prd.md#6-functional-requirements) as a new subsection, "6.16 Agent self-sign-up",
with the amendment of FR-IDN-2 below.

- **FR-ASU-1** Agent self-sign-up **must** be off unless `PM_AGENT_SIGNUP` is `on`, and **must** create
  nothing unless `PM_SIGNUP` is `open`, billing is on and started, the console is on and the operator's
  `agent_signup` switch is not `off`; otherwise `POST /v1/agent-signup` **must** answer
  `403 signup_closed`. Turning it off **must** stop new sign-ups only: an existing provisional workspace
  **must** keep its verification, claim, refusal, abandonment and expiry.
- **FR-ASU-2** `POST /v1/agent-signup`, with no key, **must** require a username, the address of the human
  responsible for the agent and the acceptance of the terms, and **must** create in one transaction a
  provisional workspace on the Free plan, one identity with one platform address, and one identity key whose
  secret is returned once and never stored, replayed or emailed.
- **FR-ASU-3** The username **must** become the workspace's slug and address suffix, with the address
  `agent.{username}@{PM_PLATFORM_DOMAIN}`; it **must** pass the address-suffix rules (reserved, confusable
  and brand names refused), two concurrent sign-ups for one name **must** give it to exactly one, and a
  username **must never** be released or reused.
- **FR-ASU-4** The human address **must** be refused when it is on a placeholder or disposable domain, when
  this deployment receives its mail (also through a `+tag` that inbound routing removes), or when its domain
  cannot receive mail, before anything is written or
  sent; and it **must not** be refused, or answered differently, because it belongs to an existing console
  user.
- **FR-ASU-5** Sign-ups **must** be bounded by `RL_SIGNIN`, by exact daily budgets per client network, per
  ASN, per person and per deployment, and by one live provisional sign-up per person, the person counted by
  base address (`+tag` removed, and for Gmail every dot removed and `googlemail.com` read as `gmail.com`);
  every refusal that concerns a person **must** be one indistinguishable answer that costs the caller the
  same budget.
- **FR-ASU-6** Until verified, every key of the workspace **must** act with at most `identities:read`,
  `messages:read`, `messages:send`, `messages:write`, `attachments:read`, `search:read`, `events:read` and
  the implicit `usage:read`; it **must not** mint keys, create webhooks, domains, identities, lists or
  drafts, request service accounts, sign assertions or HTTP requests, run agentic search, upload files or
  release quarantined mail.
- **FR-ASU-7** Until verified, the workspace's effective policy **must** hold every guard switch at its
  strict value and its caps lowered, and its identity **must** send only to the named human, at most 10
  messages a UTC day with one recipient each, and nothing once that address has hard-bounced.
- **FR-ASU-8** The human **must** be emailed a six-digit code and a confirmation link by the system
  identity, in a fixed message that carries no text the caller wrote, with a one-click refusal
  (RFC 8058); this mail **must** be counted against its own system-mail class, at most 5 a day to one
  person and 3 an hour per sign-up, and **must** be stored without the code or the link.
- **FR-ASU-9** A code **must** expire after 24 hours, allow 10 attempts and then be burned, and a sign-up
  **must** allow at most 30 failed codes in its life; codes **must** be stored as keyed hashes and compared
  in constant time, concurrent attempts **must not** exceed the limits, and a new code **must** invalidate
  the previous one.
- **FR-ASU-10** Verification by the code, by the link and a console sign-in, or by a claim **must** in one
  transaction make the human the identity's accountable human and the workspace's owner (a console person
  created, or linked by the verified address), end the provisional restrictions on the next request, emit
  `signup.verified`, send the person an `account` email, and leave a Free workspace whose send ramp counts
  from verification; it **must** be refused when the person already owns a Free workspace, when the
  address has become hosted, or when the workspace is suspended, ended or past its 7 days.
- **FR-ASU-11** Before verification the agent **may** replace the human at most twice; each replacement
  **must** invalidate the previous code and link, **must** pass the rules of FR-ASU-4 and FR-ASU-5 for the
  new address, and **must not** extend the workspace's life.
- **FR-ASU-12** A signed-in person **must** be able to claim a provisional workspace by entering its key
  only when their sign-in address equals the named human's address; a workspace created by a partner key,
  or any key that is not a provisional agent sign-up's, **must never** be claimable.
- **FR-ASU-13** The named person **must** be able to refuse in one click, without signing in, for 90 days
  after each code email, also once the sign-up has ended; a refusal or
  a complaint **must** end and erase the provisional workspace that names them and **must** refuse every
  later sign-up naming them, until they lift it in the console.
- **FR-ASU-14** A provisional workspace **must** be erased by a tenant-scope erasure, with its receipt, when
  7 days pass, when its agent abandons it, or when its person refuses it; its code emails **must** be erased
  from the system identity's mailbox, and the person **must** receive no further mail about it.
- **FR-ASU-15** Bounces and complaints of sign-up mail and of provisional workspaces **must** be counted
  deployment-wide, and above the thresholds of [§13.2](#132-the-automatic-switch) new sign-up mail and every
  send of a provisional workspace **must** stop automatically and page the operator; a sign-up spike and 80%
  of the daily cap **must** alert.
- **FR-ASU-16** No log, audit row, idempotency record or event **may** hold the human's address, a code, a
  link or refuse token, or a key; every new store **must** be covered by the tenant-scope erasure or hold
  only keyed hashes.

**Amendment of FR-IDN-2**, to replace its text: "**FR-IDN-2** An identity **must** record an accountable
human (name and email). Until it has one, it **must not** send, except that the identity of a provisional
agent sign-up **may** send to the address of the human it named, within FR-ASU-7, until that human
verifies (FR-ASU-10)."

## 24. Edge cases

New section of the [edge-case register](../edge-cases.md): "X · Agent self-sign-up".

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| X1 | A sign-up while `PM_AGENT_SIGNUP` is `off`, `PM_SIGNUP` is `waitlist` or `closed`, billing is off or not started, or the console is off; `PM_AGENT_SIGNUP` set to a value other than `on` or `off` | `403 signup_closed` (`disabled`), nothing written and no budget spent; a malformed variable is `config_invalid` | S | `it::signup::x1_feature_gates`, `platform::config::startup_rules` |
| X2 | `PM_AGENT_SIGNUP` turned `off`, or `PM_SIGNUP` changed from `open`, while provisional sign-ups exist | New sign-ups `403 signup_closed`; existing ones still verify (code, link, claim), resend, replace, refuse, abandon and expire on time | S | `it::signup::x2_off_with_live_signups` |
| X3 | The `agent_signup` switch is `off` (automatically or by hand) | New sign-ups, resends and replacements `403 signup_closed` (`paused`); every send of a provisional workspace, to its human included, `403 policy_denied` (`signup_paused`); verification, claim, refusal, abandonment and expiry continue | S | `it::signup::x2_off_with_live_signups`, `core::policy::x21_provisional_send_steps` |
| X4 | Two sign-ups race for one username, or for `acme` and `acrne` | Exactly one `201`; the other `409 username_taken`; both spent their admission slot | S | `it::signup::x4_username_race` |
| X5 | A username that is reserved, confusable with a reserved name, a brand of `RESERVED_SUFFIXES`, prefixed `pm-`, non-ASCII, malformed, or equal to an existing tenant's slug (a Pylota operator's included) | `400 address_reserved`, `address_unsupported` or `address_invalid`, or `409 username_taken`; nothing written | S | `core::signup::x5_username_rules`, `it::signup::x4_username_race` |
| X6 | The username of a sign-up that expired, was abandoned or refused, or of any erased workspace | `409 username_taken` for ever; the address stays tombstoned and gets `550 5.1.1` | S | `it::signup::x4_username_race` |
| X7 | A human address on `example.com`, `example.net`, `example.org`, under `.test`, `.example`, `.invalid` or `.localhost`, or `simulator.invalid` | `400 invalid_request` (`human_email_not_allowed`) before anything is written or sent | S | `core::signup::x7_human_email_rules` |
| X8 | A human address on a disposable domain or in `PM_SIGNUP_BLOCKED_DOMAINS` | As X7 | S | `core::signup::x7_human_email_rules`, `it::signup::create_and_answer` |
| X9 | A human address this deployment receives: on the platform domain (the agent's own future address included), or an identity's `pending`, `active` or `retiring` address on a custom domain, also written with a `+tag` (`ops+x@corp.example` for the identity `ops@corp.example`) | As X7, with the same reason, so the answer does not say which | S | `core::signup::x7_human_email_rules`, `it::signup::x44_hosted_race` |
| X10 | The human address's domain does not exist, publishes only a null MX, or has neither MX nor address records; a domain with no MX but an A record | `400 invalid_request` (`no_mail_domain`) for the first three; the implicit MX (RFC 5321 §5.1) is accepted | S | `it::signup::x10_mail_domain` |
| X11 | Both DoH resolvers fail or time out | `503 unavailable` (`dns_unavailable`, `Retry-After: 30`); nothing written or counted | S | `it::signup::x10_mail_domain` |
| X12 | Malformed JSON; a body over 4,096 bytes; an unknown field; `accept_terms` missing or `false`; an SMTPUTF8 or over-long human address; an `agent_name` with CR or LF or over 78 characters; a `source` with `@` or over 64 characters | `400 invalid_request`, `400 address_unsupported`, `400 address_invalid` or `413 payload_too_large`; nothing written or counted | S | `core::signup::x7_human_email_rules`, `it::signup::create_and_answer` |
| X13 | An 11th sign-up request from one network within 60 seconds | `429 rate_limited` (`details.bucket = "signin"`), sharing the bucket with the console's sign-in forms | S | `it::signup::x13_admission_budgets` |
| X14 | A sixth sign-up from one IPv4 address, or one IPv6 /64 with rotating addresses, in a UTC day | `429 signup_limit_reached` (`network`, `resets_at` at 00:00 UTC) | S | `it::signup::x13_admission_budgets` |
| X15 | The 101st sign-up of the day from one ASN; the deployment's daily cap reached | `429 signup_limit_reached` (`asn`, or `deployment`); `agent_signup_cap_80` fired at 80% | S | `it::signup::x13_admission_budgets` |
| X16 | The `signup` class has used 5% of the system identity's day | Sign-ups, resends and replacements `429 signup_limit_reached` (`deployment`); sign-in mail still goes out | S | `it::signup::x13_admission_budgets` |
| X17 | A sign-up naming a person who already has a live sign-up (also by a `+tag` variant, or a dotted or `googlemail.com` form of a Gmail address), who refused, or who was named 3 times or emailed 5 times today | `429 signup_limit_reached` (`human_email`), byte-identical in all five cases apart from `request_id`, each having spent the caller's network, ASN and deployment budgets alike; no email sent | S | `it::signup::x17_human_scope_identical` |
| X18 | Two sign-ups naming one person race | One commits; the other `429 signup_limit_reached` (`human_email`) | S | `it::signup::x17_human_scope_identical` |
| X19 | The human address belongs to an existing console user, a workspace owner or a pending invitee | The sign-up is accepted exactly as for an unknown address; verification adds the existing person as owner, by the verified address | S | `it::signup::x19_existing_person` |
| X20 | The agent loses the `201` answer and retries the same request | No replay (`x-idempotency: none`): the retry gets `409 username_taken` or `429` (`human_email`); the person can still confirm by the link, then mint a key in the console; otherwise the sign-up expires after 7 days | S | `it::signup::x20_lost_answer` |
| X21 | The provisional agent sends to anyone but its human: in `to`, `cc` or `bcc`, a reply to a stranger who wrote to it, a forward to a stranger, a dry run | `403 policy_denied` (`signup_unverified`); nothing stored; a forward to the human is allowed | S | `core::policy::x21_provisional_send_steps`, `it::signup::x21_send_only_to_human` |
| X22 | An 11th send of the UTC day to the human; a message with two recipients | `429 daily_cap_reached` (`identity` or `tenant`); `400 too_many_recipients` (`max_recipients` 1) | S | `core::policy::x21_provisional_send_steps`, `it::signup::x21_send_only_to_human` |
| X23 | The provisional key calls a route outside its effective set: mint a key, create a webhook, a domain, an identity, a list entry, a draft, an upload; request a service account; sign an assertion or an HTTP request; run agentic search; read a quarantined message; write the policy | `403 permission_denied` with `details.reason = "signup_unverified"` (or the existing answer for a permission its level can never hold); the same through MCP | S | `core::signup::overlay_sets`, `it::signup::x23_permission_overlay` |
| X24 | A third-party service mails a verification code or link to a provisional inbox | Held `quarantined` (`account_unapproved`), invisible to the key; `wait` with `kind=verification` gets `403 policy_denied` (`account_not_approved`); released only by the owner after verification | S | `it::signup::x24_service_codes_held` |
| X25 | A flood of inbound mail to a provisional inbox | Stored `throttled` beyond 20 per sender, 20 unauthenticated and 100 per hour; no triage, image text or agentic search runs; screening makes at most 50 model calls a day and quarantines what it cannot screen | S | `it::signup::x24_service_codes_held` |
| X26 | Ten wrong codes; 30 wrong codes across resends; the right code after either | The code is burned (`422 signup_code_invalid`, `burned`); at 30 the code path is locked (`locked`) and the link and claim still work; the right code is refused once burned or locked | S | `it::signup::x26_code_limits` |
| X27 | Twenty wrong codes submitted at the same time | At most 10 comparisons for that code; the rest get `burned` without comparing | S | `it::signup::x26_code_limits` |
| X28 | The right code submitted twice, or at the same time; the agent's next request after success | One verification; the other `200` with `already_verified`; the next request already has the verified permissions and policy (no cache) | S | `it::signup::x28_verify_effects` |
| X29 | A code more than 24 hours old | `422 signup_code_invalid` (`expired`); a resend issues a new code | S | `it::signup::x26_code_limits` |
| X30 | A fourth code email within an hour for one sign-up; a sixth code email to one person in a UTC day | `429 signup_limit_reached` (`resend`, with `Retry-After`; or `human_email`); no email | S | `it::signup::x30_resend_and_replace` |
| X31 | After a replacement: the agent submits the old person's code; the old person opens their link; the old person clicks "Not me" | `422 signup_code_invalid` (`wrong`, counted as a failure, as any code that is not the current one); "This link has expired or was already used"; only the old person's refusal row is written and the sign-up continues | S | `it::signup::x30_resend_and_replace` |
| X32 | A third replacement; a replacement with the current address; a replacement after verification; two replacements at once; the expiry after replacements | `422 signup_replace_limit`; treated as a resend; `409 signup_not_pending`; one wins; `expires_at` unchanged | S | `it::signup::x30_resend_and_replace` |
| X33 | The right code arrives as the 7 days end and the expiry cron runs | Exactly one of verification and expiry commits; after an expiry the agent gets `409 signup_not_pending` (`expiring`), then `401 key_revoked` | S | `it::signup::x33_verify_races_expiry` |
| X34 | A claim races the link or the code; a claim by a person whose address is not the named one | One verification; the same person sees success; a lost race to an end shows "changed while you were claiming"; a mismatch shows the masked hint and changes nothing | S | `it::signup::x34_claim` |
| X35 | A claim with a Pylota operator's tenant key, Pylota's partner key, a platform key, an ordinary workspace's key, a verified sign-up's key, a revoked or unknown key, or a malformed string | The page of [§8](#8-claiming-in-the-console) for each; nothing changes; the key is never logged or echoed | S | `it::signup::x34_claim` |
| X36 | The person is enrolled in two-step verification | Link: a `two_factor` pending step with `agent_signup_id`, nothing confirmed until it passes, the link still valid if abandoned; claim: the session's factors and re-authentication suffice | S | `it::signup::x36_link_flow`, `it::signup::x34_claim` |
| X37 | A mail scanner opens the link; the `POST` is sent twice; a browser has another person's session; the terms box is not ticked | `GET` changes nothing; the second `POST` shows "expired or already used"; whose session it is, and sign-out, with nothing confirmed; the page again, marked | S | `it::signup::x36_link_flow` |
| X38 | The human address hard-bounces | `human_status = 'bounced'`; sends to it `403 policy_denied` (`signup_human_unreachable`); resends `409 signup_human_unreachable`; a replacement is allowed; otherwise expiry | S | `it::signup::x38_outcomes` |
| X39 | The person reports a code email, or the agent's mail, as spam | Refusal recorded (`complaint`); the sign-up ends `refused` and is erased; later sign-ups naming them `429` (`human_email`) | S | `it::signup::x38_outcomes` |
| X40 | The person clicks "Not me": `GET`, the confirming `POST`, a mail provider's one-click `POST` without cookies or `Origin`; on a sign-up already verified; while a verification races it; from an email whose sign-up expired, was abandoned or was erased, within 90 days | Erased and refused; the one-click `POST` works with no session and never redirects; on a verified sign-up only the refusal is recorded; a lost race still records the refusal; after the sign-up ended only the refusal is recorded | S | `it::signup::x40_refusal`, `live::signup::x40_refuse_one_click_headers` |
| X41 | A refused person later wants an agent to name them | **Allow again** in `/console/settings` after sign-in (Google or GitHub when the system identity suppressed their address); the next sign-up is accepted | S | `it::signup::x40_refusal` |
| X42 | The agent abandons (`DELETE /v1/agent-signup`) | `202`; the tenant is erased; the key gets `401 key_revoked`; the username stays taken; budgets are not refunded; the person gets nothing | S | `it::signup::x42_end_and_erasure` |
| X43 | The provisional agent calls `DELETE /v1/identities/{its own}` | `403 permission_denied` (`signup_unverified`): it holds neither `identities:write` nor `erasure:manage`; abandoning is the route | S | `it::signup::x23_permission_overlay` |
| X44 | Another tenant creates an identity, or adds an address, equal to a live sign-up's human address or to its base without the `+tag`; or did so before the verification | `409 address_taken` while the sign-up is live; a verification after such a create gets `409 signup_cannot_verify` (`human_email_hosted`) | S | `it::signup::x44_hosted_race` |
| X45 | The person already owns a Free workspace, also under a `+tag` variant | `409 signup_cannot_verify` (`workspace_limit`) on the code path, the matching console page; the code is not burned; after an upgrade or deletion the same code verifies | S | `it::signup::x19_existing_person` |
| X46 | The human address is a `users` row with `status = 'disabled'` | `409 signup_cannot_verify` (`person_disabled`); nothing changes | S | `it::signup::x19_existing_person` |
| X47 | The operator suspends a provisional workspace | Inbound gets a temporary failure, sends `403 tenant_suspended`, verification, link and claim refused; the expiry erases it on time | S | `it::signup::x47_suspended_and_erased` |
| X48 | The tenant is `erasing` or `erased` | The key `401 key_revoked`; the link, claim and refuse pages show a generic "no longer exists" or expired page | S | `it::signup::x47_suspended_and_erased` |
| X49 | A platform key holds a thread of a provisional workspace, then the sign-up expires | The erasure ends `completed_with_holds`; the tenant stays `erasing` until the hold ends; the username stays taken | S | `it::signup::x42_end_and_erasure` |
| X50 | The agent's key is sent by email (to the person, to claim, or anywhere) | Before verification the key can read, send 10 a day to the person and abandon, nothing else, and cannot verify without the code; the claim page and the account email tell the owner to revoke it | S | `it::signup::x34_claim` |
| X51 | `free_sending` is `off`, or the shared-domain breaker is at stage 1, or the tenant is sending-paused | A provisional send is refused as for any Free workspace (`403 policy_denied` `free_sending_off`, `429 daily_cap_reached` `shared_domain`, `409 sending_paused`) | S | `it::signup::x21_send_only_to_human` |
| X52 | The code email cannot be submitted: platform domain `failing` or `suspended`, the system identity paused or at its cap | The sign-up stands with `verification.email: "not_sent"`; nothing is submitted while the domain is broken; the agent resends later; `system_mail_blocked` raised for a refusal | S | `it::signup::x52_mail_unavailable` |
| X53 | `POST /v1/platform/keys/link/rotate?revoke_previous=true` while sign-ups wait | Codes, links and refuse tokens under the old kid fail as `expired`; a resend issues working ones | S | `it::signup::x52_mail_unavailable` |
| X54 | Sign-up mail bounces or draws complaints above the thresholds | The switch goes `off` automatically, `containment:agent_signup_off` pages, the audit row is written; a platform key turns it back on | S | `it::signup::x38_outcomes` |
| X55 | A caller wants a test workspace through sign-up | Not available: the tenant is `live` and the key `pmk_live_`; test tenants come from a person's workspace | S | `it::signup::create_and_answer` |
| X56 | A platform key writes the policy of a provisional tenant, or reads it | Stored; the overlay keeps winning until verification; `GET …/policy` shows the overlay and `"provisional_overlay": true` | S | `core::signup::overlay_sets`, `it::signup::x56_policy_view` |
| X57 | The expiry cron is late or fails | The sign-up cannot be verified after its 7 days; the next run starts its erasure | S | `it::signup::x33_verify_races_expiry` |
| X58 | A sign-up verified on its sixth day sends on the day after | The ramp applies from `verified_at`: the 51st message of a day `429 daily_cap_reached`; the daily evaluation counts 7 days from `verified_at` | S | `it::signup::x28_verify_effects` |
| X59 | A resend or a replacement commits between the reserved attempt and the verification of the previous code | The older code verifies nothing (`422 signup_code_invalid`, `superseded`) | S | `it::signup::x30_resend_and_replace` |
| X60 | A page, an email or a webchat user steers a Pylota agent to sign up to Pylota Identity in an operator's name | Pylota's tools refuse requests to `/v1/agent-signup` and `/console/agent-signup/*` ([§22](#22-pylota-agentic-os)) | I | integrator |
| X61 | The verified person deletes their console account; the owner deletes a verified agent workspace | Person deletion replaces the row's `human_hash` with the row's ID and keeps the refusal list; tenant erasure deletes the `agent_signups` row and its probe counts zero | S | `it::signup::x61_privacy` |
| X62 | An `agent_name` or `source` with markup, an address or misleading words | Neither appears in any email the deployment sends; the console escapes both; `source` cannot hold `@` | S | `it::signup::x62_email_content` |
| X63 | A platform or partner key calls a `/v1/agent-signup` route; a key of a tenant with no sign-up; another tenant's key; the MCP tool list of an ordinary key | `403 scope_denied`; `404 signup_not_found`; `404 signup_not_found`; the four sign-up tools are not listed | S | `it::signup::x23_permission_overlay`, `it::mcp::signup_tools`, `it::security::cross_tenant_matrix` |

## 25. Tests

| Test | Proves | Covers |
|---|---|---|
| `core::signup::x5_username_rules` | Lower-casing and the pattern; the non-ASCII, confusable, reserved, `RESERVED_SUFFIXES` and `pm-` refusals with their codes; the fold used for uniqueness (`acrne` folds to `acme`); the address `agent.{username}@` stays within 40 characters for a 32-character username | X5, FR-ASU-3 |
| `core::signup::x7_human_email_rules` | Normalisation (case, IDNA); syntax and SMTPUTF8 refusals; every RFC 2606 placeholder name and `simulator.invalid`; the disposable list and `PM_SIGNUP_BLOCKED_DOMAINS`; hosted addresses (platform domain and names under it, address rows by state, an address row written with a `+tag`); one reason for the three classes; the base address (`jo+x@` → `jo@`, `J.O+x@googlemail.com` → `jo@gmail.com`, dots kept on every other domain) and its keyed hash | X7–X9, X12, FR-ASU-4, FR-ASU-5 |
| `core::signup::overlay_sets` | The provisional and verified permission sets exactly as [§4.1](#41-permissions); `overlay_permissions` is an intersection; the policy overlay takes the stricter value per field and class (numbers lower, switches strict, enums ordered) and never loosens a stricter deployment value; step 16a does not apply while provisional, so a send to the human passes whatever `drafts.require_approval` the deployment, the platform or the tenant sets | X23, X56, FR-ASU-6, FR-ASU-7 |
| `core::signup::tokens_and_codes` | Codes uniform over 000000–999999 by rejection sampling from fixed RNG bytes; the code-hash input `asu:{id}:{seq}:{code}`; the refuse token round-trips and expires 90 days after issue, and a changed field, another kid, an expired time or a notification token are refused | X40, FR-ASU-8, FR-ASU-9, FR-ASU-13 |
| `core::policy::x21_provisional_send_steps` | Step 4 passes for a provisional identity without owner; step 7a refuses any recipient but the human (to, cc, bcc, reply target, forward) with `signup_unverified`, refuses everything when `human_status = 'bounced'` and, with `signup_paused`, while the `agent_signup` switch is `off`, and does nothing for other tenants; the overlay caps give `daily_cap_reached` and `too_many_recipients` | X21, X22, FR-ASU-7 |
| `it::signup::x1_feature_gates` | Each condition of [§1](#1-when-agent-sign-up-is-on) alone gives `403 signup_closed` with its reason and writes nothing; all of them together create | X1, FR-ASU-1 |
| `it::signup::x2_off_with_live_signups` | A sign-up created while on, then the variable turned off (and, separately, `PM_SIGNUP` set to `waitlist`, and the switch set `off`): verify, link, claim, refuse, abandon and expiry work; new sign-ups refused; with the switch off, resend and replace refused too, and every provisional send refused with `signup_paused` | X2, X3, FR-ASU-1, FR-ASU-15 |
| `it::signup::create_and_answer` | The `201` body; the rows of [§2.1](#21-what-a-sign-up-creates) in one batch with their audit rows (`agent_signup.create` with `tenant_id = NULL`); the key works and is `pmk_live_`; no stored record holds the secret; the email submitted through the system identity with its idempotency key and metadata; malformed and oversized bodies refused before counting; `Idempotency-Key` ignored | X8, X12, X55, FR-ASU-2 |
| `it::signup::x4_username_race` | Two concurrent sign-ups for one name, and for `acme` and `acrne`: one `201`; the name of an erased sign-up and of a Pylota operator's slug stay `409 username_taken`; the erased address answers `550 5.1.1` | X4–X6, FR-ASU-3 |
| `it::signup::x10_mail_domain` | With the DoH fake: MX present, null MX, implicit MX by A record, NXDOMAIN, neither record; the first resolver failing and the second answering; both failing gives `503` and spends nothing | X10, X11, FR-ASU-4 |
| `it::signup::x13_admission_budgets` | `RL_SIGNIN`'s 11th request; the sixth sign-up of a network, also by rotating addresses in one IPv6 /64; the 101st of an ASN; the deployment cap and the 80% alert; the class share stopping sign-up mail while sign-in mail is sent; `resets_at` at 00:00 UTC | X13–X16, FR-ASU-5 |
| `it::signup::x17_human_scope_identical` | The five person-scope causes give byte-identical bodies apart from `request_id` and each spend the caller's network, ASN and deployment budgets once; a `+tag` variant and a dotted or `googlemail.com` form of a Gmail address count as the same person; two racing sign-ups for one person give one `201` | X17, X18, FR-ASU-5 |
| `it::signup::x19_existing_person` | A sign-up naming an existing user is answered as for an unknown address; verification adds them as owner and creates no second `users` row; an owner of a Free workspace (also by `+tag`) gets `workspace_limit` without burning the code, and the same code verifies after an upgrade; a disabled user gets `person_disabled` | X19, X45, X46, FR-ASU-10 |
| `it::signup::x20_lost_answer` | A retried sign-up after a dropped answer gets `409 username_taken`; the person confirms by the link, signs in, mints a key and the agent works with it | X20 |
| `it::signup::x21_send_only_to_human` | Sends to the human pass up to 10 a day, step 16a included; every other recipient form is refused with nothing stored; the `free_sending` switch, the breaker and a sending pause refuse as for a Free workspace | X21, X22, X51, FR-ASU-7 |
| `it::signup::x23_permission_overlay` | A table test over every route and MCP tool: with the provisional key each route outside the provisional set answers `permission_denied` (`signup_unverified`), the upload routes included; after verification the verified set is allowed; `DELETE /v1/identities/{own}` refused; platform, partner, other-tenant and no-sign-up keys on the six routes get `403 scope_denied` or `404 signup_not_found` | X23, X43, X63, FR-ASU-6 |
| `it::signup::x24_service_codes_held` | A code from a service is held `account_unapproved` and hidden; `wait(kind=verification)` refused; the inbound caps of the overlay throttle; no triage job runs; the 6th screening model call of the day for one sender address (the provisional cap is 50, a tenth per address) is refused and its message quarantined `screening_unavailable` | X24, X25, FR-ASU-7 |
| `it::signup::x26_code_limits` | Ten wrong codes burn the code; 30 across resends lock the code path while the link works; 20 concurrent wrong codes make at most 10 comparisons; a code past 24 hours is `expired` | X26, X27, X29, FR-ASU-9 |
| `it::signup::x28_verify_effects` | After the right code: the owner membership, the `users` row, the identity's owner, `provisional_until` cleared, `signup.verified` in `event_index` and the feed, the `account` email, the audit rows; the next request has the verified permissions and policy; a second correct code answers `already_verified`; the ramp counts from `verified_at` | X28, X58, FR-ASU-10 |
| `it::signup::x30_resend_and_replace` | Resend limits per hour and per person; a new code invalidates the old; replacement limits, same-address resend, refusal after verification, two concurrent replacements, `expires_at` unchanged; the old person's code (`wrong`, a failure), link and refusal after a replacement; a resend or replacement between reserve and verify leaves the older code `superseded` | X30–X32, X59, FR-ASU-11 |
| `it::signup::x33_verify_races_expiry` | With the fake clock at the 7-day boundary and the cron and a verification in parallel, exactly one commits, many times over; with the cron stopped, verification after the boundary is refused | X33, X57, FR-ASU-14 |
| `it::signup::x34_claim` | Every row of the claim table of [§8](#8-claiming-in-the-console), including a Pylota partner tenant's key and the partner key; re-authentication required; a race with the link; the success page's revoke advice; the pasted key absent from logs, audit rows and the page | X34–X36, X50, FR-ASU-12 |
| `it::signup::x36_link_flow` | The link `GET` changes nothing; the `POST` without the box changes nothing; another person's session confirms nothing; a two-factor person gets a pending step that confirms only on success; a second `POST` gets the expired page; terms recorded on the person | X36, X37, FR-ASU-10 |
| `it::signup::x38_outcomes` | Delivery fixtures on the system identity's code emails and the provisional tenant's mail: `delivered` and hard `bounced` set `human_status`; a complaint refuses and erases; `CountSignupOutcome` is idempotent by event ID; crossing each threshold of [§13.2](#132-the-automatic-switch) sets the switch, writes the audit row and pages | X38, X39, X54, FR-ASU-15 |
| `it::signup::x40_refusal` | The refuse `GET` and `POST`; a refusal from the email of an expired and erased sign-up records only the refusal; a one-click `POST` with no cookie, CSRF token or `Origin`, never redirected; on a verified sign-up only the row; a refusal racing a verification; **Allow again** in settings | X40, X41, FR-ASU-13 |
| `it::signup::x42_end_and_erasure` | Abandonment, refusal and expiry each run the end batch: the tenant-scope erasure with its receipt and probes (`agent_signups` at zero), the message-scope erasures of the code emails, no email to the person; with a platform hold the erasure ends `completed_with_holds` | X42, X49, FR-ASU-14 |
| `it::signup::x44_hosted_race` | An identity or address at a live sign-up's human address, or at its base without the `+tag`, gets `409 address_taken`; one created before verification makes verification `human_email_hosted`; a hosted human address at sign-up is refused | X9, X44, FR-ASU-4 |
| `it::signup::x47_suspended_and_erased` | A suspended provisional tenant: inbound temporary failure, sends and verification refused, expiry on time; an erasing or erased tenant: `401 key_revoked` and the generic pages | X47, X48 |
| `it::signup::x52_mail_unavailable` | Platform domain `failing`: nothing submitted, `not_sent`, a later resend sends; the system identity paused or at its cap: `not_sent` and `system_mail_blocked`; a revoked `link` key makes the code `expired` and a resend works | X52, X53 |
| `it::signup::x56_policy_view` | A platform key's policy write on a provisional tenant is stored but not effective; the view shows the overlay; after verification the stored value applies | X56 |
| `it::signup::x61_privacy` | Person deletion scrubs `human_hash` of the rows the person verified and keeps refusals; tenant erasure of a verified workspace deletes the row; no clear address remains in `agent_signups` after verification or an end | X61, FR-ASU-16 |
| `it::signup::x62_email_content` | The code email of a sign-up with a hostile `agent_name` and `source` contains neither; its subject is fixed; the console escapes both | X62, FR-ASU-8 |
| `it::signup::platform_listing_and_status` | `GET /v1/platform/agent-signups` filters and shows hints, never an address; `GET /v1/platform/status` and `idvy doctor` report the feature's state; `agent_signup_spike` fires on a fixture of audit rows | FR-ASU-15, [§21](#21-observability) |
| `it::mcp::signup_tools` | The four tools are listed only for sign-up keys, map to their routes and return their errors; no tool abandons | X63 |
| `it::console::agent_signup_pages` | The verify, claim and refuse pages and the settings notice render without JavaScript, escape untrusted text and join `browser::console::no_js` and `browser::console::axe_scan` | [§19](#19-console) |
| `it::security::cross_tenant_matrix` (existing) | The new routes join the matrix | X63, NFR-SEC-1 |
| `it::logs::i5_no_content_in_logs` (existing) | Canaries in the human address, `agent_name`, `source`, the code and the key never appear in captured logs | FR-ASU-16 |
| `platform::config::startup_rules` (existing) | `PM_AGENT_SIGNUP` and `PM_AGENT_SIGNUP_DAILY_CAP` malformed give `config_invalid`; `on` with sign-up not open starts the Worker | X1 |
| `cli::agent_signup::create_writes_profile` | `idvy agent-signup create` writes the key into the named profile with mode `0600` and prints it only with `--print-key`; the other subcommands map to their routes | [§20](#20-mcp-and-cli) |
| `live::signup::x40_refuse_one_click_headers` | On staging, a code email received in a real mailbox carries both RFC 8058 headers, a DKIM signature whose `h=` covers them, and a refusal by one-click `POST` works | X40, FR-ASU-8 |
| `live::signup::end_to_end` | On staging: sign up, receive the code in a real mailbox, verify, send to a new recipient within the ramp; a second sign-up abandoned with `DELETE /v1/agent-signup` is erased and its address then answers `550 5.1.1` (expiry itself is proved with the fake clock by `it::signup::x33_verify_races_expiry` and `it::signup::x42_end_and_erasure`) | FR-ASU-2, FR-ASU-10, FR-ASU-14 |

## 26. Milestone

To add to [Build plan](../build-plan.md) after M33. Links below are relative to this page; when pasting,
rebase them to `project/` (`agent-signup.md` → `design/agent-signup.md`, `#25-tests` →
`design/agent-signup.md#25-tests`, `../cloud-commissioning.md` → `cloud-commissioning.md`).

> ## M34 · Agent self-sign-up (after M24, M26, M27, M29, M30, M31 and M33)
>
> **Goal:** an agent with no key creates a provisional Free workspace with one inbox and one key, which
> can email only the person it names until that person verifies; unverified workspaces are erased after 7
> days. Behind `PM_AGENT_SIGNUP` (off), on Pylota Identity Cloud only ([Agent self-sign-up](agent-signup.md)).
>
> **Files:** `crates/core/src/signup.rs` (pure: normalisation, the username and human-email rules, the base
> address, the permission and policy overlays, code and token encodings); `crates/worker/src/signup/{mod.rs,
> create.rs, admit.rs, verify.rs, human.rs, end.rs, mail.rs, outcomes.rs}`; `handlers/agent_signup.rs`;
> `console/pages/agent_signup.rs`; `crons/agent_signup.rs` (on the `*/15` cron); no migration (the two
> tables, `tenants.provisional_until`, `pending_auth.agent_signup_id` and the `ops_switches` value are in
> `0001_init.sql`). Hooks in other milestones' files, each reviewed by that file's owner:
>
> | File | Hook |
> |---|---|
> | `auth/mod.rs` (M5) | Step 9: `provisional_until` read with the tenant row, the permission overlay |
> | `auth/routes.rs` (M5) | `Scope::AgentSignup` and its entry in the empty-permission exception list |
> | `crates/core/src/policy.rs` (M2, M5) | `effective(…, provisional)` with `PROVISIONAL_POLICY`; send steps 4 and 7a |
> | `handlers/send.rs` (M9) | The human-address read for a provisional tenant |
> | `consumers/inbound.rs` (M7) | Passes `provisional_until` to `effective` |
> | `consumers/delivery.rs` (M9) | Sign-up outcomes, `CountSignupOutcome`, the `agent_signup_off` rule |
> | `quota/mod.rs` (M5 stub, M24 `SystemMail`) | `SystemMailClass::Signup` and its budgets; `CountSignupOutcome` |
> | `handlers/identities.rs`, `handlers/addresses.rs` (M6) | `409 address_taken` at a live sign-up's human address |
> | `console/pending.rs` (M24) | The `two_factor` step with `agent_signup_id` |
> | `console/pages/settings.rs` (M24) | The refusal notice and **Allow again** |
> | `crons/signup_ramp.rs` (M24) | `verified_at` as the ramp's start for verified agent workspaces |
> | `notify/` (M26) | `AccountEvent::AgentSignupVerified` |
> | `jobs/erasure.rs` (M14) | `agent_signups` in `delete_d1_rows` and in the step-8 probe; person deletion's `human_hash` scrub |
> | `handlers/uploads.rs` (M33) | `403 permission_denied` (`signup_unverified`) while provisional |
> | `mcp/tools.rs` (M15) | The four `mail_signup_*` tools and their rows in M15's table test |
> | `crates/cli/src/commands/agent_signup.rs` (M16) | `idvy agent-signup …`, `idvy agent-signups list` |
> | `crons/alerts.rs` (M17) | `agent_signup_spike`, `agent_signup_cap_80`, `containment:agent_signup_off` |
>
> **Implements:** FR-ASU-1 to FR-ASU-16 and the amended FR-IDN-2; edge rows X1–X59 and X61–X63 (X60 is
> integrator-owned).
>
> **Acceptance:**
>
> - Every test of [Agent self-sign-up § 25](#25-tests): the `core::signup::*` and
>   `core::policy::x21_provisional_send_steps` tests; the `it::signup::*` tests; `it::mcp::signup_tools`;
>   `it::console::agent_signup_pages` (the pages join `browser::console::no_js` and
>   `browser::console::axe_scan`); `cli::agent_signup::create_writes_profile`.
> - The new routes join `it::security::cross_tenant_matrix`; `sdk::coverage::every_operation` passes with
>   the seven new operations; `it::logs::i5_no_content_in_logs` carries the new canaries;
>   `platform::config::startup_rules` covers the two variables.
> - With `PM_AGENT_SIGNUP = "off"` every M24, M26 and M27 suite passes unchanged.
> - `live::signup::x40_refuse_one_click_headers` and `live::signup::end_to_end` pass on staging (M20's
>   environment) before the gate below.
>
> **Gate:** `PM_AGENT_SIGNUP` stays `off` on Cloud until gate D of [Cloud commissioning](../cloud-commissioning.md#10-go-live-gates):
> phase C has run for at least 30 days with no `containment:*` page, no `shared_domain_breaker` at stage 2
> and no unresolved `signup_ramp_review`; DMARC on the platform domain is at `p=reject`; the two `live::signup`
> tests pass on production with the variable on for one hour, then it is turned off again until the owner
> decides.

Dependencies, for the graph: M24 (sign-up, the system-mail budgets, the ramp, the pending step), M26
(account emails), M27 (the ledger gate the overlay relies on), M29 (`drafts:*` and `drafts.require_approval`),
M30 (`events:read`, and the feed through which the agent learns of verification), M31 (`screening.*`), M33
(the upload routes the overlay refuses).

## 27. Compared with AgentMail (read 2026-10-17)

| AgentMail behaviour | Pylota Identity behaviour | Verdict |
|---|---|---|
| `POST /v0/agent/sign-up` returns an organisation, an inbox and a key; `human_email` optional | `POST /v1/agent-signup` returns a workspace, an inbox and a key; `human_email` required | Deliberately different: FR-IDN-2 requires an accountable human, and an inbox nobody answers for is what spammers want |
| No human: a receive-only inbox | Not offered | Deliberately different, as above |
| Sign-up idempotent on `human_email`: the same address rotates the key and resends the code | A second sign-up naming a person with a live sign-up is refused with the indistinguishable person-scope answer; nothing about the first changes | Exceed: a repeat sign-up changes nothing about the first. AgentMail documents only that "Calling it again with the same `human_email` will rotate the API key and resend the OTP if expired"; `username` is also required, and whether it or anything else must match is not stated (OpenAPI `agent_signUp`, re-read 2026-10-18) |
| No rate limit documented on sign-up | `RL_SIGNIN`; exact daily budgets per network, ASN, person and deployment; one live sign-up per person | Exceed |
| Placeholder and some provider domains refused | RFC 2606 names, disposable lists, hosted addresses and domains that cannot receive mail refused, before anything is sent | Exceed |
| An address with a console account cannot be attached (409) | Accepted like any address and linked at verification by the verified address | Exceed: no account oracle |
| Username becomes `username@agentmail.to`; a lost org keeps its username | Username becomes the workspace's suffix, address `agent.{username}@pylotamail.com`; never released | Match (never released); different address form, because the shared domain gives every workspace its own suffix |
| Unverified: 1 inbox, 10 emails a day, only to the human, no domains, no key creation, no list or pod writes | One inbox, 10 a day to the human only with one recipient, no keys, webhooks, domains, identities, lists, drafts, uploads, service accounts, signing or agentic search; every guard strict | Exceed |
| Unverified agents may receive codes from other services | Held by the ledger gate; `wait` refuses them | Exceed |
| 6-digit code, 24 h, 10 attempts; while valid, a re-attach returns the same code | 6 digits, 24 h, 10 attempts per code, 30 failures per sign-up, constant-time compare, concurrent guesses bounded; a resend issues a new code (3 an hour, 5 a day per person) | Exceed |
| Replace the human at most twice | At most twice, before verification, without extending the 7 days | Match |
| Verification moves the org to the Free plan with a new-recipient ramp; limits may be stale for 5 minutes | A Free workspace with the new-workspace ramp counted from verification; no stale limits (no cache) | Match; exceed on staleness |
| Claim in the console by pasting the key; any console user unless a human is attached; US region only | Claim only by the person the agent named; Pylota-managed tenants never claimable | Exceed |
| No documented expiry of an unverified organisation | Erased with a receipt after 7 days, or at once on abandonment or refusal; no further mail | Exceed |
| No documented way for a person to refuse | One-click "Not me" (RFC 8058), refusal list, complaints treated as refusals | Exceed |
| Suspension `unauthorized.signup` and `signup.domain_limit`, lifted by support only | Automatic containment (`agent_signup_off`), alerts, the existing auto-pauses; no support step ([ADR 0015](../adr/0015-solo-operator.md)) | Different: automation instead of a support desk |
| Sign-up over the x402 and MPP pay-per-call hosts | Not offered | Deferred ([ADR 0017](../adr/0017-agentmail-parity.md)) |

## 28. Integration

The shared pages carry this page's contracts: the PRD, the REST API reference and `openapi.yaml`, the
errors, events, limits and configuration references, the data model, security, workspace policy and the
other design pages it extends, the MCP and CLI references, the edge-case register and the build plan
([ADR 0017 § Integration](../adr/0017-agentmail-parity.md#integration), integrated on 2026-10-18). A
difference between this page and one of them is a defect: fix both in the same change.
