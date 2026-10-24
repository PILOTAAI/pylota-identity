# AgentMail comparison and gap register

This page compares Pylota Identity with AgentMail as AgentMail stood on 2026-10-17. It records every
capability gap and the decision taken on each one. [ADR 0017](adr/0017-agentmail-parity.md) holds those
decisions. AgentMail is the hosted agent-email product that Pylota is moving off. Pylota Identity must cover
everything Pylota uses from it. Where it adds a capability, it must be harder to misuse and must say what
happens in every failure case.

Re-check every AgentMail fact before quoting it outside this repository. AgentMail ships often: its
changelog has 27 dated entries between 2025-06-15 and 2026-10-14, with gaps of up to ten weeks (2026-03-18
to 2026-05-28).

## 1. Method

| Source | What was read | Read |
|---|---|---|
| AgentMail OpenAPI document, `https://docs.agentmail.to/openapi.json` | All 180 operations and 369 schemas, extracted in a browser. 50 operations sit under `/v0/pods`: 4 manage pods, and 46 are pod-scoped copies of other routes | 2026-10-17 |
| AgentMail AsyncAPI document, `https://docs.agentmail.to/asyncapi.yaml` | The WebSocket protocol: servers, the `subscribe` message and 19 server message types | 2026-10-17 |
| AgentMail documentation, `https://docs.agentmail.to/llms.txt` index | 143 pages: concepts, guides, knowledge base, the event and integration pages, and every changelog entry | 2026-10-17 |
| AgentMail site and pricing, `https://www.agentmail.to/` and `/pricing` | Plans, quotas and feature gates | 2026-10-17 |
| REA runtime capture, evidence `ev_c4c47713d3bb35fa8a0c187973a9cdcde2f80277660cdfddb63baabef5512719` | `agentmail.to` and `docs.agentmail.to` in a fresh browser profile: pages, navigation, the integrations shown (MCP endpoint and header, skills, plugins) | 2026-10-17 |
| REA runtime capture, evidence `ev_cb5466cb3919ee01d126b0b0d42d41b6e8fc57d64f271cf898a657c4beee1ea1` | The home page's live demo inbox, observed for 14 s with network and WebSocket events (195 events, complete, nothing dropped) | 2026-10-17 |

Limits of the method:

- Only public material was read. No account was created, no API call was made with a key, and no
  AgentMail system was tested.
- Most documentation pages were read through a summarising fetch. Numbers on this page are as those
  pages state them. Where pages contradicted each other, the conflict is noted.
- Behaviour that the docs do not state is recorded as not stated, never inferred. Examples: the AgentID
  token format, Agent Armor's threat categories, WebSocket heartbeats and close codes, and x402 prices
  other than inbox creation.
- The working inventories live outside the repository, in the session scratchpad
  `agentmail-rea/{api-inventory,inv-core,inv-realtime,inv-identity}.md`. Every row below can be traced to
  one of them.

**What the runtime capture showed.** The home page's demo inbox works in three steps:

1. On page load, the site creates a fresh inbox through its own server route (`POST /api/demo-inbox` on
   `www.agentmail.to`). The API key stays on the server.
2. The page then polls `GET /api/demo-inbox/threads?inboxId=<address>` every few seconds. It uses no
   WebSocket.
3. The responses carry a CDN `age` header, so they are cached and keyed by the inbox address.

Two lessons follow for any demo on Pylota Identity's own site. A demo must be keyed by an unguessable
capability token, never by the mailbox address. And a demo must not create a mailbox for every page view.

## 2. AgentMail's model in brief

- **Scopes.** Organisation › pod › inbox. Threads, drafts, webhooks, lists, metrics, API keys and
  accounts exist at all three scopes; domains exist only at organisation and pod scope (there is no
  inbox-scope domain route; OpenAPI document, re-read 2026-10-18). `GET /v0/auth/me` reports the calling key's
  scope.
