# 0017 What v1.0 adds, defers and refuses to match AgentMail

| | |
|---|---|
| Status | Proposed |
| Date | 2026-10-17 |
| Deciders | The owner (TREFT LTD) |
| Related | [AgentMail comparison and gap register](../agentmail-gap-analysis.md), [PRD § 4 and § 5](../prd.md), [ADR 0015](0015-solo-operator.md), [ADR 0016](0016-plan-items-changed-for-v1.md) |

## Context

Pylota is moving its mail off AgentMail, so Pylota Mail must cover everything Pylota uses there. The owner
asked on 2026-10-17 for AgentMail to be analysed as a product, for Pylota Mail to be compared with it, and
for every gap that Pylota Mail needs closed to be closed. The bar was to exceed production standards end
to end, handle every edge case, and work with Pylota's agentic OS.

The analysis read AgentMail's public OpenAPI document (180 operations), its AsyncAPI document, 143
documentation pages, its pricing, and two REA runtime captures of its site. All of it was read on
2026-10-17. The [gap register](../agentmail-gap-analysis.md#4-gap-register) lists 32 gaps. Before this
record, the PRD made drafts and scheduled send a v1.0 non-goal ("planned for v1.1"). It also listed
WebSocket push as P2. A coding agent reading the PRD would therefore build neither, and Pylota's approval
gate and its webhook reconciliation would have nothing to stand on.

## Decision

1. **v1.0 adds** the following. Each is specified on its own design page, with requirements, edge cases,
   tests and a milestone:

   | Feature | Page | Requirements | Edge rows | Milestone |
   |---|---|---|---|---|
   | Drafts, approval before send, scheduled send | [Drafts](../design/drafts.md) | FR-DRF | Q | M29 |
   | Event feed, durable consumers, live stream | [Event feed](../design/event-feed.md) | FR-EVT | R | M30 |
   | Screening hold for mail that steers agents | [Screening](../design/screening.md) | FR-SCR | T | M31 |
   | Calendar invitations (iMIP in and out, no calendar store) | [Calendar invitations](../design/calendar-invitations.md) | FR-CAL | V | M32 |
   | Uploads for attachments | [Uploads](../design/uploads.md) | FR-UPL | Q (Q65–Q108) | M33 |
   | Agent self-sign-up, behind `PM_AGENT_SIGNUP` (off by default) | [Agent self-sign-up](../design/agent-signup.md) | FR-ASU | X | M34 |
   | Deliverability metrics | [Metrics](../design/metrics.md) | FR-MET | Y | M35 |
   | TypeScript SDK, skills, MCP and CLI tooling for agents | [SDKs and skills](../design/sdk-and-skills.md) | FR-SDK-2 onward | Y | M36 |
   | Batch operations, mailbox-scope lists, label events, authentication domains, trusted ARC sealers, per-mailbox retention, webhook headers, zone-file export, error documentation links | [Parity additions](../design/parity-additions.md) | existing families | Z | M37 |

   The PRD non-goal "Scheduled send and server-side drafts (planned for v1.1)" is removed, and so is the
   P2 entry "WebSocket push".

2. **One send path.** A draft, a scheduled send and a calendar reply each become an outbound message
   through the existing send pipeline. That pipeline covers policy, holds, the thread lock, idempotency
   and `uncertain`. No feature adds a second way to send.

3. **One event log.** Webhooks, the pull feed, durable consumers and the stream all read the same
   per-tenant log, and every channel uses the same `evt_` ID as its deduplication key.

4. **Deferred to v1.1 or later.** Each item has a reason:
   - MCP OAuth 2.1 stays deferred as [ADR 0016](0016-plan-items-changed-for-v1.md) decided.
   - Public-key API credentials are deferred. Pylota's agents never hold a bearer secret, because Pylota's
     servers hold its keys. A design must name its signature scheme and replay window, which AgentMail
     does not document.
   - An OIDC-compatible "sign in as this agent" relying-party flow is deferred. v1.0 has signed
     assertions, a published JWKS and the service sign-up ledger.
   - A second Cloud jurisdiction is deferred; it is an owner decision with its own cost.
   - Pay-per-call through x402 or MPP is deferred. Plans are in GBP through Stripe; revisit when MPP is
     generally available to UK merchants.
   - Mailbox avatars (BIMI) are deferred to a guide.

5. **Refused.** The reasons are in [the gap register § 5](../agentmail-gap-analysis.md#5-deliberate-differences):
   - an IMAP or SMTP server;
   - open tracking;
   - a calendar store with an agenda and a recurrence engine;
   - general file storage;
   - fetching attachments from a URL.

6. **The competitive table is corrected.** [PRD § 4](../prd.md#competitive-landscape) gets the rows below,
   researched on 2026-10-17, in place of the AgentMail cells researched on 2026-10-09:

   | Capability | AgentMail (hosted) | Pylota Mail |
   |---|---|---|
   | Authentication verdicts and quarantine before an agent reads | `authentication_results`; unauthenticated and spam mail hidden by label; DMARC-failing mail dropped under an enforcing policy | Per-message verdicts; failing mail quarantined with a reason, never dropped (U5) |
   | Prompt-injection screening | Agent Armor (Labs, US only): advisory verdict; Hold mode withholds a notification but still lists the mail | Screening hold that hides the mail until a verdict, fail-closed by default, release by a person ([Screening](../design/screening.md)) |
   | Drafts and scheduled send | Yes; approval works only by withholding send permissions | Yes, with approval bound to the content and person-only approval on Cloud ([Drafts](../design/drafts.md)) |
   | Event delivery | Webhooks (8 attempts in ~27.5 h, auto-disabled after 5 days without notice), polling queue, WebSocket (key in the URL) | Webhooks (72 h, dead letters, replay), ordered pull feed, durable consumers, resumable stream ([Event feed](../design/event-feed.md)) |
   | Calendar | Per-inbox calendar store (Labs, US only) | Verified iMIP in and out; the consumer's calendar keeps state ([Calendar invitations](../design/calendar-invitations.md)) |
   | Team seats | 2 / 2 / 10 by plan | Enforced, with roles (U7) |
   | Erasure with receipts | Delete per message, thread and inbox; optional 24 h expiry | Message, thread, counterparty, identity, tenant, with receipts; per-mailbox retention (U6) |
   | Runs in your own cloud | "BYO cloud deployment" on Enterprise | Runs in your own Cloudflare account (U9) |
   | SDKs | Python, TypeScript | Rust, TypeScript ([SDKs and skills](../design/sdk-and-skills.md)) |
   | Agent self-sign-up | Yes; the human is optional | Yes, behind a flag; the accountable human is required (FR-IDN-2) |

## Integration

The nine pages were written as new files while PR #5 was still editing the shared pages, then folded into
those pages on 2026-10-18, on top of PR #5's final head (`97ba036`):

1. `SUMMARY.md` lists the nine design pages, the gap register and this record.
2. `prd.md`:
   - § 4 drops the two superseded non-goals, adds the refusals as non-goals with their reasons, and
     carries the competitive rows of Decision 6.
   - § 5's scope table gains the new features.
   - § 6 gains § 6.16–§ 6.22 (FR-DRF, FR-EVT, FR-SCR, FR-CAL, FR-UPL, FR-MET, FR-ASU). FR-SDK-2 onward
     join § 6.11, and the parity additions' requirements join their existing sections.
3. The reference pages carry the routes, schemas, errors, events, limits, policy fields, MCP tools and CLI
   commands: `api.md`, `openapi.yaml`, `errors.md`, `events.md`, `limits.md`, `configuration.md`,
   `mcp.md` and `cli.md`. `errors.md`, the `ErrorCode` enum and the routes' `x-error-codes` list the same
   161 codes.
4. The design pages each feature extends carry its storage, rules and operations.
5. `architecture.md` has the seventh Durable Object class (`TenantEvents`) and the `pm-screen` queue.
6. `edge-cases.md` has sections Q (drafts and uploads), R, T, V (calendar), X, Y and Z.
   - Calendar rows use V and uploads rows join Q as Q65–Q108. U would collide with the PRD's selling
     points U1–U10, and every other free letter already names something: M milestones, P priorities and
     S spikes.
7. `build-plan.md` has M29–M37 in its dependency graph, before M20.
8. `design/testing.md` has the new test modules and prefixes.

Still open: the Arabic attribution pattern for reply extraction waits for a conformance sample (GAP-21).
The phase timeline in the build plan has not been re-estimated for the nine milestones.

## Consequences

- v1.0 grows by nine milestones. The owner builds alone ([ADR 0015](0015-solo-operator.md)), so these
  milestones follow the core tracks rather than run in parallel with them. Their dependencies are in each
  milestone's text.
- Pylota's integration spec gains a list of additions; each design page names them under "Pylota agentic
  OS". The largest are:
  - drafts for every untrusted sender path;
  - screening hold on operator identities;
  - feed-based reconciliation;
  - Kairos invitations through Pylota Mail;
  - the TypeScript SDK in place of a hand-written client.
- New public claims are allowed only once their tests pass, as § U1–U10 require.
- AgentMail ships often. The gap register must be re-read against AgentMail's changelog before v1.0
  ships.

## Alternatives considered

- **Keep drafts and the stream in v1.1.** This is cheaper for v1.0. But Pylota's approval gate needs
  server-side drafts to hold an agent's mail for an operator, and its webhook monitoring needs a feed to
  reconcile from. Without them, Pylota would keep AgentMail for those paths, which defeats the move.
- **Copy AgentMail's surface one for one.** This would give AgentMail users the most familiar API. But it
  would import the weaknesses the gap register records: notification-only holds, silent drops, a key in
  the WebSocket URL, auto-disabled webhooks without notice, a second calendar store and URL fetching.
- **Build a calendar like AgentMail's.** Agents would get one product for mail and calendar. But Pylota
  already has Kairos, whose database constraint prevents double booking, and two calendars would
  disagree.
