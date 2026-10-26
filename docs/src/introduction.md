# Introduction

Pylota Identity is an email and identity service for AI agents. It gives every agent its own
**identity**: a stable mailbox with one or more addresses, authenticated sending, verified inbound
mail, threads, attachments with extracted text, triage and search. It is source available under the
Functional Source License (FSL-1.1-ALv2; each release becomes Apache-2.0 two years after it ships),
written entirely in Rust, and runs as one Cloudflare Worker on **your own Cloudflare account**. An identity
can also hold **phone numbers** and use credentials from a **vault** without seeing them. The TypeScript (and
JavaScript), Python and Go SDKs are separate clients, each built in its own repository from the same contract
and proven against the same conformance suite, and licensed under Apache-2.0
([ADR 0018](project/adr/0018-typescript-sdk-in-its-own-repository.md), [ADR 0022](project/adr/0022-sdk-languages.md)).

It was built for [Pylota](https://pylota.io), a platform for independent car-rental operators, where
each operator has four agents (bookings, inquiry, compliance and maintenance) that email customers,
garages, insurers and councils. Nothing in it is specific to car rental.

## What you get

| | |
|---|---|
| **An identity per agent** | Each agent has a mailbox, a primary address and aliases, a display name, a signature and an accountable human. Move the identity to another domain and its history and threads move with it |
| **Your own domains, at any DNS host** | Every identity has an address on the deployment's platform domain, which is on Cloudflare. Tenants can add their own domains without moving their DNS: six connection methods cover a domain on Cloudflare, a subdomain at any DNS host, and agents that answer as existing Google Workspace or Microsoft 365 addresses ([Custom domains](guides/custom-domains.md)) |
| **Three ways in** | A REST API (OpenAPI 3.1), an MCP server at `/mcp` and the `idvy` CLI. There is also a Rust SDK and, as P1 features, TypeScript and JavaScript, Python and Go SDKs ([SDKs](reference/sdks.md)) and agent skills for Claude Code, Codex and Cursor. Signed webhooks, an ordered event feed with durable consumers, and a resumable live stream tell your application what happened, all from one log with one event ID |
| **Inbound you can trust** | Every message is stored before it is acknowledged, parsed, and given an authentication verdict (SPF, DKIM, DMARC, ARC), a spam signal and trust metadata. Mail that fails authentication, looks like spam or carries an unsafe attachment is quarantined. An opt-in screening hold (P1) can keep mail that tries to steer agents away from them until it has a verdict. Calendar invitations arrive as verified, structured objects |
| **Triage** | Every inbound message gets a category, a needs-reply score, an urgency from 0 to 3, a short summary and risk flags such as a payment-change request or suspected prompt injection |
| **Approval before send** | An agent can write a draft that a person approves, bound to the exact text, before it leaves, and a draft can be scheduled. Drafts, scheduled sends and calendar replies all go through the one send pipeline ([Sending › Drafts, approval and scheduled send](guides/sending.md#drafts-approval-and-scheduled-send)) |
| **Safe retries** | Send, reply, reply-all and forward require an `Idempotency-Key`. A retry returns the first result instead of sending a second email. A send whose outcome cannot be known is marked `uncertain` and is never resent automatically |
| **Four search modes** | `keyword` (full-text plus exact references such as plates and invoice numbers), `semantic`, `hybrid` (the default) and `agentic`, which plans searches and returns an answer whose citations are checked by code |
| **Agents that can prove who they are** | Each identity can sign short-lived agent assertions that any service verifies against the identity's published key set. Where the operator and the workspace turn it on, agents can also sign their web requests with Web Bot Auth. Private keys never leave the Worker ([Using it from an agent](guides/agents.md#agent-assertions)) |
| **Email for the people behind the agents** | Usage alerts at 80% and 100% of an allowance, opt-in new-mail notifications that carry counts and never content, and a daily list of what needs a person ([Notifications by email](guides/receiving.md#notifications-by-email)) |
| **Your account, your data** | D1, Durable Objects, R2, Queues, Vectorize and Workers AI in the Cloudflare account you deploy to. D1, Durable Objects and R2 can be pinned to the EU |
| **Phone numbers for agents** (behind a flag) | A UK mobile or a US local number on an identity, registered in your business's own name, bought by an agent in one call inside a budget a person sets, after a one-time set-up per country. SMS in and out with consent, caps and US 10DLC handled; verification codes kept from the model when the vault fills them; calls through Vapi with an AI disclosure; and a person decides anything that costs money or lets a number go ([Phone numbers](guides/phone.md)) |
| **A vault agents use without seeing** (behind a flag) | Logins with their TOTP seeds, API keys and browser sessions kept beside the identity. An agent asks for a grant, a person approves, and a component you registered fills the form or injects the key, so no value reaches an agent, a model or a log. The vault is server-custody, not zero-knowledge, and says so ([Vault](guides/vault.md)) |
| **Privacy tools** | Retention policies, per workspace and per mailbox, erasure with receipts, legal holds and subject-access export |

P1 marks a feature that ships in v1.0 unless it threatens the release, in which case it moves to v1.1
([PRD § 5](project/prd.md#5-scope-and-priorities)).

## Who it is for

- **Integrators**: developers building an agent product who need to provision mailboxes per
  customer, send and receive reliably, react to events and erase data through a stable API.
- **Agents**: LLMs that use mail through MCP tools or an integrator's tool layer. Tools are small,
  reads fit a context window, sends are safe to retry and mail content is marked untrusted.
- **Self-hosters**: anyone who wants agent mail on infrastructure they control. A fresh deployment
  takes about 15 minutes of hands-on time ([NFR-OPS-1](project/prd.md#7-non-functional-requirements)).

## How the pieces fit

```text
                 ┌────────────────────── your Cloudflare account ──────────────────────┐
 sender ── SMTP ─▶ Email Routing ──▶ Pylota Identity Worker (Rust, WebAssembly)            │
                 │  (catch-all or      │                                               │
                 │   literal rules)    ├─ raw mail ─────────▶ R2                       │
                 │                     ├─ one mailbox per identity ─▶ Durable Objects  │
                 │                     │    (threads, messages, full-text index)       │
                 │                     ├─ tenants, keys, domains ─▶ D1                 │
                 │                     ├─ vectors (no text) ─▶ Vectorize               │
                 │                     ├─ triage, embeddings, agentic search ─▶ Workers AI
                 │                     └─ background work ─▶ Queues                    │
                 │                              │                                      │
 recipient ◀──── Email Sending ◀── send ────────┘                                      │
                 └──────────────────────────────┬──────────────────────────────────────┘
                                                │
          REST /v1 · MCP /mcp · idvy CLI ──────┤  you call it
          signed webhooks ◀─────────────────────┘  it calls you
```

1. Mail for every address on the platform domain, and on tenant domains whose DNS is on Cloudflare,
   reaches the Worker through Email Routing. The raw message goes to R2 before the sender gets an
   acknowledgement. Tenant domains whose DNS is elsewhere receive through Amazon SES, which hands each
   message to the same pipeline, or through the tenant's own mailbox, which forwards it.
2. The Worker parses and authenticates it, finds its thread and stores it in the identity's mailbox
   (one Durable Object per identity). It is searchable in the same transaction.
3. Triage and semantic indexing run in the background. Each step emits an event, which is delivered
   to your webhook endpoints, signed, and kept in the tenant's event log for the feed and the stream.
4. Your agent reads, searches and replies through the API, MCP or CLI. Outbound mail leaves through
   Cloudflare Email Sending, or, depending on how a tenant domain is connected, through Amazon SES or the
   tenant's own SMTP provider. Delivery events come back on a queue.

An identity can also hold phone numbers, each another address of the identity with its own SMS conversations
and call records, and items in the vault that its agents use without reading.

The [Architecture](project/architecture.md) page has the full picture.

## What it is not

Pylota Identity v1.0 deliberately does not include ([PRD §4](project/prd.md#4-goals-and-non-goals)):

- a mail client or webmail UI for humans. It is API-first;
- IMAP, POP3 or SMTP submission access;
- bulk marketing campaigns or list management. Marketing mail is supported one message at a time,
  with consent and one-click unsubscribe;
- open tracking;
- a calendar store: invitations are read and sent, and your own calendar keeps the state;
- general file storage: an upload only stages an attachment for a send;
- fetching attachments from a URL;
- OAuth 2.1 for the MCP endpoint (planned for v1.1; v1.0 uses API keys as bearer tokens);
- running anywhere other than Cloudflare Workers.

## Where to go next

- [Quickstart](quickstart.md): create an identity, send with an idempotency key, receive a reply,
  search, and connect an MCP client.
- [Deploy to Cloudflare](self-hosting.md): run your own deployment with `idvy setup` and
  `idvy deploy`.
- [Concepts](concepts.md): tenants, identities, addresses, domains, threads, triage, search, events,
  keys and idempotency in one place.
- Guides:
  [Using it from an agent](guides/agents.md) ·
  [Sending and safe retries](guides/sending.md) ·
  [Receiving, webhooks and quarantine](guides/receiving.md) ·
  [Search](guides/search.md) ·
  [Triage](guides/triage.md) ·
  [Custom domains](guides/custom-domains.md) ·
  [Security](guides/security.md) ·
  [Privacy, retention and erasure](guides/privacy.md) ·
  [Plans and billing](guides/plans.md)
- Reference:
  [REST API](reference/api.md) ·
  [Webhook events](reference/events.md) ·
  [Errors](reference/errors.md) ·
  [MCP server](reference/mcp.md) ·
  [CLI](reference/cli.md) ·
  [Configuration](reference/configuration.md) ·
  [Limits](reference/limits.md)
- Project documents, for contributors and coding agents:
  [Product requirements](project/prd.md) ·
  [Architecture](project/architecture.md) ·
  [Design](project/design/index.md) ·
  [Edge-case register](project/edge-cases.md) ·
  [Build plan](project/build-plan.md) ·
  [Decision records](project/adr/index.md)

Pylota Identity is pre-release. The design is complete and the implementation follows the
[build plan](project/build-plan.md). To report a vulnerability, see
[SECURITY.md](https://github.com/PILOTAAI/pylota-identity/blob/main/SECURITY.md).