- **Mail.** An inbox has one address. It offers threads, messages, attachments with signed download links
  and extracted text, raw MIME, labels (system and free-form), drafts with scheduled send, and allow and
  block lists. Lists come in three directions (send, receive, reply) at three scopes, and the most
  specific scope wins.
  - Reply extraction (`extracted_text`/`extracted_html`) strips quoted history "in English and many other
    languages, including" ten named ones, Arabic among them
    ([Reply & Attachment Extraction](https://docs.agentmail.to/reply-extraction.md), read 2026-10-18).
  - `authentication_results` gives SPF, DKIM and DMARC results with their domains and the ARC sealer.
  - Sending is idempotent through an `Idempotency-Key` held for 24 hours. Creates are idempotent through
    `client_id`.
- **Inbound policy.**
  - A message with a virus is accepted and then dropped silently.
  - A message that fails DMARC under an enforcing policy is dropped, unless a trusted ARC seal vouches
    for it. The trusted sealers are google.com, microsoft.com, outlook.com and icloud.com.
  - Other authentication failures are stored with an `unauthenticated` label and hidden by default.
  - Spam is stored and hidden.
- **Events.**
  - 20 webhook event types, signed through Svix. Delivery is 8 attempts over about 27.5 hours, at least
    once, with no ordering guarantee.
  - Endpoints are auto-disabled after 5 days of total failure, without notice and without redelivery.
  - "Polling webhooks" are a pull queue with commit offsets.
  - A WebSocket stream at `wss://ws.agentmail.to/v0` authenticates with an `api_key` query parameter.
  - An MCP events subscription exists.
- **Agent features.**
  - Agent Armor screens inbound mail for prompt injection.
    - Verdicts are `clear` or `review`, and a scan has a 15-minute deadline.
    - Monitor mode only sends events. Hold mode withholds `message.received` but still lists and
      searches the held mail.
    - Release needs an opt-in permission, and a feedback route takes 5 values.
  - Calendar (Labs) gives each inbox a calendar: events, RRULE recurrence, agenda, ETag concurrency,
    iMIP invitations in and out, RSVP, and start and end webhooks.
  - Agent Drive (Labs) gives each inbox file storage: a three-step upload with SHA-256 checks,
    100 MiB per file, and a 1 GiB quota per organisation.
- **Identity.**
  - Agent self-sign-up returns an organisation, an inbox and a key in one call. Until a human verifies a
    6-digit code, the agent may email only that human, 10 emails a day.
  - A person can claim the inbox in the console by pasting the key.
  - AgentID lets an inbox sign in to third-party apps: an app directory, a 5-minute magic link,
    30-day public-key sign-in credentials, and 10-minute ID tokens with no revocation webhook.
  - P-256 public-key API credentials exist alongside bearer keys.
  - Keys carry about 52 boolean permissions and can be scoped to an organisation, pod or inbox.
- **Interfaces.**
  - REST, with SDKs for Python and TypeScript (`agentmail` 0.5.46 on npm).
  - A hosted MCP server with 43 tools plus 2 that need OAuth, authenticated by OAuth or `x-api-key`, with
    a stdio bridge.
  - A CLI with `--schema`, JMESPath `--query` and skill generation.
  - Plugins for Claude Code, Codex, Cursor, ChatGPT and OpenClaw, and framework toolkits.
  - IMAP (993) and SMTP (465 and 587), with the API key as the password.
  - x402 (USDC) and MPP pay-per-call servers, and a separate EU environment (an Enterprise feature).
- **Plans (USD).** Free has 3 inboxes and 100 emails a day. Developer costs $20 a month for 10 inboxes
  and 1,000 a day. Startup costs $200 a month for 150 inboxes and 15,000 a day. Enterprise is priced on
  request. Custom domains start at Developer, and the SOC 2 report at Startup. Cold outreach is
  prohibited.

## 3. Where Pylota Identity already goes further

| Area | AgentMail (2026-10-17) | Pylota Identity |
|---|---|---|
| Identity and address | The address is the inbox | Identities outlive addresses and domains, with rollback (U3) |
| Unknown send outcomes | Idempotency for 24 h; no notion of an unknown outcome | Idempotency for 30 days, `uncertain` is never resent, and reconciliation follows (U2) |
| Mail failing authentication | Dropped silently (DMARC enforcing) or labelled | Never accepted and then dropped. On Email Routing, mail that fails both SPF and DKIM or the sender's DMARC policy is rejected by Cloudflare before the Worker runs ([Email lifecycle](https://developers.cloudflare.com/email-service/concepts/email-lifecycle/), read 2026-10-18; whether inside the SMTP session is not stated); every other failing message, and all failing mail through SES inbound, is stored and quarantined with a reason (FR-IN-1, FR-IN-5) |
| Mail with a virus | Dropped silently, with no event | Quarantined `risky_attachment`, with a scanner hook (FR-IN-5, P1 scanner) |
| Paused mailbox | Inbound mail is neither delivered nor bounced, and is lost after resume | Stored; sends refused with `identity_paused` (FR-IDN-3) |
| Search | Full text, 100 results | Keyword, semantic, hybrid and agentic with verified citations (U1) |
| Domains | One record set (SES records), with Domain Connect one-click setup (`GET /v0/domains/{domain_id}/setup-link`), send-only domains, subdomain inboxes and Google Workspace coexistence | Six connection methods, health checks from two resolvers, aligned fallback, never unauthenticated mail (U4) |
| Erasure | Delete calls, plus an optional 24 h expiry that leaves copies | Five erasure scopes, receipts with per-store counts and probe queries, legal holds (U6) |
| Webhook failure | Auto-disabled after 5 days, no notice, no redelivery | 72 h retries, dead-letter state, replay for 30 days. An endpoint is disabled sooner than on AgentMail, after 100 consecutive failures spread over at least 24 hours; the `webhook.disabled` event goes to platform and partner endpoints, never to the tenant's own endpoints ([Webhook events](../reference/events.md)) |
| Agent identity proof | AgentID: 10-minute tokens, no revocation webhook, signature scheme undocumented | Ed25519 assertions verifiable against a published JWKS; pausing withdraws the keys at once (U10) |
| Plan enforcement | Limits take up to 5 minutes to apply after a change | Atomic holds; `402 billing_limit` is safe to retry (FR-BILL-6) |
| Seats | 2 / 2 / 10 seats by plan | Roles, enforced seats, audit, required two-step verification (U7) |
| Hosting | Hosted, US or EU; BYO cloud on Enterprise | Runs in the deployer's own Cloudflare account, with the EU jurisdiction optional (U6, U9) |

## 4. Gap register

Each gap has a decision:

- **add**: specified for v1.0 on the page named.
- **change**: an existing v1.0 behaviour changes.
- **defer**: a later version, with the reason.
- **refuse**: deliberately not built, with the reason in §5.

| # | Capability | AgentMail | Pylota Identity before this change | Decision | Specified in |
|---|---|---|---|---|---|
| GAP-1 | Drafts, human approval before send, scheduled send | Drafts with `send_at`, reschedule, unschedule and cancel; approval works only by withholding send permissions; the draft is deleted once sent | Non-goal ("planned for v1.1") | **add**: drafts with approval bound to a content digest, two approval patterns, person-only approval on Cloud unless the partner opts its tenants in (`drafts.key_approval`), the draft kept as a record after sending, schedule driven by Durable Object alarms through the one send pipeline | [Drafts](design/drafts.md) |
| GAP-2 | Pull feed of events for agents without a public URL | Polling webhooks: a queue with commit offsets, US only, 10 per scope | `wait` long poll (messages only); webhook replay | **add**: an ordered per-tenant event log, `GET /v1/events` with cursors and long poll, durable consumers with commit | [Event feed](design/event-feed.md) |
| GAP-3 | Live event stream | WebSocket with the API key in the URL; no documented heartbeat, close codes or replay; body cut at 30 KiB | P2 ("WebSocket push") | **add**: a WebSocket stream on the event log, resumable with no loss, with documented close codes and no secret in URLs | [Event feed](design/event-feed.md) |
| GAP-4 | Holding mail that tries to steer an agent | Agent Armor: model verdict, 15-minute deadline, Hold mode that only withholds a notification, release, feedback | A deterministic steering scan and a model risk flag, advisory only; no hold | **add**: `screening.mode` with a real hold (status `screening`, hidden) that a tenant opts into (the default mode is `monitor`), deterministic and model signals, fail-closed default for held mail, release through the existing person-only rules, feedback measured per tenant | [Screening](design/screening.md) |
| GAP-5 | Calendar invitations | A full calendar per inbox; inbound invitations imported only when authenticated, single-event and ≤ 1 MB; Windows time zones not imported | `kind = 'calendar'` and a six-field summary; invitations never answered | **add**: iMIP/iTIP parsing and verification with a bounded structured object, Windows time zones mapped, multi-component bundles, `calendar.received`, outbound iMIP from a supplied iCalendar payload, a reply route. No calendar store: Kairos owns calendar state for Pylota | [Calendar invitations](design/calendar-invitations.md) |
| GAP-6 | Attachments larger than an inline JSON body | Base64 (6 MB per request) or a URL that AgentMail fetches (~30 MB); Agent Drive | Base64 inside a 7 MiB body; signed links for oversized attachments (P1) | **add**: per-identity uploads streamed through the Worker with SHA-256 checks, referenced by sends and drafts. URL fetching is refused (SSRF) and no general drive is built | [Uploads](design/uploads.md) |
| GAP-7 | Agent self-sign-up | One call returns an org, inbox and key; send only to the human until a code is verified; receive-only without a human; claim by pasting the key | Person-first sign-up only (`PM_SIGNUP`) | **add**, behind `PM_AGENT_SIGNUP` (off by default): the human email is required (FR-IDN-2), a provisional workspace expires in 7 days, and a claim needs a matching verified email | [Agent self-sign-up](design/agent-signup.md) |
| GAP-8 | Deliverability metrics | `/metrics/events`, `/usage`, `/rates` at three scopes | Usage routes and domain health; abuse windows not exposed | **add**: event counts and the exact windows the pause logic uses, with threshold and headroom | [Metrics](design/metrics.md) |
| GAP-9 | TypeScript and Python SDKs | Both | Rust SDK only | **add** TypeScript (P1, built in its own repository, [ADR 0018](adr/0018-typescript-sdk-in-its-own-repository.md)); Python decided on the SDK page | [SDKs and skills](design/sdk-and-skills.md) |
| GAP-10 | Agent skills, plugins, MCP setup for agent tools | Skills repo, plugins for five clients, a stdio bridge with tool filtering, framework toolkits | MCP over Streamable HTTP with API keys; no skills | **add**: skills package, MCP tool annotations and allow-list, setup snippets, framework guides | [SDKs and skills](design/sdk-and-skills.md) |
| GAP-11 | CLI for agents | `--schema`, `--query`, skill generation | `--json` on every command | **add**: `--schema`, `--query` | [SDKs and skills](design/sdk-and-skills.md) |
| GAP-12 | Batch read and label update | Batch get; batch update ≤ 50, atomic for found IDs | One message per call | **add**: ≤ 100 per call, atomic in the mailbox | [Parity additions §2](design/parity-additions.md#2-batch-message-operations-fr-in-13) |
| GAP-13 | Allow and block lists per mailbox | Org, pod and inbox scope; send, receive and reply; most specific wins | Tenant scope only | **add**: identity scope, most specific wins | [Parity additions §3](design/parity-additions.md#3-identity-scope-allow-and-block-lists-fr-out-17) |
| GAP-14 | Label change events | Inbox events feed (`label.added`, `label.removed`) | None | **add**: opt-in `message.labels_changed` | [Parity additions §4](design/parity-additions.md#4-label-change-events-fr-wh-6) |
| GAP-15 | Authentication domains on messages | `spf_domain`, `dkim_domain`, `dmarc_domain`, `arc_sealer` | Results without domains | **add** to `trust` | [Parity additions §5](design/parity-additions.md#5-authentication-detail-on-messages-fr-in-14) |
| GAP-16 | Forwarded mail that breaks DMARC | Rescued by a trusted ARC seal | ARC recorded, never trusted: forwarded mail that reaches the Worker is quarantined `auth_failed`. Cloudflare Email Routing rejects DMARC-failing mail before the Worker, and its docs do not say whether a valid ARC chain changes that | **change**: a platform-only list of trusted ARC sealers (`google.com`, `microsoft.com`; the others AgentMail trusts could not be verified) and an exact rescue rule. It applies wherever the Worker sees the mail (SES inbound); on Email Routing a live test (`live::arc::cloudflare_forward_behaviour`) records what Cloudflare does | [Parity additions §6](design/parity-additions.md#6-trusted-arc-sealers-fr-in-15) |
| GAP-17 | Short-lived mailboxes | Org-wide 24 h expiry, early access, copies remain | Retention per tenant (raw 90 days, messages kept) | **add**: per-identity `retention.message_hours` with erasure receipts and holds | [Parity additions §7](design/parity-additions.md#7-per-identity-message-retention-fr-prv-7) |
| GAP-18 | Static headers on webhook deliveries | Write-only custom headers | None | **add**: up to 10, values sealed and write-only | [Parity additions §8](design/parity-additions.md#8-webhook-delivery-headers-fr-wh-7) |
| GAP-19 | Zone-file export | BIND zone file | JSON records only | **add**: `?format=bind` | [Parity additions §9](design/parity-additions.md#9-zone-file-export-fr-dom-15) |
| GAP-20 | A documentation link in errors | `docs` in every standard error; `fix` "omitted when no generic remedy applies"; one response carries only `message` | `fix` and `retryable`, no link | **add**: `docs` | [Parity additions §10](design/parity-additions.md#10-a-documentation-link-in-errors-fr-api-3) |
| GAP-21 | Reply extraction in Arabic | English and many other languages, ten named, Arabic among them | 11 languages including Japanese, no Arabic | **defer**: an Arabic attribution pattern waits for a corpus sample from real clients in `crates/conformance` (a pattern must not be guessed); no requirement or milestone until then | [ADR 0017](adr/0017-agentmail-parity.md), [PRD § 5](prd.md#5-scope-and-priorities) P2 |
| GAP-22 | Remote MCP with OAuth | OAuth or `x-api-key` | API key only; OAuth 2.1 deferred to v1.1 ([ADR 0016](adr/0016-plan-items-changed-for-v1.md)) | **defer** (unchanged): the SDK page records which clients cannot connect without OAuth | [SDKs and skills](design/sdk-and-skills.md) |
| GAP-23 | Public-key API credentials | P-256 JWK credentials; the signing scheme is not documented | Bearer keys | **defer** to v1.1: Pylota's servers hold its keys, so agents never see a bearer secret. A design must name its signature scheme and replay window | [ADR 0017](adr/0017-agentmail-parity.md) |
| GAP-24 | Agents signing in to third-party apps | AgentID: app directory, magic link, remembered approval for 180 days, 10-minute tokens | Signed agent assertions plus JWKS; the service sign-up ledger approves each account | **defer** an OIDC-compatible relying-party flow to v1.1; v1.0 keeps assertions plus the ledger, which already ties consent to a person's approval | [ADR 0017](adr/0017-agentmail-parity.md) |
| GAP-25 | Separate EU environment on the hosted service | Enterprise feature | The EU jurisdiction for self-hosting; Cloud runs one jurisdiction | **defer**: a second Cloud deployment is an owner decision | [ADR 0017](adr/0017-agentmail-parity.md) |
| GAP-26 | Pay-per-call (x402, MPP) | USDC (x402) and Stripe MPP servers | Plans in GBP through Stripe | **defer**: revisit when MPP is generally available for UK merchants | [ADR 0017](adr/0017-agentmail-parity.md) |
| GAP-27 | IMAP and SMTP access | IMAP 993, SMTP 465/587 | Non-goal | **refuse** for v1.0 (§5) | [ADR 0017](adr/0017-agentmail-parity.md) |
| GAP-28 | Open tracking | `track_opens`, `message.opened` | None | **refuse** (§5) | [ADR 0017](adr/0017-agentmail-parity.md) |
| GAP-29 | A calendar store, agenda and recurrence engine | Per-inbox calendar | None | **refuse** (§5): Kairos is the calendar | [ADR 0017](adr/0017-agentmail-parity.md) |
| GAP-30 | General file storage | Agent Drive | None | **refuse** (§5): uploads cover the mail use | [ADR 0017](adr/0017-agentmail-parity.md) |
| GAP-31 | Fetching attachments from a URL | `url` attachments | None | **refuse** (§5) | [Uploads](design/uploads.md) |
| GAP-32 | Mailbox avatars (BIMI) | Not offered (guidance only) | None | **defer**: a guide when a tenant asks; BIMI needs a VMC or CMC and enforced DMARC | – |

## 5. Deliberate differences

- **No IMAP or SMTP server (GAP-27).** Workers cannot accept inbound TCP connections. An IMAP server
  would be a second, stateful system to run beside the Worker, and the operator is one person
  ([ADR 0015](adr/0015-solo-operator.md)). Agents use the API, MCP, the event feed or the stream, and
  people use the console. Revisit only if a tenant needs a desktop mail client and a managed container
  service fits the one-operator model.
- **No open tracking (GAP-28).** A tracking pixel reveals when a recipient read a message, so it is
  personal data processed without their knowledge. Apple Mail Privacy Protection preloads images, which
  makes open counts unreliable. Images in first contact also hurt deliverability. AgentMail itself fires
  opens once per message, not per recipient. Delivery events and replies are the signals Pylota Identity
  reports.
- **No calendar store (GAP-29).** A second calendar would compete with the consumer's calendar for the
  same truth: for Pylota, that is Kairos and its database constraint against double booking. Pylota Identity
  verifies and carries iCalendar in both directions, and the consumer owns state, SEQUENCE ordering and
  conflicts.
- **No general drive and no URL fetching (GAP-30, GAP-31).**
  - Fetching a caller-supplied URL from the Worker is a server-side request forgery surface. It also
    sends content that the caller's own SHA-256 never covered.
  - Uploads streamed through the Worker with a checksum cover attachments without either problem.
  - Agent Drive's other uses, storing an agent's own files, are not mail.

## 6. Corrections to the PRD's competitive table

The table in [PRD § 4](prd.md#competitive-landscape) was researched on 2026-10-09. By 2026-10-17 several of
its AgentMail cells were wrong:

- **Authentication verdicts.** "Spam labels" understates it. AgentMail exposes `authentication_results`,
  labels unauthenticated mail and hides it by default. Agent Armor (Labs) adds a prompt-injection verdict.
- **Team seats.** "–" is wrong. Plans include 2, 2 and 10 seats.
- **Runs in your own Cloudflare account.** "–" stays true for Cloudflare, but Enterprise offers a
  "BYO cloud deployment".
- **Erasure.** "Per message" should read "per message, thread and inbox, plus an optional 24 h expiry".
- **New rows needed.** Drafts and scheduled send, the event stream, prompt-injection screening,
  calendar invitations, SDK languages, and agent self-sign-up.

[ADR 0017](adr/0017-agentmail-parity.md) lists the replacement rows. The integration of this change into
`prd.md` applies them, with the read date 2026-10-17. The table is research, re-checked before any
external use; marketing copy may claim only what PRD § U1–U10 list ([PRD § 4](prd.md#unique-selling-propositions)).

## 7. Pylota agentic OS

Pylota reaches Pylota Identity Cloud as a partner. Each operator is a tenant, and Pylota's servers hold the
partner key ([Pylota integration spec](https://github.com/PILOTAAI/pylota), `docs/plans/pylota-mail-integration.md`).
The additions map onto Pylota as follows; each design page has a full **Pylota agentic OS** section.

| Pylota need | Pylota Identity feature |
|---|---|
| Mail written by the public-webchat coordinator, or by any untrusted path, must wait for an operator (Pylota's approval gate) | Drafts with `drafts:write` and no `messages:send`; the operator approves in Pylota's app; Pylota's server approves with its partner key and `on_behalf_of` (GAP-1) |
| Renter and public mail is read by the operator coordinator and 19 specialists | `screening.mode: hold` on operator identities; held mail joins Pylota's held-mail flow (spec § 8.6) (GAP-4) |
| Webhook monitoring and replay (spec § 8.4, § 8.5) | Reconciliation from a saved cursor on the event feed (GAP-2) |
| Pick-up and return appointments | Kairos produces iTIP; Pylota Identity sends it as iMIP from the operator's identity; verified replies and invitations go back to Kairos (GAP-5) |
| Pylota's browser agent receives one-time codes at third-party services | Short-lived identities (`retention.message_hours`) with the service sign-up ledger (GAP-17) |
| Specialists coordinate through labels | Batch updates and `message.labels_changed` (GAP-12, GAP-14) |
| The fleet-acquisition identity's seller outreach | Identity-scope lists (GAP-13) |
| Reputation per operator (spec § 5.5) | Deliverability metrics (GAP-8) |
| Pylota's client and router (spec W1, TypeScript) | The TypeScript SDK in place of a hand-written client (GAP-9) |

## 8. Follow-ups

- Re-read AgentMail's changelog before v1.0 ships, and record any new capability here with a decision.
- Add an Arabic reply-attribution sample to the conformance corpus before adding the pattern (GAP-21).
- Confirm the live behaviours the design pages leave to staging:
  - whether Cloudflare Email Routing honours a valid ARC chain (`live::arc::cloudflare_forward_behaviour`);
  - how Gmail, Outlook and Apple Calendar treat the iMIP forms sent (`live::calendar::*`);
  - how R2 handles a streamed upload with a checksum (`it::uploads::v_put_stream_contract`).
