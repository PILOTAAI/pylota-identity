# Product requirements (PRD)

| | |
|---|---|
| Product | Pylota Mail |
| Document owner | TREFT LTD (the owner, who is the sole developer and the Cloud operator) |
| Status | Approved for build (v1.0); the additions of [ADR 0017](adr/0017-agentmail-parity.md) and [ADR 0018](adr/0018-typescript-sdk-in-its-own-repository.md) stay Proposed until the owner accepts those records |
| Last reviewed | 2026-10-17 |
| Licence | FSL-1.1-ALv2 (Fair Source; each release becomes Apache-2.0 two years after it ships) |
| Related | [Architecture](architecture.md) · [Design](design/index.md) · [Build plan](build-plan.md) · [Edge cases](edge-cases.md) |

Requirement IDs (`FR-*`, `NFR-*`) are stable. Tests, design documents and pull requests cite them.
The words **must**, **should** and **may** are used as in RFC 2119.

## 1. Summary

Pylota Mail is an email and identity service for AI agents. Each agent gets an **identity**: a mailbox
with one or more addresses, authenticated sending, verified inbound mail, threads, attachments with
extracted text, triage and search, plus signing keys that let it prove who it is to other services. Applications and agents use it through a REST API, an MCP server and
a CLI. It reports what happened through signed webhooks.

It is source available under the Functional Source License (FSL-1.1-ALv2), written entirely in Rust (the
TypeScript SDK is a separate client built in its own repository, [ADR 0018](adr/0018-typescript-sdk-in-its-own-repository.md)), and
runs as one Cloudflare Worker on the deployer's own Cloudflare account. Pylota also operates it as a hosted
service, **Pylota Mail Cloud**, with Free, Developer and Team plans (section 13). Pylota (a platform for independent car-rental operators) is the first
user, as a partner on Pylota Mail Cloud. Pylota gives each operator four agent identities (bookings, inquiry, compliance, maintenance), and
operators move those identities from a shared platform domain to their own domain over time.

## 2. Problem

Agents that act for a business need to send and receive email as a stable, trustworthy identity. The
options before this product were:

- **Hosted agent-mail APIs.** They work, but data lives with a third party, residency is limited,
  and pricing is per inbox. They also lack features agents need: verified citations from search,
  safe-retry semantics, domain changes without breaking threads.
- **Transactional email providers.** They send, but receiving, threading, search and identity are the
  application's problem.
- **Raw Cloudflare Email Routing and Email Sending.** These are the right primitives. On their own they
  give no mailbox, threading, search, idempotency, domain lifecycle, authentication verdicts or erasure.

Pylota's own experience showed the cost of these gaps:

- operator mail never reached production because DNS records were copied from documentation instead
  of being read from an API;
- HTML-only mail was dropped;
- a failed send re-ran an LLM turn;
- agents could not search their own mail at all.

## 3. Users

