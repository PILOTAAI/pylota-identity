# Parity additions

Binding for implementation. This page specifies nine smaller additions found when Pylota Mail was
compared with AgentMail on 2026-10-17. Each one extends a design that already exists, so each section
names the section it extends and gives its rules, API, storage, errors, events, limits and privacy; the
shared pages carry the result ([§18](#18-integration)). None of them adds a Durable Object class, a queue or a binding: they reuse the
`IdentityMailbox` object and its transactional outbox, the `JobRunner` erasure machinery, the policy
pipeline of [Outbound](outbound.md#policy-pipeline), the quarantine decision of
[Inbound](inbound.md#quarantine-decision), the Standard Webhooks delivery of [Webhooks](webhooks.md) and
the error envelope of [Errors](../../reference/errors.md).

| | |
|---|---|
| Requirements | FR-IN-13, FR-IN-14, FR-IN-15, FR-OUT-17, FR-WH-6, FR-WH-7, FR-PRV-7, FR-DOM-15, FR-API-3 (this page, [§13](#13-functional-requirements)); they extend FR-IN-4, FR-IN-5, FR-IN-10, FR-IN-11, FR-OUT-4, FR-OUT-13, FR-OUT-16, FR-WH-1, FR-WH-2, FR-WH-4, FR-PRV-2, FR-PRV-3, FR-PRV-4, FR-DOM-3, FR-API-2, FR-SRCH-13 |
| Edge cases | [Z1](../edge-cases.md)–[Z80](../edge-cases.md) ([§14](#14-edge-cases)) |
| Code | Batch: `crates/api-types/src/messages/batch.rs`, `crates/worker/src/handlers/messages_batch.rs`, `mailbox/read.rs` (`BatchGet`, `BatchUpdate`). Lists: `crates/core/src/lists.rs` (precedence, pure), `handlers/lists.rs`, `consumers/inbound.rs` (step 12), `handlers/send.rs` (step 6), `consumers/outbound.rs` (composition step 1). Label events: `webhooks/payloads.rs` (`labels_changed`), `mailbox/read.rs`, `consumers/webhooks.rs` (opt-in filter). Authentication detail and ARC: `crates/core/src/auth.rs` (`arc_rescue`, `trust_detail`), `crates/core/src/trust.rs`, `consumers/inbound.rs` (steps 7 and 12), `mailbox/ingest.rs`. Retention: `handlers/identities.rs`, `mailbox/expiry.rs` (new), `mailbox/erase.rs`, `mailbox/idempotency.rs`, `crons/intents.rs`. Webhook headers: `handlers/webhooks.rs`, `webhooks/{client.rs, headers.rs}` (new), `crates/core/src/webhook_headers.rs` (name and value rules, pure), `crates/core/src/sealed.rs`. Zone export: `crates/core/src/zonefile.rs` (pure), `handlers/domains.rs`. Docs link: `crates/api-types/src/errors.rs`, `crates/worker/src/errors.rs`, `xtask/src/docs_check.rs`. Console: `console/pages/{inboxes.rs, threads.rs, webhooks.rs, domains.rs}`. MCP: `mcp/tools.rs`. CLI: `crates/cli/src/commands/{messages.rs, lists.rs, identities.rs, webhooks.rs, domains.rs}` |
| Tables | D1: `sender_lists` gains `identity_id` (in the primary key); `identities` gains `message_hours`; `webhook_endpoints` gains `headers_sealed`, `header_names_json`, `headers_version`; `rpc_intents.op` gains `set_retention`. Mailbox: `messages.verdict` gains `arc_rescued`; new `meta` keys `retention_message_hours`, `retention_version`, `alarm:expiry`, `expiry_backoff`, `expiry:{YYYY-MM-DD}`; `auth_json` gains `spf.aligned` and `arc.{sealer, sealer_trusted, aar, rescue}`; `idempotency` rows of expired messages become stubs ([Data model](data-model.md)) |
| Contracts | Routes `POST …/messages/batch-get` (`messages:read`), `POST …/messages/batch-update` (`messages:write`), `GET\|PUT\|DELETE /v1/identities/{identity_id}/lists/{direction}/{kind}[/{entry}]` (`suppressions:manage`), `PATCH /v1/webhooks/{webhook_id}/headers` (`webhooks:manage`), `GET /v1/domains/{domain_id}/records?format=bind` (`domains:read`); identity field `retention.message_hours`; webhook fields `headers` (write-only) and `header_names`; message `trust` fields `spf_domain`, `spf_aligned`, `dkim_domains`, `dmarc_domain`, `dmarc_policy`, `dmarc_aligned_by`, `arc_sealer`; verdict value `arc_rescued`; error field `docs`; event `message.labels_changed` (opt-in); policy fields `inbound.trusted_arc_sealers` (platform-only) and `events.labels_changed` (free); variable `PM_DOCS_URL`; error codes `422 list_limit_reached`, `422 webhook_header_limit_reached`, `410 message_expired`, and new `details.reason` values of `400 invalid_request` and `400 header_not_allowed`; audit actions `message.batch_update`, `identity.retention_update`, `retention.expire`, `webhook.headers_update` |
| Limits | [§2.6](#26-limits), [§3.7](#37-limits), [§4.5](#45-limits-and-cost), [§7.11](#711-limits), [§8.4](#84-limits) |
| Built in | [M37](#16-milestone) (to be added to the [build plan](../build-plan.md), §18.10) |

## 1. The nine additions

| § | Addition | Requirement | Gap | Extends | What a client sees |
|---|---|---|---|---|---|
| 2 | Batch message operations | FR-IN-13 | [GAP-12](../agentmail-gap-analysis.md) | [Inbound › Read path and release](inbound.md#read-path-and-release) | `batch-get` and `batch-update` of up to 100 messages, atomic inside one mailbox, with an atomic claim |
| 3 | Identity-scope allow and block lists | FR-OUT-17 | [GAP-13](../agentmail-gap-analysis.md) | [Outbound › Recipient filters](outbound.md#recipient-filters), [Inbound › Quarantine decision](inbound.md#quarantine-decision) | Lists per identity; the most specific scope wins |
| 4 | Label change events | FR-WH-6 | [GAP-14](../agentmail-gap-analysis.md) | [Webhooks › Event envelope and payloads](webhooks.md#event-envelope-and-payloads) | `message.labels_changed`, opt-in, one event per request |
| 5 | Authentication detail on messages | FR-IN-14 | [GAP-15](../agentmail-gap-analysis.md) | [Inbound › Authentication verdict](inbound.md#authentication-verdict) | SPF, DKIM and DMARC domains with alignment, and the ARC sealer, in `trust` |
| 6 | Trusted ARC sealers | FR-IN-15 | [GAP-16](../agentmail-gap-analysis.md) | [Inbound › Authentication verdict](inbound.md#authentication-verdict) | Forwarded mail that a trusted forwarder authenticated is `arc_rescued`, not quarantined |
| 7 | Per-identity message retention | FR-PRV-7 | [GAP-17](../agentmail-gap-analysis.md) | [Privacy › Retention](privacy.md#5-retention) | `retention.message_hours`: each message erased that many hours after it is stored |
| 8 | Webhook delivery headers | FR-WH-7 | [GAP-18](../agentmail-gap-analysis.md) | [Webhooks › Signing](webhooks.md#signing-standard-webhooks) | Up to 10 static, write-only headers on every delivery |
| 9 | Zone-file export | FR-DOM-15 | [GAP-19](../agentmail-gap-analysis.md) | [REST API › Records](../../reference/api.md#get-v1domainsdomain_idrecords--domainsread) | `?format=bind`: the required records as an RFC 1035 zone fragment |
| 10 | A documentation link in errors | FR-API-3 | [GAP-20](../agentmail-gap-analysis.md) | [Errors](../../reference/errors.md) | `docs`: a link to the error's row on the published errors page |

Rules shared by all nine:

- **Scope and visibility are unchanged.** Every route takes its scope from the key
  ([Security § 5.2](security.md#52-order-of-checks)). An ID the key cannot reach, or a message it may not
  see ([Security § 5.3](security.md#53-cross-level-read-access)), is reported exactly as a missing one, and
  the answer never says which of the two it was.
- **Inbound mail is never refused or dropped** by anything on this page (FR-BILL-8, NFR-REL-1). The ARC
  rule only moves mail out of quarantine; it never moves mail in.
- **States.** A route on this page behaves like the route it extends for every tenant and identity state:
  a suspended partner (`403 partner_suspended`), a tenant being erased and a `deleting` identity (the
  resource's `404`), the `read_only` switch (`503 unavailable` for every request that is not a `GET` from a
  non-platform key, so `batch-get` too) and a frozen deployment
  (`503 unavailable`) ([Security § 5.2](security.md#52-order-of-checks), step 4).
- **Test tenants** use every addition as live tenants do. Nothing here leaves the deployment.

## 2. Batch message operations (FR-IN-13)

Extends [Inbound › Read path and release](inbound.md#read-path-and-release) ("Labels and read state"),
[REST API › Threads and messages](../../reference/api.md#threads-and-messages) and the MCP tool
`mail_update_labels` ([MCP § 4.3](mcp.md#43-definitions)). Today labels and read state change one message
or one thread per request; an agent that triages a page of mail makes 25 to 100 requests, and two agents
cannot claim a message without a race.

### 2.1 `POST /v1/identities/{identity_id}/messages/batch-get` — `messages:read`

Every key level. `x-idempotency: none`: it returns mail and changes nothing, so, like search, it is never
recorded ([REST API › Idempotency](../../reference/api.md#idempotency)).

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

- `data` holds [Message objects](../../reference/api.md#message-object) in request order, built as
  `GET …/messages/{message_id}` builds them, without `text`, `html` and `headers` (there is no `include`),
  and with `extracted_text` cut at a UTF-8 character boundary to `extracted_text_bytes` (0–16,384,
  default 2,048), with `extracted_text_truncated: true` when it was cut.
- `not_found` lists, in request order, every ID that is not a message of this identity that the key may
  see: missing, erased, of another identity, or `quarantined`, `screening`, `hidden` or `throttled` without
  `quarantine:review`. The answer is the same for each of them.
- `remaining` and `truncated`: the serialised response body never exceeds 1,048,576 bytes. When the next
  message would pass that budget, it and every later found message are listed in `remaining`, in request
  order, and `truncated` is `true`; the client asks again for them. The first found message is always in
  `data` (one message without `text` and `html` is far below the budget).

### 2.2 `POST /v1/identities/{identity_id}/messages/batch-update` — `messages:write`

Every key level. `Idempotency-Key` optional, as on every other `POST`.

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
  archived flag of each **thread** that holds a listed message: archiving is a thread property in this
  design ([Inbound › Read path and release](inbound.md#read-path-and-release)), so FR-IN-10's per-message
  archiving is met by archiving the message's thread, as the single-thread `PATCH` does.
- `require_absent` (at most 10 labels) makes the change conditional per message: a message that carries
  any of these labels when the transaction runs is not changed and is listed in `conflicts`. It is the
  atomic claim that lets several agents share one inbox: `require_absent: ["claimed"]` with
  `labels_add: ["claimed"]` lets exactly one of two racing callers take a message.
- `updated`: messages this request changed (a label, the read state, or its thread's archived flag).
  `unchanged`: messages found and allowed whose state already matched. `conflicts`: messages skipped by
  `require_absent`. `not_found`: as for `batch-get`. Every listed ID appears in exactly one of the four.
- `threads_archived` and `threads_unarchived` count the threads whose flag this request changed.

### 2.3 Rules, in order

1. **Scope.** Authentication, permission and the identity path are checked once for the whole request
   ([Security § 5.2](security.md#52-order-of-checks)). An identity outside the key's scope, or `deleting`,
   is `404 identity_not_found` and nothing else is read. The system identity is `404` to every key but a
   platform key, as on every mailbox route.
2. **Body.** `message_ids` holds 1–100 IDs matching `^msg_[0-9A-HJKMNP-TV-Z]{26}$`, otherwise
   `400 invalid_request` with `details.errors[0].path = "message_ids"` or `"message_ids[{i}]"`. A repeated
   ID is processed once, at its first position. For `batch-update`: at least one of `labels_add`,
   `labels_remove`, `read` and `archived` (else `400 invalid_request`, path `labels_add`); a label in both
   `labels_add` and `labels_remove` is `400 invalid_request` (path `labels_remove`); `require_absent` needs
   at least one of `labels_add` and `labels_remove`.
3. **Visibility**, inside the mailbox: an ID is found when a message with that ID exists in this mailbox,
   is not in `erased_ids`, and is visible to the key (status not `quarantined`, `screening`, `hidden` or `throttled`, or
   the key holds `quarantine:review`, passed as `review` exactly as for `UpdateMessage`). Everything else
   goes to `not_found`.
4. **`batch-get`** reads the found rows, their attachments, labels and refs in one read, builds the
   objects in request order and applies the byte budget of [§2.1](#21-post-v1identitiesidentity_idmessagesbatch-get--messagesread).
5. **`batch-update`** runs in **one** `transaction_sync`:
   1. for each found message, in request order: when it carries a label of `require_absent`, it goes to
      `conflicts` and nothing else happens to it;
   2. for each other found message, its label set after the change is
      `before ∪ labels_add − labels_remove`; if any would exceed 64 labels, the transaction is abandoned and
      the request gets `400 invalid_request` with `details.reason = "label_limit"` and `details.message_ids`
      (the messages that would pass the limit). Nothing changes: the batch is atomic;
   3. labels: `INSERT OR IGNORE INTO labels (message_rowid, label) SELECT r.value, l.value FROM json_each(?1) r, json_each(?2) l`
      and `DELETE FROM labels WHERE message_rowid IN (SELECT value FROM json_each(?1)) AND label IN (SELECT value FROM json_each(?3))`,
      so no statement binds more than three parameters whatever the counts;
   4. `read`: `UPDATE messages SET read = ?` for those rows, then `unread_count` is recomputed for each
      affected thread (visible inbound messages with `read = 0`);
   5. `archived`: the distinct threads of those messages get `threads.archived = ?`; a thread already in
      that state is not counted;
   6. the outbox gets `message.labels_changed` when the handler passed `emit_label_events`
      ([§4](#4-label-change-events-fr-wh-6)) and at least one visible message's labels changed.
6. **Audit.** After the commit, a `batch-update` that updated at least one message writes one audit row,
   `message.batch_update` (`target_type: "identity"`, `details`: `requested`, `updated`, `unchanged`,
   `conflicts`, `not_found` as counts, `labels_add`, `labels_remove`, `read`, `archived`), the way the
   release handler writes `quarantine.release` after its commit. A `batch-get` by a platform or partner key
   writes one `mail.read` row before the response (`target_type: "identity"`, `details.route`,
   `details.messages` = the count returned), and fails with `503 unavailable` if the row cannot be
   written, as every content read by those keys does
   ([REST API › Audit](../../reference/api.md#get-v1audit-events--auditread)).

Single-message `PATCH …/messages/{message_id}` and `PATCH …/threads/{thread_id}` keep their behaviour.
`require_absent` exists only on `batch-update`; a claim on one message is a batch of one.

### 2.4 Concurrency, idempotency and failures

- **Concurrency.** A mailbox runs one transaction at a time, and `require_absent` is evaluated inside the
  transaction, so two concurrent claims of one message give one `updated` and one `conflicts`, never two
  owners. A batch never sees half of another batch.
- **Idempotency.** With an `Idempotency-Key`, the response (IDs and counts only, never mail content) is
  recorded in `idempotency_records` and replayed with `Idempotent-Replayed: true`; a replay writes no
  label, no event and no audit row. The same key with another body gets `409 idempotency_conflict`; while
  the first is running, `409 request_in_progress`. Without a key, a retry after a lost response applies the
  same set operation again and changes nothing more, but a caller's own earlier claim then shows as a
  conflict; agents that claim should send a key.
- **Failures.** The mailbox unavailable or the deadline passed before the transaction: `503 unavailable` or
  `504 timeout`, and nothing changed. A crash after the commit loses only the response; the retry above
  applies. A failed audit write after the commit is logged (`audit_write_failed`) and the response is still
  `200`, as for `quarantine.release`.

### 2.5 Storage

No schema change. `ReadRequest` ([Inbound › Read path and release](inbound.md#read-path-and-release))
gains two variants:

```rust
// crates/worker/src/mailbox/read.rs
BatchGet    { message_ids: Vec<String>, extracted_text_bytes: u32, review: bool },
BatchUpdate { message_ids: Vec<String>, labels_add: Vec<String>, labels_remove: Vec<String>,
              read: Option<bool>, archived: Option<bool>, require_absent: Vec<String>, review: bool,
              label_events: Option<LabelEventContext> },   // None when events.labels_changed is off
pub struct LabelEventContext { pub actor: Actor, pub request_id: String, pub source: LabelSource }
```

### 2.6 Limits

| Limit | Value | Source |
|---|---|---|
| IDs per batch | 100 | Pylota Mail (AgentMail allows 50 per `batch_update`) |
| `extracted_text_bytes` | 0–16,384, default 2,048 | Pylota Mail |
| `batch-get` response | 1,048,576 bytes, then `remaining` | Pylota Mail |
| Labels per request | 64 in `labels_add`, 64 in `labels_remove`, 10 in `require_absent` | Pylota Mail |
| Rate limit | One request against the key, tenant and partner buckets (`RL_API`), whatever the batch size | Pylota Mail |

### 2.7 Interfaces and privacy

- **MCP.** `mail_update_labels` gains `message_ids` (1–100) and `require_absent`; exactly one of
  `message_id`, `thread_id` and `message_ids` is required, checked by the handler (the input schema stays a
  plain object, [MCP § 4.3](mcp.md#43-definitions)), and `require_absent` only with `message_ids`. With
  `message_ids` it calls `batch-update` and returns its response object. There is no batch-get tool: a
  model reads one or two messages at a time, and 100 messages would exceed the tool's 96 KB result budget.
- **CLI.** `pmail messages label <message-id>… --identity <identity> [--add <label>]… [--remove <label>]…
  [--read | --unread] [--archive | --unarchive] [--require-absent <label>]…` calls `batch-update` when given
  more than one ID, in requests of 100 (atomic per request, not across requests; it says so when it splits);
  `pmail messages get <message-id>…` calls `batch-get` for more than one ID.
- **Console.** No change.
- **Privacy.** No new store. A `batch-get` response carries the same mail as the single `GET`; an
  idempotency record of `batch-update` holds IDs and counts only.

## 3. Identity-scope allow and block lists (FR-OUT-17)

Extends [Outbound › Recipient filters](outbound.md#recipient-filters) (policy step 16), the
[pm-inbound consumer](inbound.md#the-pm-inbound-consumer) (step 12), [Inbound › Quarantine decision](inbound.md#quarantine-decision)
(rules 1, 4a and 6), [Inbound › Trust signals](inbound.md#trust-signals) (`known_sender`),
[Inbound › Inbound volume caps](inbound.md#inbound-volume-caps-d5-d13),
[REST API › Suppressions and lists](../../reference/api.md#suppressions-and-lists--suppressionsmanage),
[Data model](data-model.md#1-d1-control-plane) (`sender_lists`) and [Privacy § 7.3](privacy.md#73-allow-and-block-lists).
Today lists exist only per tenant (FR-OUT-16), so a send-allow entry made for one identity's first contact
makes the address known to every identity of the tenant, and a receive-block made for one inbox hides the
sender from all of them.

### 3.1 Scopes and precedence

An entry belongs to the **tenant** scope (as today) or to one **identity** of the tenant. For a message of
identity `I` in tenant `T`, the decision for one direction and one address is:

```text
 identity entries of I that match the address or its @domain ── any ──▶ block if any is block, else allow   (scope identity)
        │ none
        ▼
 tenant entries of T that match the address or its @domain ──── any ──▶ block if any is block, else allow   (scope tenant)
        │ none
        ▼
 no entry
```

`core::lists::decide(rows: &[ListRow]) -> Option<ListDecision { kind, scope, entry }>` (pure) implements
it:

1. **The most specific scope wins.** Any identity-scope match decides, and tenant entries are then not
   read for that decision. An identity allow therefore overrides a tenant block of the same address or
   domain, and an identity block overrides a tenant allow.
2. **Inside one scope, block wins over allow**, for address and domain entries alike, as tenant lists do
   today (consumer step 12: "block wins over allow").
3. **Matching** is the existing matching of tenant lists, unchanged and shared by both scopes
   (`core::lists::candidates(address)`: the address itself and `@` followed by its domain as the existing
   inbound and outbound queries form it).

### 3.2 Where the decision is used

| Rule | Today reads | Now reads |
|---|---|---|
| Quarantine rule 1, `hidden` / `blocked_sender` | a tenant receive-block entry | the effective receive decision is `block` |
| Quarantine rules 4a and 6 exemptions; the volume-cap exemption of consumer step 13; the receive-allow part of `known_sender` | a tenant receive-allow entry | the effective receive decision is `allow` |
| Recipient filter "send-block" | a tenant send-block entry | the effective send decision is `block`; the delivery's `smtp_response` is `policy: send_block` for a tenant entry, as today, and `policy: send_block (identity)` for an identity entry |
| `policy.send_allowlist_only` | a tenant send-allow entry | the effective send decision is `allow` |
| Known recipient, rule 2 ([Outbound › Recipient filters](outbound.md#recipient-filters)) | a tenant send-allow entry | the effective send decision is `allow` |
| Dry run (`?dry_run=true`) | the rule | the rule, unchanged (`reason: "send_block"`), and a new recipient field `scope` (`identity` or `tenant`) beside it; `422 recipient_blocked` gains `details.scope` |

**How it composes with `require_known_recipient`.** The recipient filters run in this order, and the first
that applies decides: (1) an unexpired suppression → `suppressed` (`policy: suppression (…)`); (2) the
effective send decision is `block` → `suppressed` (`policy: send_block (…)`), even for a known recipient
and for the authenticated sender being answered; (3) `policy.send_allowlist_only` and the effective send
decision is not `allow` → `suppressed` (`policy: not_on_allowlist`); (4) the identity's
`require_known_recipient` and the recipient is not known → `suppressed` (`policy: unknown_recipient`).
A recipient is known by the three rules of [Outbound › Recipient filters](outbound.md#recipient-filters),
rule 2 now meaning "the effective send decision for this identity is `allow`". So an identity-scope
send-allow entry makes an address known to that identity only, and the tenant's other identities still
suppress it.

### 3.3 What a list entry never overrides

A list is configuration, not policy, so it has no ceilings, and it can never lift a limit the platform or
a partner set or a guarantee the service gives. Whatever its scope, an allow entry never overrides:

- a suppression (FR-OUT-4; an objection or a bounce must be respected, [Privacy § 7.2](privacy.md#72-suppressions-after-erasure-i7));
- `test_mode_recipient` (FR-OUT-12), the loop guard, the per-identity and tenant daily caps, the
  new-workspace ramp, the shared-domain breaker, `sending_paused`, a suspended tenant or partner, or the
  marketing transport rule;
- quarantine rules 3 and 3a (authentication), 4 (risky attachments) and 5 (unsolicited codes); only rules
  4a and 6 are exempt, as today;
- the per-sender inbound cap (only the unauthenticated and tenant caps exempt a receive-allowed sender, as
  today).

### 3.4 Routes

`GET /v1/identities/{identity_id}/lists/{direction}/{kind}` lists the identity's entries, and
`GET|PUT|DELETE /v1/identities/{identity_id}/lists/{direction}/{kind}/{entry}` reads, adds or updates, and
removes one, all with `suppressions:manage`. `direction`, `kind`, `entry` and the bodies are those of the
tenant routes. Platform, partner and tenant keys only: identity keys cannot hold `suppressions:manage`, so an
agent can never change its own lists, and an identity key gets `403 permission_denied`
(`details.required: "suppressions:manage"`).

- `PUT` adds the entry, or updates its `note` when it exists, and answers `200` with it. `DELETE` answers
  `204`, or `404 list_entry_not_found` for an entry that does not exist.
- The [list entry](../../reference/api.md#suppressions-and-lists--suppressionsmanage) gains `scope`
  (`tenant` or `identity`) and `identity_id` (`null` for tenant scope). The tenant routes are unchanged and
  read and write tenant-scope entries only.
- The system identity: `404 identity_not_found` for every key but a platform key, which gets
  `400 invalid_request` with `details.reason = "system_identity"` (its sign-in mail must never be filtered
  by a list).

```json
{ "scope": "identity", "identity_id": "idn_01JA9K…", "direction": "send", "kind": "allow",
  "entry": "seller.4471@marketplace.example", "note": "operator_approved_seller acq_01JA…",
  "created_at": "2026-10-17T10:05:00Z" }
```

### 3.5 Storage

`sender_lists` gains the identity in its primary key. `''` stands for the tenant scope, because SQLite
treats `NULL` values in a composite primary key as distinct, which would let two equal tenant entries in.

```sql
CREATE TABLE sender_lists (
  tenant_id   TEXT NOT NULL REFERENCES tenants(id),
  identity_id TEXT NOT NULL DEFAULT '',                    -- '' = tenant scope; else an idn_ of this tenant
  direction   TEXT NOT NULL CHECK (direction IN ('receive','send')),
  kind        TEXT NOT NULL CHECK (kind IN ('allow','block')),
  entry       TEXT NOT NULL,                               -- user@example.com or @example.com
  note        TEXT,
  created_at  INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, identity_id, direction, kind, entry)
);
CREATE INDEX sender_lists_identity ON sender_lists(identity_id, direction) WHERE identity_id <> '';
```

The two readers bind the identity and read both scopes in one statement:

```sql
-- consumer step 12 (receive) and handler step 6 / composition step 1 (send): ?2 the direction,
-- ?3 the identity, ?4 the candidates (address and @domain) of each address as one JSON array
SELECT identity_id, kind, entry FROM sender_lists
WHERE tenant_id = ?1 AND direction = ?2 AND identity_id IN ('', ?3)
  AND entry IN (SELECT value FROM json_each(?4));
```

`PUT` runs one D1 batch: an `UPDATE … SET note = ?` of the existing row, then a guarded insert that runs
only when the row does not exist and the scope and tenant are under their limits:

```sql
INSERT INTO sender_lists (tenant_id, identity_id, direction, kind, entry, note, created_at)
SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7
WHERE NOT EXISTS (SELECT 1 FROM sender_lists WHERE tenant_id = ?1 AND identity_id = ?2
                  AND direction = ?3 AND kind = ?4 AND entry = ?5)
  AND (SELECT COUNT(*) FROM sender_lists WHERE tenant_id = ?1 AND identity_id = ?2) < 10000
  AND (SELECT COUNT(*) FROM sender_lists WHERE tenant_id = ?1) < 50000;
```

When neither statement changed a row, the request gets `422 list_limit_reached` with `details.limit` and
`details.scope` (`identity`, `tenant` or `total`). Two requests racing for the last place both run inside
D1's serialised batches, so only one passes the count.

**Races with mail in flight.** The inbound decision uses the lists read at consumer step 12; an entry
written after that and before the mailbox commits does not apply to that message. The outbound decision is
taken at submit and again at composition step 1, which re-reads both scopes, so an identity send-block
written after a submit still suppresses that recipient before the transport (as a tenant send-block does
today, [G4](../edge-cases.md)).

### 3.6 Reply lists: not added

AgentMail has a third direction, `reply`, whose lists alone are checked for replies, because its
receive-allow list, once it has an entry, blocks every other sender, replies included. Pylota Mail does not
add a `reply` direction, because its existing rules already cover replies:

- a receive-allow entry is an **exemption** (rules 4a and 6, the unsolicited caps), never an allow-only mode,
  so no allow list can block a reply;
- a receive-block entry blocks a sender's mail whatever it answers, which is what blocking means;
- for outbound replies, the send lists apply to `reply`, `reply-all` and `forward` like any send, and the
  reply-target rule of `require_known_recipient` already lets an identity answer the authenticated sender of
  a received message;
- a reply's authenticity is decided by threading (`thread_join_unverified`, [C9](../edge-cases.md)), not by
  a list.

### 3.7 Limits

| Limit | Value | Source |
|---|---|---|
| Entries per scope (the tenant scope, and each identity) | 10,000 | Pylota Mail |
| Entries per tenant, every scope together | 50,000 | Pylota Mail. At about 200 bytes a row, 10 MB of the 10 GB D1 database; the D1 size alerts ([Limits › Storage](../../reference/limits.md#storage)) still apply |

The limits also apply to tenant-scope entries, which had none before; `422 list_limit_reached` is new.

### 3.8 Privacy, events and interfaces

- **Privacy.** Identity-scope entries hold clear addresses or domains the tenant chose, like tenant entries.
  Identity-scope erasure and identity deletion delete the identity's entries (step `scrub_control_plane`,
  [Privacy § 6.5](privacy.md#65-identity-scope-fr-idn-4)), tenant erasure deletes every entry (step
  `delete_d1_rows`, already listing `sender_lists`), and both receipts count them in the new field
  `list_entries_deleted`. Counterparty erasure leaves entries of both scopes, for the reason of
  [Privacy § 7.3](privacy.md#73-allow-and-block-lists).
- **Events and audit.** None, as for tenant lists.
- **Console.** The inbox page (`/console/inboxes/{idn}`) lists the identity's entries read-only, with the
  scope, so a person can see why mail is hidden or a recipient suppressed. Entries are managed through the
  API and the CLI, as tenant entries are.
- **CLI.** `pmail lists list|add|remove <direction> <kind> [<entry>] --identity <identity>`; `--tenant`
  keeps the tenant scope.
- **MCP.** None: agents never hold `suppressions:manage`.

## 4. Label change events (FR-WH-6)

Extends [Webhooks › Event envelope and payloads](webhooks.md#event-envelope-and-payloads) and
[Endpoint resolution and filters](webhooks.md#endpoint-resolution-and-filters),
[Webhook events › Messages](../../reference/events.md#messages), and
[Inbound › Read path and release](inbound.md#read-path-and-release), whose label changes "emit no event"
today. Several agents that share an inbox can coordinate through labels (one claims a message, another
escalates it) only if they learn of each other's changes without polling.

### 4.1 Opt-in, twice

`message.labels_changed` is the one **opt-in** event type of v1, for two reasons: label changes can be as
frequent as reads, and every event costs an outbox row in the mailbox and an `event_index` row in the D1
database that the whole deployment shares ([Data model › Notes](data-model.md#notes), "Write volume").

1. **Emission** needs the tenant's policy field `events.labels_changed: true` (a free field, default
   `false`; [Configuration](../../reference/configuration.md#tenant-policy)). While it is `false` no event is written anywhere: not to the
   outbox, `event_index` or the event feed ([Event feed](event-feed.md)).
2. **Delivery** needs the endpoint to name the type in `events`. `"*"` never includes it: `*` means every
   type that is not opt-in, including types added later. The list of opt-in types is one constant,
   `OPT_IN_EVENT_TYPES` in `crates/api-types/src/events/mod.rs`, read by the `Fanout` filter and by replay.

Creating or changing an endpoint that names it needs `messages:read`, because it is a `message.*` type
([Webhooks › Who may subscribe to mail events](webhooks.md#who-may-subscribe-to-mail-events-j29)).

### 4.2 Payload

```json
{
  "id": "evt_01JA9Q…", "type": "message.labels_changed", "api_version": "2026-10-01",
  "occurred_at": "2026-10-17T10:05:01.220Z", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…",
  "sequence": 2210,
  "data": {
    "messages": [ { "message_id": "msg_01JA4B1G7PW7X2M5N6P8R0T1YC", "thread_id": "thr_01JA4B…" } ],
    "added": ["claimed", "claimed-by-bookings"],
    "removed": ["new"],
    "actor": { "kind": "key", "key_id": "key_01J9…", "user_id": null },
    "source": "batch",
    "request_id": "req_01JA9Q…",
    "part": 1, "parts": 1
  }
}
```

| Field | Meaning |
|---|---|
| `messages` | The visible messages whose label set this request changed, at most 100, in request order (for a thread, oldest first) |
| `added`, `removed` | The requested labels, normalised, that changed at least one listed message. After the change every listed message carries every label in `added` and none in `removed` |
| `actor` | `kind: "key"` with `key_id`, or `kind: "person"` with `user_id` for a change made in the console; the other ID is `null` |
| `source` | `message` (`PATCH …/messages/{id}`), `thread` (`PATCH …/threads/{id}`) or `batch` (`batch-update`); an MCP call has the source of the route it calls |
| `request_id` | The request that made the change, so consumers can group the parts of one change |
| `part`, `parts` | 1 and 1, except for a thread change over more than 100 messages ([§4.3](#43-rules)) |

The payload holds IDs and labels only: no subject, address or text, so it is thinner than every other
`message.*` event (FR-WH-4).

### 4.3 Rules

1. The event is appended to the outbox in the transaction that changed the labels, with the identity's next
   `sequence`, so it is ordered with the identity's other events (C5) and is never emitted for a change that
   did not commit.
2. One request emits one event, or `ceil(n / 100)` events when a thread `PATCH` changes more than 100
   visible messages, each with the same `request_id`, `part` from 1 and the same `parts`.
3. Nothing is emitted when: the policy field is off; no label changed (only `read` or `archived`, or every
   label already matched); the change was a `require_absent` conflict; the labels were set when the message
   was created (the send's `labels`, a draft's labels carried to its message); labels disappeared because a
   message was erased, expired ([§7](#7-per-identity-message-retention-fr-prv-7)) or deleted; a release or a
   re-parse ran. These are system effects, not label changes: state the service sets never emits this
   event.
4. A message that is not visible (`quarantined`, `hidden`, `throttled`, or [Screening](screening.md)'s
   `screening`) is left out of `messages`, even when a key with `quarantine:review` changed its labels, so an
   endpoint never learns of mail it would not receive as `message.received`. When no visible message is
   left, no event is emitted.
5. An idempotent replay of `batch-update` emits nothing.
6. The `Fanout` consumer delivers it only to endpoints that name it (rule 2 of [§4.1](#41-opt-in-twice)) and
   match its identity filter; it is never handed to the `Notifier`.
7. Turning the policy field off stops new events; events already in the outbox are still delivered.
8. **Erasure and expiry.** `EraseRows` step 3 ([Privacy § 6.2](privacy.md#62-what-eraserows-deletes)) finds outbox
   rows by `data.message_id` or `data.message.id`, which a label event does not have. It also matches
   `message.labels_changed` rows whose `data.messages` lists the message: the entry is removed with
   `json_remove`, the row is deleted when no entry is left, and the count of deleted rows goes to
   `events_deleted`. A label event already delivered names only IDs and labels, so no content of the erased
   message remains in it.

The handler reads the effective policy with the identity in step 4 of the order of checks (one more
column of the tenant row it already reads) and passes `label_events: Some(LabelEventContext)` to the
mailbox only when the field is on.

### 4.4 Consumers

The event is at-least-once like every event; deduplicate on its `id`, which is also its ID in the event
feed ([Event feed](event-feed.md)). It says what changed, not who may act: two agents that both see
`claimed` added should not both act, and the safe way to claim is `require_absent`
([§2.2](#22-post-v1identitiesidentity_idmessagesbatch-update--messageswrite)), whose result says who won.
A consumer that writes labels itself should ignore events whose `actor.key_id` is its own key, so it does
not react to its own changes.

### 4.5 Limits and cost

| Limit | Value |
|---|---|
| Messages per event | 100 (a thread change over more is split into parts) |
| Labels per event | 64 in `added`, 64 in `removed` |
| Events per request | 1, or `ceil(n / 100)` for a thread |

An event is about 70 bytes per listed message plus the labels: at most about 16 KB.

## 5. Authentication detail on messages (FR-IN-14)

Extends [Inbound › Authentication verdict](inbound.md#authentication-verdict) (step 8, `auth_json`, and its
last paragraph, the API summary), the [Message object](../../reference/api.md#message-object) and the
`trust` object of `message.received` and `message.quarantined`. Today `trust` gives one result per method;
an integrator that wants to know *which* domain passed (an invoice signed by the supplier's own domain or by
its mailing provider) has to read the raw headers.

### 5.1 New `trust` fields

Every field comes from `auth_json`, which ingest already writes; nothing is computed again at read time.

| Field | Type | From | Meaning |
|---|---|---|---|
| `spf_domain` | string or `null` | `auth_json.spf.domain` | The domain SPF checked: `smtp.mailfrom` of the trusted `Authentication-Results` header (or the envelope sender's domain), or SES's `mail.source` for the SES source. `null` when SPF was not checked (no trusted header) |
| `spf_aligned` | boolean | `auth_json.spf.aligned` (new, step 6) | SPF passed and `spf_domain` aligns with the `From` domain under the record's `aspf` |
| `dkim_domains` | array, at most 10 | `auth_json.dkim[]` | One entry per `DKIM-Signature` evaluated, in header order: `{ "domain": d=, "selector": s=, "result", "aligned" }` |
| `dmarc_domain` | string or `null` | the `From` domain | The domain DMARC was evaluated for (`null` without a `From` address) |
| `dmarc_policy` | `none`, `quarantine`, `reject` or `null` | `auth_json.dmarc.policy` | The policy in force (`sp` for a subdomain when the record came from the organisational domain); `null` without a record |
| `dmarc_aligned_by` | `dkim`, `spf` or `null` | `auth_json.dmarc.aligned_by` | Which method aligned, as recorded today |
| `arc_sealer` | string or `null` | `auth_json.arc.sealer` (new) | The `d=` of the newest `ARC-Seal` when the ARC chain validated (`arc: "pass"`), trusted or not; `null` otherwise |

```json
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
}
```

- Domains are A-labels in lower case, as `auth_json` stores them.
- The fields are present wherever `trust` is: inbound messages, `message.received` and
  `message.quarantined`. A loopback message ([L3](../edge-cases.md)) has `null` domains and an empty
  `dkim_domains`, because it was not authenticated over the network. Search hits keep their short trust
  (`verdict`, `known_sender`, `quarantined`).
- Messages stored before this change have every field except `arc_sealer` (`null`) and `spf_aligned`, which
  is then `true` only when `dmarc_aligned_by` is `spf`, the one case the old `auth_json` records.
- The service never returns the `Authentication-Results` or `ARC-*` headers in `trust`; they stay in the raw
  MIME and in `include=headers`.

### 5.2 `auth_json` additions

Step 6 records `spf.aligned`. Step 4 records, for a chain whose result is `pass`, `arc.sealer` (the newest
seal's `d=`), `arc.sealer_trusted` (whether it is in `inbound.trusted_arc_sealers`), and `arc.aar` (from the
newest `ARC-Authentication-Results`: its authserv-id, its `dmarc` result and `header.from`, and its
`smtp.rcpttodomain` when present). [§6](#6-trusted-arc-sealers-fr-in-15) adds `arc.rescue`. Each value is a
short ASCII string, so the row budget of [Inbound › Storage caps](inbound.md#storage-caps) is unaffected.

## 6. Trusted ARC sealers (FR-IN-15)

Extends [Inbound › Authentication verdict](inbound.md#authentication-verdict) (steps 2, 4, 7 and 8),
[Trust signals](inbound.md#trust-signals), the [Quarantine decision](inbound.md#quarantine-decision)
(rules 3 and 3a), [Inbound volume caps](inbound.md#inbound-volume-caps-d5-d13), the verification rules of
[Inbound](inbound.md#verification-codes-and-unsolicited-otp-e5) and
[Service sign-up ledger § 5](service-accounts.md#5-the-verification-gate), the known-recipient rule of
[Outbound › Recipient filters](outbound.md#recipient-filters), the citation check of [Search](search.md),
triage rule 13 of [Triage](triage.md), and [Configuration › Tenant policy](../../reference/configuration.md#tenant-policy).

Today ARC "is recorded; it does not override DMARC in v1.0" (step 4). A forwarder that changes a message
(a footer, a subject tag, a rewritten body) breaks the original DKIM signature, and SPF then names the
forwarder, so a message from a domain with `p=quarantine` or `p=reject` arrives with verdict `fail` and is
quarantined `auth_failed`, although the forwarder checked it on arrival and recorded `dmarc=pass`. This is
common for `send_only` domains, whose inbound mail reaches the identity through a forwarding rule in the
customer's own mailbox (FR-DOM-10), and for SES-receiving domains behind a mailing list.

### 6.1 Sources read on 2026-10-17

- **RFC 8617** (ARC, Experimental). An ARC set is an `ARC-Authentication-Results` (AAR), an
  `ARC-Message-Signature` (AMS) and an `ARC-Seal` (AS) with the same instance `i=`, 1 to 50; the seal's
  `cv=` is `none`, `fail` or `pass` (§4.4). A validator (§5.2) fails a chain with more than 50 sets, a
  newest seal of `cv=fail`, a set missing a header, instances that are not a continuous 1..N, a `cv` that is
  not `none` on instance 1 and `pass` on every later one, a newest AMS that does not verify, or any seal
  that does not verify; a failed chain "MUST be treated the same as a message with no Authenticated Received
  Chain". The AAR records the sealer's assessment "at message arrival time" (§4.1.1). A validated chain
  shows that the sealer of instance N emitted the message with this body after that assessment (§9); ARC
  "does not make any assessment of their trustworthiness" (§9.3), "Recipients are cautioned to treat every
  Sealer of the ARC Chain with suspicion" (§9.4), and whether to accept a sealer's assessment for DMARC is
  local policy (§7.2.1). An intact passing chain can be replayed to other recipients (§9.5).
  ([RFC 8617](https://www.rfc-editor.org/rfc/rfc8617), read 2026-10-17.)
- **Google** seals with `d=google.com`: a Gmail message quoted on the exim-users list shows two sets with
  `d=google.com; s=arc-20240605` and AAR authserv-id `mx.google.com`
  ([exim-users, 2024-11-13](https://www.mail-archive.com/exim-users@lists.exim.org/msg01723.html), read
  2026-10-17), and `arc-20160816._domainkey.google.com` and `arc-20240605._domainkey.google.com` publish
  RSA keys (DNS over HTTPS, `cloudflare-dns.com`, 2026-10-17). Google's own ARC page does not name the
  domain ([ARC email authentication](https://knowledge.workspace.google.com/admin/security/arc-email-authentication),
  last updated 2026-10-13, read 2026-10-17).
- **Microsoft** (Exchange Online) seals with `d=microsoft.com; s=arcselector10001`, AAR authserv-id
  `mx.microsoft.com`, and an AMS that signs `From`, `Date`, `Subject` and `Message-ID` but not `To`, with
  `smtp.rcpttodomain` in the AAR (headers quoted in a
  [Microsoft Q&A thread, 2025-02-27](https://learn.microsoft.com/en-us/answers/questions/4727823/exchange-arc-validation-failure-when-forwarding-to),
  read 2026-10-17); `arcselector9901._domainkey.microsoft.com` and `arcselector10001._domainkey.microsoft.com`
  publish keys (DNS over HTTPS, 2026-10-17). Microsoft's own feature trusts a sealer by the `d=` of its
  `ARC-Seal`, and "ARC overrides DMARC only, not spam filtering"
  ([Configure trusted ARC sealers](https://learn.microsoft.com/en-us/defender-office-365/email-authentication-arc-configure),
  updated 2026-08-12, read 2026-10-17).
- **Not verified:** `outlook.com` and `icloud.com`, which AgentMail trusts. No header sample or document
  naming them as seal domains was found this run, and `arcselector9901._domainkey.outlook.com` has no
  record, so neither is in the default list.
- **Cloudflare Email Routing** performs "SPF, DKIM, DMARC, and ARC checks" and rejects "Mail that fails
  authentication according to the sender's DMARC policy" before the Worker
  ([Email lifecycle](https://developers.cloudflare.com/email-service/concepts/email-lifecycle/), read
  2026-10-17). It does not say whether a valid ARC chain changes that decision. So for the `routing` inbound
  source (the platform domain, which carries `send_only` forwards, and Cloudflare zones), a DMARC-failing
  forward under `p=quarantine` or `p=reject` may be refused by Cloudflare and never reach this rule; the rule
  certainly applies to the `ses` source, whose receipt rule refuses nothing for authentication, to
  `p=none` failures and to the `unverified` case. The live test `live::arc::cloudflare_forward_behaviour`
  records what Cloudflare does with a DMARC-failing message sealed by `google.com`, and the result is written
  here as a "Spike result" note.
- **`mail-auth`.** On the crate's main branch (read 2026-10-17) `ArcOutput::chain()` returns the sets oldest
  first, with `Seal { i, d, s, t, cv }` (`t` is `0` when absent) and the AMS `Signature { h, d, t, … }`.
  docs.rs documents 0.13.3, the pinned version, without the `arc` feature's items, so the build checks that
  0.13.3 exposes them; if it does not, `core::auth::arc` reads the newest `ARC-Seal`, `ARC-Message-Signature`
  and AAR tags itself from the raw headers, after `verify_arc` has validated the chain.

### 6.2 The policy field

`inbound.trusted_arc_sealers`: an array of domains, each an A-label registrable domain in lower case (a
public suffix, an IP literal or a name that is not a DNS name is `400 invalid_request` with
`details.errors[].path`), unique, at most 20. Built-in default `["google.com", "microsoft.com"]`, the two
sealers verified above. **Platform-only**: a partner key or a workspace writer gets `403 scope_denied`
(`details.field`, and for a workspace writer `details.reason = "not_writable"`), as for
`domains.cloudflare_zones`. `[]` turns the rule off. A deployment operator who sees another forwarder's
`ARC-Seal: … d=…` in real headers (for example a Cloudflare Email Routing customer forwarding through
Cloudflare, whose seal domain was not checked this run) adds it in `PM_DEFAULT_POLICY` or per tenant.

### 6.3 The rule

`core::auth::arc_rescue(input: &RescueInput) -> Rescue` (pure). It runs only when step 6 gives DMARC `fail`
(under any policy) or the step-7 `unverified` condition holds, and `multiple_from` is false. Every condition
must hold; the first that fails is recorded in `auth_json.arc.rescue = { "result": "refused", "reason": … }`.

| # | Condition | `reason` when it fails |
|---|---|---|
| 1 | The effective `inbound.trusted_arc_sealers` is not empty | `off` |
| 2 | The ARC chain validates by RFC 8617 § 5.2 (`verify_arc` gives `pass`, with the step-2 DNS rules; a key that could not be fetched fails the chain) | `chain` |
| 3 | The newest set's `ARC-Seal` `d=` and `ARC-Message-Signature` `d=` are equal and are in the list (exact match after A-label lower-casing; a subdomain does not match) | `untrusted_sealer` |
| 4 | The newest AMS's `h=` includes `from`, so the `From` header is the one the sealer signed | `from_unsigned` |
| 5 | The newest AAR has a `dmarc` result of exactly `pass` whose `header.from` equals the message's `From` domain (`bestguesspass`, `none` and every other value fail) | `no_dmarc_pass` |
| 6 | **Freshness.** The newest seal's `t=`, else the newest AMS's `t=`, is within [`received_at` − 72 hours, `received_at` + 15 minutes]; when neither has `t=`, the `Date` header, if the newest AMS signs it, is in that window; otherwise the condition fails | `stale` |
| 7 | **Recipient binding.** An `active` or `retiring` address of the receiving identity (its `send_only` addresses included) is in `To` when `to` is in the newest AMS's `h=`, or in `Cc` when `cc` is, or the newest AAR's `smtp.rcpttodomain`, when present, is the domain of such an address | `no_binding` |

All seven hold → verdict `arc_rescued` and `auth_json.arc.rescue = { "result": "rescued" }`. The verdict
table of step 7 gains one row, second:

| Condition (first match) | `verdict` |
|---|---|
| `loopback` set | `pass` |
| DMARC `pass` | `pass` |
| **DMARC `fail`, or the `unverified` condition, and `arc_rescue` succeeded** | **`arc_rescued`** |
| … the existing rows, unchanged | … |

Conditions 1–6 run in step 7. Condition 7 needs the identity's addresses: when 1–6 hold, step 12 adds
`SELECT address FROM addresses WHERE identity_id = ?1 AND status IN ('active','retiring')` to its facts batch
(only for such candidates, so ordinary mail reads nothing more), and the verdict is finished before
`MailboxRequest::Ingest`. 72 hours and 15 minutes are Pylota Mail choices: a forward normally arrives within
minutes, and a message older than three days that needs rescuing can still be released by a person.

### 6.4 What a rescue proves, and what it does not

It proves that a sealer the operator trusts received the message addressed to this identity, found that it
passed DMARC for its `From` domain, and has not been changed since that sealer signed it, except by what the
sealer itself changed (RFC 8617 § 9). It does not prove that the sealer judged correctly, that the body is
the one the original sender wrote (the sealer, or its customer's rules, may have added a footer), or anything
about earlier hops the sealer did not vouch for. A forged chain fails condition 2, a seal from anyone else
fails 3, a replay of someone else's sealed mail fails 7, an old capture fails 6, and an exact replay of a
rescued message is deduplicated by hash or Message-ID ([B3](../edge-cases.md), [B14](../edge-cases.md)).

### 6.5 How each rule treats `arc_rescued`

`arc_rescued` is a new verdict value, not `pass`, so an integrator that requires `pass` for high-value
automation keeps requiring it, and opts in where a rescued message is enough (clients already must handle
unknown enum values, [REST API › Basics](../../reference/api.md#basics)).

| Rule | `arc_rescued` counts as | Why |
|---|---|---|
| Quarantine rules 3 and 3a | not `fail` or `unverified`: not quarantined for authentication | The purpose of the feature. Rules 4–6 apply as to any message |
| Spam score | +0.05 (Pylota Mail choice; `pass` adds nothing, `none` adds 0.10) | Authenticated by a third party, not at our boundary |
| `known_sender` | not through the own-domain rule (that rule needs `pass`); the `contacts` and receive-allow rules apply as usual | The own-domain rule trusts every address of a domain at once; it stays tied to a pass at our boundary |
| Inbound volume caps | authenticated: the per-sender key is the `From` address, not `unauth:`, and the message is outside the unauthenticated bucket | The `From` address is proven |
| Known recipient, rule 3 (answering the authenticated sender) | `pass` | The `From` address is proven, so answering it is not writing to a new address; this keeps `send_only` replies working |
| `verifications` rows, the service-ledger match rule 3, and `wait`'s release | not `pass`: no verification row, no match, no release | A code is a credential; it reaches agents only from mail authenticated at our boundary. Such mail stays visible, and with `accounts.require_approval` it is held `account_unapproved` for a person ([E9](../edge-cases.md)) |
| Trusted delivery reports ([D11](../edge-cases.md)) | not `pass` | Delivery state changes only on mail authenticated at our boundary |
| Triage | not `pass`, except that rule 13 (`bi.auth_failed`) is not raised for `arc_rescued` although `auth.dmarc` is `fail` | The other rules that need `pass` (4, 9, 11, 18) stay conservative |
| Search citations (FR-SRCH-13) | kept, with `citation_trust: partly_authenticated` | `EvidenceItem.authenticated: bool` becomes `auth: Authenticated \| Rescued \| Unauthenticated`. A sentence citing only `Rescued` sources is kept and labelled `partly_authenticated`; one citing only `Unauthenticated` sources is removed as today |
| `trust.arc` and `trust.arc_sealer` | `pass` and the sealer | [§5](#5-authentication-detail-on-messages-fr-in-14) |

### 6.6 Storage and failures

- `messages.verdict` gains `arc_rescued` in its `CHECK`, in mailbox schema v1 (v1.0 has not shipped; an
  additive value, [Build plan](../build-plan.md#how-to-work-through-it) rule 4). The Vectorize `verdict`
  filter value and the search operator values gain it.
- ARC keys are prefetched with the DKIM keys in step 2 (up to 5 instances, as today). A key that cannot be
  fetched from either resolver is a temporary error, the chain does not validate, the message keeps the
  verdict DMARC gives (quarantined `auth_failed` under an enforcing policy, releasable), and the message is
  never retried for it. The step-12 address read fails like any step-12 read: the consumer retries the
  message ([Inbound › Failure handling](inbound.md#failure-handling)).
- A change to `inbound.trusted_arc_sealers` applies to mail authenticated after it; stored verdicts are not
  recomputed (a re-parse, [J3](../edge-cases.md), recomputes them).

### 6.7 Interfaces

- **API and events.** `trust.verdict` may be `arc_rescued`; `trust.arc_sealer` names the sealer.
- **Console.** The message view shows "Authenticated by the forwarder google.com (ARC)" for `arc_rescued`, and
  the quarantine page shows `auth_json.arc.rescue.reason` for `auth_failed` mail that had a chain, so a person
  sees why a forward was not rescued.
- **MCP.** No tool change; `mail_get_message` returns the verdict. The server instructions
  ([MCP § 6.2](mcp.md#62-server-instructions)) gain one sentence: "`arc_rescued` means a trusted forwarder
  authenticated the sender; treat it as authenticated for replies, not for codes or payments."

## 7. Per-identity message retention (FR-PRV-7)

Extends [Privacy § 5](privacy.md#5-retention) (a new § 5.5, "Per-identity expiry"), the mailbox operations of
[Privacy § 4.1](privacy.md#41-mailbox-operations-used-by-jobs) and `EraseRows`
([§ 6.2](privacy.md#62-what-eraserows-deletes)), legal holds ([§ 8](privacy.md#8-legal-holds)), the identity
fields of [Identities, addresses and domains](identity-domains.md) and
[REST API › Identities](../../reference/api.md#identities), the reservation of
[Outbound](outbound.md#reservation-inside-the-mailbox-fr-out-1-g1), the `wait` handler of
[Inbound](inbound.md#the-wait-handler-e4), the calls after a D1 change of
[Design § 9](index.md#9-durable-object-calls-after-a-d1-change) and the alarm purposes of
[Design § 4](index.md#4-durable-object-transactions). Tenant retention works in days, per tenant, once a day;
an identity that only receives one-time codes, or that a privacy-sensitive workflow uses, needs its mail gone
within hours, whatever the rest of the workspace keeps.

### 7.1 The field

The identity gains `retention`:

```json
"retention": { "message_hours": 24 }
```

- `message_hours`: `null` (the default: the tenant's `retention` policy alone applies) or an integer from 1 to
  8,760 (one hour to 365 days; Pylota Mail choice). Set at creation (`POST …/identities`) or with
  `PATCH /v1/identities/{identity_id}`; returned on the [Identity object](../../reference/api.md#identity-object).
- **Effective lifetime.** When the tenant's `retention.message_days` is also set, the shorter of the two
  applies: the tenant job keeps running and deletes what it reaches first.
- **Who.** `identities:write`. Setting a number where there was `null`, or lowering it, deletes mail sooner,
  so it also needs `erasure:manage` (as `DELETE /v1/identities/{identity_id}` does), otherwise
  `403 permission_denied` with `details.required: "erasure:manage"`. Raising it or setting `null` needs
  `identities:write` only, and never brings back mail already erased.
- **Audit and events.** Every change writes `identity.retention_update` (`details.from`, `details.to`) and
  emits `identity.updated` with `changed: ["retention"]`.
- **Refused:** a value outside 1–8,760 or not an integer (`400 invalid_request`, path
  `retention.message_hours`); the system identity, whose retention is fixed at 30 days
  ([Identities › The system identity](identity-domains.md#the-system-identity)) (`400 invalid_request`,
  `details.reason = "system_identity"`, for the platform key that alone reaches it).

### 7.2 How the mailbox learns it

D1 keeps `identities.message_hours`. The handler writes, in the same D1 batch as the change, an
`rpc_intents` row with the new op `set_retention`, and then sends
`MailboxRequest::SetRetention { message_hours: Option<u32>, version: i64 }` (`version` =
`identities.updated_at`), exactly as `identity.updated` reaches the mailbox
([Design § 9](index.md#9-durable-object-calls-after-a-d1-change)); the repair cron re-sends a failed call.
`InitMailbox` gains `message_hours` for an identity created with it. The mailbox stores
`meta.retention_message_hours` and `meta.retention_version`, ignores a call whose `version` is older than the
stored one, and sets `alarm:expiry = now` when the value is set or lowered (so existing messages are
re-planned) or deletes `alarm:expiry` when it becomes `null`. A change therefore applies when the mailbox
receives it: normally within a second of the `PATCH`, and after a failed call at the repair cron's next
attempt (every minute, backing off to 15 minutes).

### 7.3 What expires

A message expires when `received_at + message_hours × 3,600,000 ≤ now` (inbound and outbound, every status:
`received`, `quarantined`, `hidden`, `throttled`, [Screening](screening.md)'s `screening`, and every outbound
status), except:

- a message whose thread is under an active legal hold ([Privacy § 8](privacy.md#8-legal-holds)): it expires
  at the first expiry run after the hold ends or is removed (removing a hold sets `alarm:expiry = now`);
- an outbound message still `queued`, or one with a transport claim (`meta.claim:{msg}`) younger than
  5 minutes: it expires at the first run after it leaves `queued` and its claim has settled. An `uncertain`
  message is not exempt: reconciliation needs at most 30 minutes ([Outbound](outbound.md#uncertain-sends-and-reconciliation-fr-dlv-4)),
  under the 1-hour minimum, and a person who wants to resolve one must do so before it expires.

What expiry deletes for each message is exactly what `EraseRows` deletes (FTS and trigram rows; the
message row, cascading to deliveries, attachments, labels, refs, chunks and verifications; the outbox events
that reference it; the thread's counters, and the thread itself when it is left empty), its R2 objects
(`raw.eml`, `out/{msg}.eml`, attachments and their extracted text) in `BLOBS` and, when configured,
`BACKUP`, and its vectors (`deleteByIds`), with its ID written to `erased_ids` so that a late index job
deletes what it writes ([I10](../edge-cases.md)). The drafts of [Drafts](drafts.md) are mail content too: a
draft expires `message_hours` after its last change. Expiry keeps, as tenant retention does: `contacts`,
`rate_windows`, suppressions, `event_index` rows (IDs and types only, pruned after `events_days`) and audit
rows. The one difference from `EraseRows` is the idempotency row of an outbound message
([§7.6](#76-a-retried-send-never-sends-twice)).

### 7.4 The expiry alarm

The mailbox does the work itself, with its own alarm, through the same steps tenant retention runs
([Privacy § 5.2](privacy.md#52-steps-of-a-tenant-retention-job), step 3) in `mailbox/expiry.rs`; no
`JobRunner` and no D1 row is created per run.

```text
 ingest / submit commit ──▶ alarm:expiry = min(alarm:expiry, received_at + H)
 SetRetention set or lowered, hold removed, maintenance finds due rows ──▶ alarm:expiry = now
 alarm, purpose expiry (H = meta.retention_message_hours; nothing to do when it is absent or meta.erased):
   1 plan     ≤ 100 due rows, oldest first, skipping held threads, queued and claimed outbound
   2 vectors  deleteByIds(vector IDs from chunks)                ─ error ─▶ back-off, stop
   3 objects  delete every R2 key of the batch, BLOBS then BACKUP ─ error ─▶ back-off, stop
   4 commit   one transaction: re-check each row is still due and not held, EraseRows(mode: Expiry),
              erased_ids, add the counts to meta['expiry:{YYYY-MM-DD}']
   5 re-arm   100 rows planned → alarm:expiry = now (the next slice);
              else the next due received_at + H, but not before now + 60 s
 back-off: alarm:expiry = now + 30 s × 2^(n − 1), at most 5 minutes (meta.expiry_backoff, cleared by a success)
```

The plan:

```sql
SELECT m.rowid, m.id, m.direction, m.status, m.raw_r2_key FROM messages m
JOIN threads t ON t.seq = m.thread_seq
WHERE m.received_at <= ?1                                  -- ?1 = now − H × 3,600,000
  AND NOT (m.direction = 'outbound' AND m.status = 'queued')
  AND (t.hold_json IS NULL OR NOT <hold active at ?2>)     -- Privacy § 8: until null or later than now
ORDER BY m.received_at, m.rowid LIMIT 100;
```

with the R2 keys and vector IDs read as `ErasurePlan` reads them, and rows with a live `claim:{msg}` dropped
in Rust. A slice stops after 20 seconds or 100 messages, as `JobRunner` slices do. Steps 2 and 3 run before
the transaction because R2 and Vectorize calls never run inside one
([Design § 4](index.md#4-durable-object-transactions), rule 3), and an object or vector deleted for a row
that the re-check in step 4 then keeps (a hold set in between) is the same loss an erasure racing a hold
would have: so step 1 also skips threads whose hold changed in the last 60 seconds
(`hold_json.set_at`), and the re-check stays the guarantee for the row itself. The daily
`alarm:maintenance` sets `alarm:expiry = now` whenever due rows exist, so an alarm lost after Cloudflare's six
retries ([Design § 4](index.md#4-durable-object-transactions), rule 5) costs at most a day.

### 7.5 Timing

A message is erased at most 15 minutes after it is due while R2, Vectorize and the mailbox answer, normally
within 60 seconds (Pylota Mail target). `retention_expiry_lag_ms` (due time to commit) measures it; the
`retention_expiry_overdue:{identity_id}` alert fires when the oldest due row of a mailbox is more than 1 hour
overdue (ticket) or 24 hours (page) ([§11](#11-observability)).

### 7.6 A retried send never sends twice

`EraseRows` deletes a message's idempotency row (Privacy § 6.2, step 4), so a client that retried a send
with the same `Idempotency-Key` after the message expired would send it again, breaking U2. In
`mode: Expiry` the row is kept, without content, until its own `expires_at` (30 days after the send):

```sql
UPDATE idempotency
SET fingerprint = ?2,                                      -- hex(HMAC-SHA256(PM_HASH_KEY, fingerprint))
    response_json = json_object('expired_at', ?3, 'message_id', message_id)
WHERE message_id = ?1;
```

The reservation ([Outbound](outbound.md#reservation-inside-the-mailbox-fr-out-1-g1), step 1) reads such a
stub by its `expired_at` key and compares `HMAC-SHA256(PM_HASH_KEY, fingerprint of the new request)` with the
stored value: equal → `410 message_expired` (`retryable: false`, `details.message_id`, `details.expired_at`),
and nothing is sent; different → `409 idempotency_conflict`. The fingerprint is keyed after expiry so that the
kept row cannot be used to test a guess of the expired body without `PM_HASH_KEY`. An erasure request keeps
deleting the row, as § 6.2 says: erasure removes even the fact that the message existed.

### 7.7 One-time codes

A code's `verifications` row is deleted with its message, so a code is available for the shorter of 24 hours
and `message_hours`, at least 1 hour. `wait` (at most 60 seconds per call) and the `verification.received`
event, whose payload never carries the value, work unchanged inside that time. A `wait` that starts after the
message expired matches nothing and times out; a quarantined code (`otp_unsolicited`, `account_unapproved`)
expires on the same clock, so a person must release it before then.

### 7.8 What expiry cannot recall

Copies outside the mailbox are not recalled: webhook payloads already delivered (up to
`policy.webhook_text_bytes` of `extracted_text`), copies the event feed's consumers received
([Event feed](event-feed.md); the feed's own copies of the message's events go wherever erasure sends them),
exports completed before the message expired (until their own 7-day expiry), the recipients' copies of
outbound mail, Cloudflare's logs and the 30-day D1 Time Travel and Durable Object recovery windows
([Privacy § 11](privacy.md#11-what-remains-after-deletion)). A delivery of an event whose outbox row expired
before it succeeded becomes `dead` with `event_unavailable` and is never delivered. An export collecting
while messages expire leaves out the messages whose rows or objects are gone, and counts them in
`jobs.result_json.expired_during_export`. This is stated in the [privacy guide](../../guides/privacy.md) as
plainly as AgentMail states that copies made before its 24-hour expiry remain.

### 7.9 Receipts

Expiry runs too often for one erasure receipt per run, so its receipt is one audit row per identity per UTC
day that expired anything: `action = "retention.expire"`, `target_type = "identity"`, and `details` with the
day and the counts of the erasure receipt (`messages_deleted`, `attachments_deleted`, `r2_objects_deleted`,
`fts_rows_deleted`, `refs_deleted`, `vectors_deleted`, `events_deleted`), plus `drafts_deleted`,
`held_skipped`, `first_expired_at` and `last_expired_at` (FR-PRV-2). The mailbox accumulates them in
`meta['expiry:{YYYY-MM-DD}']`; the daily maintenance alarm writes each completed day's row to D1 with a
deterministic ID (a ULID whose time is the day's start and whose random part is the first 10 bytes of
`HMAC-SHA256(PM_HASH_KEY, "expire:" + identity_id + ":" + day)`), `INSERT OR IGNORE`, and then deletes the
key, so a repeated flush writes one row. When the identity is erased before a flush, the erasure's own
receipt covers what was left, and the pending day's counts are lost with the mailbox.

### 7.10 Interactions

| With | Behaviour |
|---|---|
| Erasure requests, tenant retention | Both delete through `EraseRows`; a row deleted by one is absent for the other, and neither counts it twice (counts come from rows deleted in the committed transaction) |
| Billing | Stored bytes fall at the next hourly usage roll-up ([Billing › Storage](billing.md#storage)) |
| Threads and tokens | A reply whose thread token names an expired thread is filed by its headers, and otherwise starts a new thread; it is never refused |
| Search | The message leaves keyword search in the expiry transaction and semantic results once Vectorize applies the delete (seconds); a vector returned before that is dropped because its row is gone |
| Drafts ([Drafts](drafts.md)) | A draft expires `message_hours` after its last change; scheduling a draft whose `send_at` is after that time is refused (`400 invalid_request`, `details.reason = "beyond_retention"`) |
| Notifications | A pending new-mail count can include a message that expired before the email went; the email never names it |
| Suspended tenant, paused identity | Expiry continues: it deletes, it never sends |

### 7.11 Limits

| Limit | Value |
|---|---|
| `message_hours` | 1–8,760; `null` keeps the tenant policy alone |
| Erasure after due time | ≤ 15 minutes while dependencies answer (target); alert at 1 hour overdue |
| Expiry slice | 100 messages or 20 seconds; the next slice at once |
| Kept idempotency stub | Until the original key's 30 days end |

### 7.12 Interfaces

- **Console.** The inbox page shows "Messages are deleted N hours after they arrive" or the tenant's rule.
  Owners and admins change it with a re-authenticated `POST`, and a value that deletes mail sooner needs the
  second, confirmed `POST` the policy page uses for such changes
  ([Workspace policy § 6](workspace-policy.md#6-the-console-page)).
- **CLI.** `pmail identities create|update <identity> --message-hours <n>`, and `--keep-messages` for `null`.
- **MCP.** None: an agent never changes retention.

## 8. Webhook delivery headers (FR-WH-7)

Extends [Webhooks › Signing](webhooks.md#signing-standard-webhooks) ("Other headers"),
[Secrets](webhooks.md#secrets), [Delivering an attempt](webhooks.md#delivering-an-attempt) (step 4),
[Test deliveries](webhooks.md#test-deliveries), [REST API › Webhooks](../../reference/api.md#webhooks),
[Security § 6.1](security.md#61-inventory) and [§ 7.2](security.md#72-encryption-envelope). Some receivers sit
behind a gateway or a WAF rule that wants a static token before the request reaches the code that verifies
the signature.

### 8.1 API

At creation, `POST /v1/webhooks` and `POST /v1/tenants/{tenant_id}/webhooks` accept:

```json
{ "url": "https://api.example.com/webhooks/mail", "events": ["message.received"],
  "headers": { "X-Ingress-Token": "t0k_8f2k3m9q…", "Authorization": "Bearer 4d1e…" } }
```

Later, `PATCH /v1/webhooks/{webhook_id}/headers` (`webhooks:manage`):

```json
{ "set": { "X-Ingress-Token": "t0k_new…" }, "remove": ["Authorization"] }
```

Both answer with the [webhook endpoint](../../reference/api.md#webhooks), which gains `header_names`, the
names in their stored casing, sorted case-insensitively. Values are **write-only**: no response, list,
replay, log, export, audit row or idempotency record ever holds one.

```json
{ "id": "whk_01JA9J…", "scope": "tenant", "url": "https://api.example.com/webhooks/mail",
  "events": ["message.received"], "header_names": ["X-Ingress-Token"], "enabled": true, "…": "…" }
```

### 8.2 Rules

1. **Permission.** `webhooks:manage`, at the levels that may write the endpoint; an identity key gets
   `403 scope_denied`, as for every webhook write. A partner key sets headers on its partner endpoints and on
   its tenants' endpoints.
2. **Names**: 1–64 characters matching `^[A-Za-z0-9][A-Za-z0-9-]*$`, compared case-insensitively. A name that
   fails the pattern, or is one of the following, is refused with `400 header_not_allowed`
   (`details.reason = "webhook_header"`, `details.name`):
   - the forbidden request-header names of the Fetch standard (`Accept-Charset`, `Accept-Encoding`,
     `Access-Control-Request-Headers`, `Access-Control-Request-Method`, `Connection`, `Content-Length`,
     `Cookie`, `Cookie2`, `Date`, `DNT`, `Expect`, `Host`, `Keep-Alive`, `Origin`, `Referer`, `Set-Cookie`,
     `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`, `Via`) and every name starting `Proxy-` or `Sec-`
     ([Fetch, forbidden request-header](https://fetch.spec.whatwg.org/#forbidden-request-header), last
     updated 6 October 2026, read 2026-10-17), and `X-HTTP-Method`, `X-HTTP-Method-Override` and
     `X-Method-Override`;
   - the headers the service sets: `Content-Type`, `Content-Encoding`, `User-Agent` and every name starting
     `webhook-` (Standard Webhooks);
   - names a receiver's proxy may trust as the client's address: `Forwarded`, `X-Real-IP`, `True-Client-IP`
     and every name starting `X-Forwarded-` or `CF-`;
   - every name starting `X-Pylota-`, reserved as on sends.

   `Authorization` is allowed: a static bearer token is the main use.
3. **Values**: 1–1,024 bytes of `0x20`–`0x7E`, not starting or ending with a space (so no CR, LF or
   non-ASCII byte can split a header), otherwise `400 invalid_request` (path `headers.{name}` or
   `set.{name}`).
4. **Counts**: at most 10 headers and 4,096 bytes of values per endpoint, after the change, otherwise
   `422 webhook_header_limit_reached` (`details.limit`, `details.reason`: `count` or `bytes`).
5. **`PATCH …/headers`** needs at least one entry in `set` or `remove` (`400 invalid_request`); a name in both,
   in any casing, or twice in `set` in different casings, is `400 invalid_request`; removing a name that is
   not set changes nothing. `set` replaces a value atomically: there is no overlap, so a receiver that
   rotates its token accepts the old and the new for the switch.
6. **A URL change to another host removes every header** in the same update (and the audit row lists the
   removed names), so a token meant for one receiver is never sent to a host someone else chose; a change of
   path on the same host keeps them.
7. **Audit.** Every change writes `webhook.headers_update` with the names set and removed, never values.

### 8.3 Delivery and storage

- **On every attempt**: first, retries, replays, released held deliveries and `POST …/test`. Custom headers
  are added first and the service's own headers last, so none can be overridden. They are not signed:
  Standard Webhooks signs `{webhook-id}.{webhook-timestamp}.{body}` only, so a header is an extra gate in
  front of the receiver, never a replacement for verifying the signature ([Webhook events](../../reference/events.md#verify-every-request)).
- **Storage.** One sealed value per endpoint, so a change is one compare-and-swap statement:

  ```sql
  ALTER TABLE webhook_endpoints ADD COLUMN headers_sealed TEXT;                 -- pm1 envelope of the JSON map, or NULL
  ALTER TABLE webhook_endpoints ADD COLUMN header_names_json TEXT NOT NULL DEFAULT '[]';
  ALTER TABLE webhook_endpoints ADD COLUMN headers_version INTEGER NOT NULL DEFAULT 0;
  -- PATCH …/headers: read headers_sealed and headers_version, unseal, apply set and remove, check the rules,
  -- seal, then
  UPDATE webhook_endpoints SET headers_sealed = ?2, header_names_json = ?3,
         headers_version = headers_version + 1, updated_at = ?4
  WHERE id = ?1 AND headers_version = ?5;
  ```

  When the update changes no row, another change won: the handler reads again and retries, up to 3 times, then
  answers `409 request_in_progress` (retryable). The value is sealed with AES-256-GCM under `PM_MASTER_KEY` in
  the envelope of [Security § 7.2](security.md#72-encryption-envelope), associated data
  `pm1|webhook_endpoints|headers_sealed|{endpoint_id}`, and has an entry in `crates/core/src/sealed.rs`, so the
  master-key rotation re-seals and counts it.
- **Use.** Unsealed per delivery and cached in the isolate for at most 60 seconds, keyed by endpoint ID and a
  hash of `headers_sealed`, as the signing secret is. A failure to unseal records the attempt as failed with
  `secret_unavailable` and alerts, as for the secret.
- **Idempotency.** The fingerprint of a create that carries `headers` is computed with each value replaced by
  `hex(HMAC-SHA256(PM_HASH_KEY, value))`, so `idempotency_records` never holds a plain hash of a token that may
  be short enough to guess. `PATCH` is not recorded (only `POST` is).

### 8.4 Limits

| Limit | Value | Source |
|---|---|---|
| Headers per endpoint | 10 | Pylota Mail (as AgentMail's limit is not documented, the number is ours) |
| Name | 1–64 characters | Pylota Mail |
| Value | 1–1,024 bytes; 4,096 bytes for all values together | Pylota Mail |

### 8.5 Privacy and interfaces

- **Privacy.** A value is a credential of the receiver, not personal data, and is deleted with its endpoint:
  by `DELETE /v1/webhooks/{id}`, partner deletion and tenant erasure, which already delete the rows.
- **Console.** The webhooks page shows the names; owners and admins add a header (a write-only input) or
  remove one with a `POST`.
- **CLI.** `pmail webhooks create … --header 'Name: value'`, repeatable, and
  `pmail webhooks headers <webhook-id> [--set 'Name: value']… [--remove <name>]…`; a value can be read from
  standard input (`--set-stdin <name>`) so it stays out of the shell history.

## 9. Zone-file export (FR-DOM-15)

Extends [REST API › `GET /v1/domains/{domain_id}/records`](../../reference/api.md#get-v1domainsdomain_idrecords--domainsread),
[Identities and domains](identity-domains.md) ("Records (FR-DOM-3)") and
[Domains on any DNS host § 4.1](domain-connections.md#41-what-each-method-asks-the-customer-to-publish). Many
DNS hosts import a zone file, which saves the customer copying five records by hand.

### 9.1 Route

`GET /v1/domains/{domain_id}/records?format=bind` (`domains:read`, every level that may read the domain).
`format` is `json` (the default, today's answer) or `bind`; any other value is `400 invalid_request` (path
`format`). The `bind` form is computed from the same provider reads and DNS checks as the JSON form, in the
same handler, and fails the same way (FR-DOM-3): it is a rendering of that answer, never a template.

```http
HTTP/1.1 200 OK
Content-Type: text/plain; charset=utf-8
Content-Disposition: attachment; filename="mail.acmecarhire.example.zone"
```

```text
; Pylota Mail DNS records for mail.acmecarhire.example (dom_01JA9G6M2WW7X2M5N6P8R0T1YR), method dns_records.
; Read from the provider APIs at 2026-10-17T10:05:00Z. Add these records to the zone of acmecarhire.example.
; Each line ends with the record's purpose, whether it is required, and the last DNS check.
$ORIGIN acmecarhire.example.
$TTL 3600
_pylota-mail.mail                  IN TXT   "pm-verify=8f2k3m9q" ; ownership, required, ok
mail                               IN MX    10 inbound-smtp.eu-west-2.amazonaws.com. ; mx, required, missing
x7k2…._domainkey.mail              IN CNAME x7k2….dkim.eu-west-2.amazonses.com. ; dkim, required, ok
pm-bounce.mail                     IN MX    10 feedback-smtp.eu-west-2.amazonses.com. ; return_path, required, ok
pm-bounce.mail                     IN TXT   "v=spf1 include:amazonses.com ~all" ; spf, required, ok
; Suggested, not required: remove the leading "; " to publish it.
; _dmarc.mail                      IN TXT   "v=DMARC1; p=quarantine" ; dmarc, suggested, missing
```

### 9.2 Rules

`core::zonefile::render(records: &DomainRecords, domain: &Domain) -> String` (pure):

1. **Origin.** `$ORIGIN` is the registrable domain (Public Suffix List) with a trailing dot, and each owner name
   is the record's `host`, which is relative to that same registrable domain ([N17](../edge-cases.md)); `@` when
   `host` is the registrable domain itself. RFC 1035 § 5 appends the origin to a name without a trailing dot
   ([RFC 1035](https://www.rfc-editor.org/rfc/rfc1035), read 2026-10-17).
2. **TTL.** `$TTL 3600` (RFC 2308 § 4 defines the directive for records without their own TTL,
   [RFC 2308](https://www.rfc-editor.org/rfc/rfc2308), read 2026-10-17). One hour is a Pylota Mail choice; the
   records carry no TTL of their own, and a DNS host may override it.
3. **Class and targets.** Every record is written with `IN`. MX, CNAME and NS targets are absolute, with a
   trailing dot; MX keeps its preference.
4. **TXT.** The value is split into character-strings of at most 255 octets (RFC 1035 § 3.3), never inside an
   escape, each in double quotes, with `"` and `\` escaped by `\` and any byte outside `0x20`–`0x7E` written as
   `\DDD` (RFC 1035 § 5.1). A DKIM key longer than 255 bytes becomes several strings on one line.
5. **Not records.** A record with `required: false` is written commented out, under one line saying it is
   suggested. The `ns` records of a `nameservers` domain are set at the registrar, not in a zone, so they are
   written as comments under "Set these nameservers at your registrar"; the `ns` records of a
   `delegated_subdomain` domain belong in the parent zone and are written as records. A record with
   `status: "unexpected"` (an extra record that conflicts, such as a second SPF record) is one to remove, never
   to add: it is written as a comment under "Remove these records", with its observed value.
6. **`cloudflare_zone` and `nameservers` domains, and the platform domain**, whose records the deployment writes
   into a Cloudflare zone itself, get a first comment saying the records are already in that zone and are listed for reference.
7. **Status.** Each line ends with `; {purpose}, {required|suggested}, {status}` from the JSON form.
8. ASCII only, `\n` line endings. The output is at most a few kilobytes.

### 9.3 Interfaces

- **CLI.** `pmail domains records <domain> --format bind > mail.acmecarhire.example.zone`.
- **Console.** The domain page gains "Download zone file", a link to
  `/console/domains/{dom}/records.zone`, which calls the same service with the session's permissions.
- **MCP.** None.

## 10. A documentation link in errors (FR-API-3)

Extends [Errors](../../reference/errors.md) (the envelope), [Design § 2](index.md#2-errors-and-the-error-envelope)
(`ApiError`), [REST API › Errors](../../reference/api.md#errors), [MCP § 5](mcp.md#5-tool-errors), the CLI's
error output ([CLI › Output](../../reference/cli.md#output)) and
[Configuration › Variables](../../reference/configuration.md#variables).

### 10.1 The field

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

- `docs` is in every error envelope the service sends: REST responses, MCP tool errors (whose text block is the
  envelope) and the CLI's `--json` output. It is not in the empty `405` body of `/mcp`, which carries no envelope.
- **Built** as `{PM_DOCS_URL}/reference/errors#code-{code}`, by the serialiser in `crates/worker/src/errors.rs`
  from `ErrorCode::anchor()` (`"code-"` followed by the code) and the docs base read once per isolate, so no
  call site builds a URL.
- **`PM_DOCS_URL`**: the base URL of the published docs, default `https://pylotamail.com/docs`, where the
  landing-page Worker serves the mdBook output (`docs/book.toml`: `site-url = "/docs/"`;
  `site/wrangler.jsonc` routes `pylotamail.com`). It must be `https`, at most 200 bytes, without query or
  fragment; a trailing `/` is removed. `pmail setup` and `pmail deploy` refuse an invalid value; the Worker, which
  validates it at isolate start, uses the default instead and logs `docs_url_invalid` once per isolate. A
  self-hoster who publishes the docs elsewhere sets it.
- **The path** is `reference/errors`, the extensionless form in which Workers static assets serve
  `reference/errors.html` with the default `html_handling` ("individual files (e.g. `foo.html`) will be served
  without a trailing slash", [Static Site Generation](https://developers.cloudflare.com/workers/static-assets/routing/static-site-generation/),
  read 2026-10-17). A host that serves only `errors.html` needs a rewrite of the extensionless path.

### 10.2 Anchors

Each code's row in `reference/errors.md` begins its code cell with `<a id="code-{code}"></a>` (a cell with
several codes, such as the `404` row, carries one anchor per code). The docs already use inline anchors of this
kind ([Observability § 8.3](observability.md#83-listing-and-redriving)), which mdBook 0.5.4, the version
`.github/workflows/site.yml` pins, renders as written.

- `cargo xtask docs-check` fails when an `ErrorCode` variant has no anchor in `errors.md`, or an anchor names
  no variant, and checks that the built `site/public/docs/reference/errors.html` has an element with each
  `id`. It runs in the CI gate after `mdbook build docs`.
- An anchor is never removed. A code retired in a later version keeps its anchor in a "Retired codes" table,
  so a link in an old log still lands.

### 10.3 Interfaces

The CLI's human output prints `docs: <url>` after `fix`. The SDK exposes `ApiError::docs`. The console's error
pages are its own and do not change.

## 11. Observability

New metrics ([Observability § 3.2](observability.md#32-catalogue)):

| Metric | Kind | Labels | Emitted by |
|---|---|---|---|
| `messages_batch_total` | counter | op (`get`, `update`), result (`ok` or the error code) | `fetch` |
| `messages_batch_ids` | observation | op | `fetch`: IDs per request |
| `label_events_total` | counter | source (`message`, `thread`, `batch`) | mailbox |
| `list_decisions_total` | counter | direction, scope (`identity`, `tenant`), kind | `pm-inbound`, `fetch` (send), `pm-outbound` |
| `arc_rescue_total` | counter | result (`rescued`, `off`, `chain`, `untrusted_sealer`, `from_unsigned`, `no_dmarc_pass`, `stale`, `no_binding`), source (`routing`, `ses`) | `pm-inbound` |
| `retention_expired_total` | counter | store (`messages`, `attachments`, `r2_objects`, `vectors`, `events`, `drafts`) | mailbox |
| `retention_expiry_lag_ms` | observation | – | mailbox: due time → commit |
| `retention_expiry_errors_total` | counter | step (`vectors`, `objects`, `commit`) | mailbox |
| `webhook_header_updates_total` | counter | result | `fetch` |
| `zone_exports_total` | counter | method | `fetch` |

New alerts ([Observability § 5.3](observability.md#53-alert-list)):

| Alert | Class | Condition | Severity | Runbook |
|---|---|---|---|---|
| `retention_expiry_overdue:{identity_id}` | B | The oldest due message of a mailbox with `retention.message_hours` is more than 1 hour past due, reported by its expiry run or its daily maintenance | ticket; page at 24 hours | [Erasure failure](observability.md#erasure-failure) (read `retention_expiry_errors_total{step}`; an R2 or Vectorize outage clears itself) |
| `arc_rescue_refused_spike` | A | `arc_rescue_total{result=no_binding\|stale}` > 50 in 1 hour: someone may be replaying sealed mail | ticket | [Abusive identity](observability.md#abusive-identity) |

Logs never carry a label name in clear for `label_events_total` (labels can be agent-written text), a webhook
header value, or an ARC header; they carry the IDs, the `arc.rescue.reason` and the counts.

## 12. Pylota agentic OS

Pylota reaches Pylota Mail Cloud as a partner (FR-KEY-4) with its partner key K1, a tenant key K2 per operator
and identity keys K3 per purpose, as its integration spec (§ 6) defines. What each addition gives it:

| Addition | Pylota agents and services | Key and permissions | What Pylota stores | How failures surface |
|---|---|---|---|---|
| Batch labels and label events | The inbound worker, the operator coordinator and the specialists that share an operator's `bookings`, `inquiry`, `maintenance` and `compliance` inboxes: the worker labels new mail by route (`route:maintenance`), a specialist claims a message with `require_absent: ["claimed"]` before it answers, and an escalation adds `needs-human`, which Pylota shows in the operator's app | K3 identity keys do not hold `messages:write`, and must not: it also allows `resolve`, which no Pylota agent may call (spec § 8.3). Label writes therefore go through an API capability route with K2, which accepts only labels with Pylota's prefixes (`route:`, `claimed`, `claimed-by-`, `needs-human`) and calls `batch-update`; events reach the partner endpoint with K1 | The claim result per message (`updated` or `conflicts`) on the agent's turn record; nothing about labels in Postgres beyond what the inbox screens already show | A `conflicts` result means another specialist owns the message, and the turn stops for that message; `400 label_limit` is a bug and pages ops |
| Identity-scope lists | `fleet-acquisition` sends seller outreach from the operator's `bookings` identity (spec § 20.2, row 42 of § 7.7) | K2 with `suppressions:manage`, through `allowRecipient` | `mail_send_allow_entries` gains the identity ID | `422 list_limit_reached` pages ops (an operator with 10,000 approved recipients on one identity) |
| Ephemeral retention | The browser agent's sign-up identity (W14), which receives one-time codes from third-party services and nothing else | K1 or K2 (they hold `erasure:manage`) sets `retention.message_hours` | Nothing new: the code is used within the agent's turn | `410 message_expired` on a retried send from that identity is final; a `wait` that times out after expiry makes the agent ask the service for a new code |
| Trusted ARC | Operators on `send_only` whose Google Workspace or Microsoft 365 mailbox forwards to their identities | No key change; the platform default list applies | `trust.verdict` and `arc_sealer` on `mail_inbound_messages` | Fewer `auth_failed` items in the Held list; a forward that is still refused shows `auth_json.arc.rescue.reason` in admin |
| Webhook headers | The partner endpoint `{PYLOTA_MAIL_WEBHOOK_BASE_URL}/webhooks/pylota-mail` (spec § 8.1) | K1, `webhooks:manage` | The token sealed in `mail_platform_secrets` (row `webhook_ingress_token`) | A rejected token at the WAF shows as failing deliveries in `mail-webhook-health` |
| Batch get, zone export, docs links | The inbound job (fetching several truncated messages at once, spec § 8.7), the reconcile script (§ 8.5), the operator's domain screen, and the error mapper | K3 for reads, K1 or K2 for records | Nothing new | `docs` is logged with every mapped error |

**What Pylota's integration spec must add** (for the spec's owner):

1. **Label coordination.** A capability route for label writes with K2 and a label allow-list, never giving K3
   `messages:write`; the claim convention (`claimed`, `claimed-by-{specialist}` with `require_absent`); the
   subscription of the partner endpoint to `message.labels_changed` by name (it is not in `*`, and the spec's
   explicit list must add it); `events.labels_changed: true` set by K1 on each operator tenant at provisioning
   (spec § 4.3); and ignoring events whose `actor.key_id` is K2's own key.
2. **Identity-scope send-allow entries.** `allowRecipient` writes
   `PUT /v1/identities/{identity_id}/lists/send/allow/{address}` for the identity that sends (seller outreach:
   `bookings`), not the tenant route, so an approved seller does not become a known recipient of `compliance`;
   `mail_send_allow_entries` records `identity_id`; renter and seller erasure delete the identity-scope entries.
3. **The sign-up identity.** When W14 is unblocked: one identity per operator (or one in a Pylota-owned tenant)
   for the browser agent, created with `retention.message_hours` (24 is a reasonable start: codes are used in
   minutes, and a day leaves time to investigate a failed sign-up), `verification.received` added to the endpoint's
   list, and a decision on the sign-up ledger (OD13), which stays off for Pylota tenants today.
4. **ARC verdicts.** Section 20.3's and W4.3's "verified" rule ("`trust` verdict `pass` and aligned") must decide
   on `arc_rescued`: count it for replies and conversation binding, not for PCN, invoice or payment automations,
   nor as a seller's word; and the mobile Held copy for an `auth_failed` forward that was not rescued.
5. **Ingress token (optional).** If the WAF in front of the webhook route wants a static token: generate it, seal
   it in `mail_platform_secrets`, set it with `PATCH /v1/webhooks/{webhook_id}/headers`, rotate it with the
   secret-rotation job (W3.4), and keep the signature check, which stays mandatory.
6. **Batch reads.** The inbound job may use `batch-get` for several truncated messages of one identity; each
   call by K1 writes a `mail.read` audit row, so the job uses K3.

## 13. Functional requirements

To be added to [PRD § 6](../prd.md#6-functional-requirements), each row at the end of the subsection named
in brackets (links rewritten relative to `prd.md` when pasted: `(#2-…)` becomes
`(design/parity-additions.md#2-…)`). Each takes the next free number of its family in this worktree's
`prd.md`; re-check the numbers against `main` after PR #5 has merged.

- **FR-IN-13** (§ 6.4) The service **must** read and update up to 100 messages of one identity in one request
  (`batch-get`, `batch-update`). An update **must** be atomic inside the identity's mailbox, **must** offer a
  per-message label precondition (an atomic claim), and **must** report IDs the key cannot see together with
  missing ones, without saying which is which ([§ 2](#2-batch-message-operations-fr-in-13)).
- **FR-IN-14** (§ 6.4) The trust metadata of every inbound message **must** name the domain SPF checked and
  whether it aligned, each DKIM signature's domain, selector, result and alignment, the domain and policy DMARC
  evaluated and which method aligned, and the ARC sealer when an ARC chain validated
  ([§ 5](#5-authentication-detail-on-messages-fr-in-14)).
- **FR-IN-15** (§ 6.4) A message that fails DMARC **must** get the verdict `arc_rescued`, and **must not** be
  quarantined for authentication, only when its ARC chain validates, its newest sealer is on the platform-only
  list `inbound.trusted_arc_sealers`, that sealer signed the `From` header and recorded `dmarc=pass` for the
  `From` domain, the seal is recent, and the sealer saw the message addressed to the receiving identity. Mail
  with that verdict **must never** release a verification code
  ([§ 6](#6-trusted-arc-sealers-fr-in-15)).
- **FR-OUT-17** (§ 6.5) Allow and block lists **must** also be settable per identity. For each direction and
  address, the identity's entries, when any matches, **must** decide before the tenant's. No list entry **may**
  override a suppression, a test-mode rule, a daily cap, a sending pause or an authentication quarantine
  ([§ 3](#3-identity-scope-allow-and-block-lists-fr-out-17)).
- **FR-WH-6** (§ 6.10) A tenant **may** opt in to `message.labels_changed`. The event **must** be delivered only
  to endpoints that name it (never through `*`), **must** cover a whole request in one event per 100 messages,
  **must never** be emitted for state the service sets itself, and **must never** name a message the endpoint
  could not receive ([§ 4](#4-label-change-events-fr-wh-6)).
- **FR-WH-7** (§ 6.10) A webhook endpoint **may** carry up to 10 static request headers. Their values **must** be
  write-only and sealed at rest; names that would change how the request is framed, routed or attributed
  **must** be refused; and a change of the endpoint's host **must** remove them
  ([§ 8](#8-webhook-delivery-headers-fr-wh-7)).
- **FR-PRV-7** (§ 6.12) An identity **may** set `retention.message_hours` (1–8,760). Each of its messages, with its
  attachments, extracted text, index rows, vectors and drafts, **must** then be erased within 15 minutes of
  reaching that age, except under a legal hold; a retried send **must never** be sent again after its message
  expired; and each day's expiries **must** be recorded in the audit log with counts per store
  ([§ 7](#7-per-identity-message-retention-fr-prv-7)).
- **FR-DOM-15** (§ 6.3) `GET /v1/domains/{domain_id}/records?format=bind` **must** return the domain's records
  as an RFC 1035 zone fragment built from the same provider-sourced records as the JSON form
  ([§ 9](#9-zone-file-export-fr-dom-15)).
- **FR-API-3** (§ 6.11) Every error envelope **must** carry `docs`, an absolute link to the code's anchor on the
  deployment's published errors page, and every error code **must** have that anchor
  ([§ 10](#10-a-documentation-link-in-errors-fr-api-3)).

## 14. Edge cases

Section **Z · Parity additions** of the [edge-case register](../edge-cases.md). Every row is owned by the
service (`S`) except where marked.

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| Z1 | `batch-get` or `batch-update` with a mix of IDs: visible, missing, of another identity or tenant, erased, and `quarantined`, `screening`, `hidden` or `throttled` while the key lacks `quarantine:review` | `200`. Visible messages are returned or changed in request order; every other ID is in `not_found`, in request order, with nothing that tells the kinds apart; no `404` for the request | S | `it::messages::z1_batch_visibility` |
| Z2 | The same batches with a key that holds `quarantine:review` | `quarantined`, `screening`, `hidden` and `throttled` messages are found, returned and changed as `GET` and `PATCH` allow them; a change to them emits no label event (Z27) | S | `it::messages::z1_batch_visibility` |
| Z3 | Malformed batch: 0 or 101 IDs, a malformed ID, a label that fails the pattern, no change field, a label in both `labels_add` and `labels_remove`, `require_absent` without a label change, `extracted_text_bytes` outside 0–16,384 | `400 invalid_request` with the path of the first problem; nothing read or changed. A repeated ID is processed once, at its first position, and listed once | S | `core::batch::z3_validate_request` |
| Z4 | `batch-get` whose response would pass 1,048,576 bytes | The messages that fit are in `data`; the next found message and every later one are in `remaining`, `truncated: true`; the first found message is always returned; `remaining` never lists an ID of `not_found` | S | `it::messages::z4_batch_get_budget` |
| Z5 | `batch-update` where one message would pass 64 labels; or where every message already matches | The first: `400 invalid_request`, `details.reason = "label_limit"`, `details.message_ids`; no message changes, no event, no audit row. The second: `200` with every ID in `unchanged`, no event, no audit row | S | `it::messages::z5_batch_update_atomic` |
| Z6 | Two agents claim the same message at once with `require_absent: ["claimed"]` and `labels_add: ["claimed"]`; a third claims it later | Exactly one gets it in `updated`, the other in `conflicts`; the third gets `conflicts`; the message never carries two claims from one race | S+I | `it::messages::z6_batch_claim_race` |
| Z7 | `batch-update` with `archived` and `read` over messages of three threads, one already archived | Two threads change and are counted; the third is not counted; `unread_count` is recomputed for each thread; a later visible inbound message unarchives its thread as usual | S | `it::messages::z7_batch_archive_and_read` |
| Z8 | `batch-update` repeated with the same `Idempotency-Key`; with another body; while the first is running | The stored body with `Idempotent-Replayed: true` and no second event or audit row; `409 idempotency_conflict`; `409 request_in_progress` | S | `it::messages::z8_batch_idempotency` |
| Z9 | `batch-get` by a platform or partner key; the audit write fails | One `mail.read` row for the request (`target_type: "identity"`, `details.messages`); a failed write gives `503 unavailable` and no mail. Tenant and identity keys write no row | S | `it::audit::z9_batch_mail_read_row` |
| Z10 | A batch on a `deleting` identity, the system identity (non-platform key), a tenant being erased, a suspended partner, the `read_only` switch, a frozen deployment | As the single-message routes: `404 identity_not_found` for the first three, `403 partner_suspended`; under `read_only`, `503 unavailable` (`details.reason = "read_only"`) for both routes from non-platform keys, because both are `POST` (as search is); `503 unavailable` for both while frozen | S | `it::messages::z10_batch_states` |
| Z11 | MCP `mail_update_labels` with `message_ids`: also with `message_id` or `thread_id`; with `require_absent` and no `message_ids`; with 101 IDs | Two or none of the three → `invalid_request` naming the path; `require_absent` alone → `invalid_request`; 101 → `invalid_request`; a valid call returns the `batch-update` response object | S | `it::mcp::z11_update_labels_batch` |
| Z12 | `pmail messages label` with 250 IDs | Three requests of 100, 100 and 50, and a notice that the change is atomic per request only; exit code 0 only when all three succeed | S | `cli::messages::z12_label_many_chunks` |
| Z13 | Identity receive-block and tenant receive-allow for the same sender (and the reverse) | Identity block: the identity's copy is stored `hidden` (`blocked_sender`) while another identity of the tenant receives it. Identity allow over a tenant block: the identity's copy is not hidden, skips rules 4a and 6, and is still quarantined when authentication fails (rule 3) | S | `core::lists::z13_precedence`, `it::lists::z13_receive_scope` |
| Z14 | One scope holds an allow for `jo@example.net` and a block for `@example.net` | Block wins inside the scope, for both directions | S | `core::lists::z13_precedence` |
| Z15 | An identity-scope send-allow entry for an address the identity has never written to; another identity of the tenant sends to it too | The first identity's send is delivered (the address is known to it); the second's delivery is `suppressed` with `policy: unknown_recipient` | S+I | `it::lists::z15_send_allow_per_identity` |
| Z16 | An identity-scope send-block for an address that is a known contact, the authenticated sender being answered, and on the tenant's send-allow list; with `policy.send_allowlist_only` | `suppressed` with `policy: send_block (identity)` in each case; a dry run reports `reason: "send_block"` with `scope: "identity"`; with `send_allowlist_only`, an identity allow passes and a tenant allow overridden by an identity block does not | S | `it::lists::z16_send_block_scope` |
| Z17 | An allow entry, of either scope, for a suppressed address, a non-simulator address in a test tenant, an identity over its daily cap, a paused tenant, or a sender whose DMARC fails | The suppression, `403 test_mode_recipient`, `429 daily_cap_reached`, `409 sending_paused` and quarantine rule 3 apply exactly as without the entry | S | `it::lists::z17_allow_never_overrides` |
| Z18 | Identity list routes with an identity key; with a key of another tenant or partner; on the system identity; the tenant routes after identity entries exist | `403 permission_denied` (`details.required: "suppressions:manage"`); `404 identity_not_found`; `404` for non-platform keys and `400 invalid_request` (`system_identity`) for a platform key; tenant routes list and delete tenant entries only | S | `it::lists::z18_scope_matrix` |
| Z19 | The 10,001st entry of one identity; the 50,001st of a tenant; two requests racing for the last place; a note update of an existing entry at the limit | `422 list_limit_reached` with `details.scope` `identity`, `tenant` or `total`; exactly one racer succeeds; the note update succeeds | S | `it::lists::z19_limits` |
| Z20 | An identity send-block written after a send was accepted and before transport | The recipient's delivery becomes `suppressed` (`policy: send_block (identity)`) at composition; the others go out | S | `it::lists::z20_block_after_submit` |
| Z21 | A receive entry written while a message is between consumer step 12 and the mailbox commit | The message is decided on the lists read at step 12; the next message uses the new entry | S | `it::lists::z21_inbound_race` |
| Z22 | Identity erasure, identity deletion, tenant erasure and counterparty erasure with list entries in both scopes | Identity erasure and deletion delete the identity's entries and count them in `list_entries_deleted`; tenant erasure deletes every entry; counterparty erasure changes none | S | `it::erasure::z22_list_entries` |
| Z23 | Quarantine rule 4a with lists: an identity receive-allow for a service; a tenant allow overridden by an identity block | The first is exempt from `account_unapproved`; the second is stored `hidden` (`blocked_sender`, rule 1 first) | S | `it::lists::z23_rule_4a_scope` |
| Z24 | Label changes while `events.labels_changed` is `false` (the default); the field turned off while events are still in the outbox | No outbox row, no `event_index` row and no feed entry; events already in the outbox are delivered, later changes emit none | S | `it::label_events::z24_off_by_default` |
| Z25 | `events.labels_changed: true`, with one endpoint subscribed to `*`, one naming the type, and a replay to the `*` endpoint; a key without `messages:read` creating an endpoint that names the type | Only the naming endpoint receives it; replay never sends it to `*`; `403 permission_denied` (`details.required: ["messages:read"]`) | S | `it::label_events::z25_opt_in_delivery` |
| Z26 | A `batch-update` that changes 100 messages; a thread `PATCH` that changes 250 visible messages | One event listing 100 messages; three events with the same `request_id`, `part` 1–3 and `parts: 3`, oldest messages first | S | `it::label_events::z26_batching` |
| Z27 | A key with `quarantine:review` changes labels of a quarantined message, alone and together with a visible one | Alone: no event. Together: one event naming only the visible message | S | `it::label_events::z27_hidden_left_out` |
| Z28 | Changes of read state or archive only; a no-op; a `require_absent` conflict; labels given at send time or carried from a draft; labels removed by erasure, expiry or deletion; a release; a re-parse | No `message.labels_changed` for any of them | S | `it::label_events::z28_system_effects_silent` |
| Z29 | A label event replayed, retried and read from the event feed; an agent that sees its own change | The same `evt_` ID everywhere, so receivers deduplicate; its `sequence` orders it after the message's `message.received`; a consumer that ignores its own `actor.key_id` does not loop | S+I | `it::label_events::z29_replay_and_order` |
| Z30 | Trust detail for a message with no trusted `Authentication-Results` (SPF unchecked); for the SES source | `spf_domain: null`, `spf_aligned: false`; for SES, `spf_domain` is SES's `mail.source` and `spf_aligned` follows `aspf` | S | `core::auth::z30_trust_detail_fields` |
| Z31 | A message with 12 DKIM signatures, some aligned, one from an IDN domain | `dkim_domains` holds the first 10 in header order, each with its result and alignment; domains are A-labels in lower case | S | `core::auth::z30_trust_detail_fields` |
| Z32 | A message stored before this change; a loopback message; the `message.received` and `message.quarantined` payloads; a search hit | Old row: every field, `arc_sealer: null`, `spf_aligned` true only when `dmarc_aligned_by` is `spf`. Loopback: `null` domains, empty `dkim_domains`. Both events carry the fields; a search hit keeps its short trust | S | `it::messages::z32_trust_detail_surfaces` |
| Z33 | A forward sealed by `google.com`: chain valid, AAR `dmarc=pass` for the `From` domain, `from` and `to` in the newest AMS `h=`, `To` naming the identity's `send_only` address, `t=` 2 minutes old, under `p=reject` | Verdict `arc_rescued`, stored `received`, evented as `message.received`, `trust.arc_sealer: "google.com"`, `auth_json.arc.rescue.result: "rescued"` | S | `core::auth::z33_arc_rescue_rules`, `it::ses::z33_forward_rescued` |
| Z34 | The newest sealer is not trusted while an older set is; the newest seal's `d=` is a subdomain of a trusted domain; AS and AMS `d=` differ | No rescue (`untrusted_sealer`); quarantined `auth_failed` under an enforcing policy | S | `core::auth::z33_arc_rescue_rules` |
| Z35 | A chain whose newest seal has `cv=fail`; a gap in instances; 51 sets; a newest AMS that does not verify; a seal key neither resolver returns | No rescue (`chain`); the verdict DMARC gives; the message is not retried for the DNS failure | S | `core::auth::z33_arc_rescue_rules`, `conf::arc::z35_chain_corpus` |
| Z36 | The newest AAR has `dmarc=pass` for another domain, `dmarc=bestguesspass`, `dmarc=none`, or no `dmarc` result | No rescue (`no_dmarc_pass`) | S | `core::auth::z33_arc_rescue_rules` |
| Z37 | The newest AMS does not sign `From` | No rescue (`from_unsigned`) | S | `core::auth::z33_arc_rescue_rules` |
| Z38 | The newest seal's `t=` is 73 hours before receipt, or 20 minutes after; no `t=` and an unsigned `Date`; no `t=` and a `Date` 5 minutes old that the newest AMS signs | No rescue (`stale`) in the first three; rescue in the last | S | `core::auth::z33_arc_rescue_rules` |
| Z39 | Someone else's Gmail-sealed message replayed to the identity (`To`, signed, names another address); a Microsoft-sealed message (`To` unsigned) whose AAR `smtp.rcpttodomain` is the identity's `send_only` domain; one whose `smtp.rcpttodomain` is unrelated; an exact replay of a rescued message | No rescue (`no_binding`); rescue; no rescue (`no_binding`); the replay is deduplicated (B3, B14) | S | `core::auth::z33_arc_rescue_rules`, `it::inbound::z39_replay_not_rescued` |
| Z40 | `inbound.trusted_arc_sealers` set to `[]`; written by a partner key or a workspace writer; containing `co.uk`, an IP literal or 21 entries | No rescue (`off`); `403 scope_denied` (`details.field`, and `not_writable` for a workspace writer); `400 invalid_request` | S | `it::policy::z40_trusted_sealers_field` |
| Z41 | A rescued message that carries a verification code, with and without `accounts.require_approval`, and a `wait(kind=verification)` for its domain | No `verifications` row and no `verification.received`; with the ledger gate it is held `account_unapproved`; `wait` returns the message with `verification: null` | S | `it::inbound::z41_rescued_codes_withheld` |
| Z42 | A reply to the sender of a rescued message under `require_known_recipient`; a rescued message from the tenant's own domain; the inbound volume caps | The reply is delivered (rule 3); `known_sender` is not set by the own-domain rule; the per-sender cap is keyed by the `From` address and the unauthenticated bucket is not charged; the spam score has +0.05 | S | `it::inbound::z42_rescued_consumers`, `core::trust::z42_rescued_known_sender` |
| Z43 | An agentic answer whose sentence cites only rescued mail; triage of a rescued message | The sentence is kept with `citation_trust: partly_authenticated`; triage rule 13 (`auth_failed`) is not raised, and rules 4, 9, 11 and 18 treat it as not `pass` | S | `core::citations::z43_rescued_partly_authenticated`, `core::triage::z43_rescued_rules` |
| Z44 | Several `From` addresses with a trusted chain; `p=none` failing with a trusted chain; the `unverified` case (no trusted authserv) with a trusted chain | Capped at `unaligned`, no rescue; `arc_rescued` instead of `unaligned`; `arc_rescued` instead of `unverified` | S | `core::auth::z33_arc_rescue_rules` |
| Z45 | A DMARC-failing forward sealed by `google.com` arriving through Cloudflare Email Routing (the platform domain) under `p=reject` | Whatever Cloudflare does is recorded as the spike result in § 6.1; when it delivers the message, the rule rescues it; when it refuses it, nothing reaches the Worker and the forwarder gets the refusal | S | `live::arc::cloudflare_forward_behaviour` |
| Z46 | The step-12 read of the identity's addresses fails for a rescue candidate | The consumer retries the message (`retry(30 s)`); it is never stored with a verdict guessed without the binding check | S | `it::inbound::z46_binding_read_retry` |
| Z47 | An identity with `retention.message_hours: 1` receives and sends mail | Each message is erased within 15 minutes after it is an hour old: its row, FTS and trigram rows, refs, labels, attachments, verifications, outbox events, R2 objects in `BLOBS` and `BACKUP`, and vectors; keyword and semantic probes find nothing; `quarantined`, `hidden`, `throttled` and `screening` messages expire on the same clock | S | `it::retention::z47_expiry_erases_all_stores` |
| Z48 | A held thread with due messages; the hold is removed, or its `until` passes; a hold set seconds before a slice | Held messages stay; they expire at the first run after the hold ends; a thread whose hold changed in the last 60 seconds is skipped, and the in-transaction re-check keeps any held row | S | `it::retention::z48_hold_respected` |
| Z49 | Due outbound messages that are `queued`, that have a live transport claim, or that are `uncertain` | The first two are skipped and expire at the first run after they settle; the `uncertain` one expires when due (its reconciliation, at most 30 minutes, ends inside the 1-hour minimum) | S | `it::retention::z49_outbound_in_flight` |
| Z50 | `message_hours` lowered from 72 to 1; raised from 1 to 72; set to `null`; the tenant's `message_days` shorter than `message_hours` | Older messages are erased at the next run; raising brings nothing back; `null` stops expiry; the tenant job deletes what it reaches first | S | `it::retention::z50_change_value` |
| Z51 | Setting or lowering `message_hours` without `erasure:manage`; raising it or `null` with `identities:write` only; 0, 8,761 or 1.5; the system identity | `403 permission_denied` (`details.required: "erasure:manage"`); accepted; `400 invalid_request` (path `retention.message_hours`); `400 invalid_request` (`system_identity`). Each accepted change writes `identity.retention_update` and emits `identity.updated` with `changed: ["retention"]` | S | `it::identities::z51_retention_field_rules` |
| Z52 | A one-time code on an identity with `message_hours: 1`: `wait` within the hour; `wait` after the message expired | The code is released; after expiry `wait` matches nothing and times out, and no verification row remains | S+I | `it::retention::z52_codes_within_window` |
| Z53 | A send retried with the same `Idempotency-Key` and body after its message expired; with another body; after an erasure request erased it | `410 message_expired` (`details.message_id`, `details.expired_at`) and nothing is sent; `409 idempotency_conflict`; the erasure deleted the row, as Privacy § 6.2 says. The kept stub holds a keyed fingerprint and no subject or recipient | S | `it::retention::z53_expired_send_never_resent` |
| Z54 | A webhook delivery still retrying when its message expires; an event delivered before; a replay of the event | The retrying delivery ends `dead` with `event_unavailable`; the delivered copy is the receiver's; the replay gets `event_unavailable` | S+I | `it::retention::z54_events_after_expiry` |
| Z55 | An export completed before the messages expired; an export collecting while they expire | The completed ZIP stays until its 7-day expiry; the running export leaves out the gone messages and counts them in `expired_during_export` | S | `it::retention::z55_exports` |
| Z56 | Vectorize or R2 fails during an expiry slice; failures go on for more than an hour | The batch's rows are kept, the alarm backs off (30 s doubling, at most 5 minutes); `retention_expiry_errors_total{step}` counts; `retention_expiry_overdue` raises a ticket at 1 hour and pages at 24 hours | S | `it::retention::z56_dependency_failure` |
| Z57 | The expiry alarm fails Cloudflare's six retries | The daily maintenance alarm sees due rows and re-arms expiry | S | `it::retention::z57_alarm_recovered` |
| Z58 | The `set_retention` call fails after the `PATCH`; two `PATCH`es seconds apart; a call arriving after a newer one | The repair cron delivers it; the mailbox ends with the newest value; the older `version` is ignored | S | `it::intents::z58_set_retention_retried` |
| Z59 | A day with expiries; the flush repeated; the identity erased before the flush | One `retention.expire` audit row per identity and UTC day with the counts; a repeated flush writes nothing more; an erased identity's pending day has no row and its erasure receipt counts what was left | S | `it::retention::z59_daily_receipt` |
| Z60 | An erasure request or the tenant retention job running at the same time as expiry | No error and no row counted twice; the receipts count only what each deleted | S | `it::retention::z60_concurrent_erasure` |
| Z61 | Drafts of an identity with `message_hours: 24`; a draft scheduled to send after its expiry | A draft is erased 24 hours after its last change; the schedule is refused with `400 invalid_request` (`beyond_retention`) | S | `it::drafts::z61_drafts_expire` |
| Z62 | A reply whose thread token names an expired thread | Filed by its headers when they match a stored message, otherwise a new thread; never refused | S | `it::retention::z62_reply_to_expired_thread` |
| Z63 | Storage after expiry; expiry on a paused identity and a suspended tenant | Stored bytes fall at the next hourly roll-up; expiry continues in both states | S | `it::retention::z63_storage_and_states` |
| Z64 | Creating a webhook with `headers`; reading, listing and replaying the create; the audit row and logs | `header_names` only; no response, idempotency record, audit row or log line holds a value; the create's fingerprint uses `HMAC-SHA256(PM_HASH_KEY, value)` | S | `it::webhooks::z64_headers_write_only` |
| Z65 | `PATCH …/headers` with `set` and `remove`; a name in both in different casings; a name twice in `set`; an empty body; removing a name that is not set; two concurrent `PATCH`es | Applied together; `400 invalid_request`; `400`; `400`; `200` with nothing changed; both apply one after the other through the compare-and-swap, and after 3 lost attempts `409 request_in_progress` | S | `it::webhooks::z65_headers_patch` |
| Z66 | Header names `Content-Type`, `Host`, `webhook-signature`, `Proxy-Authorization`, `Sec-Fetch-Mode`, `Cookie`, `X-Forwarded-For`, `CF-Connecting-IP`, `X-Pylota-Trace`, `X-HTTP-Method-Override`, `Bad Name`; and `Authorization` | `400 header_not_allowed` (`details.reason = "webhook_header"`, `details.name`) for each; `Authorization` is accepted | S | `core::webhook_headers::z66_name_rules` |
| Z67 | A value with CR or LF, a non-ASCII byte, a leading space, 1,025 bytes; values totalling 4,097 bytes; an 11th header | `400 invalid_request` for the first four; `422 webhook_header_limit_reached` with `reason` `bytes` and `count` | S | `core::webhook_headers::z66_name_rules`, `it::webhooks::z65_headers_patch` |
| Z68 | Deliveries of an endpoint with headers: first attempt, retry, replay, a delivery released after a partner suspension, `POST …/test` | Every attempt carries the headers; `Content-Type`, `User-Agent` and the `webhook-*` headers are the service's; the signature is computed as before | S | `it::webhooks::z68_headers_on_every_attempt` |
| Z69 | `PATCH /v1/webhooks/{id}` changes the URL to another host; to another path on the same host | The headers are removed in the same update, and the audit row lists them; the path change keeps them | S | `it::webhooks::z69_host_change_clears_headers` |
| Z70 | `headers_sealed` cannot be unsealed; the master key is rotated | The attempt fails with `secret_unavailable` and alerts; the rotation re-seals the column and counts it | S | `it::webhooks::z70_headers_sealed`, `it::secrets::master_key_rotation` |
| Z71 | An identity key changes headers; reads the endpoint with `webhooks:read`; a partner key reaches another partner's endpoint | `403 scope_denied`; `header_names` visible, values never; `404 webhook_not_found` | S | `it::webhooks::z71_headers_scope` |
| Z72 | `?format=bind` for a `dns_records` subdomain whose registrable domain differs from the domain | `$ORIGIN` is the registrable domain with a trailing dot, `$TTL 3600`, owner names are the records' `host`, `@` for the registrable domain itself, `IN` on every record, absolute targets with a trailing dot | S | `core::zonefile::z72_render`, `conf::zonefile::z72_parses` |
| Z73 | A DKIM TXT value of 410 bytes; a value with `"`, `\` and a byte outside `0x20`–`0x7E` | Two quoted strings of at most 255 octets, never split inside an escape; `\"`, `\\` and `\DDD` | S | `core::zonefile::z72_render` |
| Z74 | Suggested records; an `unexpected` record (a second SPF record); the `ns` records of a `nameservers` domain and of a `delegated_subdomain` domain; a `cloudflare_zone` domain | Suggested records commented out; the `unexpected` record as a comment under "Remove these records", never as a record; `nameservers` NS as comments under the registrar heading; `delegated_subdomain` NS as records; a first comment saying a Cloudflare zone already holds the records | S | `core::zonefile::z74_methods` |
| Z75 | `format=xml`; the provider API failing; another tenant's domain; the platform domain | `400 invalid_request` (path `format`); the same answer as the JSON form gives; `404 domain_not_found`; readable by every key with `domains:read`, with rule 6's first comment (its records are in the deployment's own zone) | S | `it::domains::z75_records_bind_route` |
| Z76 | The bind output of each method parsed by an independent zone-file parser | Every record parses to the type, owner and data of the JSON form | S | `conf::zonefile::z72_parses` |
| Z77 | Any error from REST, from an MCP tool and from `pmail --json`; `404 route_not_found`; the empty `405` of `/mcp` | Each envelope carries `docs` = `{PM_DOCS_URL}/reference/errors#code-{code}`; `route_not_found` too; the `/mcp` `405` has no body and no `docs` | S | `it::errors::z77_docs_in_every_envelope` |
| Z78 | An `ErrorCode` without an anchor in `errors.md`; an anchor for no code; the built page | `cargo xtask docs-check` fails for the first two; every anchor is an element `id` in the built `errors.html` | S | `core::errors::z78_every_code_has_anchor`, `live::docs::z78_anchor_resolves` |
| Z79 | `PM_DOCS_URL` set to `http://…`, to a URL with a query, or with a trailing slash | `pmail setup` and `pmail deploy` refuse the first two; the Worker uses the default and logs `docs_url_invalid` once per isolate; a trailing slash is removed | S | `cli::setup::z79_docs_url_validated`, `it::errors::z77_docs_in_every_envelope` |
| Z80 | A message erased (any scope) or expired while a `message.labels_changed` outbox row lists it with two other messages; and while a row lists only it | The first row keeps the two other entries and is delivered without the erased one; the second row is deleted and counted in `events_deleted`; nothing of the erased message is delivered afterwards | S | `it::label_events::z80_erasure_trims_events` |

## 15. Tests

| Test | Proves | Covers |
|---|---|---|
| `core::batch::z3_validate_request` | The batch body rules: counts, ID pattern, repeats collapsed at their first position, label patterns and counts, the change-field and overlap rules, `require_absent` and `extracted_text_bytes` bounds, each with its error path | §2.3, Z3 |
| `it::messages::z1_batch_visibility` | `not_found` covers missing, erased, other-identity and invisible messages alike, in request order; `review` finds invisible messages; the response never distinguishes the kinds | §2.3, Z1, Z2, NFR-SEC-1 |
| `it::messages::z4_batch_get_budget` | The 1 MiB budget, `remaining`, `truncated`, the first message always returned, `extracted_text_bytes` cuts at a character boundary with `extracted_text_truncated`, no `text`, `html` or `headers` | §2.1, Z4 |
| `it::messages::z5_batch_update_atomic` | The 64-label refusal changes nothing (rows, events, audit); a no-op batch is all `unchanged` with no event and no audit row; one transaction per batch | §2.3, Z5 |
| `it::messages::z6_batch_claim_race` | Concurrent claims give one `updated` and one `conflicts`; a later claim conflicts | §2.2, §2.4, Z6 |
| `it::messages::z7_batch_archive_and_read` | Thread archiving by message, the counts, `unread_count`, unarchiving by new mail | §2.3, Z7, FR-IN-10 |
| `it::messages::z8_batch_idempotency` | Replay, conflict and in-progress answers; a replay writes nothing | §2.4, Z8 |
| `it::audit::z9_batch_mail_read_row` | One `mail.read` row per platform or partner `batch-get`; `503` when it cannot be written; none for tenant and identity keys; one `message.batch_update` row per changing `batch-update` | §2.3, Z9 |
| `it::messages::z10_batch_states` | The batch routes in each tenant, partner, identity and deployment state answer as the single routes do; both routes join `it::security::cross_tenant_matrix` | §1, Z10 |
| `it::mcp::z11_update_labels_batch` | `mail_update_labels` with `message_ids`: the exactly-one rule, `require_absent` only with `message_ids`, the 100 limit, the response object; the input schema is still a plain object (`it::mcp::input_schemas_plain_objects`) | §2.7, Z11 |
| `cli::messages::z12_label_many_chunks` | Chunking into requests of 100, the notice, the exit code | §2.7, Z12 |
| `core::lists::z13_precedence` | `decide`: identity scope before tenant scope, block over allow inside a scope, address and domain candidates as today | §3.1, Z13, Z14 |
| `it::lists::z13_receive_scope` | Identity receive-block and receive-allow against tenant entries, for two identities of one tenant; rule 3 still applies | §3.2, Z13 |
| `it::lists::z15_send_allow_per_identity` | An identity send-allow makes an address known to that identity only | §3.2, Z15, FR-OUT-13 |
| `it::lists::z16_send_block_scope` | Identity send-block over contacts, the reply rule and a tenant allow; the scope in `smtp_response` and in the dry run; `send_allowlist_only` with both scopes | §3.2, Z16 |
| `it::lists::z17_allow_never_overrides` | Suppressions, test mode, caps, pauses and authentication quarantine apply despite allow entries of either scope | §3.3, Z17 |
| `it::lists::z18_scope_matrix` | Permissions and scope of the identity routes, the system identity, and the tenant routes' tenant-only reach; the routes join `it::security::cross_tenant_matrix` | §3.4, Z18 |
| `it::lists::z19_limits` | The scope and tenant limits, the race for the last place, note updates at the limit | §3.5, §3.7, Z19 |
| `it::lists::z20_block_after_submit` | Composition step 1 re-reads identity entries | §3.5, Z20 |
| `it::lists::z21_inbound_race` | The step-12 read decides the message in flight (a fault hook holds the consumer between step 12 and `Ingest`) | §3.5, Z21 |
| `it::erasure::z22_list_entries` | Identity, tenant and counterparty erasure on list entries, with `list_entries_deleted` in the receipts | §3.8, Z22 |
| `it::lists::z23_rule_4a_scope` | Rule 4a's exemption follows the effective receive decision | §3.2, Z23 |
| `it::label_events::z24_off_by_default` | No outbox, `event_index` or feed rows while the policy field is off; switching it off stops new events only | §4.1, §4.3, Z24 |
| `it::label_events::z25_opt_in_delivery` | `*` excludes the type for fan-out and replay; naming it delivers; J29's `messages:read` rule; `it::webhooks::filters` now asserts that `*` includes new types except those in `OPT_IN_EVENT_TYPES` | §4.1, Z25 |
| `it::label_events::z26_batching` | One event per batch; thread changes split into parts with one `request_id`; the payload's fields and its golden file | §4.2, §4.3, Z26 |
| `it::label_events::z27_hidden_left_out` | Invisible messages never appear in the event | §4.3, Z27 |
| `it::label_events::z28_system_effects_silent` | No event for read state, archive, no-ops, conflicts, creation labels, erasure, expiry, release or re-parse | §4.3, Z28 |
| `it::label_events::z29_replay_and_order` | One event ID across webhook, replay and feed; `sequence` ordering; idempotent replays emit nothing | §4.3, §4.4, Z29 |
| `it::label_events::z80_erasure_trims_events` | `EraseRows` (erasure and expiry) removes the message from pending label events and deletes emptied rows | §4.3, Z80, FR-PRV-3 |
| `core::auth::z30_trust_detail_fields` | The seven fields from `auth_json` for routing and SES sources, SPF unchecked, 12 DKIM signatures, IDN domains | §5.1, Z30, Z31 |
| `it::messages::z32_trust_detail_surfaces` | Old rows, loopback rows, the two event payloads, search hits | §5.1, Z32 |
| `core::auth::z33_arc_rescue_rules` | Every condition of the rule and its `reason`, with recorded Gmail and Exchange Online header sets (the `d=`, `s=`, `h=` and AAR forms of § 6.1) and synthetic chains; multiple `From`, `p=none` and `unverified` | §6.3, Z33–Z39, Z44 |
| `conf::arc::z35_chain_corpus` | ARC chains that RFC 8617 § 5.2 fails (cv, gaps, 51 sets, broken AMS, missing key) all fail through `mail-auth` 0.13.3 and the prefetch cache | §6.3, Z35 |
| `it::ses::z33_forward_rescued` | A DMARC-failing, `google.com`-sealed forward through the SES source is stored `received` with `arc_rescued` and evented | §6.3, Z33 |
| `it::inbound::z39_replay_not_rescued` | A replayed sealed message without binding is quarantined; an exact replay of a rescued message is deduplicated | §6.4, Z39 |
| `it::policy::z40_trusted_sealers_field` | The field's class, validation and `[]` | §6.2, Z40 |
| `it::inbound::z41_rescued_codes_withheld` | No verification row, event or release for rescued code mail; the ledger gate holds it | §6.5, Z41 |
| `it::inbound::z42_rescued_consumers` | The reply rule, the volume caps and the spam weight for `arc_rescued` | §6.5, Z42 |
| `core::trust::z42_rescued_known_sender` | The own-domain `known_sender` rule needs `pass` | §6.5, Z42 |
| `core::citations::z43_rescued_partly_authenticated` | `Rescued` evidence keeps a sentence with `partly_authenticated`; `Unauthenticated` alone still removes it | §6.5, Z43, FR-SRCH-13 |
| `core::triage::z43_rescued_rules` | Rule 13 is not raised for `arc_rescued`; rules 4, 9, 11 and 18 treat it as not `pass` | §6.5, Z43 |
| `live::arc::cloudflare_forward_behaviour` | What Cloudflare Email Routing does with a DMARC-failing, `google.com`-sealed forward under `p=reject` (a real Google Workspace forwarding rule to the staging platform domain); the result is recorded in § 6.1 | §6.1, Z45 |
| `it::inbound::z46_binding_read_retry` | A failed binding read retries the message and stores nothing | §6.6, Z46 |
| `it::retention::z47_expiry_erases_all_stores` | Every store of a message is gone within one alarm of its due time (time-controlled harness), in `BLOBS` and `BACKUP`; keyword and semantic probes are empty; every inbound status expires | §7.3, §7.4, Z47 |
| `it::retention::z48_hold_respected` | Holds skip rows, release them after they end, and the 60-second rule and in-transaction re-check | §7.3, §7.4, Z48, FR-PRV-4 |
| `it::retention::z49_outbound_in_flight` | `queued`, claimed and `uncertain` outbound messages | §7.3, Z49 |
| `it::retention::z50_change_value` | Lowering, raising, `null`, and the shorter of the identity and tenant rules | §7.1, §7.2, Z50 |
| `it::identities::z51_retention_field_rules` | Permissions, bounds, the system identity, the audit row and the event | §7.1, Z51 |
| `it::retention::z52_codes_within_window` | Codes inside and after the window | §7.7, Z52 |
| `it::retention::z53_expired_send_never_resent` | The stub, its keyed fingerprint, `410 message_expired`, `409` for another body, erasure deleting the row | §7.6, Z53, U2 |
| `it::retention::z54_events_after_expiry` | `event_unavailable` for pending deliveries and replays | §7.8, Z54 |
| `it::retention::z55_exports` | Completed exports kept; running exports leave out and count expired messages | §7.8, Z55 |
| `it::retention::z56_dependency_failure` | Back-off, kept rows, the error counter and the overdue alert | §7.4, §7.5, Z56 |
| `it::retention::z57_alarm_recovered` | The maintenance alarm re-arms expiry | §7.4, Z57 |
| `it::intents::z58_set_retention_retried` | The `set_retention` intent, its repair, and version ordering | §7.2, Z58 |
| `it::retention::z59_daily_receipt` | The daily `retention.expire` row, its counts and its idempotent flush | §7.9, Z59, FR-PRV-2 |
| `it::retention::z60_concurrent_erasure` | Expiry, erasure and tenant retention together, with exact receipts | §7.10, Z60 |
| `it::drafts::z61_drafts_expire` | Draft expiry and the `beyond_retention` refusal (runs once M29 has landed) | §7.10, Z61 |
| `it::retention::z62_reply_to_expired_thread` | Threading after expiry | §7.10, Z62 |
| `it::retention::z63_storage_and_states` | Storage roll-up after expiry; expiry while paused or suspended | §7.10, Z63 |
| `it::webhooks::z64_headers_write_only` | Values never leave the seal; the keyed fingerprint | §8.1, §8.3, Z64 |
| `it::webhooks::z65_headers_patch` | `PATCH …/headers` rules, the compare-and-swap, the limits | §8.2, §8.3, Z65, Z67 |
| `core::webhook_headers::z66_name_rules` | Name and value rules, the full refused list, `Authorization` allowed | §8.2, Z66, Z67 |
| `it::webhooks::z68_headers_on_every_attempt` | Headers on every kind of attempt, service headers last, signature unchanged | §8.3, Z68 |
| `it::webhooks::z69_host_change_clears_headers` | Host change removes headers; path change keeps them | §8.2, Z69 |
| `it::webhooks::z70_headers_sealed` | Envelope and associated data of `headers_sealed`; unseal failure handling | §8.3, Z70 |
| `it::secrets::master_key_rotation` (exists, M17 Foundation) | Gains the `webhook_endpoints.headers_sealed` case: the rotation re-seals the column, counts it, and deliveries keep their headers throughout | §8.3, Z70 |
| `it::webhooks::z71_headers_scope` | Level and scope rules for headers | §8.2, Z71 |
| `core::zonefile::z72_render` | Origin, TTL, names, targets, TXT splitting and escaping | §9.2, Z72, Z73 |
| `conf::zonefile::z72_parses` | The output of each method parses with an independent RFC 1035 master-file parser (a native dev-dependency chosen at build time) and matches the JSON form | §9.2, Z72, Z76 |
| `core::zonefile::z74_methods` | Suggested and `unexpected` records, NS by method, the Cloudflare comment | §9.2, Z74 |
| `it::domains::z75_records_bind_route` | `format` values, failures as the JSON form, scope, headers of the response | §9.1, Z75 |
| `it::errors::z77_docs_in_every_envelope` | `docs` in REST, MCP and CLI envelopes, the URL form, the fallback for an invalid `PM_DOCS_URL` | §10.1, Z77, Z79 |
| `core::errors::z78_every_code_has_anchor` | Every `ErrorCode` has exactly one `code-…` anchor in `docs/src/reference/errors.md`, and every anchor names a code (the `cargo xtask docs-check` source check) | §10.2, Z78 |
| `live::docs::z78_anchor_resolves` | On staging, a `docs` URL from a real error loads and the page has the element | §10.2, Z78 |
| `cli::setup::z79_docs_url_validated` | `pmail setup` and `pmail deploy` refuse an invalid `PM_DOCS_URL` | §10.1, Z79 |

## 16. Milestone

To be added to the [Build plan](../build-plan.md) after M36, in the build plan's style (links rewritten
relative to `build-plan.md` when pasted):

### M37 · Parity additions (after M7 to M16, M21, M23 and M27)

**Goal:** the nine additions of this page, each wired end to end, with the register's section Z green. The
nine are independent of each other and can land as separate pull requests in any order, each with its own
rows of section Z.

**Tasks**, in a workable order:

1. Docs link (§10): `docs`, `ErrorCode::anchor()`, `PM_DOCS_URL`, the anchors in `errors.md`, `cargo xtask
   docs-check`. First, because every later error code needs its anchor.
2. Trust detail (§5): `spf.aligned`, `arc.{sealer, sealer_trusted, aar}` in `auth_json`; the seven `trust` fields.
3. Trusted ARC sealers (§6): the policy field, `core::auth::arc_rescue`, the step-12 binding read, the verdict
   value, each consumer of the §6.5 table; the live test on staging.
4. Batch operations (§2): the two routes, `BatchGet` and `BatchUpdate`, the audit rows, MCP and CLI.
5. Label events (§4), on top of 4: the policy field, `OPT_IN_EVENT_TYPES`, the outbox append in the three
   label-changing requests.
6. Identity-scope lists (§3): the table change, `core::lists::decide`, the four routes, the inbound and
   outbound readers, erasure counts.
7. Webhook headers (§8): the columns and sealed entry, create and `PATCH …/headers`, delivery, host-change
   clearing.
8. Retention (§7): the identity field, the `set_retention` intent, `mailbox/expiry.rs`, the idempotency stub,
   the daily receipt, the alert.
9. Zone export (§9): `core::zonefile::render` and the `format` parameter.

**Files:** as the page's Code row. One migration, `migrations/d1/00NN_parity_additions.sql` only if
`0001_init.sql` has shipped to a production deployment when M37 lands; otherwise `0001_init.sql` and mailbox
schema v1 are edited in place (`sender_lists`, `identities.message_hours`, the three `webhook_endpoints`
columns, the `rpc_intents.op` value and the `messages.verdict` value). Hooks in other milestones' files, each
reviewed by that file's owner:

| File | Hook |
|---|---|
| `mailbox/read.rs` (M7) | `BatchGet`, `BatchUpdate`, and the label-event append in `UpdateMessage` and `UpdateThread` |
| `consumers/inbound.rs`, `mailbox/ingest.rs` (M7) | Identity-scope list rows at step 12; the ARC rescue in steps 7 and 12; `arc_rescued` in the trust signals, caps and verification rules |
| `crates/core/src/auth.rs`, `trust.rs` (M2, M7) | `arc_rescue`, the trust detail fields, `auth_json` keys |
| `handlers/wait.rs` (M7), `accounts` gate (M27) | `arc_rescued` is not `pass` for releases and the ledger match |
| `handlers/send.rs`, `consumers/outbound.rs`, `mailbox/idempotency.rs` (M9) | Identity-scope send entries at step 6 and composition step 1; the expired stub in the reservation |
| `webhooks/payloads.rs`, `consumers/webhooks.rs`, `handlers/webhooks.rs`, `webhooks/client.rs` (M8) | The `labels_changed` builder, `OPT_IN_EVENT_TYPES`, headers on delivery, `PATCH …/headers`, host-change clearing |
| `search/citations.rs` (M11), `triage/rules.rs` (M12) | The `Rescued` evidence state; rule 13 |
| `handlers/domains.rs` (M13, M23) | `format=bind` |
| `mailbox/erase.rs`, `jobs/erasure.rs` (M14) | `EraseRows { mode: Expiry }`; `list_entries_deleted`; the identity-scope list rows of identity erasure |
| `handlers/identities.rs`, `crons/intents.rs` (M6) | `retention`, the `set_retention` intent |
| `crates/api-types/src/errors.rs` (M3), `crates/worker/src/errors.rs` (M5) | `docs`, `ErrorCode::anchor()`, `PM_DOCS_URL` |
| `mcp/tools.rs` (M15) | `mail_update_labels` with `message_ids` and `require_absent` |
| `crates/cli/src/commands/*` (M16) | The commands of the page |
| `console/pages/*` (M21) | Inbox retention and lists, ARC wording, webhook header names, zone download |
| `crates/core/src/sealed.rs` (M17 Foundation) | `webhook_endpoints.headers_sealed`, with its case in `it::secrets::master_key_rotation` |
| `drafts` (M29), event feed (M30) | Draft expiry and `beyond_retention` (M29); feed entries of label events and of expired messages (M30). If M29 or M30 lands after M37, that milestone adds its hook and runs the M37 test named for it |

**Implements:** FR-IN-13, FR-IN-14, FR-IN-15, FR-OUT-17, FR-WH-6, FR-WH-7, FR-PRV-7, FR-DOM-15, FR-API-3, and edge
rows Z1–Z80.

**Acceptance:**

- Every test of [§15](#15-tests), by row: Z1–Z12 (batch), Z13–Z23
  (lists), Z24–Z29 and Z80 (label events), Z30–Z32 (trust detail), Z33–Z46 (ARC), Z47–Z63 (retention), Z64–Z71 (webhook
  headers), Z72–Z76 (zone export), Z77–Z79 (docs links).
- The new routes (seven operations) and the changed ones join `it::security::cross_tenant_matrix`;
  `sdk::coverage::every_operation` passes with the new operations; `it::mcp::input_schemas_plain_objects` passes.
- `it::webhooks::filters` asserts that `*` includes every type except `message.labels_changed`.
- `live::arc::cloudflare_forward_behaviour` has run on staging and its result is recorded in
  [§6.1](#61-sources-read-on-2026-10-17) as a "Spike result" note, on the first staging candidate that
  carries M37 ([Testing § 10](testing.md#10-live-end-to-end-suite-live)); `live::docs::z78_anchor_resolves`
  runs in the same suite.
- `cargo xtask docs-check` is in the CI gate and passes; `mdbook build docs` passes.
- With every new setting at its default (`events.labels_changed: false`, no identity lists,
  `retention.message_hours: null`, no webhook headers), the M7, M8, M9 and M14 suites pass unchanged, except
  the trust golden files, which gain the seven fields, and `it::webhooks::filters`.

## 17. Compared with AgentMail (read 2026-10-17)

| AgentMail | Pylota Mail | |
|---|---|---|
| `batch_update`: one label change to up to 50 messages, atomic for found messages; unknown or invisible IDs skipped | `batch-update` of up to 100, atomic, labels, read state and thread archive in one request, IDs the key cannot see reported with missing ones; plus `require_absent`, an atomic claim AgentMail's docs say labels cannot give ("label-based dedupe not atomic") | Exceed |
| `batch-get` returning metadata and a preview | `batch-get` of up to 100 with `extracted_text` capped per message and a 1 MiB budget | Match |
| Lists at org, pod and inbox scope; the most specific scope checked first | Tenant and identity scope, the most specific first; partners have no scope of their own, because a partner never owns mail | Match |
| `reply` lists: replies checked against reply lists only | No `reply` direction: receive-allow is an exemption, never an allow-only mode, so nothing blocks a reply that a reply list would have to let through ([§3.6](#36-reply-lists-not-added)) | Deliberately different |
| An allow list with one entry blocks every other address of its direction | Allow-only stays the explicit `send_allowlist_only` switch; adding an entry never silently blocks everyone else | Deliberately different |
| Inbox events feed: `label.added` and `label.removed`, including system labels such as `received` | `message.labels_changed`, opt-in, one event per request with the actor, never for system effects, never naming invisible mail, in the same feed and webhooks as every event | Exceed |
| `authentication_results`: spf, dkim, dmarc and one domain each, `arc_sealer` | The same, with every DKIM signature (domain, selector, result, alignment), SPF alignment, the DMARC policy in force and which method aligned | Exceed |
| DMARC failures under `quarantine` or `reject` are dropped without a bounce or an event, unless a trusted ARC seal from google.com, microsoft.com, outlook.com or icloud.com vouches for them | Never dropped: quarantined and releasable. A rescue needs a valid chain, a trusted newest sealer, a signed `From`, the sealer's `dmarc=pass` for that domain, a recent seal and a recipient binding; the default list holds only the two sealers verified this run; rescued mail never releases codes | Exceed (and deliberately narrower list) |
| Optional org-wide message expiry 24 hours after creation; access ends, copies already made remain | Per identity, 1 to 8,760 hours; the message, its objects, index rows, vectors and drafts are erased, legal holds respected, a daily audit receipt, and a retried send still never sends twice; copies already delivered remain, and say so | Exceed |
| Write-only custom delivery headers; names readable; set and remove in one update | The same, with a deny list, limits, sealing under the master key, an audit row, and removal when the endpoint's host changes | Exceed |
| BIND zone-file export of a domain's records | `?format=bind` from the same provider-read records, with each record's purpose, requirement and DNS status, and NS records placed by method | Exceed |
| Error envelope with `docs` | `docs` with an anchor per code, checked in CI, from the deployment's own docs base | Match |

## 18. Integration

The shared pages carry this page's contracts: the PRD, the REST API reference and `openapi.yaml`, the
errors, events, limits and configuration references, the data model, security, workspace policy and the
other design pages it extends, the MCP and CLI references, the edge-case register and the build plan
([ADR 0017 § Integration](../adr/0017-agentmail-parity.md#integration), integrated on 2026-10-18). A
difference between this page and one of them is a defect: fix both in the same change.