| Persona | Needs |
|---|---|
| **Integrator** (a developer building an agent product, e.g. Pylota's API, which uses a partner key on Pylota Mail Cloud) | Provision a tenant and identities per customer, send and receive reliably, get events, change domains, erase data, all through a stable API; on a shared deployment, with a partner key that reaches only its own customers' tenants (FR-KEY-4) |
| **Agent** (an LLM driving tools through MCP or an integrator's tool layer) | Small, well-described tools; search that finds the right email; reads that fit a context window; sends that are safe to retry; content marked untrusted |
| **Operator** (the integrator's customer, e.g. a car-rental business) | Their agents email from their brand, history survives domain changes, nothing is sent from a broken domain, clear instructions when DNS breaks |
| **Self-hoster / maintainer** | Deploy to their own Cloudflare account in minutes, upgrade safely, observe health, restore after mistakes |

## 4. Goals and non-goals

### Goals (v1.0)

1. Agent identities with addresses that can change domain without losing history or breaking threads,
   and that can prove who they are to other services with short-lived signed assertions, verifiable
   against a published key set.
2. Reliable inbound mail: no acknowledged message is ever lost, and every message carries an
   authentication verdict and trust metadata.
3. Safe outbound mail. A retry never produces a second email. An outcome that cannot be known is
   reported as uncertain, never guessed.
4. Search that agents can rely on: keyword, semantic, hybrid and agentic (cited answers whose citations
   are verified by code).
5. Triage on every inbound message: category, needs-reply, urgency, summary and risk flags.
6. REST API, MCP server, CLI and Rust SDK, all generated from or checked against one contract, and a
   TypeScript SDK generated from the same contract (P1, FR-SDK-2).
7. Deployable by a stranger to their own Cloudflare account in under 15 minutes of hands-on time.
8. Privacy by design: EU jurisdiction when chosen, retention policies, erasure with receipts,
   subject-access export.
9. A console for the people who run the agents: passwordless sign-in (email, Google or GitHub, with
   optional two-step verification), self-serve sign-up on Pylota Mail Cloud, workspaces with members,
   roles and enforced seats, inbox views, quarantine review, keys, domains, plan and usage, and email
   notifications (usage alerts, new mail, and a daily list of what needs a person). It works with no
   JavaScript.
10. Plans that are enforced exactly: atomic holds so two requests can never both pass on the last unit, a
    `402 billing_limit` that is safe to retry with the same idempotency key after an upgrade, and metering
    that never depends on a billing provider being reachable.
11. Custom domains wherever their DNS is hosted. Six connection methods cover a domain on Cloudflare, a
    domain whose DNS stays at any other host, and a domain whose mailbox stays with the customer's own
    provider. Every method keeps the rule of never sending unauthenticated mail.
12. Drafts that a person or the operator's own system approves before they are sent, with the approval
    bound to the exact content approved, and scheduled sends driven by the mailbox itself, all through
    the one send pipeline, so no draft or schedule can send twice or send what nobody approved.

### Non-goals (v1.0)

- A full webmail client. The console is for oversight and the actions that need a person (keys, domains,
  members, quarantine release, billing). Agents work through the API and MCP.
- IMAP, POP3 or SMTP submission access, for people or agents. Workers cannot accept inbound TCP
  connections, and a mail server beside the Worker would be a second stateful system for one operator
  ([gap register § 5](agentmail-gap-analysis.md#5-deliberate-differences)).
- Bulk marketing campaigns. Marketing mail is supported per message with consent and unsubscribe
  headers, but there are no list-management or campaign features.
- Open tracking. A tracking pixel records when a person read a message without their knowledge, and image
  preloading makes the counts unreliable; delivery events and replies are the signals the service reports.
- A calendar. Pylota Mail keeps no events, agenda, recurrence engine or reminders; it reads and verifies
  iCalendar in mail, sends iMIP built by the integrator, and answers verified invitations on request
  ([Calendar invitations](design/calendar-invitations.md)). A second calendar would compete with the
  integrator's own for the same facts.
- A file drive. Uploads have no paths, folders, downloads or sharing and live at most 7 days; long-lived
  files belong in the integrator's own storage.
- Attachments fetched from URLs. Pylota Mail never fetches a URL to build an attachment (SSRF, content
  integrity, prompt injection; [Uploads § 1](design/uploads.md#1-what-uploads-are-and-are-not)). Upload
  the file instead.
- OAuth 2.1 for the MCP endpoint (planned for v1.1; v1.0 uses API keys as bearer tokens; [ADR 0016](adr/0016-plan-items-changed-for-v1.md)).
- MCP server-sent event notifications. MCP 2026-07-28's `subscriptions/listen` carries only list and
  resource changes, and an events primitive is not yet specified; `mail_events` and the WebSocket stream
  cover agents meanwhile ([Event feed § 15.1](design/event-feed.md#151-mcp-mail_events)).
- A Python SDK (P2). v1.0 offers Python users the OpenAPI document, the MCP server and the CLI
  ([SDKs, skills and agent tooling §3](design/sdk-and-skills.md#3-python)).
- Framework toolkit packages. Guides use each framework's MCP client or the TypeScript SDK.
- Running on platforms other than Cloudflare Workers.

### Unique selling propositions

Each proposition is a guarantee enforced in code, with the requirements and tests that prove it. Marketing
copy (README, landing page) may only claim what this table lists.

| # | Proposition | Guaranteed by | Proved by |
|---|---|---|---|
| U1 | **Answers you can check.** Agentic search cites message IDs, and a deterministic verifier removes any sentence the evidence does not support | FR-SRCH-8, FR-SRCH-9 | F10–F13, NFR-QUAL-2 |
| U2 | **One email per intent.** Idempotency is required; an unknown outcome becomes `uncertain` and is never resent; plan limits never break a retry | FR-OUT-1, FR-OUT-2, FR-BILL-6 | G1, G2, L2, W3 |
| U3 | **Identities outlive domains.** Addresses move between domains with history and threads intact, with rollback and a clean `550 5.1.6` after retirement | FR-ADR-1–5 | A11, C3, live domain-change test |
| U4 | **Never sends mail that fails authentication.** Two-resolver health checks, aligned fallback in the same thread, suspension when ownership changes, and, for a relay we do not control, an alignment probe before the first send and every day | FR-DOM-4–6, FR-DOM-11 | H1, H4, H7, N18 |
| U5 | **Built for untrusted input.** Verdicts and trust flags on every message, hidden text stripped, fenced model input, quarantine release only with the human-review permission | FR-IN-4–9, FR-CON-6 | B10, B11, D2, D9, E1 |
| U6 | **Your account, your receipts.** Runs in the deployer's Cloudflare account (EU optional); erasure returns per-store counts and empty probe queries | FR-PRV-1–6 | I1–I7 |
| U7 | **Real team seats.** Members, roles and seat limits are enforced, with an audit log of every privileged action and two-step verification that a workspace can require | FR-CON-2–5, FR-CON-10 | W8–W10, W27 |
| U8 | **Tested against the edge cases.** A public edge-case register where every row the service owns names its test, a MIME conformance corpus, and quality gates in CI | Release criteria §9 | the register itself |
| U9 | **Nothing to keep running.** One Rust Worker on Cloudflare primitives: no servers, no external database, no billing vendor in the request path | NFR-COST-1, NFR-BILL-2 | W2 |
| U10 | **Agents that can prove who they are.** Each identity signs short-lived assertions with its own Ed25519 key, which any service verifies against the identity's published key set; the private key never leaves the Worker, and pausing the identity withdraws its keys at once | FR-IDN-6, FR-IDN-7, FR-IDN-9 | O1–O8, `it::assertions::sdk_verifies` |

### Competitive landscape

Researched on 2026-10-09 from each product's public documentation and repository. The AgentMail cells
from "Authentication verdicts" down were researched again on 2026-10-17
([ADR 0017](adr/0017-agentmail-parity.md), [gap register § 6](agentmail-gap-analysis.md#6-corrections-to-the-prds-competitive-table));
goshen-email was not re-read then, so its cells in the rows added on that date say "not compared". On
2026-10-18 the AgentMail search cell was corrected from AgentMail's OpenAPI document, whose search
operations are "Full-text search", and the plugin cell from its integration pages.
Re-check before quoting externally. "–" means the capability is not offered or not documented.

| Capability | AgentMail (hosted) | goshen-email (FSL, self-host or hosted) | Pylota Mail |
|---|---|---|---|
| Identity separate from address; domain change with rollback | – (the address is the inbox) | – | Yes (U3) |
| Required idempotency; uncertain sends never resent | Optional keys | Yes | Yes, plus reconciliation (U2) |
| Keyword search | Yes | Yes (full-text) | Yes, with exact references |
| Semantic and hybrid search | – (full-text search only) | – | Yes, with reranking |
| Agentic search with verified citations | – | – | Yes (U1) |
| Domain health checks with aligned fallback | – | – | Yes (U4) |
| Triage | – | Yes (metered) | Yes, with deterministic rules |
| Plan metering | Hosted | Third-party (Autumn), fails closed when it is down | In-process atomic holds (U9) |
| Language | – | TypeScript | Rust |
| Authentication verdicts and quarantine before an agent reads | `authentication_results`; unauthenticated and spam mail hidden by label; DMARC-failing mail dropped under an enforcing policy | At the gateway | Per-message verdicts. Mail that Cloudflare Email Routing rejects for failing authentication never reaches the Worker; failing mail the Worker receives is quarantined with a reason. Nothing is accepted and then dropped silently (U5) |
| Prompt-injection screening | Agent Armor (Labs, US only): advisory verdict; Hold mode withholds a notification but still lists the mail | not compared | Opt-in screening hold (`screening.mode: hold`; the default `monitor` only records verdicts) that hides mail until a verdict; once held, it fails closed by default; release by a person ([Screening](design/screening.md)) |
| Drafts and scheduled send | Yes; approval works only by withholding send permissions | not compared | Yes, with approval bound to the content; on Cloud only a person approves, unless the partner opts its tenants in to key approval (`drafts.key_approval`) ([Drafts](design/drafts.md)) |
| Event delivery | Webhooks (8 attempts in ~27.5 h, auto-disabled after 5 days without notice), polling queue, WebSocket (key in the URL) | not compared | Webhooks (72 h, dead letters, replay; an endpoint is disabled after 100 consecutive failures over at least 24 h, and `webhook.disabled` reaches platform and partner endpoints, never the tenant's own), ordered pull feed, durable consumers, resumable stream ([Event feed](design/event-feed.md)) |
| Calendar | Per-inbox calendar store (Labs, US only) | not compared | Verified iMIP in and out; the consumer's calendar keeps state ([Calendar invitations](design/calendar-invitations.md)) |
| Team seats | 2 / 2 / 10 by plan | Listed; one member per account today | Enforced, with roles (U7) |
| Erasure with receipts | Delete per message, thread and inbox; optional 24 h expiry | Per inbox | Message, thread, counterparty, identity, tenant, with receipts (U6); per-mailbox retention (FR-PRV-7, P1) |
| Runs in your own cloud | "BYO cloud deployment" on Enterprise | Yes | Runs in your own Cloudflare account (U6) |
| SDKs | Python, TypeScript | not compared | Rust, TypeScript ([SDKs and skills](design/sdk-and-skills.md)) |
| Agent self-sign-up | Yes; the human is optional | not compared | Yes, behind a flag; the accountable human is required (FR-IDN-2) |
| Agent skills and plugins checked against the contract | Skills repository; plugins for Claude Code, Codex, Cursor, ChatGPT and OpenClaw | not compared | Yes (FR-SDK-10) |

## 5. Scope and priorities

`P0` must ship in v1.0. `P1` ships in v1.0 unless it threatens the release, in which case it moves to
v1.1 with a written ADR. `P2` is v1.1 or later.

| Area | P0 | P1 | P2 |
|---|---|---|---|
| Tenancy and keys | Tenants (live and test), API keys at four levels, partner keys for integrators on a shared deployment (FR-KEY-4), workspace policy self-service within ceilings (FR-TEN-4) | – | Public-key API credentials (GAP-23) |
| Identities | CRUD, idempotent create, pause, accountable human; agent signing keys and assertions (JWKS); the service sign-up ledger (FR-IDN-10) | Signed HTTP requests (Web Bot Auth, spike S13) | An OIDC-compatible "sign in as this agent" relying-party flow (GAP-24) |
| Addresses | Platform domain, aliases, promote, retire, rollback | – | – |
| Domains | Platform domain; `cloudflare_zone`; `nameservers` | `dns_records` (spike S11); `send_only` (S8); `smtp_relay` (S12); `delegated_subdomain` behind `PM_CF_SUBDOMAIN_SETUP` (S10); zone-file export (FR-DOM-15) | Mailgun and SendGrid inbound sources; a mailbox-avatar (BIMI) guide (GAP-32) |
| Inbound | Parse, verdicts, quarantine, loops, attachments, extracted text; calendar objects with verification and `calendar.received` (FR-CAL-1–7, FR-CAL-12, FR-CAL-13) | Malware scanner hook; screening hold for agent-steering mail (FR-SCR-1–12); batch reads and label changes (FR-IN-13); authentication detail (FR-IN-14); trusted ARC sealers (FR-IN-15) | Arabic reply attribution, once a conformance sample from real clients exists (GAP-21) |
| Outbound | Send, reply, reply-all, forward, idempotency, uncertain, suppression; drafts, approval before send and scheduled send (FR-DRF); uploads: a file staged once per identity (≤ 25 MiB, SHA-256 verified, classified and scanned) and attached by `upload_id` (FR-UPL) | Signed links for large files; iMIP sends, the stale-sequence guard and calendar replies (FR-CAL-8–11), on the Cloudflare transport once spike S1 records the raw send; identity-scope allow and block lists (FR-OUT-17) | – |
| Delivery | All six provider event types, per-recipient status; deliverability metrics: pause windows with thresholds and headroom, and event counts per hour and day by identity, tenant and domain (FR-MET) | Uncertain-send reconciliation | – |
| Search | Keyword, semantic, hybrid, agentic; facets; tenant scope | Contacts, find-related | – |
| Triage | Category, needs-reply, urgency, summary, risk flags; rules | Custom categories | – |
| Integrations | REST, OpenAPI, MCP (API key), CLI, Rust SDK, webhooks; `wait` long-poll and verification extraction (quarantine rule 5, unsolicited OTP, depends on it: E4, E5); the event feed, durable event consumers and the WebSocket event stream (FR-EVT-1–11); the calendar object on REST, MCP, the CLI and the console (FR-CAL-14) | TypeScript SDK `@pylotahq/mail` (FR-SDK-2–9); agent skills and plugins (FR-SDK-10); MCP annotations from effect classes and client hints (FR-SDK-11); per-key MCP tool allow-list (FR-SDK-12); `pmail mcp stdio` (FR-SDK-13); CLI `--schema`, `--query` and `pmail skills generate` (FR-SDK-14); framework guides (FR-SDK-15); `calendar-reply` on REST, `mail_calendar_reply` and `pmail calendar-reply` (FR-CAL-10, FR-CAL-14); opt-in label events (FR-WH-6); webhook delivery headers (FR-WH-7); documentation links in errors (FR-API-3) | MCP OAuth 2.1; Python SDK; TypeScript agent-assertion verifier |
| Privacy | Retention, erasure with receipts, legal hold | Subject-access export; per-identity message expiry (FR-PRV-7) | – |
| Operations | Setup, deploy, doctor, metrics, DLQ consumers, test mode | Restore drill tooling | A second Pylota Mail Cloud jurisdiction, an owner decision (GAP-25) |
| Console | Sign-in, workspaces, members, roles, seats, inboxes, quarantine release, keys, domains, plan and usage; Cloud sign-up (waitlist and open); Google and GitHub sign-in; two-step verification (TOTP); the Overview; notifications; agent self-sign-up behind `PM_AGENT_SIGNUP` (Cloud, FR-ASU) | – | Passkeys; SSO (SAML/OIDC) |
| Plans and billing | Plan catalog, metering with holds, `402 billing_limit`, usage API, Stripe checkout and portal, top-ups; usage alerts | – | Annual billing, invoicing; pay-per-call through x402 or MPP (GAP-26) |

## 6. Functional requirements

### 6.1 Tenancy and access

- **FR-TEN-1** The service **must** support many tenants in one deployment. Every record belongs to
  exactly one tenant, except platform-level settings and the platform domain.
- **FR-TEN-2** A tenant **must** be either `live` or `test`. Mail a `test` tenant sends **must not**
  leave the deployment (see FR-OUT-12).
- **FR-TEN-3** A tenant **must** be suspendable. While it is suspended, inbound mail is answered with a
  temporary failure for up to five days, then refused permanently, and every send is refused. On domains
  that receive through SES, which accepts mail before the Worker sees it, inbound mail is held for the same
  five days and then dropped without a bounce (FR-DOM-9).
- **FR-KEY-1** API keys **must** be scoped at one of four levels: `platform`, `partner`, `tenant` or `identity`.
  Each key holds a list of permissions. A key can never create, read, rotate or revoke a key wider than
  itself. The one exception: a platform or partner key **may** grant `identities:sign`, which it cannot
  hold, to a tenant or identity key it mints, and the grant is audit-logged. Deleting a workspace (a
  tenant-scope erasure) **must** need its own permission, `tenants:erase`, which a workspace admin and the
  keys an admin mints never hold.
- **FR-KEY-2** Key secrets **must** be shown once, stored only as a keyed hash, support expiry, and
  support rotation with an overlap window. No stored record, an idempotency record included, **may** hold
  a secret: an idempotent replay of a response that carried one returns it without the secret
  (`"secret_replayed": false`).
- **FR-KEY-3** Tenant and identity scope **must** come from the authenticated key, never from the request body.
- **FR-KEY-4** A deployment **must** support **partners**: integrators that run their own customers as
  tenants of a shared deployment (Pylota on Pylota Mail Cloud). A platform key creates, suspends and
  deletes a partner and mints its **partner keys**. A partner key **must** be able to create tenants and
  act on them as a platform key does, and on nothing else: a tenant created by another partner or by no
  partner, and everything in it, **must** answer it as a missing one does. It **must never** mint a partner
  or platform key, hold `platform:ops`, `partners:manage` or `identities:sign`, or change a tenant's
  billing mode, which comes from the partner's `default_billing_mode`. A partner key **must not** raise its
  tenants' limits above the deployment default or the platform's value (lower-only policy fields),
  set a platform-only policy field, lift a platform suspension or resume an abuse pause, and a partner
  **must** be bounded by `max_tenants` and a tenant-creation rate. A partner's webhook endpoints **must**
  receive only its own tenants' events. A suspended partner **must** be contained: its keys and its
  tenants' API keys refused and deliveries to its and its tenants' endpoints held, while inbound mail is
  still stored. A partner **must not** be deletable while it has a tenant that is not erased, and deletion
  **must** keep the row (soft delete), so a tenant's `partner_id` never changes
  ([REST API › Partners](../reference/api.md#partners), [Security › Partner keys](design/security.md#partner-keys)).
- **FR-KEY-5** A key **must** record the console user behind it, if any, and keys minted from it **must**
  inherit that record. Removing that person from a workspace **must** revoke their keys of that workspace,
  and lowering their role **must** revoke those holding a permission the new role lacks. However many keys
  one tenant or one partner mints, its request rate **must** stay bounded by a shared per-tenant or
  per-partner limit, and the number of its active keys by a cap
  ([Security › Who minted a key](design/security.md#who-minted-a-key), [Security § 10](design/security.md#10-rate-limiting-and-abuse)).
- **FR-TEN-4** A workspace **must** be able to change its own tenant policy: a tenant key holding
  `policy:write`, and a console owner or admin, **may** set the free fields and lower the lower-only fields,
  never above the strictest of the deployment default, the platform operator's value and, for a partner's
  tenant, the partner's value. Platform-only fields and `quarantine.key_release` **must** stay out of its
  reach, identity keys **must not** hold `policy:write`, and a key **must not** loosen a guard field
  (allow-list only, quarantine on authentication failure, the spam threshold, unsolicited-code quarantine)
  where only people may release quarantined mail (FR-CON-6). Every policy write **must** be audited and
  evented, and concurrent writes **must not** lose one another ([Workspace policy](design/workspace-policy.md)).

### 6.2 Identities and addresses

- **FR-IDN-1** Create, read, list, update, pause, resume and delete identities. A create with a
  `client_id` the tenant has already used **must** return the existing identity, or `409` if the
  request differs.
- **FR-IDN-2** An identity **must** record an accountable human (name and email). Until it has one,
  it **must not** send, except that the identity of a provisional agent sign-up **may** send to the
  address of the human it named, within FR-ASU-7, until that human verifies (FR-ASU-10).
- **FR-IDN-3** A paused identity **must** still receive and store mail. It **must** refuse every send
  with `identity_paused`.
- **FR-IDN-4** Deleting an identity **must** run an identity-scope erasure and tombstone all of its
  addresses permanently.
- **FR-IDN-5** Not assigned. IDs are never reused, so the gap stays.
- **FR-IDN-6** Each identity **must** have Ed25519 signing keys that are generated, sealed and used only
  inside the Worker and are never exported or imported. An identity has one `active` key, created on its
  first signing request or on request, and **must** support rotation with an overlap
  (`PM_IDENTITY_KEY_OVERLAP_DAYS`, default 7 days) during which the previous key stays published, and
  revocation that removes a key from publication at once. Key IDs are RFC 7638 thumbprints
  ([Agent signing keys](design/agent-keys.md)).
- **FR-IDN-7** An identity **must** be able to mint short-lived agent assertions (JWT signed with
  `EdDSA`, 60–600 seconds) for an audience it names. An assertion carries the identity's address,
  display name and workspace name and whether it has an accountable human, and **must never** carry the
  owner's personal data. Each identity's public keys **must** be published as a JWKS at
  `/.well-known/jwks/{identity_id}.json`, so any service can verify an assertion; the Rust SDK and the
  CLI **must** include a verifier.
- **FR-IDN-8** An identity **should** be able to obtain Web Bot Auth signature headers (RFC 9421, signed
  with the deployment key, with its address in a signed `From` header) for HTTP requests its agent makes,
  with the deployment's key directory published at `/.well-known/http-message-signatures-directory`. It is
  off unless the operator sets `PM_WEB_BOT_AUTH=on` (allowed only once spike S13 has passed) and the
  tenant's policy has `web_bot_auth.allowed: true`.
- **FR-IDN-9** Pausing an identity, or suspending its tenant, **must** stop new signatures at once and
  withdraw the identity's JWKS (`404`). Deleting or erasing an identity **must** delete its keys and
  tombstone their key IDs, so a deleted key ID is never published again.
- **FR-ADR-1** An identity **must** support several addresses over time. Each address has a role
  (`primary` or `alias`) and a status (`pending`, `active`, `retiring` or `retired`).
- **FR-ADR-2** Promoting an address **must**:
  - make it the primary, so new threads send from it;
  - keep existing threads replying from the address the other party wrote to;
  - move the old primary to `retiring` for a configurable grace period (default 90 days), except the
    identity's platform address, which becomes an `active` alias. The platform address is the fallback
    address of FR-DOM-6, so it **must never** be retired or deleted (`409 address_in_use`).
- **FR-ADR-3** A retiring address **must** keep receiving mail into the same identity. After the grace
  period it becomes `retired`, and mail to it **must** be refused with `550 5.1.6`.
- **FR-ADR-4** Promoting a retiring address again **must** roll back the change.
- **FR-ADR-5** A deleted or erased address **must** never be assigned to a different identity. Mail to an
  erased address **must** get `550 5.1.1`, with no hint that the address existed.
- **FR-ADR-6** Reserved and confusable local parts **must** be refused:
  - on the shared platform domain, every RFC 2142 role name (including `info`, `marketing`, `sales` and
    `support`) where it would stand alone as the local part (usernames of the default tenant, whose
    address suffix is empty); mail to the operational names goes to the operator rather than an agent;
  - on a tenant's own domain, only `postmaster` and `abuse`, which route to the tenant's owner contact;
    the other role names are allowed there, because the tenant owns the domain;
  - everywhere, `noreply`, `mailer-daemon` and similar names, and names the service uses itself;
  - homoglyph look-alikes of the names reserved on that domain.
- **FR-ADR-7** Internationalised (SMTPUTF8) local parts **must** be refused with a clear error, because
  Cloudflare Email Routing cannot route them. Unicode display names are allowed.
- **FR-IDN-10** An agent **must** create a third-party account with its identity's address only through
  the **service sign-up ledger**: it records the service, the account identifier, the address and the
  purpose, and an operator approves or rejects each entry, in the console or with a key holding
  `accounts:approve`, which identity keys **must not** hold; approval by an API key follows the rule of
  FR-CON-6. Where the tenant's `accounts.require_approval` is on (the default on Pylota Mail Cloud, which no
  workspace or partner can turn off), a verification code or link **must** reach an agent only when an
  approved entry matches the mail (the authenticated sender's organisational domain and the receiving
  address); other such mail **must** be held for review, never dropped. Entries **must** emit
  `account.requested`, `account.approved`, `account.rejected` and `account.closed`, expire when undecided,
  and be deleted with the identity or tenant ([Service sign-up ledger](design/service-accounts.md)).

### 6.3 Domains

- **FR-DOM-1** The deployment **must** have one platform domain: a zone apex with catch-all routing to
  the Worker. All tenants can use it. Addresses on it take the form `{username}{tenant_suffix}@{platform}`.
- **FR-DOM-2** A tenant **may** add its own domains. The connection method chosen when a domain is added
  (FR-DOM-7) fixes its kind: `zone` (a zone, apex or subdomain, in the deployment's Cloudflare account),
  `delegated` (a child zone for one subdomain) or `external` (a domain whose DNS stays at any other host).
- **FR-DOM-3** DNS records shown to users **must** come from the provider API at request time. They are
  never copied from documentation or templates.
- **FR-DOM-4** Verification and health checks **must** run on a schedule:
  - every 15 minutes for each domain;
  - immediately after any change;
  - with two independent DNS-over-HTTPS resolvers, and two consecutive results before state changes.
- **FR-DOM-5** The domain health states `healthy`, `degraded`, `failing` and `suspended`, and the
  recovery transition (reported as `domain.recovered`), **must** behave as specified in
  [Identities, addresses and domains](design/identity-domains.md#domain-health).
  In particular, the service **must never** send as a domain whose authentication records are broken, and
  **must never** send as a domain whose ownership signals have changed.
- **FR-DOM-6** When a domain fails, sending **must** fall back to the identity's platform address,
  keeping the display name and thread continuity. Each fallback message is marked `sent_via_fallback`.
  The platform address therefore stays `active` for the identity's whole life (FR-ADR-2).
- **FR-DOM-7** Adding a domain **must** use one of six connection methods: `cloudflare_zone`,
  `nameservers`, `dns_records`, `send_only`, `smtp_relay` and `delegated_subdomain`. The method fixes the
  domain's `kind`, its inbound source (`inbound`) and its outbound transport (`transport`), as specified
  in [Domains on any DNS host](design/domain-connections.md#2-inbound-source-and-outbound-transport-are-separate-choices).
  Users **must not** combine them by hand. A request without `method` maps the old `kind` (`zone` →
  `cloudflare_zone`, `external` → `send_only`).
- **FR-DOM-8** `dns_records` **must** connect a domain, apex or subdomain, whose DNS stays at any host,
  with Amazon SES in both directions. The customer publishes one MX, three DKIM CNAMEs, a MAIL FROM MX and
  TXT, and an ownership TXT, each returned by the API with `name` and `host`. The domain **must** use a
  custom MAIL FROM (`pm-bounce.{domain}`), and health checks **must** cover every record and setting its
  method needs.
- **FR-DOM-9** Mail received through SES **must** be ingested exactly once per S3 object and recipient:
  an SNS HTTPS push, an SQS backstop drained every minute, and the `ses_ingest` ledger. The SNS endpoints
  **must** accept only signature version 2. On an SES domain, mail to an unknown address **must** be dropped without
  a bounce, and mail to a retired address **must** be bounced with `550 5.1.6`.
- **FR-DOM-10** `send_only` **must** let agents send as addresses on a domain whose mailbox stays with the
  customer's provider: SES sends, and inbound mail arrives through a forwarding rule in the customer's
  mailbox to the identity's platform address. Each address on such a domain **must** report its
  `forwarding` state (`unverified`, `ok` or `failed`), and a forwarding test **must** be available.
- **FR-DOM-11** `smtp_relay` **must** send through the customer's own provider over SMTP submission on
  port 465 or 587 only, and **must** require TLS before any credentials are sent. The credentials are
  sealed and never returned. An alignment probe **must** pass before the first send and again every day; a
  failing probe moves the domain to `failing`, and sends fall back to the platform address (FR-DOM-6).
- **FR-DOM-12** `nameservers` **must** create a Cloudflare zone for a domain used only for mail. It
  **must** refuse a name that has A, AAAA or MX records, or a CNAME, A or AAAA record at `www`, with
  `409 domain_not_dedicated` unless the request carries `"confirm_dedicated": true`. A tenant key **may**
  use it only when its policy has `domains.allow_create_zone: true`. `delegated_subdomain` **must** stay
  off unless `PM_CF_SUBDOMAIN_SETUP=on`, the Cloudflare account is Enterprise, and spike S10 has passed.
- **FR-DOM-13** Onboarding **must never** take over, change or delete a provider object (a Cloudflare zone,
  sending domain, routing setup or catch-all, or an SES identity) that the deployment did not create for
  that tenant's domain: a tenant's add that finds one is refused with `409 domain_exists`. The deployment
  **must** record the provider IDs it creates for each domain, and removal and cleanup **must** act on
  those IDs only. Removing one mail domain **must not** stop mail to the other mail domains of its zone.
- **FR-DOM-14** A domain that has never been verified **must** be removed 14 days after it was added, and
  **must** be evictable by another tenant that proves control of its DNS with a claim record; a tenant
  **may** hold at most 5 such domains. The answer to a name held by someone else **must** be the same
  whoever holds it and in whatever state, so it reveals nothing about other tenants.
- **FR-DOM-15** `GET /v1/domains/{domain_id}/records?format=bind` **must** return the domain's records
  as an RFC 1035 zone fragment built from the same provider-sourced records as the JSON form
  ([Parity additions § 9](design/parity-additions.md#9-zone-file-export-fr-dom-15)).

### 6.4 Inbound

- **FR-IN-1** Raw mail **must** be written to object storage before the message is acknowledged. If the
  write fails, the sender **must** get a temporary failure.
- **FR-IN-2** Unknown addresses get `550 5.1.1`. Retired addresses get `550 5.1.6`. Suspended tenants
  get a temporary failure (4xx) for up to five days, then `550 5.2.1` (FR-TEN-3). Domains that receive
  through SES cannot answer during the SMTP session; FR-DOM-9 sets their behaviour.
- **FR-IN-3** Every message **must** be parsed with size and depth caps. Mail that cannot be parsed is
  kept raw and flagged `parse_degraded`, never dropped.
- **FR-IN-4** Every message **must** carry:
  - an authentication verdict (SPF, DKIM, DMARC, ARC), with the domain each method checked and whether it
    aligned (FR-IN-14);
  - a spam signal;
  - `known_sender`;
  - a quarantine decision.
- **FR-IN-5** Messages that fail authentication, carry a risky attachment or exceed the spam threshold
  **must** be quarantined. Quarantined mail is hidden from agents unless the key holds `quarantine:review`.
- **FR-IN-6** Automated mail **must** be classified and marked `automated`, so agents never auto-reply to
  it. This covers:
  - auto-replies (RFC 3834) and out-of-office replies;
  - mailing lists;
  - bounces (DSNs, which update delivery state instead of reaching agents);
  - read receipts.
- **FR-IN-7** Every message **must** expose `text`, sanitised `html`, and `extracted_text` (the new
  content with quoted history and signatures removed). HTML-only mail **must** produce text.
- **FR-IN-8** Attachments **must** be stored and fetchable. Text **must** be extracted for PDF, Office
  and text formats, and for images when the tenant enables it.
- **FR-IN-9** Hidden text (zero-width characters, CSS-hidden content, white-on-white) **must** be removed
  from agent-facing text and raised as a risk flag.
- **FR-IN-10** Messages and threads **must** be listable and readable through the API, MCP and console.
  Thread lists and their counts show only visible mail (never quarantined, hidden or throttled
  messages). Labels, read state and archiving **must** be settable per message and per thread, and
  `is:unread` **must** follow the read state.
- **FR-IN-11** An agent **must** be able to wait for new mail (`wait`, a long poll with filters), and
  verification codes and links in received mail **must** be extracted into a structured field.
- **FR-IN-12** Unsolicited inbound mail **must** be capped per identity and per tenant by source, so that
  mail nobody asked for cannot exhaust storage, triage or model spend.
- **FR-IN-13** The service **must** read and update up to 100 messages of one identity in one request
  (`batch-get`, `batch-update`). An update **must** be atomic inside the identity's mailbox, **must** offer a
  per-message label precondition (an atomic claim), and **must** report IDs the key cannot see together with
  missing ones, without saying which is which ([Parity additions § 2](design/parity-additions.md#2-batch-message-operations-fr-in-13)).
- **FR-IN-14** The trust metadata of every inbound message **must** name the domain SPF checked and
  whether it aligned, each DKIM signature's domain, selector, result and alignment, the domain and policy DMARC
  evaluated and which method aligned, and the ARC sealer when an ARC chain validated
  ([Parity additions § 5](design/parity-additions.md#5-authentication-detail-on-messages-fr-in-14)).
- **FR-IN-15** A message that fails DMARC **must** get the verdict `arc_rescued`, and **must not** be
  quarantined for authentication, only when its ARC chain validates, its newest sealer is on the platform-only
  list `inbound.trusted_arc_sealers`, that sealer signed the `From` header and recorded `dmarc=pass` for the
  `From` domain, the seal is recent, and the sealer saw the message addressed to the receiving identity. Mail
  with that verdict **must never** release a verification code
  ([Parity additions § 6](design/parity-additions.md#6-trusted-arc-sealers-fr-in-15)).

### 6.5 Outbound

- **FR-OUT-1** Send, reply, reply-all and forward **must** require an `Idempotency-Key`:
  - the same key with the same request returns the original result with `deduplicated: true`;
  - the same key with a different request returns `409 idempotency_conflict`.

  Keys are kept for 30 days.
- **FR-OUT-2** A transport outcome that cannot be known (a timeout, or a connection lost after
  submission) **must** set the status to `uncertain`. The service **must never** resend it automatically.
- **FR-OUT-3** Policy **must** run before transport:
  - the identity and tenant are active, and the identity has an accountable human;
  - the approval requirement of tenant policy (FR-DRF-4);
  - per-identity and per-tenant caps;
  - suppressions and block lists;
  - the recipient-count limit;
  - the size limit;
  - no automatic reply to automated mail.
- **FR-OUT-4** Suppressed recipients **must** be removed per recipient, with the rest delivered and each
  outcome reported. A send where every recipient is suppressed ends `suppressed`.
- **FR-OUT-5** Replies **must** set `In-Reply-To` and `References`. They **must** send from the address
  the counterparty last wrote to, unless that address is retired.
- **FR-OUT-6** Every outbound message **must** carry a thread token in its `Reply-To` sub-address, where
  the domain supports it.
- **FR-OUT-7** Agent-initiated automatic replies **must** carry `Auto-Submitted: auto-replied`.
- **FR-OUT-8** Marketing mail (`kind: marketing`) **must** include RFC 8058 one-click unsubscribe headers
  and a visible link, and **must** be refused without them.
- **FR-OUT-9** Two concurrent sends into the same thread **must** be serialised. The second gets
  `thread_busy` if the first holds the lock for longer than 10 seconds.
- **FR-OUT-10** Attachments **must** be refused once the message exceeds the transport limit (5 MiB with
  Cloudflare). A tenant **may** enable expiring signed links instead. Attachments may also come from
  uploads (FR-UPL-4); the transport limit and `large_attachments` apply to them alike, and a message's
  attachments total at most 26,214,400 decoded bytes (FR-UPL-5).
- **FR-OUT-11** A message **may** be cancelled while it is `queued`.
- **FR-OUT-12** Test tenants **must** use the simulator transport:
  - mail to `*@simulator.invalid` produces a scripted outcome (`delivered`, `bounce`, `softbounce`,
    `complaint`, `deferred`, `reject`, `timeout`);
  - mail to identities of the same tenant, or of a tenant with the same partner, is delivered internally;
  - all other recipients are refused with `test_mode_recipient`.
- **FR-OUT-13** By default an identity **must** send only to a recipient that:
  1. it has written to before;
  2. is allowed by an effective send-allow entry for that identity (FR-OUT-17);
  3. is the authenticated sender of mail it is answering;
  4. for a send from a draft whose current revision carries a valid approval, is one of the recipients
     that approval covers (FR-DRF-6);
  5. for a calendar reply, is the organiser of the verified invitation it answers (FR-CAL-10).

  Other recipients are suppressed unless the identity's `require_known_recipient` is turned off.
- **FR-OUT-14** Any send whose computed hop count reaches the loop limit **must** be refused with
  `loop_detected`, whatever its `kind`.
- **FR-OUT-15** Marketing mail **must** leave through the SES or SMTP transport, never through Cloudflare
  Email Service (transactional only). A marketing send that would use Cloudflare is refused with
  `marketing_needs_ses`, at submit and again before transport.
- **FR-OUT-16** Tenants **must** be able to manage allow and block lists for sending and receiving, by
  address or by domain, for the whole tenant or for one identity (FR-OUT-17).
- **FR-OUT-17** Allow and block lists **must** also be settable per identity. For each direction and
  address, an identity entry that matches **must** override the tenant's entries of the same or a coarser
  granularity, but an identity entry for a domain **must not** override a tenant entry for the exact
  address; among the entries left, block **must** win over allow. No list entry **may**
  override a suppression, a test-mode rule, a daily cap, a sending pause or an authentication quarantine
  ([Parity additions § 3](design/parity-additions.md#3-identity-scope-allow-and-block-lists-fr-out-17)).

### 6.6 Delivery

- **FR-DLV-1** Provider events (`delivered`, `deferred`, `bounced`, `failed`, `rejected`, `complained`)
  **must** update each recipient's status and emit webhook events.
- **FR-DLV-2** A hard bounce **must** create a suppression. A complaint **must** create a permanent
  suppression and count towards the identity's complaint rate.
- **FR-DLV-3** An identity whose complaint rate exceeds 0.3% over its last 1,000 sends, or whose bounce
  rate exceeds 5% over its last 200, **must** be paused automatically with reason `abuse_threshold`. This
  applies to every identity except the deployment's system identity, whose outcomes are recorded but
  never pause it (its bounces and complaints pause the affected person's notifications instead). The
  identity's current rates, thresholds and headroom are readable before the pause applies (FR-MET-1).
- **FR-DLV-4** Uncertain sends **should** be reconciled from provider events by matching sender,
  recipient and subject within 30 minutes. A match moves the send to its real status with `reconciled: true`.
- **FR-DLV-5** DSNs that do not match a message we sent (backscatter) **must** be dropped and counted.
- **FR-DLV-6** A tenant or a tenant sending domain whose complaint or bounce rate reaches the provider's
  review rate **must** be paused automatically, and a paused tenant or domain **must never** fall back to
  the platform domain. The tenant's or domain's current rate, threshold and headroom are readable before
  the pause applies (FR-MET-1).
- **FR-DLV-7** A person **must** be able to resolve an `uncertain` send as `sent` or `not_sent`.

### 6.7 Threading

- **FR-THR-1** An inbound message joins a thread by, in order:
  1. a valid thread token in the recipient sub-address;
  2. `In-Reply-To` or `References` matching a stored message;
  3. the `UID` and `ORGANIZER` of a verified calendar object matching a calendar message of the mailbox;
  4. otherwise it starts a new thread.

  The subject alone **must never** join a thread.
- **FR-THR-2** A forwarded message (`message/rfc822` part) **must** be parsed as a nested message and
  **must not** be merged into the outer thread.

### 6.8 Search

- **FR-SRCH-1** Four modes **must** ship together, returning one result shape: `keyword`, `semantic`,
  `hybrid` (the default) and `agentic`.
- **FR-SRCH-2** Keyword search **must** be consistent with the mailbox: a message is searchable in the
  same transaction that stores it.
- **FR-SRCH-3** The query language **must** support:
  - `from:`, `to:`, `participant:`, `subject:`, `ref:`, `label:`;
  - `has:attachment`, `filename:`, `type:`;
  - `after:`, `before:`, `newer_than:`, `older_than:`;
  - `in:inbound|outbound`, `thread:`, `is:unread|needs_reply|quarantined`, `category:`;
  - quoted phrases, `OR`, and `-` negation.

  Queries **must** be parsed into a typed tree, and raw input **must never** reach the FTS engine.
- **FR-SRCH-4** Exact references (vehicle plates, invoice, order and PCN numbers, amounts, phone numbers,
  plus tenant-defined patterns such as booking numbers) **must** be extracted at ingest and matched
  exactly.
- **FR-SRCH-5** Results **must** include facets (`sender`, `sender_domain`, `month`, `label`,
  `attachment_type`, `category`), a `why` list per hit, and trust metadata. They **must** respect size budgets (`limit`,
  `snippet_chars`, a byte cap that sets `truncated`).
- **FR-SRCH-6** Cursors **must** pin an `as_of` point, so pagination is stable while mail arrives.
- **FR-SRCH-7** Semantic results **must** report `semantic_coverage`, the share of the mailbox embedded.
- **FR-SRCH-8** Agentic search **must**:
  - run a bounded loop of planning, searching, judging and refining;
  - stream progress over SSE on request;
  - return the evidence, an answer, a confidence value and a step trace;
  - remove any answer sentence whose citations fail a deterministic check (each cited ID is in the
    evidence set, and each quoted phrase appears in the source).
- **FR-SRCH-9** Agentic search **must** return `insufficient_evidence` when the evidence does not answer
  the question, `budget_exhausted` when it runs out of budget, and degrade to hybrid search (with
  `degraded: true`) when the model is unavailable. It **must never** return a fabricated answer.
- **FR-SRCH-10** Tenant scope (all of a tenant's identities) **must** require a tenant-level key.
- **FR-SRCH-11** Erasure **must** remove keyword rows, references and vectors together. A probe query
  after erasure **must** return nothing.
- **FR-SRCH-12** Search **should** offer a contacts lookup and a find-related lookup for a message.
- **FR-SRCH-13** An agentic answer **must** be marked as untrusted, model-written text. Each citation
  **must** carry the trust of its source, and a sentence that cites only unauthenticated or
  steering-suspected mail **must** be removed.

### 6.9 Triage

- **FR-TRI-1** Every inbound, non-quarantined message **must** be triaged asynchronously. Triage sets:
  - `category`: from the built-in list, or the tenant's custom list;
  - `needs_reply`: 0 to 1;
  - `urgency`: 0 to 3;
  - `summary`: at most 280 characters;
  - `language`: BCP 47;
  - `risk_flags`.
- **FR-TRI-2** Deterministic rules (tenant-defined and built-in) **must** run before the model. They can
  set fields and skip the model.
- **FR-TRI-3** Triage is advisory. It **must never** send, delete or release a message.
- **FR-TRI-4** The triage model **must** receive mail content as fenced, untrusted data. Its output
  **must** be validated against a schema, and invalid output is recorded as `failed`, never guessed.
- **FR-TRI-5** Triage **may** be re-run on a stored, visible message. A re-run on a quarantined,
  screening, hidden, throttled or delivery-report message **must** be refused with `triage_not_eligible`.

### 6.10 Events and webhooks

- **FR-WH-1** Webhook endpoints **must** be configurable at platform, partner and tenant level, with event-type
  and identity filters.
- **FR-WH-2** Payloads **must** be signed with Standard Webhooks, using a separate secret per endpoint.
  Rotation **must** support an overlap window, during which both signatures are sent.
- **FR-WH-3** Delivery **must** retry with backoff for up to 72 hours, then move to a dead-letter state,
  from which events can be replayed.
- **FR-WH-4** Payloads **must** be thin: IDs, a header summary, verdicts, triage, and at most 64 KB of
  extracted text. Full content is fetched through the API.
- **FR-WH-5** Endpoint URLs **must** be HTTPS. Private, loopback and reserved addresses are refused,
  redirects are not followed, and responses are capped in size and time.
- **FR-WH-6** A tenant **may** opt in to `message.labels_changed`. The event **must** be delivered only
  to endpoints that name it (never through `*`), **must** cover a whole request in one event per 100 messages,
  **must never** be emitted for state the service sets itself, and **must never** name a message the endpoint
  could not receive ([Parity additions § 4](design/parity-additions.md#4-label-change-events-fr-wh-6)).
- **FR-WH-7** A webhook endpoint **may** carry up to 10 static request headers. Their values **must** be
  write-only and sealed at rest; names that would change how the request is framed, routed or attributed
  **must** be refused; and a change of the endpoint's host **must** remove them
  ([Parity additions § 8](design/parity-additions.md#8-webhook-delivery-headers-fr-wh-7)).

### 6.11 Interfaces

- **FR-API-1** The REST API is versioned under `/v1` and described by OpenAPI 3.1, generated from
  Rust types and served at `/openapi.json`.
- **FR-API-2** Every error **must** use the [error envelope](../reference/errors.md), with `retryable` set
  and a `docs` link (FR-API-3).
- **FR-MCP-1** An MCP server **must** be served at `/mcp` over Streamable HTTP, exposing the tools in
  [MCP reference](../reference/mcp.md). It is authenticated with an API key in v1.0. Tool scope **must**
  follow the key.
- **FR-CLI-1** The `pmail` CLI **must** cover setup, deploy, doctor, administration and mail operations,
  with `--json` output for every command.
- **FR-SDK-1** The Rust SDK **must** cover the whole REST API.
- **FR-SDK-2** The project **must** ship a TypeScript SDK, the npm package `@pylotahq/mail`, built in its
  own repository `PILOTAAI/pylota-mail-typescript` ([ADR 0018](adr/0018-typescript-sdk-in-its-own-repository.md)), with exactly one method per
  `openapi.yaml` operation not marked `x-sdk: false`, generated from the `openapi.json` a release
  publishes, with no runtime dependencies. It **must** run on every Node.js line from its first release to
  its end of life, starting with Node.js 22, on Cloudflare Workers, on Deno and on Bun, and its
  declarations **must** type-check under `strict`, `exactOptionalPropertyTypes` and
  `noUncheckedIndexedAccess`
  ([SDKs, skills and agent tooling §2](design/sdk-and-skills.md#2-the-typescript-sdk)).
- **FR-SDK-3** The SDKs **must never** generate an idempotency key for an operation marked
  `x-idempotency: required` (today send, reply, reply-all, forward, calendar reply and draft send), and
  **must** send the same key and the same body bytes on every retry of one call. The TypeScript SDK
  **must** retry only failures that the error envelope marks `retryable`, and network failures and proxy
  `502`, `503` and `504` only for a `GET` or a request that carries an idempotency key; **must** honour
  `Retry-After` up to a bound and throw beyond it; and **must** end a call whose outcome it could not learn
  with an error that carries the key.
- **FR-SDK-4** The SDK **must** give an `uncertain` message a typed outcome that no SDK code path resends,
  and **must** treat a status it does not know as neither terminal nor safe to resend.
- **FR-SDK-5** The SDK **must** verify Standard Webhooks signatures over the raw request bytes only, in
  constant time, against every configured secret (rotation overlap), with a timestamp tolerance of 300
  seconds by default and at most 600, and a distinct reason for each failure. Its webhook handler
  **must never** answer `410`, or a `4xx` to a validly signed delivery.
- **FR-SDK-6** The SDK's list, `wait` and event helpers **must never** lose an item silently: pagination
  ends with the error on `410 cursor_expired`, bounded collection reports `truncated`, `wait` keeps one
  `since` across retries, the stream resumes from its last cursor and stops on an authentication failure,
  and `410 cursor_expired` from the feed or stream raises a gap error instead of continuing from another
  position.
- **FR-SDK-7** The SDKs **must** tolerate unknown fields, enum values, event types and error codes from a
  newer deployment. The TypeScript SDK **must** report an operation that a deployment lacks as unsupported
  by that deployment, never as a missing resource.
- **FR-SDK-8** The SDK **must never** expose the API key in errors, logs or serialised objects, **must**
  refuse to run in a browser or against a base URL that is not `https` (localhost excepted), and **must**
  follow no redirect.
- **FR-SDK-9** The TypeScript SDK's major version **must** be the API's path version, and each SDK
  release **must** name the Pylota Mail release whose contract it was generated from. It **must** be
  published only by the SDK repository's release workflow through npm trusted publishing with
  provenance, from a tarball listed in that repository's owner-signed `SHA256SUMS`, and a pre-release
  **must not** be published.
- **FR-SDK-10** Each release **must** include agent skills in the Agent Skills format, with plugins for
  Claude Code, Codex and Cursor, that teach safe sending, mail as untrusted data, verification codes
  through the service sign-up ledger and `wait`, drafts for approval, search with citations and the event
  feed. They **must** carry the release version and **must not** contain a secret or an executable
  component. CI **must** fail when a skill names a tool, route, field, error code, command or limit that
  the contract of the same commit lacks, or lacks a rule the rule registry assigns to it.
- **FR-SDK-11** Every MCP tool **must** set all four annotations from a declared effect class, **must**
  carry `_meta["anthropic/maxResultSizeChars"]` when its result can exceed 50,000 characters, and its
  description, like the server instructions, **must** stay within 4,096 characters.
- **FR-SDK-12** An API key **must** be able to carry `mcp_tools`, a list of MCP tools that narrows what
  its permissions and level allow. A tool outside the list **must** be hidden and answer as an unknown
  tool. The list **must** be validated when the key is minted, kept by rotation, returned on the key
  object and on `GET /v1/me`, and recorded in the audit log.
- **FR-SDK-13** `pmail mcp stdio` **must** bridge a stdio MCP client to the deployment's `/mcp` with the
  key from the environment or a profile, never from a flag; it **must** only narrow the tool list and
  **must never** retry `tools/call`. `pmail mcp config` **must** print setup for Claude Code, Claude
  Desktop, Cursor, Codex and the Claude Agent SDK without the key.
- **FR-SDK-14** Every CLI command **must** describe itself with `--schema` (input, output and error
  schemas and exit codes) without reading configuration, needing a key or using the network, and **must**
  accept a JMESPath `--query` that applies only to success documents and never changes the exit code.
  `pmail skills generate` **must** write the skills of its own version and **must not** overwrite a
  locally modified file unless forced.
- **FR-SDK-15** The docs **must** include guides for the Vercel AI SDK, the OpenAI Agents SDK, the Claude
  Agent SDK and LangChain, a TypeScript SDK guide, a Python guide and an agent skills guide, whose code
  examples type-check in CI against pinned versions.
- **FR-API-3** Every error envelope **must** carry `docs`, an absolute link to the code's anchor on the
  deployment's published errors page, and every error code **must** have that anchor
  ([Parity additions § 10](design/parity-additions.md#10-a-documentation-link-in-errors-fr-api-3)).

### 6.12 Privacy

- **FR-PRV-1** The jurisdiction (`eu` or `default`) **must** be chosen at setup and applied to D1, R2 and
  every Durable Object.
- **FR-PRV-2** Retention **must** be configurable per tenant:
  - raw MIME: 90 days by default;
  - parsed messages: kept by default;
  - events and delivery logs: 30 days by default (`retention.events_days`, 1–365);
  - messages of one identity: deleted a set number of hours after they are stored, when the identity sets
    `retention.message_hours` (FR-PRV-7).

  Every purge **must** write an audit event.
- **FR-PRV-3** Erasure **must** be supported per message, thread, counterparty, identity and tenant. It
  **must** return a receipt counting what was deleted in each store. Every scope **must** also delete the
  drafts it covers and count them (`drafts_deleted`, FR-DRF-10).
- **FR-PRV-4** A legal hold on a thread **must** stop retention and erasure from deleting it. Erasure
  receipts **must** list each held item.
- **FR-PRV-5** Subject-access export **must** produce every message to or from a counterparty across a
  tenant's identities, as `.eml` plus JSON.
- **FR-PRV-6** Logs **must never** contain message bodies, attachment content, or clear-text email addresses.
- **FR-PRV-7** An identity **may** set `retention.message_hours` (1–8,760). Each of its messages, with its
  attachments, extracted text, index rows, vectors and drafts, **must** then be erased within 15 minutes of
  reaching that age, except under a legal hold; a retried send **must never** be sent again after its message
  expired; and each day's expiries **must** be recorded in the audit log with counts per store
  ([Parity additions § 7](design/parity-additions.md#7-per-identity-message-retention-fr-prv-7)).

### 6.13 Operations

- **FR-OPS-1** `pmail setup` **must** create every Cloudflare resource idempotently, and **must** be
  safe to re-run.
- **FR-OPS-2** `pmail deploy` **must** deploy a prebuilt, checksum-verified Worker bundle by default, so
  self-hosters need no Rust toolchain. It **must** also support `--from-source`.
- **FR-OPS-3** `pmail doctor` **must** check DNS, routing, sending, event subscriptions, bindings, secrets
  and quota, and print a fix for each failure.
- **FR-OPS-4** Every queue **must** have a dead-letter queue with a consumer that records and alerts.
- **FR-OPS-5** Pylota Mail Cloud production **must** be commissioned from the written
  [Cloud commissioning](cloud-commissioning.md) page: its full configuration, scoped and short-lived
  credentials, the partner bootstrap, the checks, and three gated phases (partner-only, waitlist, open),
  so that Pylota's integration runs on Cloud before public sign-up opens.

### 6.14 Console and workspaces

- **FR-CON-1** The console **must** be served by the same Worker at `/console`, rendered on the server in
  Rust, and **must** work fully without JavaScript. Every state change is a POST with a CSRF token.
- **FR-CON-2** A workspace is a tenant. Each workspace **must** have exactly one owner and any number of
  members up to its seat limit. Roles are `owner`, `admin`, `member` and `viewer`, with the permissions in
  [Console design](design/console.md#roles). Ownership can be transferred to an admin.
- **FR-CON-3** Sign-in **must** be passwordless: a single-use magic link or a six-digit code sent by email,
  valid for 10 minutes. It **must** be rate-limited: 3 link or code requests per 10 minutes per address;
  10 attempts per code (the token is burned after 10 failures); 30 failed codes per address per UTC day,
  after which only the link works until the next UTC day; and `RL_SIGNIN`, 10 requests per 60 s per
  client network (an IPv6 /64 counts as one) on the sign-in, sign-up, waitlist and pending-step routes
  and the OAuth start. Passkeys (WebAuthn) are P2: they need browser JavaScript, which the console does
  not use (FR-CON-1).
- **FR-CON-4** Members are invited by email. A pending invitation **must** count against the seat limit and
  expire after 7 days. Removing a member **must** end their sessions immediately.
- **FR-CON-5** Sensitive actions **must** require a sign-in within the last 10 minutes and be written to the
  audit log: creating keys, inviting or removing members, changing roles, adding or removing domains,
  releasing quarantine, approving a draft, erasure, and billing changes.
- **FR-CON-6** The console **must** show inboxes, threads and messages (with untrusted content rendered as
  sanitised, inert HTML in a sandboxed frame or as text), search, quarantine with release, keys, domains,
  webhooks, members, plan and usage, and settings. On Cloud, releasing quarantine **must** be possible
  only for a signed-in person (`PM_QUARANTINE_KEY_RELEASE=off`), except in a workspace whose policy has
  `quarantine.key_release: true`, which only a platform key or the workspace's partner key can set; there,
  those two keys can release too, never the workspace's own tenant or identity keys. Self-hosters can allow release by API keys with
  `quarantine:review` everywhere (`on`, the default for self-hosting).
- **FR-CON-7** A self-hosted deployment **must** create its first owner during `pmail setup`
  (`--owner-email`). The console **must** be optional (`PM_CONSOLE=off` removes its routes).
- **FR-CON-8** Self-serve sign-up **must** follow `PM_SIGNUP`: `closed` (the default; no public sign-up),
  `waitlist` or `open`. A waitlist entry **must** be confirmed by email (double opt-in) and creates no
  account. The operator invites confirmed entries in batches (`POST /v1/platform/waitlist/invite`), each
  with a sign-up link valid for 7 days for the waitlisted address only. With email sign-up, the account
  **must** be created only when the link or code is used.
- **FR-CON-9** Sign-in with Google and GitHub **must** be available when their client credentials are
  configured (on Pylota Mail Cloud). It **must** accept only a verified email address, use PKCE and a
  `state` bound to the browser by a cookie, and link to an existing person with the same verified email
  only as FR-CON-17 allows.
- **FR-CON-10** A person **must** be able to enrol two-step verification with an authenticator app (TOTP,
  RFC 6238) and ten single-use recovery codes. Once enrolled, it is asked for after every first factor and
  at re-authentication. A workspace owner **may** set `require_two_factor`; a member without two-step
  verification **must** enrol before entering that workspace.
- **FR-CON-11** After sign-in, the console **must** route each person by the rules in
  [Cloud sign-up › Where people land](design/cloud-signup.md#7-where-people-land). A `next` value **must**
  be followed only when it is a relative path under `/console/` with no `//`, backslash or scheme.
- **FR-CON-12** The workspace home (`/console`, the Overview) **must** show banners for conditions that
  need action, the first-run checklist (each step's state derived from real data on every render, never
  stored), "Needs a person" (the actions only a person should take, drafts awaiting approval and scheduled
  sends that failed among them) and usage against each allowance.
- **FR-CON-13** Returning from Stripe Checkout **must** never change the plan: Stripe webhooks are the only
  source of plan state (FR-BILL-10), and another workspace's Checkout session changes nothing. Pylota Mail
  Cloud **must** apply abuse controls: `RL_SIGNIN`, a new-workspace send ramp (a tenant daily cap of at
  most 50 for the first 7 days on Free, lifted by a daily evaluation of bounce and complaint rates or by a
  paid plan once its invoice is paid), refusal of disposable addresses (`PM_SIGNUP_BLOCKED_DOMAINS`), and
  system mail sent from `PM_SYSTEM_FROM`.
- **FR-CON-14** A person **may** opt in, per workspace, to email notifications of new mail in the inboxes
  they choose (`instant`, `hourly` or `daily`). Notifications **must** be coalesced (one email per person
  and inbox per window), **must** count only mail that becomes visible in the inbox, and **must never**
  include content from the mail: no subject, sender, snippet or attachment name
  ([Notifications](design/notifications.md)).
- **FR-CON-15** Owners and admins **must** receive a daily "needs a person" email by default
  (quarantined mail, uncertain sends, drafts awaiting approval, failed scheduled sends, failing domains
  and failing webhooks), and the person concerned **must** receive `account` emails for security and
  billing events, which cannot be turned off. Every
  other notification **must** carry RFC 8058 one-click unsubscribe. Notification email is capped at 50 per
  person and 200 per workspace a day, beyond which items go into one daily digest email (itself
  unsubscribable and not capped).
- **FR-CON-16** A first factor **must never** create a session while a second step is outstanding: the
  pending step expires within 10 minutes and can be finished once. Accepting an invitation **must** be an
  explicit action by the invited person, never automatic at sign-in, and a person enrolled in two-step
  verification **must** pass it before the membership and the session exist. A workspace's
  `require_two_factor` **must** be checked on every console request
  ([Cloud sign-up §5.1](design/cloud-signup.md#51-the-pending-step)).
- **FR-CON-17** A Google or GitHub identity **must** be linked to an existing person only after a code
  emailed to that person's address is entered, and a person **must** be able to unlink one. A console
  sign-in address (an account, an invitation or a workspace owner) **must not** be a mailbox that the
  deployment itself receives, and an identity address **must not** equal a console sign-in address.
- **FR-CON-18** Pylota Mail Cloud **must** protect its shared system mail and shared domain: budgets for
  system mail per recipient, per client network and ASN, and per inviting tenant, with a share of each
  day kept for sign-in mail; one self-serve Free workspace per person; and, when the account's daily
  sending quota is configured, a breaker that stops Free and ramped workspaces at 60% of it and every
  tenant at 90% until 00:00 UTC ([Cloud sign-up §10](design/cloud-signup.md#10-abuse-and-safety-on-cloud)).
  The `signup` class of system mail ([Agent self-sign-up](design/agent-signup.md)) may use at most 5% of
  the system identity's day, so at least 25% stays for sign-in and account mail.

### 6.15 Plans, metering and billing

- **FR-BILL-1** Each workspace **must** have a billing mode: `metered` (a plan applies), `exempt` (no checks,
  for the operator's own tenants, or a partner's tenants that the operator does not bill per workspace) or
  `disabled` (self-hosting without billing: only the daily caps in tenant policy apply). A tenant created
  with a partner key takes its partner's `default_billing_mode`; only a platform key changes it (FR-KEY-4).
- **FR-BILL-2** The plan catalog **must** be data (`PM_PLAN_CATALOG`), defaulting to the Pylota Mail Cloud
  plans in section 13. A plan defines allowances for `inboxes`, `sends`, `triage`, `custom_domains`,
  `storage_gb` and `seats`, a price, and whether top-ups are allowed.
- **FR-BILL-3** Monthly allowances (`sends`, `triage`) **must** reset at the start of each billing period.
  Counts (`inboxes`, `custom_domains`, `seats`, `storage_gb`) **must not** reset.
- **FR-BILL-4** Every metered action **must** take an atomic hold in the workspace's `TenantQuota` object
  before it runs, and settle it (consume or release) when the outcome is known. Holds that are never
  settled **must** expire after 10 minutes. Two requests **must never** both pass on the last unit.
- **FR-BILL-5** One send is one recipient: a message to three recipients uses three sends. A send is
  consumed when the transport accepts it; a rejected, failed or cancelled send releases its hold; an
  `uncertain` send releases its hold and is consumed later only if reconciliation shows it was sent.
- **FR-BILL-6** A denied metered action **must** return `402 billing_limit` before any idempotency record is
  written, so the same `Idempotency-Key` succeeds after the workspace upgrades or adds a top-up. A replay of
  an already-completed send **must** return its original result even when the allowance is spent.
- **FR-BILL-7** Triage **must** hold one unit when a message arrives and consume it when the analysis is
  stored; a failed analysis refunds it. Quarantined mail is charged only when someone releases it.
- **FR-BILL-8** Inbound mail **must never** be refused or dropped because a plan limit is reached. When
  `storage_gb` is exceeded, new identities, new domains and outbound attachments are refused with
  `402 billing_limit` until usage falls or the plan changes; mail keeps arriving.
- **FR-BILL-9** A downgrade **must never** delete data. Existing identities, domains and members are kept;
  creating more is refused until the counts fit the new plan.
- **FR-BILL-10** Payment **must** go through Stripe: Checkout for plans and top-ups, the Customer Portal for
  payment methods, invoices and cancellation, and signed webhooks (verified, deduplicated by event ID) as
  the only source of subscription state. A failed payment keeps the plan for a 7-day grace period, then
  moves the workspace to Free limits.
- **FR-BILL-11** `GET /v1/usage` **must** return the billing mode, plan, each feature's granted, used,
  remaining and reset time, and the plan catalog, so agents can read their own limits.
- **FR-BILL-12** A self-hosted deployment **must** run with billing off by default and **must not** need a
  Stripe account. Turning billing on (`PM_BILLING=stripe`) is an operator choice.
- **FR-BILL-13** Owners and admins **must** be emailed when an allowance reaches 80% and 100% of its
  limit (top-ups included), unless they turn it off. Each threshold alerts at most once per billing period
  for allowances that reset, and at most once a day per feature and threshold for counts that do not.
  With billing off, no usage alert is sent. Webhook events (`quota.warning`, `billing.limit_reached`) are
  unchanged.
- **FR-BILL-14** An allowance **must** rise only after the payment for it succeeded: a plan or top-up
  increase applies when its invoice is paid, never on a subscription's status alone, and decreases apply
  at once. A disputed payment **must** stop the workspace's sends and apply the default plan until the
  dispute closes; a lost dispute cancels the workspace's subscriptions. A workspace **must** have at most
  one Stripe customer and one subscription per plan or top-up feature; a duplicate is cancelled with its
  unused time credited ([Billing › Disputes and refunds](design/billing.md#disputes-and-refunds)).

### 6.16 Drafts, approvals and scheduled send

- **FR-DRF-1** The service **must** store drafts in the identity's mailbox, with create, list, get,
  update and delete through the API (`drafts:read`, `drafts:write`). A create with a `client_id` the
  identity has already used **must** return the existing draft, or `409 client_id_conflict` if the
  request differs. A draft's kind (`new`, `reply`, `reply_all`, `forward`) **must** be fixed at creation,
  and a field its kind does not take **must** be refused. Reply and forward drafts **must** be created only
  by a key holding `messages:read`, and **must** name a source message the creating key can see, never
  `hidden` or `throttled` mail, and `quarantined` or `screening` mail only with `quarantine:review`
  ([Drafts](design/drafts.md)).
- **FR-DRF-2** Every draft **must** carry a `revision` and an `etag`. An update with a stale `If-Match`,
  and an approval of a revision or content digest that is no longer current, **must** fail with
  `409 draft_revision_mismatch` and change nothing. A draft being sent **must** refuse updates and deletes
  with a retryable `409`.
- **FR-DRF-3** A draft **must** become an outbound message only through the existing send pipeline, with
  the idempotency key `draft:{draft_id}:{revision}`; a draft revision **must** produce at most one message,
  whatever retries, concurrent requests, crashes or repeated alarms occur. `POST …/drafts/{draft_id}/send`
  **must** require `messages:send` and an `Idempotency-Key` and support `?dry_run=true`. A sent draft
  **must** be kept as a record with its `message_id`, its labels **must** carry over to the message, and
  the message **must** record `draft_id`.
- **FR-DRF-4** Tenant policy `drafts.require_approval` (`never`, `new_recipients`, `external`, `always`)
  **must** decide when a send needs approval, with `new_recipients` meaning a recipient that is neither
  known (FR-OUT-13 rules 1–3 and 5) nor internal to the workspace and `external` meaning a recipient that
  is not internal, and an identity's `send_policy.require_approval` **may** only make it stricter. It
  **must** apply to every send of the tenant's identities except the system identity's and a provisional
  agent sign-up's (whose sends reach only its human): an ordinary send,
  reply, reply-all or forward that needs approval **must** be refused with `403 approval_required` before
  anything is stored. It is a guard field (FR-TEN-4). Whatever the level, a send of a draft whose writer
  requested approval, or whose current content was written by a key without `messages:send`, **must**
  need approval.
- **FR-DRF-5** Approving and rejecting **must** need `drafts:approve`, which identity keys **must never**
  hold; approving with `send: true` **must** also need `messages:send`. On a deployment where only people
  may release quarantine (FR-CON-6), an API key **must** approve only on a tenant whose policy has
  `drafts.key_approval: true`, which only a platform key or the tenant's own partner key can set, and
  then only if it is a platform key or that partner key, never a tenant key. A key
  **must not** approve content it wrote. An approval **must** bind to the digest of the content
  approved (sender identity, recipients as resolved, subject, bodies, attachments and their hashes,
  headers, kind, source; not `send_at`), and any change of that content **must** return the draft to
  `pending_approval` and emit `draft.approval_requested` again. The approval **must** be audit-logged with
  the approver and the optional `on_behalf_of` of the caller's own user.
- **FR-DRF-6** For a send from a draft whose current revision carries a valid approval, every approved
  recipient **must** count as known (FR-OUT-13 rule 4) for that send; suppressions, send-block entries and
  `send_allowlist_only` **must** still apply.
- **FR-DRF-7** A draft **must** be schedulable at a UTC `send_at` at least 60 seconds and at most 90 days
  ahead, rescheduled and unscheduled through `send_at`, and cancelled by deletion. The identity's mailbox
  alarm **must** drive scheduled sends, multiplexing every draft onto its one alarm, and a scheduled draft
  whose alarm was missed **must** still be found and sent, or failed, without operator action. At the
  scheduled time every send check **must** run again with the authority recorded when it was scheduled,
  and every outcome **must** be defined. A scheduled send not attempted by `send_at` + 1 hour, and every
  failed scheduled send, **must never** be retried automatically.
- **FR-DRF-8** Draft attachments **must** be given inline (within the 7 MiB request body) or as upload
  references to the same identity's `ready`, unexpired uploads, whose bytes **must** be copied into the
  draft's own objects when the revision is saved, so that deleting or expiring the upload never changes the
  draft; a revision's attachments **must** pass the classification and the 26,214,400-byte total of a send
  when it is saved; at send the draft's copies **must** be copied into the message's own attachment objects
  with their SHA-256 checked, without being held in memory, and keep their `upload_id`.
- **FR-DRF-9** A forward draft **must** snapshot its source at send time, and fail with
  `409 source_unavailable` when the source was erased, is no longer visible to the sender, or cannot be
  reproduced in full (a truncated body whose raw MIME has expired, or a missing attachment object). Reply
  recipients **must** be resolved at send time.
- **FR-DRF-10** Drafts **must** be deleted by every erasure scope (message: drafts replying to or
  forwarding it and the draft that became it; thread; counterparty; identity; tenant), with
  `drafts_deleted` in the receipt, kept by legal holds on their thread, and purged by retention:
  pending approvals expire after 7 days (or at `send_at` + 1 hour), other drafts 30 days after their last
  change, or sooner when `retention.message_days` is shorter.
- **FR-DRF-11** A draft **must** take no plan hold until it is actually sent; the send **must** take and
  settle the `sends` hold through `TenantQuota` exactly as any send, and a scheduled send refused with
  `402 billing_limit` **must** fail with that reason, notified through the existing usage alerts.
- **FR-DRF-12** Drafts **must** emit `draft.approval_requested`, `draft.approved`, `draft.rejected`,
  `draft.sent` and `draft.failed`, from the mailbox outbox with the identity's sequence, carrying no
  subject, body, address or note.
- **FR-DRF-13** The tenant's drafts awaiting approval, scheduled and failed **must** be listable without a
  fan-out over mailboxes (`GET /v1/tenants/{tenant_id}/drafts`), from a D1 index that holds no mail
  content. The console **must** list them, show them in "Needs a person", and let an owner or admin approve
  (a sensitive action), reject or cancel them, without JavaScript.
- **FR-DRF-14** MCP **must** offer tools to create, update, list, get, send and delete drafts, and **must
  not** offer one to approve or reject; the CLI **must** cover every draft route.

### 6.17 Event feed and live stream

- **FR-EVT-1** The service **must** keep one append-only event log per tenant, to which the webhook
  consumer appends every event of the tenant once, deduplicated by event ID, before any webhook delivery of
  it is queued. Log positions **must** follow the order of appends and never go backwards, and the log
  **must** hold the same envelope bytes that webhook deliveries send.
- **FR-EVT-2** `GET /v1/events` **must** return a tenant's events after an opaque cursor, in log order,
  with type and identity filters, a long poll of up to 60 seconds, and a `next_cursor` that is never behind
  an event already returned. Platform and partner keys **must** name one tenant per request.
- **FR-EVT-3** The log **must** keep events for the tenant's `retention.events_days` after they were
  appended, within a per-tenant storage budget. A cursor behind the retained log **must** get
  `410 cursor_expired` with the oldest valid cursor; erasure **must** remove events without invalidating any
  cursor.
- **FR-EVT-4** A key **must** see only the events whose subject it could read through the API: identity keys
  only their own identity's events, mail events only with `messages:read`, quarantine and screening events
  only with `quarantine:review`, and never an event of the system identity. Webhook endpoint filters **must
  not** affect the feed, and types a key may not see **must** be reported as withheld.
- **FR-EVT-5** Durable event consumers **must** keep a named position per tenant on the server: create is
  idempotent by name; reads return the events after the committed position and redeliver them until a
  commit; a commit only moves forward; a consumer is bound to the key that created it; a tenant has at most
  20; and a consumer whose position falls out of retention **must** get `410 cursor_expired` until it is
  reset explicitly.
- **FR-EVT-6** `GET /v1/events/stream` **must** push the same events over a WebSocket, in log order, with a
  documented client protocol (subscribe by cursor, time or consumer; optional acknowledgements; ping), a
  flow-control window, documented close codes and heartbeats, and **must** let a client resume from a cursor
  or a consumer with no event lost.
- **FR-EVT-7** The stream **must** accept a key in the `Authorization` header, or a single-use stream token
  of at most 60 seconds carried in `Sec-WebSocket-Protocol`. No credential **may** be accepted in a URL, and
  a stream token **must never** be logged or echoed.
- **FR-EVT-8** A stream whose key is revoked or expires, or whose tenant or partner is suspended, or whose
  tenant or identity is being erased, **must** be closed within 60 seconds, and no event **may** be sent on a
  connection whose key was last validated more than 60 seconds earlier.
- **FR-EVT-9** The MCP server **must** offer `mail_events`, a cursor poll of the feed, and the CLI **must**
  offer `pmail events list`, `pmail events tail` (resuming from a saved cursor or a consumer) and the
  consumer commands.
- **FR-EVT-10** Message, thread, counterparty, identity and tenant erasure **must** remove the log's copies
  of erased data, consumers and streams, report `log_events_deleted` and a zero `log_hits` probe, keep the
  events of held threads, and refuse a late append of erased data. Message retention and per-identity expiry
  **must** remove the log's copies of the messages they delete, including their entries in events that list
  several messages.
- **FR-EVT-11** The service **must** measure the time from outbox commit to log append (target: p95 at most
  10 seconds), append failures, consumers behind, stream connections and close codes, and alert when appends
  fail or lag.

### 6.18 Screening

- **FR-SCR-1** Each tenant **must** have a screening policy: `screening.mode` (`off`, `monitor` or `hold`, default
  `monitor`), `screening.on_unavailable` (`quarantine` or `deliver`, default `quarantine`) and
  `screening.deadline_seconds` (30–900, default 120). All three are guard fields: a key **must not** loosen them
  where only people may release quarantined mail.
- **FR-SCR-2** In `hold` mode, an inbound message that no quarantine rule held **must** be stored with status
  `screening` and **must not** be listed, counted, searched, embedded, triaged, notified, auto-replied to, returned
  by `wait` or evented as received until it has a verdict; only keys with `quarantine:review` **may** see it, and it
  **must not** be released while `screening`.
- **FR-SCR-3** The verdict **must** combine a deterministic scan of every field an agent can read (including
  HTML text that differs from the plain part, hidden content and attachment text) with a separate model
  classifier: a decisive deterministic signal or a model judgement of steering **must** give `suspect` (status
  `quarantined`, reason `steering_suspected` in `hold` mode); no signal **must** give `clear` (status `received`,
  with `message.received` emitted then and not before); no verdict **must** give `unavailable`, handled by
  `screening.on_unavailable` (`quarantined` with reason `screening_unavailable`, or `received`), except that
  an `unavailable` the mail itself can cause (`model_cap`, `invalid_output`) **must** always be held. A message
  with an attachment whose text was not scanned **must** carry the trust flag `attachments_unscreened`.
- **FR-SCR-4** Every held message **must** reach a final status by its deadline, enforced by the mailbox's alarm
  and an independent backstop timer. Screening **must not** depend on triage being enabled and **must not** change
  triage's advisory role (FR-TRI-3).
- **FR-SCR-5** Every screened message **must** carry a screening record (status, mode, reason code, signals with
  source, kind, field and severity, model, start, deadline, completion, release). The service **must never**
  store, log or return text written by the classifier.
- **FR-SCR-6** Held mail **must** be released only through the existing release route and its person-only rule.
  A release **must** emit `message.released` and then, for a message never yet evented as received,
  `message.received`, in one transaction: `message.received` is emitted exactly once per message. Released mail
  **must never** be screened again.
- **FR-SCR-7** Delivery reports, read receipts and the system identity's mail **must never** be screened.
  Screening **must never** override `hidden`, `throttled` or another quarantine reason. Mail from known
  correspondents, receive-allowed senders and authenticated senders **must** be screened.
- **FR-SCR-8** Keys with `messages:write` **must** be able to give feedback on a screened message (`correct`,
  `false_positive`, `missed_threat`, `legitimate_instruction`, `spam`; notes up to 500 characters; at most 20 per
  message), audit-logged without the notes. Feedback **must never** change a message's state. The service
  **must** report per-tenant verdict counts, precision and recall from feedback.
- **FR-SCR-9** Text that becomes readable after the verdict (attachment text, a re-parse) **must** be scanned
  deterministically; a decisive signal **must** move a delivered message back to quarantine, unless a person
  released it.
- **FR-SCR-10** Screening records and feedback **must** follow their message's retention, legal holds and every
  erasure scope, and erasure receipts **must** count them.
- **FR-SCR-11** Screening **must never** be charged to a plan allowance. Its model calls **must** be bounded by the
  tenant's daily model cap and `RL_AI`; a reached cap **must** give `unavailable` unless the deterministic scan is
  decisive. One sender **must not** be able to spend more than a tenth of the cap, senders that are not known
  correspondents **must not** together be able to spend its last fifth, and triage and agentic
  search **must not** be able to spend screening's share of `RL_AI`.
- **FR-SCR-12** The service **must** record verdict counts by reason, unavailable rates, deadline misses and
  latency, and **must** alert the operator when a tenant's held mail becomes unavailable at volume, when model
  failures reach the deployment as a whole, or when a hold outlives its deadline.

### 6.19 Calendar invitations

- **FR-CAL-1** Every inbound message **must** have each of its calendar parts (`text/calendar`,
  `application/ics`, `.ics` files and calendar files inside TNEF; never parts of nested messages, DSNs or
  MDNs) read under size, line, component, override, attendee, time-zone and string bounds, and **must**
  expose one structured, bounded `calendar` object, or `null`. A part that cannot be read **must not**
  stop the message from being stored; its object says why (`invalid`, `too_large`, `too_complex`,
  `unsupported`).
- **FR-CAL-2** Every time in the object **must** be given as a UTC instant where one can be computed, with
  the original `TZID` and how it was resolved: IANA names, Windows names through the CLDR mapping,
  `/`-prefixed globally unique names, and the object's own `VTIMEZONE` definitions. All-day dates,
  floating times and unknown zones **must** be marked as such, never guessed.
- **FR-CAL-3** Recurrence rules **must** be validated and returned as text, never expanded; a master with
  up to 100 overrides **must** be read as one object.
- **FR-CAL-4** Every iTIP method (`PUBLISH`, `REQUEST`, `REPLY`, `ADD`, `CANCEL`, `REFRESH`, `COUNTER`,
  `DECLINECOUNTER`) **must** be classified, as well as objects without a method and with an unknown one.
  `VTODO`, `VJOURNAL` and `VFREEBUSY` **must** be listed as ignored; `VALARM`, `ATTACH` and extension
  properties **must not** appear in the object agents read.
- **FR-CAL-5** Each object **must** say whether it is verified, by which rule, and otherwise why not. An
  organizer method is verified only when the message's authenticated sender is the organizer, an attendee
  method only when it is the replying attendee, and only when the part holds no component of another
  `UID` or organizer; a `SENT-BY` **must never** verify an object.
- **FR-CAL-6** A `calendar.received` event **must** be emitted when a message with a calendar object first
  becomes visible as `received`, and never while it is quarantined, in screening, hidden or throttled. It
  **must** be thin, and an endpoint that receives it **must** be created only by a key with
  `messages:read`.
- **FR-CAL-7** Every text value of the object **must** be cleaned of control and hidden characters and
  treated as untrusted content.
- **FR-CAL-8** Send, reply and draft send **must** accept a caller-supplied iCalendar object, bounded in
  size, and **must** refuse it unless it has exactly one known `METHOD`, one `UID`, a `DTSTAMP` in every
  component and the per-method structure of RFC 5546, and unless its organizer (organizer methods) or
  replying attendee (attendee methods) is the address the message will be sent from, and its recipients
  are the object's attendees (organizer methods) or its organizer (attendee methods).
- **FR-CAL-9** A calendar message **must** be sent as iMIP: the human-readable text and a
  `text/calendar` part whose `method` parameter equals `METHOD`, as alternatives, optionally with an
  `application/ics` copy. Missing `VTIMEZONE`s for zones that resolve by name **must** be added, other
  systems' extension properties removed and reported, and the object re-serialised with CRLF and 75-octet
  folding. A calendar message **must** be `kind: transactional` and **must never** be sent through domain
  fallback.
- **FR-CAL-10** An identity **must** be able to answer a verified invitation it is an attendee of with
  `accepted`, `declined` or `tentative` and an optional comment, through one idempotent route that sends an
  iTIP `REPLY` to the organizer from the attendee address in the invitation's thread, and refuses
  unverified, held (quarantined or in screening), cancelled, superseded, past and non-attendee cases with
  defined errors; only verified messages from the invitation's own organizer may make it cancelled or
  superseded.
- **FR-CAL-11** An identity **must not** be able to send, for one `UID`, an organizer method with a lower
  `SEQUENCE` than one it already sent, also when two sends race.
- **FR-CAL-12** A verified calendar message without thread headers **must** join the thread that holds a
  calendar message with the same `UID` and `ORGANIZER` (for an attendee's answer, an invitation this
  identity sent that names that attendee); an unverified one **must never** join by `UID`.
- **FR-CAL-13** Pylota Mail **must not** keep calendar state beyond the message's own row and attachment.
  Calendar data **must** be retained, erased (including by counterparty erasure of any address the object
  names), held and exported with its message.
- **FR-CAL-14** The calendar object **must** be readable, and the reply route usable, through the REST API,
  MCP and the CLI; the console **must** show the object read-only.

### 6.20 Uploads

- **FR-UPL-1** The service **must** let a key holding `messages:send` or `drafts:write` create an upload
  for an identity in its scope, with a filename, a content type, a size of 1–26,214,400 bytes and a
  SHA-256, and return the upload's ID, a signed upload URL valid for at most one hour, and its
  `expires_at`.
- **FR-UPL-2** The service **must** accept an upload's bytes only through its signed upload URL on the
  Worker, require a `Content-Length` equal to the declared size, stream the bytes into R2 without holding
  the file in memory, and make the upload `ready` only after R2 has verified the declared SHA-256 and the
  stored size matches.
- **FR-UPL-3** The service **must** classify every upload with the inbound attachment rules and, when a
  scanner is configured, scan it before it can be attached; a quarantining risk or an `infected` verdict
  **must** reject the upload, delete its bytes and refuse every later use with `422 attachment_risky`.
- **FR-UPL-4** The service **must** let send, reply, reply-all, forward and drafts attach a `ready`
  upload of the sending identity by `upload_id`, re-check the risk of the final filename, and copy the
  bytes into the message; an upload of another identity or tenant **must** be refused exactly as a
  missing one (`404 upload_not_found`).
- **FR-UPL-5** The service **must** apply the transport limit to attachments from uploads as to inline
  ones (`413 message_too_large`, or signed links with `large_attachments: "link"`), and **must** refuse a
  message whose attachments total more than 26,214,400 decoded bytes.
- **FR-UPL-6** The service **must** admit one `PUT` per upload at a time, answer a repeated `PUT` of a
  `ready` upload with success without reading its body, resume an upload whose bytes were stored but
  whose response was lost, and expire an upload after ten failed attempts caused by the client.
- **FR-UPL-7** The service **must** delete every upload and its bytes at its `expires_at` (default
  24 hours, at most 7 days, never later than the identity's mail retention), used or not, and deleting
  or expiring an upload **must never** change a message or a draft that used it.
- **FR-UPL-8** The service **must** limit each identity to 100 live uploads and 524,288,000 declared
  bytes, count upload bytes toward `storage_gb`, and refuse a create with `402 billing_limit` while
  storage is at or over its allowance.
- **FR-UPL-9** The service **must** delete uploads in every erasure scope, including the uploads that
  erased messages were copied from, count them in the receipt as `uploads_deleted`, leave them out of
  subject-access exports and the R2 backup copy, and delete upload objects that no row names.
- **FR-UPL-10** The service **must** classify inline base64 attachments of every send with the same
  rules, so that sending base64 never bypasses the checks an upload gets.
- **FR-UPL-11** The service **must never** fetch attachment content from a URL, and **must not** offer
  uploads as a file store: no paths, no download route and no sharing between identities.
- **FR-UPL-12** The MCP server **must** offer `mail_create_upload` and `mail_get_upload` and accept
  `upload_id` items in `mail_send`, `mail_reply` and `mail_forward`; the CLI **must** offer
  `pmail uploads create|list|get|delete` and use uploads for `--attach` files over 1 MiB.

### 6.21 Deliverability metrics

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

### 6.22 Agent self-sign-up

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
  deployment-wide, and above the thresholds of [Agent self-sign-up §13.2](design/agent-signup.md#132-the-automatic-switch) new sign-up mail
  and every send of a provisional workspace **must** stop automatically and page the operator; a sign-up
  spike and 80% of the daily cap **must** alert.
- **FR-ASU-16** No log, audit row, idempotency record or event **may** hold the human's address, a code, a
  link or refuse token, or a key; every new store **must** be covered by the tenant-scope erasure or hold
  only keyed hashes.

## 7. Non-functional requirements

| ID | Requirement | Target |
|---|---|---|
| NFR-REL-1 | Acknowledged inbound messages lost | 0 |
| NFR-REL-2 | Valid inbound mail accepted | ≥ 99.9% per month |
| NFR-REL-3 | Inbound accepted → webhook delivered | p95 ≤ 30 s, p99 ≤ 120 s |
| NFR-REL-4 | Webhook delivered within 24 h, including retries | ≥ 99.99% |
| NFR-PERF-1 | Send API (accepted into queue) | p95 ≤ 500 ms |
| NFR-PERF-2 | Queued → handed to transport | p95 ≤ 60 s |
| NFR-PERF-3 | Keyword search, one identity | p95 ≤ 200 ms |
| NFR-PERF-4 | Hybrid search, one identity | p95 ≤ 800 ms |
| NFR-PERF-5 | Tenant search across up to 10 identities | p95 ≤ 1 s |
| NFR-PERF-6 | Agentic search | p95 ≤ 8 s, first evidence ≤ 1.5 s |
| NFR-QUAL-1 | Search recall@10 on the golden mailbox | ≥ 0.90 (hybrid), CI fails on a drop over 1 point |
| NFR-QUAL-2 | Agentic citation precision | ≥ 0.98 after verification |
| NFR-QUAL-3 | Triage category accuracy on the labelled set | ≥ 0.85 |
| NFR-SEC-1 | Cross-tenant access in attack tests | 0 |
| NFR-SEC-2 | Worker bundle size | ≤ 10 MiB compressed (startup ≤ 1 s) |
| NFR-PRV-1 | Erasure completes | ≤ 24 h, receipt always produced |
| NFR-OPS-1 | Fresh deploy, hands-on time | ≤ 15 minutes |
| NFR-OPS-2 | Recovery point / time objectives | RPO ≤ 1 min (indexes); ≤ 15 min (blobs) against infrastructure loss, from R2's durability. R2 has no versioning or replication, so blobs deleted by a bug are recoverable only with the optional nightly backup bucket (RPO 24 h, off by default); RTO ≤ 4 h |
| NFR-OPS-3 | Operator alerting with one operator | Every page alert reaches the operator through two independent channels (alert email from the deployment, and an external scheduled heartbeat that fails when the deployment does not answer or a page alert fires) within 15 minutes; a stopped deployment or a stopped heartbeat is detected within 1 hour; conditions that need a safe action at once are contained automatically ([Observability § 5.5–5.6](design/observability.md#55-alert-email-and-the-external-heartbeat)) |
| NFR-COST-1 | Idle deployment cost beyond Workers Paid | ≈ 0 (no always-on compute) |
| NFR-BILL-1 | Metered actions allowed beyond a granted allowance | 0 (holds are atomic per workspace) |
| NFR-BILL-2 | Metered actions failed because a billing provider was unreachable | 0 (metering is in-process; Stripe is only needed to change plans) |
| NFR-CON-1 | Console pages, server render time | p95 ≤ 300 ms |

## 8. Success metrics

- Pylota cut over from AgentMail. Every operator has four identities, inbound mail works end to end,
  and none of the edge-case register's `S` rows fails in production for 30 days.
- A fresh agent session deploys from the self-hosting page alone, with no help, inside 15 minutes of hands-on time (measured by the M20 rehearsal, [ADR 0015](adr/0015-solo-operator.md)).
- Agents in Pylota's eval suite find and cite the right email in at least 90% of mail-retrieval tasks.
- Zero duplicate sends attributed to retries.

## 9. Release criteria (v1.0)

1. Every `P0` requirement maps to at least one passing test (unit, conformance, or integration against workerd).
2. Every row in the [edge-case register](edge-cases.md) marked `S` or `S+I` has a named, passing test.
3. The live end-to-end suite passes on a staging deployment. It covers real inbound from Gmail and
   Outlook, real outbound to both, bounce and complaint simulation, a domain change, a domain failure
   with fallback, and erasure.
4. Search quality gates (NFR-QUAL-1/2) and triage gate (NFR-QUAL-3) pass.
5. A threat model is written, and the findings rated high are fixed.
6. A fresh-account deploy rehearsal has been run from the docs.
7. The release gate passes: `release-gates/v{version}.md` ticks every item of the
   [pre-release checklist](design/security.md#16-pre-release-checklist) with a link to its evidence, and
   the release carries SBOMs, signed checksums and build provenance (`cargo xtask release-gate`).

## 10. Risks

| Risk | Mitigation |
|---|---|
| Cloudflare Email Sending is in public beta | A `MailTransport` trait with an SES adapter, plus a documented failover runbook |
| `workers-rs` is pre-1.0 | Exact pins. Only `crates/platform` touches it. Gaps (Vectorize, `toMarkdown`) are covered with `wasm-bindgen` externs |
| Vectorize has no documented jurisdiction option | Vectors hold IDs and filter fields only, never text. Documented in privacy notes |
| A shared platform domain means shared reputation | Per-identity caps, complaint and bounce auto-pause, DMARC ramp, custom domains encouraged |
| An LLM fabricates in agentic search or triage | Deterministic citation check, schema validation, untrusted-content fencing, read-only tools |
| Web Bot Auth is still an IETF draft, and Cloudflare's verifier can change | Spike S13 checks the format against Cloudflare's test endpoint. Signed HTTP requests are P1 and stay off (`PM_WEB_BOT_AUTH=off`) unless S13 passes; agent assertions rely only on published RFCs (7517, 7519, 7638, 8037) |
| SES receiving, SMTP from a Worker and child zones are unproven from a Worker | Spikes S11, S12 and S10 gate `dns_records`, `smtp_relay` and `delegated_subdomain` (`smtp_relay` with `inbound: ses` needs S11 too). A method whose spike fails moves to v1.1 by ADR; `cloudflare_zone`, `nameservers` and `send_only` do not depend on them |

## 11. Open questions

None blocking v1.0. Decisions are recorded in [ADRs](adr/index.md).

## 12. Licensing

- The source is published under the **Functional Source License, Version 1.1, ALv2 Future License**
  (`LICENSE.md`). Anyone may use, modify and redistribute it for any purpose except a *Competing Use*:
  offering it to others in a commercial product or service that substitutes for it or offers substantially
  similar functionality. Internal use, non-commercial education and research, and professional services for
  licensees are explicitly permitted.
- Each version becomes available under the Apache License 2.0 on the second anniversary of its release.
- FSL is a Fair Source licence, not an OSI-approved open-source licence. Public copy says "source available"
  or "Fair Source", never "open source".
- The licensor is **TREFT LTD**, the company behind Pylota (`LICENSE.md`: "Copyright 2026 TREFT LTD").
- Contributions are licensed to TREFT LTD under Apache-2.0 with a DCO sign-off (`CONTRIBUTING.md`), so it can
  publish them under FSL now and Apache-2.0 later.

## 13. Business model and pricing

Self-hosting is free under FSL-1.1-ALv2 with no plan limits. Pylota Mail Cloud is Pylota's hosted deployment
of the same code, with billing on. The plans mirror goshen-email's published plans (read 2026-10-09) with
every allowance identical. Each price is half goshen's figure and is charged in pounds sterling: goshen's
$20 and $99 become £10 and £49.50, and its $2 top-up becomes £1.

| | Free | Developer | Team | Self-host |
|---|---|---|---|---|
| Price (GBP, excl. VAT) | £0 | £10 a month | £49.50 a month | £0 under FSL-1.1-ALv2 |
| Inboxes (identities) | 5 | 10 | 100 | no plan limits |
| Sends per month | 1,000 | 10,000 | 100,000 | |
| Triage analyses per month | 500 | 10,000 | 100,000 | |
| Custom domains | none | 5 | 50 | |
| Storage | 1 GB | 10 GB | 100 GB | |
| Seats | 1 | 2 | 10 | |
| Top-ups | none | £1 per unit | £1 per unit | |
| Support | GitHub issues | email | priority email | contracts available |

- A **top-up unit** is one inbox, 1,000 sends or 1,000 triage analyses, added to the plan each month while
  it is subscribed. Custom domains, storage and seats have no top-up: they come with the plan.
- Every plan includes the full API, MCP server, CLI, console, quarantine review and all four search modes.
  The per-identity rolling send limit stays on every plan as an abuse backstop.
- Prices are in pounds sterling (GBP) and exclude VAT. Stripe Tax adds UK VAT, and VAT or sales tax in
  other countries, where it is due. Every customer is billed in GBP; there are no local-currency prices in v1.

### Cost controls

Sending and model calls are the costs that grow with use. Storage and Vectorize are small next to them.

- Triage and agentic-search models are chosen by evaluation, with cost per call as a scored criterion
  alongside quality ([Triage](design/triage.md), [Search](design/search.md)).
- A top-up is priced above the provider cost of what it adds.
- Usage per workspace is metered (`GET /v1/usage`), so plan allowances can be reviewed against real use.

- Agentic searches are not a separate allowance (goshen has no agentic search), so they are rate-limited per
  key and capped per workspace per day (`agentic_daily_cap`) to bound model cost.
